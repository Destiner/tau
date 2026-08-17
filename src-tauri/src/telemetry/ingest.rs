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

use opentelemetry::logs::Severity;
use opentelemetry::Value as OtelValue;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::State;

use super::exporter::{otel_value_json, wrap_log_json, wrap_span_json};
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

/// The wire shape for a frontend-originated log record (Stage 4's
/// `frontend.error`). Deliberately narrower than a span: no trace/span
/// identity, and no frontend-supplied event name or severity —
/// `fixed_log_metadata` maps `family` to both, so an untrusted caller can
/// never choose either. This shape and a span's are structurally disjoint
/// (a span always requires `traceId`/`spanId`/`sampled`/`startTimeUnixNano`/
/// `endTimeUnixNano`, none of which a log record carries), so
/// `ingest_records` can try deserializing as a span first and fall back to
/// this without an explicit discriminant field.
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FrontendLogRecord {
    family: String,
    time_unix_nano: String,
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
        if ingest_one(telemetry, raw) {
            outcome.accepted += 1;
        } else {
            outcome.rejected += 1;
        }
    }
    outcome
}

/// Tries a record as a span first, then as a log; a value that matches
/// neither shape (or fails its family's validation) is rejected. Cloning
/// `raw` for the second attempt is cheap: batches are capped at
/// `MAX_RECORDS_PER_CALL` small JSON objects.
fn ingest_one(telemetry: &Telemetry, raw: Value) -> bool {
    if let Some(record) = serde_json::from_value::<FrontendSpanRecord>(raw.clone())
        .ok()
        .and_then(|record| validated_span_json(&record, &telemetry.resource_json, SCOPE_NAME))
    {
        telemetry.record_trace(record);
        return true;
    }
    if let Some(record) = serde_json::from_value::<FrontendLogRecord>(raw)
        .ok()
        .and_then(|record| validated_log_json(&record, &telemetry.resource_json, SCOPE_NAME))
    {
        telemetry.record_log(record);
        return true;
    }
    false
}

/// The fixed `(eventName, severity)` pair a frontend-owned log family
/// produces. Neither is ever taken from the frontend: trusting an
/// untrusted caller's choice of event name or severity would add a second
/// categorical dimension to review for no benefit, since today every
/// family has exactly one of each.
fn fixed_log_metadata(family: &str) -> Option<(&'static str, Severity)> {
    if family == attributes::FRONTEND_ERROR.name {
        Some(("frontend.error", Severity::Error))
    } else if family == attributes::TELEMETRY_HEALTH.name {
        Some(("telemetry.queue_overflow", Severity::Warn))
    } else {
        None
    }
}

/// Validates one frontend log record against the Stage 0 catalog and, if it
/// passes every check, converts it to the same OTLP JSON shape native logs
/// use. Mirrors `validated_span_json`'s structure and required-attribute
/// check.
fn validated_log_json(
    record: &FrontendLogRecord,
    resource_json: &Value,
    scope_name: &str,
) -> Option<Value> {
    let (event_name, severity) = fixed_log_metadata(&record.family)?;
    validate_frontend_attribute_keys(&record.family, &record.attributes)?;

    let time = canonical_timestamp(&record.time_unix_nano)?;

    let mut attribute_values = Vec::with_capacity(record.attributes.len());
    for (key, value) in &record.attributes {
        let otel_value = value.as_otel_value();
        attributes::validate(&record.family, key, &otel_value).ok()?;
        attribute_values.push(json!({ "key": key, "value": otel_value_json(&otel_value) }));
    }

    let mut log_object = serde_json::Map::new();
    log_object.insert("timeUnixNano".into(), Value::String(time.1));
    log_object.insert("eventName".into(), Value::String(event_name.to_string()));
    log_object.insert("severityNumber".into(), Value::from(severity as i32));
    log_object.insert(
        "severityText".into(),
        Value::String(severity.name().to_string()),
    );
    log_object.insert("body".into(), json!({ "stringValue": event_name }));
    if !attribute_values.is_empty() {
        log_object.insert("attributes".into(), Value::Array(attribute_values));
    }

    Some(wrap_log_json(
        resource_json,
        scope_name,
        Value::Object(log_object),
    ))
}

