//! Native telemetry providers, bounded local storage, and lifecycle diagnostics.
//! Command spans link native work to its frontend request; the panic hook
//! records only sanitized locations without recursing into the writer.

pub mod attributes;
mod exporter;
pub mod ingest;
#[cfg(feature = "otlp_export")]
mod otlp_export;
pub mod privacy;
mod store;
pub mod trace_context;

use std::cell::Cell;
use std::fs;
use std::io::Write as _;
use std::panic::PanicHookInfo;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use opentelemetry::logs::{AnyValue, LogRecord as _, Logger as _, LoggerProvider as _, Severity};
use opentelemetry::metrics::{Counter, Gauge, Histogram, Meter, MeterProvider as _};
use opentelemetry::trace::{
    Span as _, SpanContext, TraceContextExt, TraceFlags, TraceState, Tracer as _,
    TracerProvider as _,
};
use opentelemetry::{Context, KeyValue, SpanId, TraceId};
use opentelemetry_sdk::logs::SdkLoggerProvider;
use opentelemetry_sdk::metrics::{PeriodicReader, SdkMeterProvider};
use opentelemetry_sdk::trace::{Sampler, SdkTracerProvider};
use opentelemetry_sdk::Resource;
use opentelemetry_semantic_conventions::resource::{
    DEPLOYMENT_ENVIRONMENT_NAME, HOST_ARCH, OS_TYPE, SERVICE_INSTANCE_ID, SERVICE_NAME,
    SERVICE_VERSION,
};

use crate::profile::{self, ENVIRONMENT_NAME};
use privacy::{sanitize_source_location, truncate_to_limit, DEFAULT_MAX_ATTRIBUTE_LEN};
use trace_context::TraceContext;

/// Start marker shares the segments’ owner-only directory; presence on the next launch means no
/// clean exit.
const RUN_MARKER_FILE: &str = "run.marker";

// Skip telemetry on panic-hook re-entry to avoid recursion; still invoke the previous hook.
thread_local! {
    static IN_PANIC_HOOK: Cell<bool> = const { Cell::new(false) };
}

pub const TELEMETRY_DIR_NAME: &str = "telemetry";

pub const TRACE_SEGMENT_FILE: &str = "traces.jsonl";
pub const LOG_SEGMENT_FILE: &str = "logs.jsonl";
pub const METRIC_SEGMENT_FILE: &str = "metrics.jsonl";

const SCOPE_NAME: &str = "tau";

/// Collect frequently so a crash loses little completed metric telemetry.
const METRIC_EXPORT_INTERVAL: std::time::Duration = std::time::Duration::from_secs(15);

struct Metrics {
    invoke_duration: Histogram<f64>,
    ui_action_duration: Histogram<f64>,
    pi_rpc_duration: Histogram<f64>,
    controller_start_duration: Histogram<f64>,
    event_loop_lag: Histogram<f64>,
    long_task_duration: Histogram<f64>,
    pi_process_starts: Counter<u64>,
    pi_process_exits: Counter<u64>,
    pi_rpc_failures: Counter<u64>,
    pi_rpc_abandoned: Counter<u64>,
    pi_rpc_unmatched: Counter<u64>,
    pi_reader_malformed: Counter<u64>,
    /// Populated from the frontend's own periodic `frontend.heartbeat` log
    /// once it has already been validated — never a separate wire format.
    pending_rpc_gauge: Gauge<u64>,
    controller_count_gauge: Gauge<u64>,
    active_controller_gauge: Gauge<u64>,
    runtime_count_gauge: Gauge<u64>,
    queue_length_gauge: Gauge<u64>,
    /// Populated from the frontend's own `telemetry.health` log (dropped
    /// count) and natively at `record_writer_health` (failed-write count).
    telemetry_dropped_gauge: Gauge<u64>,
    telemetry_failed_write_gauge: Gauge<u64>,
}

