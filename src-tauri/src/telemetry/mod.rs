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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    static PANIC_HOOK_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn segment_files_are_distinct_jsonl_files() {
        let files = [TRACE_SEGMENT_FILE, LOG_SEGMENT_FILE, METRIC_SEGMENT_FILE];
        assert!(files.iter().all(|file| file.ends_with(".jsonl")));
        assert_eq!(files.iter().collect::<HashSet<_>>().len(), files.len());
    }

    #[test]
    fn os_type_uses_the_darwin_enum_value_on_macos() {
        if cfg!(target_os = "macos") {
            assert_eq!(os_type_value(), "darwin");
        }

        assert_eq!(
            host_arch_value(),
            match std::env::consts::ARCH {
                "x86_64" => "amd64",
                "aarch64" => "arm64",
                other => other,
            }
        );
        assert_ne!(host_arch_value(), "x86_64");
    }

    #[test]
    fn resource_carries_exactly_the_stage_0_resource_attributes() {
        let resource = build_resource();
        let mut expected: Vec<&str> = attributes::RESOURCE_ATTRIBUTES
            .iter()
            .map(|spec| spec.key)
            .collect();
        let mut actual: Vec<&str> = resource.iter().map(|(key, _)| key.as_str()).collect();
        expected.sort_unstable();
        actual.sort_unstable();
        assert_eq!(actual, expected);

        {
            let long_id = "x".repeat(DEFAULT_MAX_ATTRIBUTE_LEN * 2);
            let truncated = truncate_to_limit(&long_id, DEFAULT_MAX_ATTRIBUTE_LEN);
            assert_eq!(truncated.len(), DEFAULT_MAX_ATTRIBUTE_LEN);
        }
    }

    #[test]
    fn generated_instance_ids_are_random_uuids() {
        let first = generate_instance_id();
        let second = generate_instance_id();
        assert_eq!(
            uuid::Uuid::parse_str(&first)
                .expect("UUID")
                .get_version_num(),
            4
        );
        assert_ne!(first, second);
    }

    #[test]
    fn telemetry_uses_the_current_storage_profile() {
        assert_eq!(
            resolve_telemetry_dir(),
            profile::app_data_dir().join("telemetry")
        );
    }

    #[test]
    fn record_app_started_persists_a_valid_log_record() {
        let directory = tempfile::tempdir().expect("temp dir");
        let clock: Arc<dyn store::Clock> = Arc::new(store::SystemClock);
        let telemetry = Telemetry::new(directory.path().to_path_buf(), clock, true);

        telemetry.record_app_started();

        let contents = std::fs::read_to_string(directory.path().join(LOG_SEGMENT_FILE))
            .expect("log segment should exist");
        let line = contents.lines().next().expect("one log line");
        let value: serde_json::Value = serde_json::from_str(line).expect("valid json");

        let log_record = &value["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0];
        assert_eq!(log_record["eventName"], "app.started");
        assert_eq!(log_record["body"]["stringValue"], "app.started");
        assert_eq!(value["tauStoreSequence"], 0);

        let resource_attributes = value["resourceLogs"][0]["resource"]["attributes"]
            .as_array()
            .expect("attributes");
        assert!(resource_attributes
            .iter()
            .any(|attribute| attribute["key"] == "service.name"
                && attribute["value"]["stringValue"] == "tau"));

        {
            let directory = tempfile::tempdir().expect("temp dir");
            let clock: Arc<dyn store::Clock> = Arc::new(store::SystemClock);
            let telemetry = Telemetry::new(directory.path().to_path_buf(), clock, true);

            telemetry.record_app_exited();
            telemetry.shutdown();

            let contents = std::fs::read_to_string(directory.path().join(LOG_SEGMENT_FILE))
                .expect("log segment should exist");
            let line = contents.lines().next().expect("one log line");
            let value: serde_json::Value = serde_json::from_str(line).expect("valid json");
            let log_record = &value["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0];
            assert_eq!(log_record["eventName"], "app.exited");
        }
    }

    #[test]
    fn a_disabled_run_writes_no_telemetry_and_leaves_no_directory() {
        let parent = tempfile::tempdir().expect("temp dir");
        let dir = parent.path().join("telemetry");
        let telemetry = Telemetry::new(dir.clone(), Arc::new(store::SystemClock), false);

        telemetry.record_app_started();
        telemetry.record_pi_process_start();
        telemetry.record_app_exited();
        telemetry.force_flush_metrics();

        assert!(!telemetry.is_enabled());
        assert!(
            !dir.exists(),
            "a disabled run must not create its own directory"
        );

        {
            let parent = tempfile::tempdir().expect("temp dir");
            let dir = parent.path().join("telemetry");
            let telemetry = Telemetry::new(dir.clone(), Arc::new(store::SystemClock), false);
            telemetry.record_app_started();

            telemetry.set_enabled(true);
            assert!(dir.join(RUN_MARKER_FILE).is_file());
            let after_enabling = read_event_names(&dir);
            assert_eq!(after_enabling, vec!["app.started".to_string()]);

            telemetry.set_enabled(false);
            assert!(
                !dir.join(RUN_MARKER_FILE).exists(),
                "a run that stops observing itself records no clean exit to detect"
            );
            let after_disabling = read_event_names(&dir);
            assert_eq!(
                after_disabling,
                vec![
                    "app.started".to_string(),
                    "app.telemetry_disabled".to_string()
                ]
            );

            telemetry.record_app_exited();
            assert_eq!(read_event_names(&dir), after_disabling);
        }
    }

    fn read_event_names(dir: &Path) -> Vec<String> {
        let Ok(contents) = std::fs::read_to_string(dir.join(LOG_SEGMENT_FILE)) else {
            return Vec::new();
        };
        contents
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .filter_map(|value| {
                value["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0]["eventName"]
                    .as_str()
                    .map(str::to_string)
            })
            .collect()
    }

    fn test_telemetry() -> (Telemetry, tempfile::TempDir) {
        let directory = tempfile::tempdir().expect("temp dir");
        let telemetry = Telemetry::new(
            directory.path().to_path_buf(),
            Arc::new(store::SystemClock),
            true,
        );
        (telemetry, directory)
    }

    fn sample_context(trace_id: &str, span_id: &str) -> TraceContext {
        TraceContext {
            trace_id: trace_id.to_string(),
            span_id: span_id.to_string(),
            sampled: true,
        }
    }

    #[test]
    fn command_span_persists_a_native_child_of_the_given_context() {
        let (telemetry, _directory) = test_telemetry();
        let context = sample_context("4bf92f3577b34da6a3ce929d0e0e4736", "00f067aa0ba902b7");

        telemetry
            .start_command_span(&context, "load_workspace")
            .expect("valid context");

        let records = telemetry.store.read_records(store::Signal::Trace);
        assert_eq!(records.len(), 1);
        let span = &records[0]["resourceSpans"][0]["scopeSpans"][0]["spans"][0];
        assert_eq!(span["traceId"], context.trace_id);
        assert_eq!(span["parentSpanId"], context.span_id);
        assert_eq!(span["name"], attributes::TAURI_INVOKE.name);
        assert_eq!(
            span["attributes"][0]["value"]["stringValue"],
            "load_workspace"
        );
        assert_ne!(span["spanId"], context.span_id);

        {
            let (telemetry, _directory) = test_telemetry();
            for malformed in [
                sample_context("not-hex", "00f067aa0ba902b7"),
                sample_context("00000000000000000000000000000000", "00f067aa0ba902b7"),
                sample_context("4BF92F3577B34DA6A3CE929D0E0E4736", "00f067aa0ba902b7"),
            ] {
                assert!(telemetry
                    .start_command_span(&malformed, "load_workspace")
                    .is_none());
            }
            assert!(telemetry
                .store
                .read_records(store::Signal::Trace)
                .is_empty());
        }
    }

    #[test]
    fn concurrent_command_spans_do_not_leak_trace_context() {
        let (telemetry, _directory) = test_telemetry();
        let telemetry = Arc::new(telemetry);
        let contexts = [
            sample_context("11111111111111111111111111111111", "1111111111111111"),
            sample_context("22222222222222222222222222222222", "2222222222222222"),
        ];

        let threads: Vec<_> = contexts
            .into_iter()
            .map(|context| {
                let telemetry = Arc::clone(&telemetry);
                std::thread::spawn(move || {
                    telemetry
                        .start_command_span(&context, "load_workspace")
                        .expect("valid context");
                })
            })
            .collect();
        for thread in threads {
            thread.join().expect("command span thread");
        }

        let records = telemetry.store.read_records(store::Signal::Trace);
        let trace_ids: std::collections::HashSet<String> = records
            .iter()
            .map(|record| {
                record["resourceSpans"][0]["scopeSpans"][0]["spans"][0]["traceId"]
                    .as_str()
                    .expect("trace id")
                    .to_string()
            })
            .collect();
        assert_eq!(records.len(), 2);
        assert_eq!(
            trace_ids,
            std::collections::HashSet::from([
                "11111111111111111111111111111111".to_string(),
                "22222222222222222222222222222222".to_string(),
            ])
        );
    }

    fn process_log_records(telemetry: &Telemetry) -> Vec<serde_json::Value> {
        telemetry.store.read_records(store::Signal::Log)
    }

    /// Force-flushes the metric provider (the `PeriodicReader` otherwise
    /// only collects on its own 15s interval) and reads back every
    /// persisted metric line.
    fn process_metric_records(telemetry: &Telemetry) -> Vec<serde_json::Value> {
        telemetry.force_flush_metrics();
        telemetry.store.read_records(store::Signal::Metric)
    }

    fn find_metric<'a>(
        records: &'a [serde_json::Value],
        name: &str,
    ) -> Option<&'a serde_json::Value> {
        records.iter().find(|record| {
            record["resourceMetrics"][0]["scopeMetrics"][0]["metrics"][0]["name"] == name
        })
    }

    #[test]
    fn record_operation_duration_persists_a_histogram_for_each_reviewed_family() {
        let (telemetry, _directory) = test_telemetry();

        telemetry.record_operation_duration(attributes::TAURI_INVOKE.name, 12.5);
        telemetry.record_operation_duration(attributes::UI_ACTION.name, 40.0);
        telemetry.record_operation_duration(attributes::PI_RPC.name, 250.0);
        telemetry.record_controller_start_duration(500.0);
        telemetry.record_operation_duration("not.a.real.family", 999.0);

        let records = process_metric_records(&telemetry);
        let invoke = find_metric(&records, "tau.invoke.duration").expect("invoke histogram");
        let action = find_metric(&records, "tau.ui_action.duration").expect("action histogram");
        let rpc = find_metric(&records, "tau.pi_rpc.duration").expect("rpc histogram");
        let controller_start = find_metric(&records, "tau.controller_start.duration")
            .expect("controller start histogram");

        let metric = &invoke["resourceMetrics"][0]["scopeMetrics"][0]["metrics"][0];
        assert_eq!(metric["unit"], "ms");
        let point = &metric["histogram"]["dataPoints"][0];
        assert_eq!(point["count"], "1");
        assert_eq!(point["sum"], 12.5);
        assert!(point["attributes"]
            .as_array()
            .expect("attributes")
            .is_empty());

        assert_eq!(
            action["resourceMetrics"][0]["scopeMetrics"][0]["metrics"][0]["histogram"]
                ["dataPoints"][0]["sum"],
            40.0
        );
        assert_eq!(
            rpc["resourceMetrics"][0]["scopeMetrics"][0]["metrics"][0]["histogram"]["dataPoints"]
                [0]["sum"],
            250.0
        );
        assert_eq!(
            controller_start["resourceMetrics"][0]["scopeMetrics"][0]["metrics"][0]["histogram"]
                ["dataPoints"][0]["sum"],
            500.0
        );
        assert!(find_metric(&records, "not.a.real.family").is_none());
    }

    #[test]
    fn record_rpc_failure_increments_a_counter_never_a_gauge() {
        let (telemetry, _directory) = test_telemetry();

        telemetry.record_rpc_failure();
        telemetry.record_rpc_failure();

        let records = process_metric_records(&telemetry);
        let metric = find_metric(&records, "tau.pi_rpc.failures").expect("failure counter");
        let point =
            &metric["resourceMetrics"][0]["scopeMetrics"][0]["metrics"][0]["sum"]["dataPoints"][0];
        assert_eq!(point["asInt"], "2");
    }

    #[test]
    fn record_pi_process_start_and_exit_persist_bounded_counters() {
        let (telemetry, _directory) = test_telemetry();

        telemetry.record_pi_process_start();
        telemetry.record_pi_process_start();
        telemetry.record_pi_process_exit(true);
        telemetry.record_pi_process_exit(false);

        let records = process_metric_records(&telemetry);
        let starts = find_metric(&records, "tau.pi_process.starts").expect("starts counter");
        assert_eq!(
            starts["resourceMetrics"][0]["scopeMetrics"][0]["metrics"][0]["sum"]["dataPoints"][0]
                ["asInt"],
            "2"
        );

        let exits = find_metric(&records, "tau.pi_process.exits").expect("exits counter");
        let points = exits["resourceMetrics"][0]["scopeMetrics"][0]["metrics"][0]["sum"]
            ["dataPoints"]
            .as_array()
            .expect("data points");
        assert_eq!(points.len(), 2, "clean and unexpected are separate series");
        let outcomes: Vec<&str> = points
            .iter()
            .map(|point| {
                point["attributes"][0]["value"]["stringValue"]
                    .as_str()
                    .expect("outcome string")
            })
            .collect();
        assert!(outcomes.contains(&"clean"));
        assert!(outcomes.contains(&"unexpected"));
    }

    #[test]
    fn record_heartbeat_gauges_persists_every_reviewed_gauge() {
        let (telemetry, _directory) = test_telemetry();

        telemetry.record_heartbeat_gauges(3, 5, 2, 4, 7);

        let records = process_metric_records(&telemetry);
        let assert_gauge = |name: &str, expected: &str| {
            let metric = find_metric(&records, name).unwrap_or_else(|| panic!("{name} gauge"));
            let point = &metric["resourceMetrics"][0]["scopeMetrics"][0]["metrics"][0]["gauge"]
                ["dataPoints"][0];
            assert_eq!(point["asInt"], expected, "{name}");
        };
        assert_gauge("tau.telemetry.pending_rpc_count", "3");
        assert_gauge("tau.telemetry.controller_count", "5");
        assert_gauge("tau.telemetry.active_controller_count", "2");
        assert_gauge("tau.telemetry.runtime_count", "4");
        assert_gauge("tau.telemetry.queue_length", "7");
    }

    #[test]
    fn writer_health_records_the_failed_write_gauge_even_when_zero() {
        let (telemetry, _directory) = test_telemetry();

        telemetry.record_app_exited();

        let records = process_metric_records(&telemetry);
        let metric =
            find_metric(&records, "tau.telemetry.failed_write_count").expect("failed-write gauge");
        let point = &metric["resourceMetrics"][0]["scopeMetrics"][0]["metrics"][0]["gauge"]
            ["dataPoints"][0];
        assert_eq!(point["asInt"], "0");

        {
            let directory = tempfile::tempdir().expect("temp dir");
            let blocked_path = directory.path().join("telemetry");
            std::fs::write(&blocked_path, b"file").expect("blocking file");
            let telemetry =
                Telemetry::new(blocked_path.clone(), Arc::new(store::SystemClock), true);

            // Fails: `blocked_path` is a file, not a directory, so this lands in
            // the store's bounded in-memory fallback instead of on disk.
            telemetry.record_app_started();
            assert!(telemetry.store.failed_writes() > 0);

            std::fs::remove_file(&blocked_path).expect("remove blocking file");

            telemetry.record_app_exited();

            let records = process_log_records(&telemetry);
            assert!(records.iter().any(|record| {
                let log_record = &record["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0];
                log_record["eventName"] == "telemetry.writer_failure"
                    && log_record["attributes"]
                        .as_array()
                        .is_some_and(|attributes| {
                            attributes.iter().any(|attribute| {
                                attribute["key"] == "tau.telemetry.failed_write_count"
                                    && attribute["value"]["intValue"] == "1"
                            })
                        })
            }));
        }
    }

    #[test]
    fn record_event_loop_lag_carries_only_the_two_reviewed_dimensions() {
        let (telemetry, _directory) = test_telemetry();

        telemetry.record_event_loop_lag(
            42.0,
            &[
                KeyValue::new("tau.heartbeat.visibility", "hidden"),
                KeyValue::new("tau.heartbeat.focused", "false"),
            ],
        );

        let records = process_metric_records(&telemetry);
        let metric = find_metric(&records, "tau.frontend.event_loop_lag").expect("lag histogram");
        let point = &metric["resourceMetrics"][0]["scopeMetrics"][0]["metrics"][0]["histogram"]
            ["dataPoints"][0];
        assert_eq!(point["sum"], 42.0);
        let mut attribute_keys: Vec<&str> = point["attributes"]
            .as_array()
            .expect("attributes")
            .iter()
            .map(|attribute| attribute["key"].as_str().expect("key"))
            .collect();
        attribute_keys.sort_unstable();
        assert_eq!(
            attribute_keys,
            vec!["tau.heartbeat.focused", "tau.heartbeat.visibility"]
        );

        {
            let (telemetry, _directory) = test_telemetry();

            telemetry.record_long_task_duration(88.0);

            let records = process_metric_records(&telemetry);
            let metric = find_metric(&records, "tau.frontend.long_task.duration")
                .expect("long task histogram");
            let point = &metric["resourceMetrics"][0]["scopeMetrics"][0]["metrics"][0]["histogram"]
                ["dataPoints"][0];
            assert_eq!(point["sum"], 88.0);
            assert!(point["attributes"]
                .as_array()
                .expect("attributes")
                .is_empty());
        }

        {
            let (telemetry, _directory) = test_telemetry();

            telemetry.record_operation_duration(attributes::PI_RPC.name, 10.0);
            telemetry.record_rpc_failure();
            telemetry.record_pi_process_start();
            telemetry.record_pi_process_exit(true);
            telemetry.record_heartbeat_gauges(1, 2, 3, 4, 5);
            telemetry.record_telemetry_dropped_gauge(6);
            telemetry.record_event_loop_lag(
                1.0,
                &[
                    KeyValue::new("tau.heartbeat.visibility", "visible"),
                    KeyValue::new("tau.heartbeat.focused", "true"),
                ],
            );
            telemetry.record_long_task_duration(2.0);

            let records = process_metric_records(&telemetry);
            let forbidden = [
                "tau.project.id",
                "tau.session.id",
                "tau.controller.id",
                "tau.runtime.id",
                "pi.generation",
                "pi.rpc.request_id",
                "traceId",
                "spanId",
            ];
            for record in &records {
                let encoded = record.to_string();
                for key in forbidden {
                    assert!(
                        !encoded.contains(key),
                        "a metric record contained the forbidden dimension {key}: {encoded}"
                    );
                }
            }
        }
    }

    #[test]
    fn process_lifecycle_persists_runtime_and_optional_generation() {
        let (telemetry, _directory) = test_telemetry();

        telemetry.record_process_lifecycle(
            "pi.process.resolved",
            "runtime-1",
            None,
            None,
            &[(
                "tau.process.resolution",
                opentelemetry::Value::String("found".into()),
            )],
        );
        telemetry.record_process_lifecycle("pi.process.started", "runtime-1", Some(3), None, &[]);

        let records = process_log_records(&telemetry);
        assert_eq!(records.len(), 2);
        let resolved = &records[0]["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0];
        assert_eq!(resolved["eventName"], "pi.process.resolved");
        let resolved_attributes = resolved["attributes"].as_array().expect("attributes");
        assert!(resolved_attributes
            .iter()
            .any(|attribute| attribute["key"] == "tau.process.resolution"
                && attribute["value"]["stringValue"] == "found"));
        assert!(resolved_attributes
            .iter()
            .any(|attribute| attribute["key"] == "tau.runtime.id"
                && attribute["value"]["stringValue"] == "runtime-1"));
        assert!(!resolved_attributes
            .iter()
            .any(|attribute| attribute["key"] == "pi.generation"));

        let started = &records[1]["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0];
        assert_eq!(started["eventName"], "pi.process.started");
        assert!(started["attributes"]
            .as_array()
            .expect("attributes")
            .iter()
            .any(|attribute| attribute["key"] == "pi.generation"
                && attribute["value"]["intValue"] == "3"));

        {
            let (telemetry, _directory) = test_telemetry();

            telemetry.record_process_lifecycle(
                "pi.process.stopped",
                "runtime-1",
                Some(1),
                None,
                &[(
                    "tau.process.stop_reason",
                    opentelemetry::Value::String("not-a-real-reason".into()),
                )],
            );

            let records = process_log_records(&telemetry);
            assert_eq!(records.len(), 1);
            let log_record = &records[0]["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0];
            assert!(!log_record["attributes"]
                .as_array()
                .expect("attributes")
                .iter()
                .any(|attribute| attribute["key"] == "tau.process.stop_reason"));

            let (telemetry, _directory) = test_telemetry();
            let canary = privacy::FORBIDDEN_CONTENT_CANARIES[0].1;

            telemetry.record_process_lifecycle(
                "pi.process.stopped",
                "runtime-1",
                Some(1),
                None,
                &[(
                    "tau.process.stop_reason",
                    opentelemetry::Value::String(canary.into()),
                )],
            );

            let records = process_log_records(&telemetry);
            let log_record = &records[0]["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0];
            let encoded = log_record.to_string();
            assert!(!encoded.contains(canary));
            assert!(!log_record["attributes"]
                .as_array()
                .expect("attributes")
                .iter()
                .any(|attribute| attribute["key"] == "tau.process.stop_reason"));
        }

        {
            let (telemetry, _directory) = test_telemetry();

            telemetry.record_process_lifecycle(
                "pi.process.exited",
                "runtime-1",
                Some(1),
                None,
                &[("tau.process.exit_code", opentelemetry::Value::I64(127))],
            );

            let records = process_log_records(&telemetry);
            let log_record = &records[0]["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0];
            assert!(log_record["attributes"]
                .as_array()
                .expect("attributes")
                .iter()
                .any(|attribute| attribute["key"] == "tau.process.exit_code"
                    && attribute["value"]["intValue"] == "127"));
        }

        {
            let (telemetry, _directory) = test_telemetry();
            let parent = sample_context("4bf92f3577b34da6a3ce929d0e0e4736", "00f067aa0ba902b7");
            let command = telemetry
                .start_command_span(&parent, "start_pi")
                .expect("command span");
            let context = command.span_context();

            telemetry.record_process_lifecycle(
                "pi.process.started",
                "runtime-1",
                Some(1),
                Some(&context),
                &[],
            );

            let records = process_log_records(&telemetry);
            let log_record = &records[0]["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0];
            assert_eq!(log_record["traceId"], context.trace_id().to_string());
            assert_eq!(log_record["spanId"], context.span_id().to_string());
            assert_eq!(log_record["flags"], context.trace_flags().to_u8());
        }
    }

    #[test]
    fn reader_event_persists_runtime_context_like_process_lifecycle() {
        let (telemetry, _directory) = test_telemetry();

        telemetry.record_reader_event(
            "pi.reader.line_dropped",
            "runtime-1",
            Some(2),
            &[(
                "tau.reader.drop_reason",
                opentelemetry::Value::String("oversized".into()),
            )],
        );

        let records = process_log_records(&telemetry);
        let log_record = &records[0]["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0];
        assert_eq!(log_record["eventName"], "pi.reader.line_dropped");
        let persisted = log_record["attributes"].as_array().expect("attributes");
        assert!(persisted
            .iter()
            .any(|attribute| attribute["key"] == "tau.reader.drop_reason"
                && attribute["value"]["stringValue"] == "oversized"));
        assert!(persisted
            .iter()
            .any(|attribute| attribute["key"] == "tau.runtime.id"
                && attribute["value"]["stringValue"] == "runtime-1"));
        assert!(persisted
            .iter()
            .any(|attribute| attribute["key"] == "pi.generation"
                && attribute["value"]["intValue"] == "2"));

        {
            let (telemetry, _directory) = test_telemetry();

            telemetry.record_reader_event(
                "pi.reader.line_dropped",
                "runtime-1",
                Some(2),
                &[(
                    "tau.reader.drop_reason",
                    opentelemetry::Value::String("malformed".into()),
                )],
            );

            let records = process_metric_records(&telemetry);
            let metric = find_metric(&records, "tau.pi_reader.malformed_lines")
                .expect("malformed line counter");
            assert_eq!(
                metric["resourceMetrics"][0]["scopeMetrics"][0]["metrics"][0]["sum"]["dataPoints"]
                    [0]["asInt"],
                "1"
            );
        }

        {
            let (telemetry, _directory) = test_telemetry();

            telemetry.record_reader_event(
                "pi.reader.failed",
                "runtime-1",
                None,
                &[(
                    "tau.reader.error_kind",
                    opentelemetry::Value::String("not-a-real-kind".into()),
                )],
            );

            let records = process_log_records(&telemetry);
            assert_eq!(records.len(), 1);
            let log_record = &records[0]["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0];
            assert!(!log_record["attributes"]
                .as_array()
                .expect("attributes")
                .iter()
                .any(|attribute| attribute["key"] == "tau.reader.error_kind"));
        }
    }

    #[test]
    fn detects_an_unclean_previous_run_and_records_it_at_the_next_start() {
        let directory = tempfile::tempdir().expect("temp dir");
        let dir = directory.path().to_path_buf();

        let first = Telemetry::new(dir.clone(), Arc::new(store::SystemClock), true);
        assert!(!first.previous_run_unclean);
        // Deliberately do not call `first.record_app_exited()`: the marker
        // it would clear is left behind, simulating a crash or force quit.

        let second = Telemetry::new(dir.clone(), Arc::new(store::SystemClock), true);
        assert!(second.previous_run_unclean);
        second.record_app_started();

        let records = process_log_records(&second);
        assert!(records.iter().any(|record| {
            record["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0]["eventName"]
                == "app.unclean_exit_detected"
        }));

        {
            let directory = tempfile::tempdir().expect("temp dir");
            let dir = directory.path().to_path_buf();

            let first = Telemetry::new(dir.clone(), Arc::new(store::SystemClock), true);
            first.record_app_exited();

            let second = Telemetry::new(dir.clone(), Arc::new(store::SystemClock), true);
            assert!(!second.previous_run_unclean);
        }
    }

    #[test]
    #[cfg(unix)]
    fn run_marker_and_directory_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().expect("temp dir");
        let dir = directory.path().join("telemetry");
        let _telemetry = Telemetry::new(dir.clone(), Arc::new(store::SystemClock), true);
        let dir_mode = fs::metadata(&dir)
            .expect("dir metadata")
            .permissions()
            .mode()
            & 0o777;
        let marker_mode = fs::metadata(dir.join(RUN_MARKER_FILE))
            .expect("marker metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(dir_mode, 0o700);
        assert_eq!(marker_mode, 0o600);
    }

    #[test]
    fn guarded_panic_record_skips_reentrant_calls_on_the_same_thread() {
        let mut outer_ran = false;
        guarded_panic_record(|| {
            outer_ran = true;
            let mut inner_ran = false;
            guarded_panic_record(|| {
                inner_ran = true;
            });
            assert!(!inner_ran, "a re-entrant call must not run its recorder");
        });
        assert!(outer_ran);
    }

    #[test]
    fn panic_hook_calls_the_previous_hook_and_records_a_sanitized_location_only() {
        use std::panic::AssertUnwindSafe;

        let _hook_guard = PANIC_HOOK_TEST_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        use std::sync::atomic::{AtomicBool, Ordering};

        let (telemetry, _directory) = test_telemetry();
        let original_hook = std::panic::take_hook();

        static PREVIOUS_HOOK_RAN: AtomicBool = AtomicBool::new(false);
        PREVIOUS_HOOK_RAN.store(false, Ordering::SeqCst);
        std::panic::set_hook(Box::new(|_info| {
            PREVIOUS_HOOK_RAN.store(true, Ordering::SeqCst);
        }));
        telemetry.install_panic_hook();

        const PANIC_MESSAGE: &str = "tau-canary-panic-message-3f1c9a";
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            panic!("{PANIC_MESSAGE}");
        }));
        assert!(result.is_err());

        std::panic::set_hook(original_hook);

        assert!(
            PREVIOUS_HOOK_RAN.load(Ordering::SeqCst),
            "the previously installed hook must still run"
        );

        let records = process_log_records(&telemetry);
        let panic_record = records
            .iter()
            .find(|record| {
                record["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0]["eventName"]
                    == "rust.panic"
            })
            .expect("a rust.panic record");
        let encoded = panic_record.to_string();
        assert!(
            !encoded.contains(PANIC_MESSAGE),
            "the panic message must never be persisted"
        );
        let log_record = &panic_record["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0];
        let location = log_record["attributes"]
            .as_array()
            .expect("attributes")
            .iter()
            .find(|attribute| attribute["key"] == "tau.error.location")
            .expect("a location attribute");
        assert!(location["value"]["stringValue"]
            .as_str()
            .expect("location string")
            .starts_with("mod.rs"));
    }

    #[test]
    fn panic_hook_never_persists_any_forbidden_content_canary_as_the_panic_message() {
        use privacy::FORBIDDEN_CONTENT_CANARIES;
        use std::panic::AssertUnwindSafe;

        let _hook_guard = PANIC_HOOK_TEST_LOCK
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());

        let (telemetry, _directory) = test_telemetry();
        let original_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_info| {}));
        telemetry.install_panic_hook();

        for (_, canary) in FORBIDDEN_CONTENT_CANARIES {
            let canary = *canary;
            let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
                panic!("{canary}");
            }));
            assert!(result.is_err());
        }

        std::panic::set_hook(original_hook);

        let records = process_log_records(&telemetry);
        let panic_records: Vec<&serde_json::Value> = records
            .iter()
            .filter(|record| {
                record["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0]["eventName"]
                    == "rust.panic"
            })
            .collect();
        assert_eq!(
            panic_records.len(),
            FORBIDDEN_CONTENT_CANARIES.len(),
            "expected one rust.panic record per triggered panic"
        );
        for record in &panic_records {
            let encoded = record.to_string();
            for (name, canary) in FORBIDDEN_CONTENT_CANARIES {
                assert!(
                    !encoded.contains(canary),
                    "a rust.panic record leaked the {name} canary: {encoded}"
                );
            }
        }
    }

    /// Exercise every forbidden-content canary in panic payloads; none may reach telemetry.
    #[test]
    fn disk_write_volume_per_record_stays_within_a_generous_bound() {
        let (telemetry, directory) = test_telemetry();
        let context = sample_context("4bf92f3577b34da6a3ce929d0e0e4736", "00f067aa0ba902b7");

        for generation in 0..50u64 {
            telemetry.record_process_lifecycle(
                "pi.process.started",
                "runtime-1",
                Some(generation),
                None,
                &[],
            );
        }
        for _ in 0..50 {
            telemetry
                .start_command_span(&context, "load_workspace")
                .expect("valid context");
        }
        telemetry.shutdown();

        let average_bytes = |path: PathBuf| -> usize {
            let contents = std::fs::read_to_string(path).unwrap_or_default();
            let lines: Vec<&str> = contents.lines().collect();
            if lines.is_empty() {
                return 0;
            }
            lines.iter().map(|line| line.len()).sum::<usize>() / lines.len()
        };
        let average_log_bytes = average_bytes(directory.path().join(LOG_SEGMENT_FILE));
        let average_span_bytes = average_bytes(directory.path().join(TRACE_SEGMENT_FILE));

        assert!(
            average_log_bytes > 0 && average_log_bytes < 2000,
            "average log record was {average_log_bytes} bytes, expected 0 < n < 2000"
        );
        assert!(
            average_span_bytes > 0 && average_span_bytes < 2000,
            "average span record was {average_span_bytes} bytes, expected 0 < n < 2000"
        );
    }

    /// Checks bounded series cardinality rather than heap bytes: metric memory
    /// depends on attribute combinations, not recording frequency.
    #[test]
    fn metric_series_count_stays_fixed_regardless_of_recording_volume() {
        let (telemetry, _directory) = test_telemetry();

        for i in 0..500 {
            telemetry.record_operation_duration(attributes::TAURI_INVOKE.name, i as f64);
            telemetry.record_operation_duration(attributes::UI_ACTION.name, i as f64);
            telemetry.record_operation_duration(attributes::PI_RPC.name, i as f64);
            telemetry.record_rpc_failure();
            telemetry.record_pi_process_start();
            telemetry.record_pi_process_exit(i % 2 == 0);
            telemetry.record_heartbeat_gauges(i, i, i, i, i);
            telemetry.record_telemetry_dropped_gauge(i);
            for visibility in ["visible", "hidden"] {
                for focused in ["true", "false"] {
                    telemetry.record_event_loop_lag(
                        i as f64,
                        &[
                            KeyValue::new("tau.heartbeat.visibility", visibility),
                            KeyValue::new("tau.heartbeat.focused", focused),
                        ],
                    );
                }
            }
            telemetry.record_long_task_duration(i as f64);
        }

        let records = process_metric_records(&telemetry);
        let data_point_count = |name: &str| -> usize {
            records
                .iter()
                .filter(|record| {
                    record["resourceMetrics"][0]["scopeMetrics"][0]["metrics"][0]["name"] == name
                })
                .flat_map(|record| {
                    let metric = &record["resourceMetrics"][0]["scopeMetrics"][0]["metrics"][0];
                    let shape = ["histogram", "sum", "gauge"]
                        .into_iter()
                        .find_map(|kind| metric.get(kind))
                        .expect("one known shape");
                    shape["dataPoints"].as_array().cloned().unwrap_or_default()
                })
                .count()
        };

        // Collection count depends on bounded dimension combinations, not recording frequency.
        assert_eq!(data_point_count("tau.invoke.duration"), 1);
        assert_eq!(data_point_count("tau.ui_action.duration"), 1);
        assert_eq!(data_point_count("tau.pi_rpc.duration"), 1);
        assert_eq!(data_point_count("tau.pi_rpc.failures"), 1);
        assert_eq!(data_point_count("tau.pi_process.starts"), 1);
        assert_eq!(
            data_point_count("tau.pi_process.exits"),
            2,
            "exactly the clean/unexpected outcome dimension, never more"
        );
        assert_eq!(data_point_count("tau.telemetry.pending_rpc_count"), 1);
        assert_eq!(data_point_count("tau.telemetry.dropped_count"), 1);
        assert_eq!(
            data_point_count("tau.frontend.event_loop_lag"),
            4,
            "exactly the visibility × focused dimension combinations, never more"
        );
        assert_eq!(data_point_count("tau.frontend.long_task.duration"), 1);
    }
}
