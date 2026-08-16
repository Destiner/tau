//! Native telemetry: an OTel `LoggerProvider` backed by the bounded
//! segmented JSON store in `store.rs`, exported through `exporter.rs`. This
//! module owns provider initialization and the two lifecycle events Stage 1
//! records (`app.started`, `app.exited`); it does not yet touch the
//! frontend, Pi, or any other operation (see `OBSERVABILITY_PLAN.md`).

// Stage 3 wires per-family attribute validation against this catalog.
#[allow(dead_code)]
pub mod attributes;
mod exporter;
pub mod privacy;
mod store;
// Stage 2 wires IPC trace-context propagation against this.
#[allow(dead_code)]
pub mod trace_context;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use opentelemetry::logs::{AnyValue, LogRecord as _, Logger as _, LoggerProvider as _, Severity};
use opentelemetry::KeyValue;
use opentelemetry_sdk::logs::SdkLoggerProvider;
use opentelemetry_sdk::Resource;
use opentelemetry_semantic_conventions::resource::{
    DEPLOYMENT_ENVIRONMENT_NAME, HOST_ARCH, OS_TYPE, SERVICE_INSTANCE_ID, SERVICE_NAME,
    SERVICE_VERSION,
};

use crate::profile::{APP_DIRECTORY_NAME, ENVIRONMENT_NAME};
use privacy::{truncate_to_limit, DEFAULT_MAX_ATTRIBUTE_LEN};

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
        let exporter = exporter::JsonFileLogExporter::new(store, &resource);
        let logger_provider = SdkLoggerProvider::builder()
            .with_resource(resource)
            .with_simple_exporter(exporter)
            .build();
        Telemetry { logger_provider }
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
}
