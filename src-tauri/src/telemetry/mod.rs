//! Native telemetry providers and bounded, memory-only cloud delivery.
//! Command spans link native work to its frontend request; the panic hook
//! records only sanitized locations.

pub mod attributes;
mod exporter;
pub mod ingest;
#[cfg(test)]
mod tests;

pub mod privacy;
pub mod trace_context;
pub mod transport;

use std::cell::Cell;
use std::panic::PanicHookInfo;
#[cfg(test)]
use std::path::PathBuf;
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

use crate::profile::ENVIRONMENT_NAME;
use privacy::{sanitize_source_location, truncate_to_limit, DEFAULT_MAX_ATTRIBUTE_LEN};
use trace_context::TraceContext;

// Skip telemetry on panic-hook re-entry to avoid recursion; still invoke the previous hook.
thread_local! {
    static IN_PANIC_HOOK: Cell<bool> = const { Cell::new(false) };
}

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
    /// Populated from the validated frontend `telemetry.health` log.
    telemetry_dropped_gauge: Gauge<u64>,
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
        }
    }
}

/// Per-launch pipeline. Nothing is collected without both runtime admin mode and a target.
pub struct Telemetry {
    logger_provider: SdkLoggerProvider,
    tracer_provider: SdkTracerProvider,
    metric_pipeline: std::sync::Mutex<Option<MetricPipeline>>,
    resource: Resource,
    pub(crate) pipeline: Arc<transport::Pipeline>,
    resource_json: serde_json::Value,
}

struct MetricPipeline {
    provider: SdkMeterProvider,
    metrics: Metrics,
    revision: u64,
}
impl MetricPipeline {
    fn new(pipeline: Arc<transport::Pipeline>, resource: &Resource, revision: u64) -> Self {
        let exporter = exporter::CloudMetricExporter::new(pipeline, resource, revision);
        let reader = PeriodicReader::builder(exporter)
            .with_interval(METRIC_EXPORT_INTERVAL)
            .build();
        let provider = SdkMeterProvider::builder()
            .with_resource(resource.clone())
            .with_reader(reader)
            .build();
        let metrics = Metrics::new(&provider.meter(SCOPE_NAME));
        Self {
            provider,
            metrics,
            revision,
        }
    }
}

impl Telemetry {
    pub fn init() -> Self {
        Telemetry::new(Arc::new(transport::Pipeline::new(Arc::new(
            crate::cloud::Network,
        ))))
    }

    #[cfg(test)]
    pub(crate) fn for_test(_dir: PathBuf) -> Self {
        struct NoNetwork;
        impl crate::cloud::Http for NoNetwork {
            fn send(&self, _: crate::cloud::Request) -> crate::cloud::Result<serde_json::Value> {
                Err(crate::cloud::error("unavailable"))
            }
        }
        Self::new(Arc::new(transport::Pipeline::new(Arc::new(NoNetwork))))
    }

