//! Native telemetry: OTel `LoggerProvider`/`TracerProvider`s backed by the
//! bounded segmented JSON store in `store.rs`, exported through
//! `exporter.rs`. This module owns provider initialization, the app
//! lifecycle events (`app.started`, `app.exited`), the shared
//! `start_command_span` every ordinary Tauri command now uses to link its
//! native work into the frontend action that requested it (Stage 2 proved it
//! on one call site; Stage 3 applies it broadly), and
//! `record_process_lifecycle`, the native Pi process log family `pi.rs` uses
//! for resolution, start, stop, and exit telemetry (see
//! `OBSERVABILITY_PLAN.md`).

pub mod attributes;
mod exporter;
pub mod ingest;
pub mod privacy;
mod store;
pub mod trace_context;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use opentelemetry::logs::{AnyValue, LogRecord as _, Logger as _, LoggerProvider as _, Severity};
use opentelemetry::trace::{
    Span as _, SpanContext, TraceContextExt, TraceFlags, TraceState, Tracer as _,
    TracerProvider as _,
};
use opentelemetry::{Context, KeyValue, SpanId, TraceId};
use opentelemetry_sdk::logs::SdkLoggerProvider;
use opentelemetry_sdk::trace::{Sampler, SdkTracerProvider};
use opentelemetry_sdk::Resource;
use opentelemetry_semantic_conventions::resource::{
    DEPLOYMENT_ENVIRONMENT_NAME, HOST_ARCH, OS_TYPE, SERVICE_INSTANCE_ID, SERVICE_NAME,
    SERVICE_VERSION,
};

use crate::profile::{APP_DIRECTORY_NAME, ENVIRONMENT_NAME};
use privacy::{truncate_to_limit, DEFAULT_MAX_ATTRIBUTE_LEN};
use trace_context::TraceContext;

/// Directory holding telemetry segments, relative to Tau's per-profile
/// application-data directory (see `profile::APP_DIRECTORY_NAME`).
pub const TELEMETRY_DIR_NAME: &str = "telemetry";

/// One newline-delimited OTLP JSON segment file per signal.
pub const TRACE_SEGMENT_FILE: &str = "traces.jsonl";
pub const LOG_SEGMENT_FILE: &str = "logs.jsonl";
pub const METRIC_SEGMENT_FILE: &str = "metrics.jsonl";

const SCOPE_NAME: &str = "tau";

/// The native telemetry pipeline for one app launch. Construction never
/// fails: a directory or writer problem degrades to the store's in-memory
/// fallback instead of stopping app startup.
pub struct Telemetry {
    logger_provider: SdkLoggerProvider,
    tracer_provider: SdkTracerProvider,
    store: Arc<store::Store>,
    /// Precomputed once so `ingest.rs` does not rebuild it per frontend
    /// record; see `exporter::resource_to_json`.
    resource_json: serde_json::Value,
}

impl Telemetry {
    /// Initializes telemetry using the real clock and Tau's per-profile
    /// application-data directory. Call this before constructing the Tauri
    /// builder so setup-time work is covered too.
    pub fn init() -> Self {
        Telemetry::new(resolve_telemetry_dir(), Arc::new(store::SystemClock))
    }

    fn new(dir: PathBuf, clock: Arc<dyn store::Clock>) -> Self {
        let store = Arc::new(store::Store::new(
            dir,
            store::StoreConfig::production(),
            clock,
        ));
        let resource = build_resource();
        let resource_json = exporter::resource_to_json(&resource);
        let log_exporter = exporter::JsonFileLogExporter::new(Arc::clone(&store), &resource);
        let span_exporter = exporter::JsonFileSpanExporter::new(Arc::clone(&store), &resource);
        let logger_provider = SdkLoggerProvider::builder()
            .with_resource(resource.clone())
            .with_simple_exporter(log_exporter)
            .build();
        // Do not sample: the design principles call for recording all of
        // this deliberately low-volume telemetry.
        let tracer_provider = SdkTracerProvider::builder()
            .with_resource(resource)
            .with_sampler(Sampler::AlwaysOn)
            .with_simple_exporter(span_exporter)
            .build();
        Telemetry {
            logger_provider,
            tracer_provider,
            store,
            resource_json,
        }
    }

    pub fn record_app_started(&self) {
        self.emit_lifecycle_log("app.started");
    }

    /// Records the clean-exit marker and force-flushes it. Callers must
    /// invoke this from a `RunEvent::Exit` handler: `App::run` calls
    /// `std::process::exit` internally once it returns, which skips `Drop`,
    /// so nothing here can rely on destructors running.
    pub fn record_app_exited(&self) {
        self.emit_lifecycle_log("app.exited");
        let _ = self.logger_provider.force_flush();
    }