impl Metrics {
    fn new(meter: &Meter) -> Self {
        Metrics {
            invoke_duration: meter
                .f64_histogram("tau.invoke.duration")
                .with_unit("ms")
                .build(),
            ui_action_duration: meter
                .f64_histogram("tau.ui_action.duration")
                .with_unit("ms")
                .build(),
            pi_rpc_duration: meter
                .f64_histogram("tau.pi_rpc.duration")
                .with_unit("ms")
                .build(),
            controller_start_duration: meter
                .f64_histogram("tau.controller_start.duration")
                .with_unit("ms")
                .build(),
            event_loop_lag: meter
                .f64_histogram("tau.frontend.event_loop_lag")
                .with_unit("ms")
                .build(),
            long_task_duration: meter
                .f64_histogram("tau.frontend.long_task.duration")
                .with_unit("ms")
                .build(),
            pi_process_starts: meter.u64_counter("tau.pi_process.starts").build(),
            pi_process_exits: meter.u64_counter("tau.pi_process.exits").build(),
            pi_rpc_failures: meter.u64_counter("tau.pi_rpc.failures").build(),
            pi_rpc_abandoned: meter.u64_counter("tau.pi_rpc.abandoned").build(),
            pi_rpc_unmatched: meter.u64_counter("tau.pi_rpc.unmatched_responses").build(),
            pi_reader_malformed: meter.u64_counter("tau.pi_reader.malformed_lines").build(),
            pending_rpc_gauge: meter.u64_gauge("tau.telemetry.pending_rpc_count").build(),
            controller_count_gauge: meter.u64_gauge("tau.telemetry.controller_count").build(),
            active_controller_gauge: meter
                .u64_gauge("tau.telemetry.active_controller_count")
                .build(),
            runtime_count_gauge: meter.u64_gauge("tau.telemetry.runtime_count").build(),
            queue_length_gauge: meter.u64_gauge("tau.telemetry.queue_length").build(),
            telemetry_dropped_gauge: meter.u64_gauge("tau.telemetry.dropped_count").build(),
            telemetry_failed_write_gauge: meter
                .u64_gauge("tau.telemetry.failed_write_count")
                .build(),
        }
    }
}

/// Per-launch pipeline. Store failures fall back to bounded memory, never blocking startup.
/// Outside admin mode providers exist but remain disabled.
pub struct Telemetry {
    logger_provider: SdkLoggerProvider,
    tracer_provider: SdkTracerProvider,
    meter_provider: SdkMeterProvider,
    metrics: Metrics,
    store: Arc<store::Store>,
    resource_json: serde_json::Value,
    /// Where the run marker lives, so `record_app_exited` can clear it.
    telemetry_dir: PathBuf,
    /// Capture the previous run’s marker before writing this run’s marker.
    previous_run_unclean: bool,
    /// Gate marker/lifecycle events separately; they bypass the store gate.
    enabled: std::sync::atomic::AtomicBool,
}

impl Telemetry {
    pub fn init() -> Self {
        Telemetry::new(
            resolve_telemetry_dir(),
            Arc::new(store::SystemClock),
            crate::admin::admin_mode_enabled(),
        )
    }

    #[cfg(test)]
    pub(crate) fn for_test(dir: PathBuf) -> Self {
        Telemetry::new(dir, Arc::new(store::SystemClock), true)
    }

