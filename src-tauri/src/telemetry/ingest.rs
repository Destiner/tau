//! The frontend telemetry ingest command: the only way frontend-originated
//! records reach disk. Every record is revalidated against the Stage 0
//! catalog before it is persisted; nothing here trusts the frontend's own
//! validation, and no unrecognized field, family, key, type, or oversized
//! value is accepted.
//!
//! This command must never itself be traced. Wrapping its own invocation
//! with a span would enqueue a record about sending the batch, which would
//! only reach disk by calling this same command again — recursing forever.
//! Unlike `storage::load_workspace`, it takes no `TraceContext` argument and
//! never calls `Telemetry::start_command_span`.
use std::collections::HashMap;

use opentelemetry::Value as OtelValue;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::State;

use super::exporter::{otel_value_json, wrap_span_json};
use super::trace_context::{is_non_zero_lower_hex, TraceContext, SPAN_ID_LEN};
use super::{attributes, Telemetry, SCOPE_NAME};

/// Bounds worst-case per-call work regardless of what the frontend sends;
/// the frontend's own bounded queue caps real batches far below this.
const MAX_RECORDS_PER_CALL: usize = 64;
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FrontendSpanRecord {
    family: String,
    trace_id: String,
    span_id: String,
    #[serde(default)]
    parent_span_id: Option<String>,
    sampled: bool,
    start_time_unix_nano: String,
    end_time_unix_nano: String,
    #[serde(default)]
    attributes: HashMap<String, FrontendAttributeValue>,
}

/// Deliberately narrower than `serde_json::Value`: a frontend record can
/// only ever carry the same string/int shapes the catalog allows, never an
/// arbitrary object or array.
#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
enum FrontendAttributeValue {
    Str(String),
    Int(i64),
}

impl FrontendAttributeValue {
    fn as_otel_value(&self) -> OtelValue {
        match self {
            FrontendAttributeValue::Str(value) => OtelValue::String(value.clone().into()),
            FrontendAttributeValue::Int(value) => OtelValue::I64(*value),
        }
    }
}

#[derive(Debug, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IngestOutcome {
    pub accepted: u32,
    pub rejected: u32,
}

/// Takes `Value` rather than `Vec<FrontendSpanRecord>` deliberately: a
/// batch is a JSON array of otherwise-untyped values, so one record that
/// fails to deserialize (an unknown field, a wrong-shaped attribute) is
/// rejected on its own rather than invalidating every other record in the
/// same call.
#[tauri::command]
pub fn ingest_telemetry(telemetry: State<'_, Telemetry>, records: Vec<Value>) -> IngestOutcome {
    ingest_records(&telemetry, records)
}

fn ingest_records(telemetry: &Telemetry, records: Vec<Value>) -> IngestOutcome {
    let overflow = records.len().saturating_sub(MAX_RECORDS_PER_CALL) as u32;
    let mut outcome = IngestOutcome {
        accepted: 0,
        rejected: overflow,
    };

    for raw in records.into_iter().take(MAX_RECORDS_PER_CALL) {
        let value = serde_json::from_value::<FrontendSpanRecord>(raw)
            .ok()
            .and_then(|record| validated_span_json(&record, &telemetry.resource_json, SCOPE_NAME));
        match value {
            Some(value) => {
                telemetry.record_trace(value);
                outcome.accepted += 1;
            }
            None => outcome.rejected += 1,
        }
    }
    outcome
}

/// Validates one frontend record against the Stage 0 catalog and, if it
/// passes every check, converts it to the same OTLP JSON shape native spans
/// use. Any failure rejects the whole record rather than persisting a
/// partially-validated one.
fn validated_span_json(
    record: &FrontendSpanRecord,
    resource_json: &Value,
    scope_name: &str,
) -> Option<Value> {
    if record.family != attributes::TAURI_INVOKE.name || record.attributes.len() != 1 {
        return None;
    }

    let flags = if record.sampled { "01" } else { "00" };
    TraceContext::parse(&format!(
        "00-{}-{}-{flags}",
        record.trace_id, record.span_id
    ))
    .ok()?;
    if let Some(parent) = &record.parent_span_id {
        if !is_non_zero_lower_hex(parent, SPAN_ID_LEN) {
            return None;
        }
    }

    let start_time = canonical_timestamp(&record.start_time_unix_nano)?;
    let end_time = canonical_timestamp(&record.end_time_unix_nano)?;
    if start_time.0 > end_time.0 {
        return None;
    }

    let mut attribute_values = Vec::with_capacity(record.attributes.len());
    for (key, value) in &record.attributes {
        let otel_value = value.as_otel_value();
        attributes::validate(&record.family, key, &otel_value).ok()?;
        attribute_values.push(json!({ "key": key, "value": otel_value_json(&otel_value) }));
    }

    let mut span_object = serde_json::Map::new();
    span_object.insert("traceId".into(), Value::String(record.trace_id.clone()));
    span_object.insert("spanId".into(), Value::String(record.span_id.clone()));
    if let Some(parent) = &record.parent_span_id {
        span_object.insert("parentSpanId".into(), Value::String(parent.clone()));
    }
    span_object.insert("name".into(), Value::String(record.family.clone()));
    span_object.insert("kind".into(), Value::from(1)); // SPAN_KIND_INTERNAL
    span_object.insert("startTimeUnixNano".into(), Value::String(start_time.1));
    span_object.insert("endTimeUnixNano".into(), Value::String(end_time.1));
    if !attribute_values.is_empty() {
        span_object.insert("attributes".into(), Value::Array(attribute_values));
    }
    span_object.insert("status".into(), json!({ "code": 0 }));

    Some(wrap_span_json(
        resource_json,
        scope_name,
        Value::Object(span_object),
    ))
}