/// Validates one frontend record against the Stage 0 catalog and, if it
/// passes every check, converts it to the same OTLP JSON shape native spans
/// use. Any failure rejects the whole record rather than persisting a
/// partially-validated one.
/// Every attribute key a frontend-owned family's record must carry, in
/// addition to the trace/span identity fields already required of every
/// record. Reviewed context identifiers may be added; every key/value still
/// passes the family catalog, and the total is bounded. Families not
/// listed here are not owned by this frontend transport at all: `send_pi`'s
/// RPC bodies never reach here, and `ingest_telemetry` must never trace
/// itself.
fn required_attributes(family: &str) -> Option<&'static [&'static str]> {
    if family == attributes::TAURI_INVOKE.name {
        Some(&["tau.invoke.command"])
    } else if family == attributes::UI_ACTION.name {
        Some(&["tau.action.name"])
    } else if family == attributes::PI_RPC.name {
        Some(&[
            "pi.rpc.method",
            "pi.rpc.request_id",
            "pi.rpc.outcome",
            "tau.runtime.id",
            "pi.generation",
        ])
    } else if family == attributes::PI_STREAM.name {
        Some(&[
            "pi.stream.delta_count",
            "pi.stream.character_count",
            "tau.runtime.id",
            "pi.generation",
        ])
    } else if family == attributes::TELEMETRY_HEALTH.name {
        // Only the frontend's own counter is accepted from the frontend;
        // `tau.telemetry.failed_write_count` is native-only (see
        // `Telemetry::record_writer_health`) and never sent here.
        Some(&["tau.telemetry.dropped_count"])
    } else if family == attributes::FRONTEND_ERROR.name {
        Some(&["tau.error.source", "tau.error.kind", "tau.error.location"])
    } else {
        None
    }
}

fn optional_frontend_attributes(family: &str) -> &'static [&'static str] {
    if family == attributes::TAURI_INVOKE.name {
        &["tau.invoke.outcome"]
    } else {
        &[]
    }
}

fn validate_frontend_attribute_keys(
    family: &str,
    values: &HashMap<String, FrontendAttributeValue>,
) -> Option<()> {
    let required = required_attributes(family)?;
    let optional = optional_frontend_attributes(family);
    if !required.iter().all(|key| values.contains_key(*key)) {
        return None;
    }
    let keys_are_owned = values.keys().all(|key| {
        required.contains(&key.as_str())
            || optional.contains(&key.as_str())
            || attributes::CONTEXT_ATTRIBUTES
                .iter()
                .any(|spec| spec.key == key)
    });
    keys_are_owned.then_some(())
}