    fn new(store: Arc<transport::Pipeline>) -> Self {
        let resource = build_resource();
        let resource_json = exporter::resource_to_json(&resource);
        let log_exporter = exporter::CloudLogExporter::new(Arc::clone(&store), &resource);
        let span_exporter = exporter::CloudSpanExporter::new(Arc::clone(&store), &resource);
        let logger_provider = SdkLoggerProvider::builder()
            .with_resource(resource.clone())
            .with_simple_exporter(log_exporter)
            .build();
        let tracer_provider = SdkTracerProvider::builder()
            .with_resource(resource.clone())
            .with_sampler(Sampler::AlwaysOn)
            .with_simple_exporter(span_exporter)
            .build();
        Telemetry {
            logger_provider,
            tracer_provider,
            metric_pipeline: std::sync::Mutex::new(None),
            resource,
            pipeline: store,
            resource_json,
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.pipeline.is_enabled()
    }

    pub fn record_app_started(&self) {
        self.emit_lifecycle_log("app.started");
    }

    /// Queue the final lifecycle/metrics records on `RunEvent::Exit`, best-effort.
    pub fn record_app_exited(&self) {
        if !self.is_enabled() {
            return;
        }
        self.emit_lifecycle_log("app.exited");
        let _ = self.logger_provider.force_flush();
        self.force_flush_metrics();
    }

    pub fn shutdown(&self) {
        let _ = self.logger_provider.shutdown();
        let _ = self.tracer_provider.shutdown();
        if let Ok(mut pipeline) = self.metric_pipeline.lock() {
            if let Some(pipeline) = pipeline.take() {
                let _ = pipeline.provider.shutdown();
            }
        }
    }

    /// Force collection for tests and explicit flush points; otherwise the periodic reader
    /// waits.
    pub fn force_flush_metrics(&self) {
        if let Ok(pipeline) = self.metric_pipeline.lock() {
            if let Some(pipeline) = pipeline.as_ref() {
                let _ = pipeline.provider.force_flush();
            }
        }
    }

    /// The simple processor syncs each emit already; explicit flush preserves exit/failure
    /// boundaries.
    pub fn force_flush_logs(&self) {
        let _ = self.logger_provider.force_flush();
    }

    fn emit_lifecycle_log(&self, event_name: &'static str) {
        if !self.is_enabled() {
            return;
        }
        let logger = self.logger_provider.logger(SCOPE_NAME);
        let mut record = logger.create_log_record();
        record.set_timestamp(SystemTime::now());
        record.set_event_name(event_name);
        record.set_severity_number(Severity::Info);
        record.set_body(AnyValue::String(event_name.into()));
        logger.emit(record);
    }

    fn with_metrics(&self, record: impl FnOnce(&Metrics)) {
        if !self.is_enabled() {
            return;
        }
        let revision = self.pipeline.revision();
        let Ok(mut pipeline) = self.metric_pipeline.lock() else {
            return;
        };
        if pipeline.as_ref().is_none_or(|p| p.revision != revision) {
            // A new target/admin epoch never inherits accumulated counters or gauges.
            *pipeline = Some(MetricPipeline::new(
                self.pipeline.clone(),
                &self.resource,
                revision,
            ));
        }
        if self.is_enabled() && self.pipeline.revision() == revision {
            if let Some(pipeline) = pipeline.as_ref() {
                record(&pipeline.metrics);
            }
        }
    }

    /// Unknown families are no-ops so new span families cannot break ingestion.
    pub(super) fn record_operation_duration(&self, family: &str, duration_ms: f64) {
        if !self.is_enabled() {
            return;
        }
        self.with_metrics(|metrics| {
            let histogram = if family == attributes::TAURI_INVOKE.name {
                &metrics.invoke_duration
            } else if family == attributes::UI_ACTION.name {
                &metrics.ui_action_duration
            } else if family == attributes::PI_RPC.name {
                &metrics.pi_rpc_duration
            } else {
                return;
            };
            histogram.record(duration_ms, &[]);
        });
    }

    pub(super) fn record_controller_start_duration(&self, duration_ms: f64) {
        if !self.is_enabled() {
            return;
        }
        self.with_metrics(|m| m.controller_start_duration.record(duration_ms, &[]));
    }

    /// Increments the `pi.rpc` failure counter: called once per ingested
    /// `pi.rpc` span whose own `pi.rpc.outcome` attribute is present and not
    /// `"success"`.
    pub(super) fn record_rpc_failure(&self) {
        if !self.is_enabled() {
            return;
        }
        self.with_metrics(|m| m.pi_rpc_failures.add(1, &[]));
    }

    pub(super) fn record_rpc_abandoned(&self) {
        if !self.is_enabled() {
            return;
        }
        self.with_metrics(|m| m.pi_rpc_abandoned.add(1, &[]));
    }

    pub(super) fn record_rpc_unmatched(&self) {
        if !self.is_enabled() {
            return;
        }
        self.with_metrics(|m| m.pi_rpc_unmatched.add(1, &[]));
    }

    pub fn record_pi_process_start(&self) {
        if !self.is_enabled() {
            return;
        }
        self.with_metrics(|m| m.pi_process_starts.add(1, &[]));
    }

    /// Only two hardcoded exit outcomes; no untrusted metric dimension enters here.
    pub fn record_pi_process_exit(&self, clean: bool) {
        if !self.is_enabled() {
            return;
        }
        let outcome = if clean { "clean" } else { "unexpected" };
        self.with_metrics(|m| {
            m.pi_process_exits
                .add(1, &[KeyValue::new("tau.process.exit_outcome", outcome)])
        });
    }

    pub(super) fn record_heartbeat_gauges(
        &self,
        pending_rpc_count: u64,
        controller_count: u64,
        active_controller_count: u64,
        runtime_count: u64,
        queue_length: u64,
    ) {
        if !self.is_enabled() {
            return;
        }
        self.with_metrics(|m| {
            m.pending_rpc_gauge.record(pending_rpc_count, &[]);
            m.controller_count_gauge.record(controller_count, &[]);
            m.active_controller_gauge
                .record(active_controller_count, &[]);
            m.runtime_count_gauge.record(runtime_count, &[]);
            m.queue_length_gauge.record(queue_length, &[]);
        });
    }

    /// Records the frontend's own `telemetry.health` drop count (already
    /// validated by the time this is called) into the native gauge.
    pub(super) fn record_telemetry_dropped_gauge(&self, dropped_count: u64) {
        if !self.is_enabled() {
            return;
        }
        self.with_metrics(|m| m.telemetry_dropped_gauge.record(dropped_count, &[]));
    }

    pub(super) fn record_event_loop_lag(&self, lag_ms: f64, dimensions: &[KeyValue]) {
        if !self.is_enabled() {
            return;
        }
        self.with_metrics(|m| m.event_loop_lag.record(lag_ms, dimensions));
    }

    pub(super) fn record_long_task_duration(&self, duration_ms: f64) {
        if !self.is_enabled() {
            return;
        }
        self.with_metrics(|m| m.long_task_duration.record(duration_ms, &[]));
    }

    /// Links native work to the frontend trace via explicit context, never ambient
    /// state: concurrent invocations must not share trace context. Invalid IDs
    /// or catalog names skip telemetry without failing the command.
    pub fn start_command_span(
        &self,
        context: &TraceContext,
        command: &'static str,
    ) -> Option<CommandSpan> {
        if !self.is_enabled() {
            return None;
        }
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

    /// Queues an already-validated OTLP trace JSON record. Used only by
    /// `ingest.rs`, after it has revalidated a frontend record against the
    /// attribute catalog.
    pub(super) fn record_trace(&self, value: serde_json::Value) -> bool {
        self.pipeline.append(transport::Signal::Trace, value)
    }

    /// Queues an already-validated OTLP log JSON record. Used only by
    /// `ingest.rs`, for frontend-originated logs (`frontend.error`) after
    /// it has revalidated the record against the attribute catalog.
    pub(super) fn record_log(&self, value: serde_json::Value) -> bool {
        self.pipeline.append(transport::Signal::Log, value)
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
        if self.is_enabled()
            && event_name == "pi.reader.line_dropped"
            && attributes.iter().any(|(key, value)| {
                *key == "tau.reader.drop_reason"
                    && matches!(
                        value,
                        opentelemetry::Value::String(reason)
                            if reason.as_str() == "malformed" || reason.as_str() == "invalid_utf8"
                    )
            })
        {
            self.with_metrics(|m| m.pi_reader_malformed.add(1, &[]));
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
        if !self.is_enabled() {
            return;
        }
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
        let store = Arc::clone(&self.pipeline);
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
/// occur while this thread holds the queue lock. Only the sanitized location
/// is attached; neither the panic payload nor arbitrary attributes are stored.
fn record_panic_to(store: &transport::Pipeline, resource_json: &serde_json::Value, location: &str) {
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
    let _ = store.try_append(transport::Signal::Log, wrapped);
}

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