    fn new(dir: PathBuf, clock: Arc<dyn store::Clock>, enabled: bool) -> Self {
        let previous_run_unclean = enabled && previous_run_was_unclean(&dir);
        if enabled {
            write_run_marker(&dir);
        }
        let store = Arc::new(store::Store::with_enabled(
            dir.clone(),
            store::StoreConfig::production(),
            clock,
            enabled,
        ));
        let resource = build_resource();
        let resource_json = exporter::resource_to_json(&resource);
        let log_exporter = exporter::JsonFileLogExporter::new(Arc::clone(&store), &resource);
        let span_exporter = exporter::JsonFileSpanExporter::new(Arc::clone(&store), &resource);
        let metric_exporter = exporter::JsonFileMetricExporter::new(Arc::clone(&store), &resource);
        let logger_provider = SdkLoggerProvider::builder()
            .with_resource(resource.clone())
            .with_simple_exporter(log_exporter)
            .build();
        let metric_reader = PeriodicReader::builder(metric_exporter)
            .with_interval(METRIC_EXPORT_INTERVAL)
            .build();
        let meter_provider = SdkMeterProvider::builder()
            .with_resource(resource.clone())
            .with_reader(metric_reader)
            .build();
        let metrics = Metrics::new(&meter_provider.meter(SCOPE_NAME));
        let tracer_provider = SdkTracerProvider::builder()
            .with_resource(resource)
            .with_sampler(Sampler::AlwaysOn)
            .with_simple_exporter(span_exporter)
            .build();
        Telemetry {
            logger_provider,
            tracer_provider,
            meter_provider,
            metrics,
            store,
            resource_json,
            telemetry_dir: dir,
            previous_run_unclean,
            enabled: std::sync::atomic::AtomicBool::new(enabled),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// On admin enable, start a run marker and lifecycle; on disable, flush and clear it so
    /// toggling does not imply a crash.
    pub fn set_enabled(&self, enabled: bool) {
        if self
            .enabled
            .swap(enabled, std::sync::atomic::Ordering::Relaxed)
            == enabled
        {
            return;
        }
        if enabled {
            write_run_marker(&self.telemetry_dir);
            self.store.set_enabled(true);
            self.emit_lifecycle_log("app.started");
            self.force_flush_logs();
            return;
        }
        self.emit_lifecycle_log("app.telemetry_disabled");
        self.force_flush_logs();
        self.force_flush_metrics();
        self.store.set_enabled(false);
        clear_run_marker(&self.telemetry_dir);
    }

    /// Flush the prior unclean-exit event in case this launch also crashes.
    pub fn record_app_started(&self) {
        self.emit_lifecycle_log("app.started");
        if self.previous_run_unclean {
            self.emit_lifecycle_log("app.unclean_exit_detected");
            let _ = self.logger_provider.force_flush();
        }
    }

    /// Flush exit and writer-health events before clearing the marker; call from
    /// `RunEvent::Exit`.
    pub fn record_app_exited(&self) {
        if !self.is_enabled() {
            return;
        }
        self.emit_lifecycle_log("app.exited");
        self.record_writer_health();
        let _ = self.logger_provider.force_flush();
        self.force_flush_metrics();
        clear_run_marker(&self.telemetry_dir);
    }

    pub fn shutdown(&self) {
        let _ = self.logger_provider.shutdown();
        let _ = self.tracer_provider.shutdown();
        let _ = self.meter_provider.shutdown();
    }

    /// Force collection for tests and explicit flush points; otherwise the periodic reader
    /// waits.
    pub fn force_flush_metrics(&self) {
        let _ = self.meter_provider.force_flush();
    }

    /// The simple processor syncs each emit already; explicit flush preserves exit/failure
    /// boundaries.
    pub fn force_flush_logs(&self) {
        let _ = self.logger_provider.force_flush();
    }

    fn emit_lifecycle_log(&self, event_name: &'static str) {
        let logger = self.logger_provider.logger(SCOPE_NAME);
        let mut record = logger.create_log_record();
        record.set_timestamp(SystemTime::now());
        record.set_event_name(event_name);
        record.set_severity_number(Severity::Info);
        record.set_body(AnyValue::String(event_name.into()));
        logger.emit(record);
    }

    /// Count failed writes even when fallback records have since been evicted.
    fn record_writer_health(&self) {
        let failed_writes = self.store.failed_writes();
        self.metrics
            .telemetry_failed_write_gauge
            .record(failed_writes, &[]);
        if failed_writes == 0 {
            return;
        }
        self.emit_family_log(
            attributes::TELEMETRY_HEALTH.name,
            "telemetry.writer_failure",
            None,
            None,
            None,
            &[(
                "tau.telemetry.failed_write_count",
                opentelemetry::Value::I64(i64::try_from(failed_writes).unwrap_or(i64::MAX)),
            )],
        );
    }

    /// Unknown families are no-ops so new span families cannot break ingestion.
    pub(super) fn record_operation_duration(&self, family: &str, duration_ms: f64) {
        let histogram = if family == attributes::TAURI_INVOKE.name {
            &self.metrics.invoke_duration
        } else if family == attributes::UI_ACTION.name {
            &self.metrics.ui_action_duration
        } else if family == attributes::PI_RPC.name {
            &self.metrics.pi_rpc_duration
        } else {
            return;
        };
        histogram.record(duration_ms, &[]);
    }

    pub(super) fn record_controller_start_duration(&self, duration_ms: f64) {
        self.metrics
            .controller_start_duration
            .record(duration_ms, &[]);
    }

    /// Increments the `pi.rpc` failure counter: called once per ingested
    /// `pi.rpc` span whose own `pi.rpc.outcome` attribute is present and not
    /// `"success"`.
    pub(super) fn record_rpc_failure(&self) {
        self.metrics.pi_rpc_failures.add(1, &[]);
    }

    pub(super) fn record_rpc_abandoned(&self) {
        self.metrics.pi_rpc_abandoned.add(1, &[]);
    }

    pub(super) fn record_rpc_unmatched(&self) {
        self.metrics.pi_rpc_unmatched.add(1, &[]);
    }

    pub fn record_pi_process_start(&self) {
        self.metrics.pi_process_starts.add(1, &[]);
    }

    /// Only two hardcoded exit outcomes; no untrusted metric dimension enters here.
    pub fn record_pi_process_exit(&self, clean: bool) {
        let outcome = if clean { "clean" } else { "unexpected" };
        self.metrics
            .pi_process_exits
            .add(1, &[KeyValue::new("tau.process.exit_outcome", outcome)]);
    }

    pub(super) fn record_heartbeat_gauges(
        &self,
        pending_rpc_count: u64,
        controller_count: u64,
        active_controller_count: u64,
        runtime_count: u64,
        queue_length: u64,
    ) {
        self.metrics
            .pending_rpc_gauge
            .record(pending_rpc_count, &[]);
        self.metrics
            .controller_count_gauge
            .record(controller_count, &[]);
        self.metrics
            .active_controller_gauge
            .record(active_controller_count, &[]);
        self.metrics.runtime_count_gauge.record(runtime_count, &[]);
        self.metrics.queue_length_gauge.record(queue_length, &[]);
    }

    /// Records the frontend's own `telemetry.health` drop count (already
    /// validated by the time this is called) into the native gauge.
    pub(super) fn record_telemetry_dropped_gauge(&self, dropped_count: u64) {
        self.metrics
            .telemetry_dropped_gauge
            .record(dropped_count, &[]);
    }

    pub(super) fn record_event_loop_lag(&self, lag_ms: f64, dimensions: &[KeyValue]) {
        self.metrics.event_loop_lag.record(lag_ms, dimensions);
    }

    pub(super) fn record_long_task_duration(&self, duration_ms: f64) {
        self.metrics.long_task_duration.record(duration_ms, &[]);
    }

    /// Links native work to the frontend trace via explicit context, never ambient
    /// state: concurrent invocations must not share trace context. Invalid IDs
    /// or catalog names skip telemetry without failing the command.
    pub fn start_command_span(
        &self,
        context: &TraceContext,
        command: &'static str,
    ) -> Option<CommandSpan> {
        let context = TraceContext::parse(&context.to_traceparent()).ok()?;
        let trace_id = TraceId::from_hex(&context.trace_id).ok()?;
        let span_id = SpanId::from_hex(&context.span_id).ok()?;
        let flags = if context.sampled {
            TraceFlags::SAMPLED
        } else {
            TraceFlags::NOT_SAMPLED
        };
        let parent_span_context =
            SpanContext::new(trace_id, span_id, flags, true, TraceState::default());
        let parent_cx = Context::new().with_remote_span_context(parent_span_context);

        let command_value = opentelemetry::Value::String(command.into());
        attributes::validate(
            attributes::TAURI_INVOKE.name,
            "tau.invoke.command",
            &command_value,
        )
        .ok()?;

        let tracer = self.tracer_provider.tracer(SCOPE_NAME);
        let mut span = tracer.start_with_context(attributes::TAURI_INVOKE.name, &parent_cx);
        span.set_attribute(KeyValue::new("tau.invoke.command", command));
        Some(CommandSpan { span })
    }

    /// Persists an already-validated OTLP trace JSON record. Used only by
    /// `ingest.rs`, after it has revalidated a frontend record against the
    /// attribute catalog.
    pub(super) fn record_trace(&self, value: serde_json::Value) -> bool {
        self.store.append(store::Signal::Trace, value)
    }

    /// Persists an already-validated OTLP log JSON record. Used only by
    /// `ingest.rs`, for frontend-originated logs (`frontend.error`) after
    /// it has revalidated the record against the attribute catalog.
    pub(super) fn record_log(&self, value: serde_json::Value) -> bool {
        self.store.append(store::Signal::Log, value)
    }

    /// Records a bounded native ownership transition. Owner ids and raw
    /// failures are intentionally not accepted by this API.
    pub fn record_ownership_event(
        &self,
        kind: &'static str,
        outcome: &'static str,
        stale_process_count: usize,
        span_context: Option<SpanContext>,
    ) {
        self.emit_family_log(
            attributes::PI_OWNERSHIP.name,
            "pi.ownership.claimed",
            None,
            None,
            span_context.as_ref(),
            &[
                (
                    "tau.ownership.kind",
                    opentelemetry::Value::String(kind.into()),
                ),
                (
                    "tau.ownership.outcome",
                    opentelemetry::Value::String(outcome.into()),
                ),
                (
                    "tau.ownership.stale_process_count",
                    opentelemetry::Value::I64(
                        i64::try_from(stale_process_count).unwrap_or(i64::MAX),
                    ),
                ),
            ],
        );
        self.force_flush_logs();
    }

    /// Resolution lacks a generation; validate all lifecycle attributes before emission.
    pub fn record_process_lifecycle(
        &self,
        event_name: &'static str,
        runtime_id: &str,
        generation: Option<u64>,
        span_context: Option<&SpanContext>,
        attributes: &[(&'static str, opentelemetry::Value)],
    ) {
        self.emit_family_log(
            attributes::PI_PROCESS_LIFECYCLE.name,
            event_name,
            Some(runtime_id),
            generation,
            span_context,
            attributes,
        );
    }

    /// Reader/forwarding diagnostics only, not Pi process lifecycle events.
    pub fn record_reader_event(
        &self,
        event_name: &'static str,
        runtime_id: &str,
        generation: Option<u64>,
        attributes: &[(&'static str, opentelemetry::Value)],
    ) {
        self.emit_family_log(
            attributes::PI_READER.name,
            event_name,
            Some(runtime_id),
            generation,
            None,
            attributes,
        );
        if event_name == "pi.reader.line_dropped"
            && attributes.iter().any(|(key, value)| {
                *key == "tau.reader.drop_reason"
                    && matches!(
                        value,
                        opentelemetry::Value::String(reason)
                            if reason.as_str() == "malformed" || reason.as_str() == "invalid_utf8"
                    )
            })
        {
            self.metrics.pi_reader_malformed.add(1, &[]);
        }
        self.force_flush_logs();
    }

    /// Named wrappers constrain call-site event names; revalidate context and attributes here.
    fn emit_family_log(
        &self,
        family: &str,
        event_name: &'static str,
        runtime_id: Option<&str>,
        generation: Option<u64>,
        span_context: Option<&SpanContext>,
        attributes: &[(&'static str, opentelemetry::Value)],
    ) {
        let mut validated: Vec<(&'static str, opentelemetry::Value)> = Vec::new();

        if let Some(runtime_id) = runtime_id {
            let runtime_value = opentelemetry::Value::String(
                truncate_to_limit(runtime_id, DEFAULT_MAX_ATTRIBUTE_LEN).into(),
            );
            if attributes::validate(family, "tau.runtime.id", &runtime_value).is_ok() {
                validated.push(("tau.runtime.id", runtime_value));
            }
        }
        if let Some(generation) = generation.and_then(|value| i64::try_from(value).ok()) {
            let generation_value = opentelemetry::Value::I64(generation);
            if attributes::validate(family, "pi.generation", &generation_value).is_ok() {
                validated.push(("pi.generation", generation_value));
            }
        }
        for (key, value) in attributes {
            if attributes::validate(family, key, value).is_ok() {
                validated.push((key, value.clone()));
            }
        }

        let logger = self.logger_provider.logger(SCOPE_NAME);
        let mut record = logger.create_log_record();
        record.set_timestamp(SystemTime::now());
        record.set_event_name(event_name);
        record.set_severity_number(Severity::Info);
        record.set_body(AnyValue::String(event_name.into()));
        if let Some(context) = span_context {
            record.set_trace_context(
                context.trace_id(),
                context.span_id(),
                Some(context.trace_flags()),
            );
        }
        for (key, value) in validated {
            record.add_attribute(key, otel_value_to_any_value(&value));
        }
        logger.emit(record);
    }

    /// Preserves the previous panic hook, then records only a sanitized location,
    /// never the payload. A thread-local guard prevents recursive recording.
    pub fn install_panic_hook(&self) {
        let store = Arc::clone(&self.store);
        let resource_json = self.resource_json.clone();
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info: &PanicHookInfo<'_>| {
            previous(info);

            guarded_panic_record(|| {
                let location = info
                    .location()
                    .map(|location| {
                        sanitize_source_location(
                            location.file(),
                            Some(location.line()),
                            Some(location.column()),
                        )
                    })
                    .unwrap_or_default();
                record_panic_to(&store, &resource_json, &location);
            });
        }));
    }
}

/// Test the same thread-local panic re-entry guard used by the hook.
fn guarded_panic_record(record: impl FnOnce()) {
    let already_in_hook = IN_PANIC_HOOK.with(|flag| flag.replace(true));
    if !already_in_hook {
        record();
    }
    IN_PANIC_HOOK.with(|flag| flag.set(already_in_hook));
}

/// Uses nonblocking `try_append` instead of the OTel logger: a panic may
/// occur while this thread holds the writer lock. Only the sanitized location
/// is attached; neither the panic payload nor arbitrary attributes are stored.
fn record_panic_to(store: &store::Store, resource_json: &serde_json::Value, location: &str) {
    let family = attributes::RUST_PANIC.name;
    let location_value =
        opentelemetry::Value::String(truncate_to_limit(location, DEFAULT_MAX_ATTRIBUTE_LEN).into());

    let mut log_record = serde_json::Map::new();
    log_record.insert(
        "timeUnixNano".into(),
        serde_json::Value::String(exporter::unix_nanos_string(SystemTime::now())),
    );
    log_record.insert(
        "eventName".into(),
        serde_json::Value::String("rust.panic".into()),
    );
    log_record.insert(
        "severityNumber".into(),
        serde_json::Value::from(Severity::Error as i32),
    );
    log_record.insert(
        "severityText".into(),
        serde_json::Value::String(Severity::Error.name().to_string()),
    );
    log_record.insert(
        "body".into(),
        serde_json::json!({ "stringValue": "rust.panic" }),
    );
    if attributes::validate(family, "tau.error.location", &location_value).is_ok() {
        log_record.insert(
            "attributes".into(),
            serde_json::json!([{
                "key": "tau.error.location",
                "value": exporter::otel_value_json(&location_value),
            }]),
        );
    }

    let wrapped = exporter::wrap_log_json(
        resource_json,
        SCOPE_NAME,
        serde_json::Value::Object(log_record),
    );
    let _ = store.try_append(store::Signal::Log, wrapped);
}

/// Check before writing this run’s marker to identify the previous unclean exit.
fn previous_run_was_unclean(dir: &Path) -> bool {
    dir.join(RUN_MARKER_FILE).is_file()
}

/// Sync the marker until clean exit so the next launch can detect an interruption.
fn write_run_marker(dir: &Path) {
    let _ = fs::create_dir_all(dir);
    set_owner_only_marker_dir_permissions(dir);
    let marker = dir.join(RUN_MARKER_FILE);
    if let Ok(mut file) = fs::File::create(&marker) {
        set_owner_only_marker_file_permissions(&marker);
        let _ = file.write_all(b"running");
        let _ = file.sync_all();
        sync_directory(dir);
    }
}

/// Removes this run's start marker, recording a clean exit.
fn clear_run_marker(dir: &Path) {
    if fs::remove_file(dir.join(RUN_MARKER_FILE)).is_ok() {
        sync_directory(dir);
    }
}

fn sync_directory(dir: &Path) {
    if let Ok(directory) = fs::File::open(dir) {
        let _ = directory.sync_all();
    }
}

#[cfg(unix)]
fn set_owner_only_marker_dir_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o700));
}
#[cfg(not(unix))]
fn set_owner_only_marker_dir_permissions(_path: &Path) {}

#[cfg(unix)]
fn set_owner_only_marker_file_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
}
#[cfg(not(unix))]
fn set_owner_only_marker_file_permissions(_path: &Path) {}

/// Drop ends the span even on early return; each invocation owns its context.
pub struct CommandSpan {
    span: opentelemetry_sdk::trace::Span,
}

impl CommandSpan {
    pub fn span_context(&self) -> SpanContext {
        self.span.span_context().clone()
    }
}

impl Drop for CommandSpan {
    fn drop(&mut self) {
        self.span.end();
    }
}

/// Build the resource from the reviewed six attributes, without SDK extras.
fn build_resource() -> Resource {
    let instance_id = truncate_to_limit(&generate_instance_id(), DEFAULT_MAX_ATTRIBUTE_LEN);
    let version = truncate_to_limit(env!("CARGO_PKG_VERSION"), DEFAULT_MAX_ATTRIBUTE_LEN);
    Resource::builder_empty()
        .with_attributes([
            KeyValue::new(SERVICE_NAME, "tau"),
            KeyValue::new(SERVICE_VERSION, version),
            KeyValue::new(SERVICE_INSTANCE_ID, instance_id),
            KeyValue::new(DEPLOYMENT_ENVIRONMENT_NAME, ENVIRONMENT_NAME),
            KeyValue::new(OS_TYPE, os_type_value()),
            KeyValue::new(HOST_ARCH, host_arch_value()),
        ])
        .build()
}

fn generate_instance_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Unsupported values become a fixed marker, never potentially sensitive `Debug` output.
fn otel_value_to_any_value(value: &opentelemetry::Value) -> AnyValue {
    match value {
        opentelemetry::Value::String(value) => AnyValue::String(value.as_str().to_string().into()),
        opentelemetry::Value::I64(value) => AnyValue::Int(*value),
        opentelemetry::Value::F64(value) => AnyValue::Double(*value),
        opentelemetry::Value::Bool(value) => AnyValue::Boolean(*value),
        _ => AnyValue::String("[unsupported telemetry value]".into()),
    }
}

/// Maps Rust's `std::env::consts::OS` to the OTel `os.type` enum, which uses
/// `darwin` rather than `macos`.
fn os_type_value() -> &'static str {
    match std::env::consts::OS {
        "macos" => "darwin",
        other => other,
    }
}

fn host_arch_value() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => other,
    }
}

fn resolve_telemetry_dir() -> PathBuf {
    profile::app_data_dir().join(TELEMETRY_DIR_NAME)
}
