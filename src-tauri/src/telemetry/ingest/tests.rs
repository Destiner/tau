use super::*;
use crate::telemetry::tests::{fake, target};
use std::time::SystemTime;

fn span() -> Value {
    let now = super::super::exporter::unix_nanos_string(SystemTime::now());
    json!({"family":"tauri.invoke","traceId":"0123456789abcdef0123456789abcdef","spanId":"0123456789abcdef","sampled":true,"startTimeUnixNano":now,"endTimeUnixNano":now,"attributes":{"tau.invoke.command":"start_pi"}})
}
#[test]
fn frontend_privacy_revalidation_precedes_cloud_export() {
    let (telemetry, _requests) = fake();
    telemetry.pipeline.configure(Some(target()));
    let mut records = Vec::new();
    for key in [
        "prompt",
        "description",
        "url",
        "path",
        "token",
        "transcript",
        "stderr",
    ] {
        let mut record = span();
        record["attributes"][key] = json!("private content");
        records.push(record);
    }
    let mut record = span();
    record["attributes"]["tau.invoke.command"] = json!("private/path");
    records.push(record);
    let mut record = span();
    record["payload"] = json!("private content");
    records.push(record);
    let result = ingest_records(&telemetry, records);
    assert_eq!(result.accepted, 0);
    assert_eq!(result.rejected, 9);
    assert!(telemetry.metric_pipeline.lock().unwrap().is_none());
    let record = span();
    // Contention is allowed to drop valid records; retry boundedly until accepted.
    for _ in 0..1000 {
        if ingest_records(&telemetry, vec![record.clone()]).accepted == 1 {
            telemetry.shutdown();
            return;
        }
        std::thread::yield_now();
    }
    panic!("valid telemetry not accepted");
}
#[test]
fn frontend_records_from_previous_admin_epoch_are_rejected() {
    let (telemetry, _requests) = fake();
    telemetry.pipeline.configure(Some(target()));
    let record = span();
    telemetry.pipeline.configure(None);
    telemetry.pipeline.configure(Some(target()));
    assert_eq!(ingest_records(&telemetry, vec![record]).accepted, 0);
    telemetry.pipeline.configure(None);
    assert_eq!(ingest_records(&telemetry, vec![span()]).accepted, 0);
    assert!(telemetry.metric_pipeline.lock().unwrap().is_none());
}