fn validated_span_json(
    record: &FrontendSpanRecord,
    resource_json: &Value,
    scope_name: &str,
) -> Option<Value> {
    validate_frontend_attribute_keys(&record.family, &record.attributes)?;

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
    fn accepts_a_reviewed_invoke_outcome() {
        let resource_json = json!({ "attributes": [] });
        let mut attributes = command_attribute("load_workspace");
        attributes.insert(
            "tau.invoke.outcome".to_string(),
            FrontendAttributeValue::Str("success".to_string()),
        );
        let value = validated_span_json(&record("tauri.invoke", attributes), &resource_json, "tau")
            .expect("valid outcome");
        let persisted = value["resourceSpans"][0]["scopeSpans"][0]["spans"][0]["attributes"]
            .as_array()
            .expect("attributes");
        assert!(persisted.iter().any(|attribute| {
            attribute["key"] == "tau.invoke.outcome"
                && attribute["value"]["stringValue"] == "success"
        }));
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

    fn string_attribute(key: &str, value: &str) -> (String, FrontendAttributeValue) {
        (
            key.to_string(),
            FrontendAttributeValue::Str(value.to_string()),
        )
    }

    fn int_attribute(key: &str, value: i64) -> (String, FrontendAttributeValue) {
        (key.to_string(), FrontendAttributeValue::Int(value))
    }

    #[test]
    fn accepts_a_well_formed_ui_action_record() {
        let resource_json = json!({ "attributes": [] });
        let attributes = HashMap::from([string_attribute("tau.action.name", "session.select")]);
        let value = validated_span_json(&record("ui.action", attributes), &resource_json, "tau")
            .expect("valid ui.action record");
        let span_object = &value["resourceSpans"][0]["scopeSpans"][0]["spans"][0];
        assert_eq!(span_object["name"], "ui.action");
    }

    #[test]
    fn accepts_reviewed_context_identifiers_on_a_frontend_record() {
        let resource_json = json!({ "attributes": [] });
        let attributes = HashMap::from([
            string_attribute("tau.action.name", "session.select"),
            string_attribute("tau.session.id", "session-1"),
            string_attribute("tau.controller.id", "controller-1"),
        ]);
        let value = validated_span_json(&record("ui.action", attributes), &resource_json, "tau")
            .expect("valid scoped ui.action record");
        let persisted = value["resourceSpans"][0]["scopeSpans"][0]["spans"][0]["attributes"]
            .as_array()
            .expect("attributes");
        assert!(persisted
            .iter()
            .any(|attribute| attribute["key"] == "tau.session.id"));
    }

    #[test]
    fn rejects_an_unreviewed_action_name() {
        let resource_json = json!({ "attributes": [] });
        let attributes = HashMap::from([string_attribute("tau.action.name", "not.a.real.action")]);
        assert!(
            validated_span_json(&record("ui.action", attributes), &resource_json, "tau").is_none()
        );
    }

    #[test]
    fn rejects_a_forbidden_content_canary_disguised_as_an_action_name() {
        let resource_json = json!({ "attributes": [] });
        let canary = crate::telemetry::privacy::FORBIDDEN_CONTENT_CANARIES[0].1;
        let attributes = HashMap::from([string_attribute("tau.action.name", canary)]);
        assert!(
            validated_span_json(&record("ui.action", attributes), &resource_json, "tau").is_none()
        );
    }

    fn pi_rpc_attributes(method: &str, outcome: &str) -> HashMap<String, FrontendAttributeValue> {
        HashMap::from([
            string_attribute("pi.rpc.method", method),
            string_attribute("pi.rpc.request_id", "tau-prompt-1"),
            string_attribute("pi.rpc.outcome", outcome),
            string_attribute("tau.runtime.id", "runtime-1"),
            int_attribute("pi.generation", 3),
        ])
    }

    #[test]
    fn accepts_a_well_formed_pi_rpc_record() {
        let resource_json = json!({ "attributes": [] });
        let attributes = pi_rpc_attributes("get_state", "success");
        let value = validated_span_json(&record("pi.rpc", attributes), &resource_json, "tau")
            .expect("valid pi.rpc record");
        let span_object = &value["resourceSpans"][0]["scopeSpans"][0]["spans"][0];
        assert_eq!(span_object["name"], "pi.rpc");
        assert!(span_object["attributes"]
            .as_array()
            .expect("attributes")
            .iter()
            .any(|attribute| attribute["key"] == "pi.generation"
                && attribute["value"]["intValue"] == "3"));
    }

    #[test]
    fn rejects_a_pi_rpc_record_missing_a_required_attribute() {
        let resource_json = json!({ "attributes": [] });
        let mut attributes = pi_rpc_attributes("get_state", "success");
        attributes.remove("tau.runtime.id");
        assert!(
            validated_span_json(&record("pi.rpc", attributes), &resource_json, "tau").is_none()
        );
    }

    #[test]
    fn rejects_an_unreviewed_rpc_outcome() {
        let resource_json = json!({ "attributes": [] });
        let attributes = pi_rpc_attributes("get_state", "not-a-real-outcome");
        assert!(
            validated_span_json(&record("pi.rpc", attributes), &resource_json, "tau").is_none()
        );
    }

    #[test]
    fn accepts_a_well_formed_pi_stream_aggregate_record() {
        let resource_json = json!({ "attributes": [] });
        let attributes = HashMap::from([
            int_attribute("pi.stream.delta_count", 12),
            int_attribute("pi.stream.character_count", 480),
            string_attribute("tau.runtime.id", "runtime-1"),
            int_attribute("pi.generation", 3),
        ]);
        let value = validated_span_json(&record("pi.stream", attributes), &resource_json, "tau")
            .expect("valid pi.stream record");
        let span_object = &value["resourceSpans"][0]["scopeSpans"][0]["spans"][0];
        assert_eq!(span_object["name"], "pi.stream");
    }

    #[test]
    fn rejects_a_pi_stream_record_with_an_extra_attribute() {
        let resource_json = json!({ "attributes": [] });
        let mut attributes = HashMap::from([
            int_attribute("pi.stream.delta_count", 12),
            int_attribute("pi.stream.character_count", 480),
            string_attribute("tau.runtime.id", "runtime-1"),
            int_attribute("pi.generation", 3),
        ]);
        attributes.insert("unexpected.key".to_string(), FrontendAttributeValue::Int(1));
        assert!(
            validated_span_json(&record("pi.stream", attributes), &resource_json, "tau").is_none()
        );
    }

    fn log_record(
        family: &str,
        attributes: HashMap<String, FrontendAttributeValue>,
    ) -> FrontendLogRecord {
        FrontendLogRecord {
            family: family.to_string(),
            time_unix_nano: "1000000000".to_string(),
            attributes,
        }
    }

    fn frontend_error_attributes(
        source: &str,
        kind: &str,
        location: &str,
    ) -> HashMap<String, FrontendAttributeValue> {
        HashMap::from([
            string_attribute("tau.error.source", source),
            string_attribute("tau.error.kind", kind),
            string_attribute("tau.error.location", location),
        ])
    }

    #[test]
    fn accepts_a_well_formed_frontend_error_record() {
        let resource_json = json!({ "attributes": [] });
        let attributes = frontend_error_attributes("window_error", "TypeError", "index.ts:10:4");
        let value = validated_log_json(
            &log_record("frontend.error", attributes),
            &resource_json,
            "tau",
        )
        .expect("valid frontend.error record");
        let log_object = &value["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0];
        assert_eq!(log_object["eventName"], "frontend.error");
        assert_eq!(log_object["severityNumber"], Severity::Error as i32);
        let persisted = log_object["attributes"].as_array().expect("attributes");
        assert!(persisted
            .iter()
            .any(|attribute| attribute["key"] == "tau.error.location"
                && attribute["value"]["stringValue"] == "index.ts:10:4"));
    }

    #[test]
    fn accepts_a_frontend_queue_overflow_health_record() {
        let resource_json = json!({ "attributes": [] });
        let attributes = HashMap::from([int_attribute("tau.telemetry.dropped_count", 3)]);
        let value = validated_log_json(
            &log_record("telemetry.health", attributes),
            &resource_json,
            "tau",
        )
        .expect("valid telemetry.health record");
        let log_object = &value["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0];
        assert_eq!(log_object["eventName"], "telemetry.queue_overflow");
        assert_eq!(log_object["severityNumber"], Severity::Warn as i32);
    }

    #[test]
    fn rejects_the_native_writer_failure_counter_from_frontend_health() {
        let resource_json = json!({ "attributes": [] });
        let attributes = HashMap::from([
            int_attribute("tau.telemetry.dropped_count", 3),
            int_attribute("tau.telemetry.failed_write_count", 1),
        ]);
        assert!(validated_log_json(
            &log_record("telemetry.health", attributes),
            &resource_json,
            "tau"
        )
        .is_none());
    }

    #[test]
    fn rejects_an_unreviewed_frontend_error_source() {
        let resource_json = json!({ "attributes": [] });
        let attributes =
            frontend_error_attributes("not-a-real-source", "TypeError", "index.ts:10:4");
        assert!(validated_log_json(
            &log_record("frontend.error", attributes),
            &resource_json,
            "tau"
        )
        .is_none());
    }

    #[test]
    fn rejects_a_forbidden_content_canary_disguised_as_a_frontend_error_kind() {
        let resource_json = json!({ "attributes": [] });
        let canary = crate::telemetry::privacy::FORBIDDEN_CONTENT_CANARIES[0].1;
        let attributes = frontend_error_attributes("console_error", canary, "index.ts:10:4");
        assert!(validated_log_json(
            &log_record("frontend.error", attributes),
            &resource_json,
            "tau"
        )
        .is_none());
    }

    #[test]
    fn rejects_a_frontend_error_record_missing_a_required_attribute() {
        let resource_json = json!({ "attributes": [] });
        let mut attributes = frontend_error_attributes("console_error", "Error", "index.ts:10:4");
        attributes.remove("tau.error.location");
        assert!(validated_log_json(
            &log_record("frontend.error", attributes),
            &resource_json,
            "tau"
        )
        .is_none());
    }

    #[test]
    fn rejects_a_log_shaped_record_for_a_family_the_frontend_does_not_own_as_a_log() {
        // `tauri.invoke` is frontend-owned as a span, never as a log: a
        // log-shaped payload naming it must still be rejected.
        let resource_json = json!({ "attributes": [] });
        assert!(validated_log_json(
            &log_record("tauri.invoke", HashMap::new()),
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

    fn to_log_value(record: &FrontendLogRecord) -> Value {
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

    #[test]
    fn ingest_persists_a_frontend_error_log_record() {
        let (telemetry, _directory) = test_telemetry();
        let attributes =
            frontend_error_attributes("unhandled_rejection", "RangeError", "main.ts:5:1");
        let records = vec![to_log_value(&log_record("frontend.error", attributes))];

        let outcome = ingest_records(&telemetry, records);

        assert_eq!(
            outcome,
            IngestOutcome {
                accepted: 1,
                rejected: 0
            }
        );
        assert_eq!(telemetry.store.read_records(Signal::Log).len(), 1);
        assert_eq!(telemetry.store.read_records(Signal::Trace).len(), 0);
    }

    #[test]
    fn ingest_dispatches_spans_and_logs_in_the_same_batch() {
        let (telemetry, _directory) = test_telemetry();
        let span = to_value(&record("tauri.invoke", command_attribute("load_workspace")));
        let log = to_log_value(&log_record(
            "frontend.error",
            frontend_error_attributes("console_error", "none", ""),
        ));

        let outcome = ingest_records(&telemetry, vec![span, log]);

        assert_eq!(
            outcome,
            IngestOutcome {
                accepted: 2,
                rejected: 0
            }
        );
        assert_eq!(telemetry.store.read_records(Signal::Trace).len(), 1);
        assert_eq!(telemetry.store.read_records(Signal::Log).len(), 1);
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