    pub fn shutdown(&self) {
        let _ = self.logger_provider.shutdown();
        let _ = self.tracer_provider.shutdown();
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

    /// Starts a native span in the `tauri.invoke` family as a child of the
    /// frontend-originated `context`, so the frontend span that requested
    /// `command` and this native work share one trace. `context` is passed
    /// explicitly by the caller on every invocation — never read from an
    /// ambient/global "current span" — so concurrent calls cannot leak trace
    /// context into each other.
    ///
    /// Returns `None` if `context` is not a well-formed trace/span id or the
    /// command name fails catalog validation; telemetry failure never fails
    /// the command it is attached to.
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
    /// Stage 0 catalog.
    pub(super) fn record_trace(&self, value: serde_json::Value) -> bool {
        self.store.append(store::Signal::Trace, value)
    }

    /// Records one `pi.process.lifecycle` log: resolution, start, stop, or
    /// exit. `generation` is included only when known (resolution happens
    /// before a runtime's first process has one). Every attribute is
    /// revalidated against the catalog before being attached, so a caller
    /// mistake drops the attribute rather than persisting an unreviewed one;
    /// telemetry failure never affects Pi process management itself.
    pub fn record_process_lifecycle(
        &self,
        event_name: &'static str,
        runtime_id: &str,
        generation: Option<u64>,
        span_context: Option<&SpanContext>,
        attributes: &[(&'static str, opentelemetry::Value)],
    ) {
        let family = attributes::PI_PROCESS_LIFECYCLE.name;
        let mut validated: Vec<(&'static str, opentelemetry::Value)> = Vec::new();

        let runtime_value = opentelemetry::Value::String(
            truncate_to_limit(runtime_id, DEFAULT_MAX_ATTRIBUTE_LEN).into(),
        );
        if attributes::validate(family, "tau.runtime.id", &runtime_value).is_ok() {
            validated.push(("tau.runtime.id", runtime_value));
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
}

/// Ends the wrapped native span when dropped, so it is recorded whether the
/// guarded scope returns normally or via `?`. Concurrent callers each own
/// their own `CommandSpan`; nothing here is shared mutable state.
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

/// Builds the per-launch resource from exactly the six attributes Stage 0
/// cataloged: no SDK-provided extras, so the resource never carries more
/// than the reviewed allowlist.
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

/// Converts a validated `opentelemetry::Value` to the log API's `AnyValue`.
/// Fails closed like `exporter::any_value_json`/`otel_value_json`: an
/// unsupported variant renders as a fixed marker, never `Debug` output that
/// could leak an unexpected value's content.
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

/// Maps Rust's `std::env::consts::ARCH` to the OTel `host.arch` enum values.
fn host_arch_value() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => other,
    }
}

fn telemetry_dir(data_dir: &std::path::Path, app_directory_name: &str) -> PathBuf {
    data_dir.join(app_directory_name).join(TELEMETRY_DIR_NAME)
}

fn resolve_telemetry_dir() -> PathBuf {
    let data_dir = dirs::data_dir().unwrap_or_else(std::env::temp_dir);
    telemetry_dir(&data_dir, APP_DIRECTORY_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

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
    }

    #[test]
    fn host_arch_uses_the_otel_enum_values() {
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
    }

    #[test]
    fn resource_string_attributes_are_truncated_to_the_shared_limit() {
        let long_id = "x".repeat(DEFAULT_MAX_ATTRIBUTE_LEN * 2);
        let truncated = truncate_to_limit(&long_id, DEFAULT_MAX_ATTRIBUTE_LEN);
        assert_eq!(truncated.len(), DEFAULT_MAX_ATTRIBUTE_LEN);
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
    fn production_and_development_profiles_use_separate_directories() {
        let data_dir = std::path::Path::new("/app-data");
        assert_eq!(
            telemetry_dir(data_dir, "tau"),
            data_dir.join("tau/telemetry")
        );
        assert_eq!(
            telemetry_dir(data_dir, "tau-dev"),
            data_dir.join("tau-dev/telemetry")
        );
        assert_ne!(
            telemetry_dir(data_dir, "tau"),
            telemetry_dir(data_dir, "tau-dev")
        );
    }

    #[test]
    fn record_app_started_persists_a_valid_log_record() {
        let directory = tempfile::tempdir().expect("temp dir");
        let clock: Arc<dyn store::Clock> = Arc::new(store::SystemClock);
        let telemetry = Telemetry::new(directory.path().to_path_buf(), clock);

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
    }

    #[test]
    fn record_app_exited_force_flushes_so_the_marker_is_durable() {
        let directory = tempfile::tempdir().expect("temp dir");
        let clock: Arc<dyn store::Clock> = Arc::new(store::SystemClock);
        let telemetry = Telemetry::new(directory.path().to_path_buf(), clock);

        telemetry.record_app_exited();
        telemetry.shutdown();

        let contents = std::fs::read_to_string(directory.path().join(LOG_SEGMENT_FILE))
            .expect("log segment should exist");
        let line = contents.lines().next().expect("one log line");
        let value: serde_json::Value = serde_json::from_str(line).expect("valid json");
        let log_record = &value["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0];
        assert_eq!(log_record["eventName"], "app.exited");
    }

    fn test_telemetry() -> (Telemetry, tempfile::TempDir) {
        let directory = tempfile::tempdir().expect("temp dir");
        let telemetry =
            Telemetry::new(directory.path().to_path_buf(), Arc::new(store::SystemClock));
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
    }

    #[test]
    fn command_span_rejects_a_malformed_context() {
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
    }

    #[test]
    fn process_lifecycle_links_to_the_active_command_span() {
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

    #[test]
    fn process_lifecycle_drops_an_unreviewed_stop_reason_without_failing() {
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
    }

    #[test]
    fn process_lifecycle_drops_a_forbidden_content_canary_disguised_as_a_stop_reason() {
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

    #[test]
    fn process_lifecycle_exit_code_supports_a_canary_free_fail_closed_conversion() {
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
}
