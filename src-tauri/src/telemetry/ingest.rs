use super::Telemetry;
pub use tau_observability::telemetry::ingest::IngestOutcome;
use tauri::State;

#[tauri::command]
pub fn ingest_telemetry(
    telemetry: State<'_, Telemetry>,
    records: Vec<serde_json::Value>,
) -> IngestOutcome {
    tau_observability::telemetry::ingest::ingest_records(&telemetry, records)
}