fn canonical_timestamp(value: &str) -> Option<(u64, String)> {
    if value.is_empty() || value.len() > 20 || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let parsed = value.parse::<u64>().ok()?;
    Some((parsed, parsed.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::store::Signal;
    use std::sync::Arc;

    fn record(
        family: &str,
        attributes: HashMap<String, FrontendAttributeValue>,
    ) -> FrontendSpanRecord {
        FrontendSpanRecord {
            family: family.to_string(),
            trace_id: "4bf92f3577b34da6a3ce929d0e0e4736".to_string(),
            span_id: "00f067aa0ba902b7".to_string(),
            parent_span_id: None,
            sampled: true,
            start_time_unix_nano: "1000000000".to_string(),
            end_time_unix_nano: "1000005000".to_string(),
            attributes,
        }
    }

    fn command_attribute(command: &str) -> HashMap<String, FrontendAttributeValue> {
        HashMap::from([(
            "tau.invoke.command".to_string(),
            FrontendAttributeValue::Str(command.to_string()),
        )])
    }

    #[test]
    fn accepts_a_well_formed_record() {
        let resource_json = json!({ "attributes": [] });
        let value = validated_span_json(
            &record("tauri.invoke", command_attribute("load_workspace")),
            &resource_json,
            "tau",
        )
        .expect("valid record");
        let span_object = &value["resourceSpans"][0]["scopeSpans"][0]["spans"][0];
        assert_eq!(span_object["name"], "tauri.invoke");
        assert_eq!(span_object["traceId"], "4bf92f3577b34da6a3ce929d0e0e4736");
        assert_eq!(
            span_object["attributes"][0],
            json!({ "key": "tau.invoke.command", "value": { "stringValue": "load_workspace" } })
        );
    }

    #[test]
    fn rejects_an_unknown_family() {
        let resource_json = json!({ "attributes": [] });
        assert!(validated_span_json(
            &record("not.a.family", HashMap::new()),
            &resource_json,
            "tau"
        )
        .is_none());
    }

    #[test]
    fn rejects_a_cataloged_family_not_owned_by_the_frontend_transport() {
        let resource_json = json!({ "attributes": [] });
        assert!(validated_span_json(
            &record("app.lifecycle", HashMap::new()),
            &resource_json,
            "tau"
        )
        .is_none());
    }

    #[test]
    fn rejects_an_attribute_outside_the_family_allowlist() {
        let resource_json = json!({ "attributes": [] });
        let attributes = HashMap::from([(
            "prompt.text".to_string(),
            FrontendAttributeValue::Str("user content".to_string()),
        )]);
        assert!(
            validated_span_json(&record("tauri.invoke", attributes), &resource_json, "tau")
                .is_none()
        );
    }

    #[test]
    fn rejects_an_oversized_attribute_value() {
        let resource_json = json!({ "attributes": [] });
        let oversized = "x".repeat(129);
        let attributes = HashMap::from([(
            "tau.invoke.command".to_string(),
            FrontendAttributeValue::Str(oversized),
        )]);
        assert!(
            validated_span_json(&record("tauri.invoke", attributes), &resource_json, "tau")
                .is_none()
        );
    }

    #[test]
    fn rejects_unreviewed_command_values_even_when_the_type_and_length_are_valid() {
        let resource_json = json!({ "attributes": [] });
        let canary = crate::telemetry::privacy::FORBIDDEN_CONTENT_CANARIES[0].1;
        assert!(validated_span_json(
            &record("tauri.invoke", command_attribute(canary)),
            &resource_json,
            "tau"
        )
        .is_none());
    }

    #[test]
    fn rejects_malformed_or_reversed_timestamps() {
        let resource_json = json!({ "attributes": [] });
        for (start, end) in [
            ("not-a-time", "1000005000"),
            ("1000000000", "prompt-canary"),
            ("1000005000", "1000000000"),
            ("18446744073709551616", "18446744073709551616"),
        ] {
            let mut invalid = record("tauri.invoke", command_attribute("load_workspace"));
            invalid.start_time_unix_nano = start.to_string();
            invalid.end_time_unix_nano = end.to_string();
            assert!(validated_span_json(&invalid, &resource_json, "tau").is_none());
        }
    }

    #[test]
    fn rejects_a_malformed_trace_id() {
        let resource_json = json!({ "attributes": [] });
        let mut malformed = record("tauri.invoke", command_attribute("load_workspace"));
        malformed.trace_id = "not-hex".to_string();
        assert!(validated_span_json(&malformed, &resource_json, "tau").is_none());
    }

    #[test]
    fn rejects_a_malformed_parent_span_id() {
        let resource_json = json!({ "attributes": [] });
        let mut malformed = record("tauri.invoke", command_attribute("load_workspace"));
        malformed.parent_span_id = Some("not-hex".to_string());
        assert!(validated_span_json(&malformed, &resource_json, "tau").is_none());
    }

    #[test]
    fn rejects_a_record_without_the_required_command_attribute() {
        let resource_json = json!({ "attributes": [] });
        assert!(validated_span_json(
            &record("tauri.invoke", HashMap::new()),
            &resource_json,
            "tau"
        )
        .is_none());
    }

    fn test_telemetry() -> (Telemetry, tempfile::TempDir) {
        let directory = tempfile::tempdir().expect("temp dir");
        let telemetry = Telemetry::new(
            directory.path().to_path_buf(),
            Arc::new(crate::telemetry::store::SystemClock),
        );
        (telemetry, directory)
    }

    fn to_value(record: &FrontendSpanRecord) -> Value {
        serde_json::to_value(record).expect("serializable record")
    }

    #[test]
    fn ingest_persists_only_the_valid_records_in_a_mixed_batch() {
        let (telemetry, _directory) = test_telemetry();
        let records = vec![
            to_value(&record("tauri.invoke", command_attribute("load_workspace"))),
            to_value(&record("not.a.family", HashMap::new())),
        ];

        let outcome = ingest_records(&telemetry, records);

        assert_eq!(
            outcome,
            IngestOutcome {
                accepted: 1,
                rejected: 1
            }
        );
        assert_eq!(telemetry.store.read_records(Signal::Trace).len(), 1);
    }

    #[test]
    fn ingest_rejects_an_unrecognized_field_without_rejecting_the_rest_of_the_batch() {
        let (telemetry, _directory) = test_telemetry();
        let mut malformed = to_value(&record("tauri.invoke", command_attribute("load_workspace")));
        malformed
            .as_object_mut()
            .expect("object")
            .insert("promptText".to_string(), json!("unexpected content"));
        let records = vec![
            malformed,
            to_value(&record("tauri.invoke", command_attribute("load_workspace"))),
        ];

        let outcome = ingest_records(&telemetry, records);

        assert_eq!(
            outcome,
            IngestOutcome {
                accepted: 1,
                rejected: 1
            }
        );
    }

    #[test]
    fn ingest_rejects_batches_beyond_the_per_call_bound() {
        let (telemetry, _directory) = test_telemetry();
        let records = (0..MAX_RECORDS_PER_CALL + 5)
            .map(|_| to_value(&record("tauri.invoke", command_attribute("load_workspace"))))
            .collect();

        let outcome = ingest_records(&telemetry, records);

        assert_eq!(outcome.accepted, MAX_RECORDS_PER_CALL as u32);
        assert_eq!(outcome.rejected, 5);
    }

    #[test]
    fn frontend_parent_and_native_child_persist_in_one_trace() {
        let (telemetry, _directory) = test_telemetry();
        let frontend = record("tauri.invoke", command_attribute("load_workspace"));
        let context = TraceContext {
            trace_id: frontend.trace_id.clone(),
            span_id: frontend.span_id.clone(),
            sampled: frontend.sampled,
        };

        assert_eq!(
            ingest_records(&telemetry, vec![to_value(&frontend)]).accepted,
            1
        );
        telemetry
            .start_command_span(&context, "load_workspace")
            .expect("native child");

        let records = telemetry.store.read_records(Signal::Trace);
        assert_eq!(records.len(), 2);
        assert!(records
            .iter()
            .all(|record| record["tauObservedTimeUnixNano"].is_string()));
        let spans: Vec<&Value> = records
            .iter()
            .map(|record| &record["resourceSpans"][0]["scopeSpans"][0]["spans"][0])
            .collect();
        assert!(spans.iter().any(|span| span["spanId"] == context.span_id));
        assert!(spans.iter().any(|span| {
            span["traceId"] == context.trace_id && span["parentSpanId"] == context.span_id
        }));
    }

    /// The ingest command itself must never be traced: processing a batch
    /// persists exactly the records the batch validated, and nothing else.
    #[test]
    fn ingesting_does_not_record_a_span_about_itself() {
        let (telemetry, _directory) = test_telemetry();
        let outcome = ingest_records(
            &telemetry,
            vec![to_value(&record(
                "tauri.invoke",
                command_attribute("load_workspace"),
            ))],
        );
        assert_eq!(outcome.accepted, 1);
        assert_eq!(telemetry.store.read_records(Signal::Trace).len(), 1);
    }
}
