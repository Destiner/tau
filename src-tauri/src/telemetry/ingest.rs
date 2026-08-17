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

/// Hard upper bound on a reported `FrontendMetricRecord` value (24 hours in
/// milliseconds), mirroring `src/lib/telemetry/metric.ts`'s own bound.
/// Neither an event-loop-lag reading nor a long-task duration can
/// genuinely reach this; it exists only to reject a corrupted or
/// nonsensical value.
const MAX_METRIC_VALUE_MS: f64 = 24.0 * 60.0 * 60.0 * 1000.0;
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
    /// Optional linked span identity (`controller.lifecycle`,
    /// `operation.checkpoint`): present only for the handful of log
    /// families a Stage 5 caller links to an active span. Both fields must
    /// be present and well-formed together, or neither is attached; see
    /// `validated_log_json`.
    #[serde(default)]
    trace_id: Option<String>,
    #[serde(default)]
    span_id: Option<String>,
}

/// The wire shape for a raw frontend metric value with no native span/log
/// counterpart to derive a metric from (event-loop lag, long-task
/// duration; see `src/lib/telemetry/metric.ts`). `value` is required and
/// rejected by `FrontendLogRecord`'s `deny_unknown_fields`, and this shape
/// lacks every field a span requires, so all three wire shapes stay
/// structurally disjoint without an explicit discriminant.
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FrontendMetricRecord {
    family: String,
    value: f64,
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

/// Tries a record as a span, then a log, then a raw metric value; a value
/// that matches none of the three shapes (or fails its family's
/// validation) is rejected. Cloning `raw` for the first two attempts is
/// cheap: batches are capped at `MAX_RECORDS_PER_CALL` small JSON objects.
fn ingest_one(telemetry: &Telemetry, raw: Value) -> bool {
    if let Ok(record) = serde_json::from_value::<FrontendSpanRecord>(raw.clone()) {
        if let Some(value) = validated_span_json(&record, &telemetry.resource_json, SCOPE_NAME) {
            telemetry.record_trace(value);
            record_span_metrics(telemetry, &record);
            return true;
        }
    }
    if let Ok(record) = serde_json::from_value::<FrontendLogRecord>(raw.clone()) {
        if let Some(value) = validated_log_json(&record, &telemetry.resource_json, SCOPE_NAME) {
            telemetry.record_log(value);
            record_log_metrics(telemetry, &record);
            return true;
        }
    }
    if let Ok(record) = serde_json::from_value::<FrontendMetricRecord>(raw) {
        if validate_metric(&record) {
            record_metric(telemetry, &record);
            return true;
        }
    }
    false
}

/// Derives duration-histogram and RPC-failure-counter metrics from an
/// already-validated, already-persisted frontend span — no separate wire
/// format needed for these: the duration is simply the span's own
/// `end - start`, and the outcome is simply its own `pi.rpc.outcome`
/// attribute. Called only after `validated_span_json` has already accepted
/// the record, so both timestamps are known well-formed; still re-parsed
/// defensively rather than assumed.
fn record_span_metrics(telemetry: &Telemetry, record: &FrontendSpanRecord) {
    let (Some(start), Some(end)) = (
        canonical_timestamp(&record.start_time_unix_nano),
        canonical_timestamp(&record.end_time_unix_nano),
    ) else {
        return;
    };
    if end.0 < start.0 {
        return;
    }
    let duration_ms = (end.0 - start.0) as f64 / 1_000_000.0;
    if duration_ms > MAX_METRIC_VALUE_MS {
        return;
    }
    telemetry.record_operation_duration(&record.family, duration_ms);
    if record.family == attributes::TAURI_INVOKE.name
        && matches!(
            record.attributes.get("tau.invoke.command"),
            Some(FrontendAttributeValue::Str(command))
                if command == "start_pi" || command == "start_pi_remote"
        )
    {
        telemetry.record_controller_start_duration(duration_ms);
    }
    if record.family == attributes::PI_RPC.name {
        if let Some(FrontendAttributeValue::Str(outcome)) = record.attributes.get("pi.rpc.outcome")
        {
            if outcome != "success" {
                telemetry.record_rpc_failure();
            }
            if outcome.starts_with("abandoned_") {
                telemetry.record_rpc_abandoned();
            }
        }
    }
}

/// Derives native gauge updates from an already-validated,
/// already-persisted frontend log — `frontend.heartbeat`'s own counters and
/// `telemetry.health`'s own drop count, both already checked against the
/// catalog by `validated_log_json`. Any other family is a no-op.
fn record_log_metrics(telemetry: &Telemetry, record: &FrontendLogRecord) {
    let int_attribute = |key: &str| -> u64 {
        match record.attributes.get(key) {
            Some(FrontendAttributeValue::Int(value)) => u64::try_from(*value).unwrap_or(0),
            _ => 0,
        }
    };
    if record.family == attributes::FRONTEND_HEARTBEAT.name {
        telemetry.record_heartbeat_gauges(
            int_attribute("tau.heartbeat.pending_rpc_count"),
            int_attribute("tau.heartbeat.controller_count"),
            int_attribute("tau.heartbeat.active_controller_count"),
            int_attribute("tau.heartbeat.runtime_count"),
            int_attribute("tau.heartbeat.queue_length"),
        );
    } else if record.family == attributes::PI_RPC_ANOMALY.name {
        telemetry.record_rpc_unmatched();
    } else if record.family == attributes::TELEMETRY_HEALTH.name
        && record
            .attributes
            .contains_key("tau.telemetry.dropped_count")
    {
        telemetry.record_telemetry_dropped_gauge(int_attribute("tau.telemetry.dropped_count"));
    }
}

/// Validates a raw frontend metric value against the catalog and this
/// module's own hard value bound. `family` selects the native instrument
/// (`record_metric`), the same way a log record's `family` selects its
/// fixed event name/severity.
fn validate_metric(record: &FrontendMetricRecord) -> bool {
    if !record.value.is_finite() || record.value < 0.0 || record.value > MAX_METRIC_VALUE_MS {
        return false;
    }
    if canonical_timestamp(&record.time_unix_nano).is_none() {
        return false;
    }
    let Some(required) = required_attributes(&record.family) else {
        return false;
    };
    if record.attributes.len() != required.len()
        || !required
            .iter()
            .all(|key| record.attributes.contains_key(*key))
    {
        return false;
    }
    record.attributes.iter().all(|(key, value)| {
        attributes::validate(&record.family, key, &value.as_otel_value()).is_ok()
    })
}

/// Records one validated raw frontend metric value into its native
/// instrument. `family` is one of the two reviewed `FrontendMetricRecord`
/// families; any other value cannot reach here since `validate_metric`
/// already rejected it via `required_attributes`.
fn record_metric(telemetry: &Telemetry, record: &FrontendMetricRecord) {
    if record.family == attributes::FRONTEND_EVENT_LOOP_LAG.name {
        let mut dimensions = Vec::with_capacity(2);
        if let Some(FrontendAttributeValue::Str(value)) =
            record.attributes.get("tau.heartbeat.visibility")
        {
            dimensions.push(opentelemetry::KeyValue::new(
                "tau.heartbeat.visibility",
                value.clone(),
            ));
        }
        if let Some(FrontendAttributeValue::Str(value)) =
            record.attributes.get("tau.heartbeat.focused")
        {
            dimensions.push(opentelemetry::KeyValue::new(
                "tau.heartbeat.focused",
                value.clone(),
            ));
        }
        telemetry.record_event_loop_lag(record.value, &dimensions);
    } else if record.family == attributes::FRONTEND_LONG_TASK.name {
        telemetry.record_long_task_duration(record.value);
    }
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
    } else if family == attributes::PI_RPC_ANOMALY.name {
        Some(("pi.rpc.response_anomaly", Severity::Warn))
    } else if family == attributes::CONTROLLER_LIFECYCLE.name {
        Some(("controller.state_transition", Severity::Info))
    } else if family == attributes::OPERATION_CHECKPOINT.name {
        Some(("operation.checkpoint", Severity::Info))
    } else if family == attributes::FRONTEND_HEARTBEAT.name {
        Some(("frontend.heartbeat", Severity::Info))
    } else if family == attributes::FRONTEND_STATE_SUMMARY.name {
        Some(("frontend.state_summary", Severity::Info))
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
    // Both fields must be present and well-formed together, or neither is
    // attached: an asymmetric or malformed pair rejects the whole record,
    // the same strictness a span's own trace/span id gets.
    let linked_context = match (&record.trace_id, &record.span_id) {
        (Some(trace_id), Some(span_id)) => {
            if record.family != attributes::CONTROLLER_LIFECYCLE.name
                && record.family != attributes::OPERATION_CHECKPOINT.name
            {
                return None;
            }
            TraceContext::parse(&format!("00-{trace_id}-{span_id}-00")).ok()?;
            Some((trace_id.clone(), span_id.clone()))
        }
        (None, None) => None,
        _ => return None,
    };

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
    if let Some((trace_id, span_id)) = linked_context {
        log_object.insert("traceId".into(), Value::String(trace_id));
        log_object.insert("spanId".into(), Value::String(span_id));
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
    } else if family == attributes::PI_RPC_ANOMALY.name {
        Some(&["pi.rpc.anomaly.kind", "pi.rpc.request_id"])
    } else if family == attributes::CONTROLLER_LIFECYCLE.name {
        Some(&[
            "tau.controller.state.before",
            "tau.controller.state.after",
            "tau.controller.transition.cause",
        ])
    } else if family == attributes::OPERATION_CHECKPOINT.name {
        Some(&["tau.operation.family", "tau.operation.name"])
    } else if family == attributes::FRONTEND_HEARTBEAT.name {
        Some(&[
            "tau.heartbeat.visibility",
            "tau.heartbeat.focused",
            "tau.heartbeat.pending_rpc_count",
            "tau.heartbeat.controller_count",
            "tau.heartbeat.active_controller_count",
            "tau.heartbeat.runtime_count",
            "tau.heartbeat.queue_length",
        ])
    } else if family == attributes::FRONTEND_STATE_SUMMARY.name {
        Some(&[
            "tau.state.controller_count",
            "tau.state.runtime_count",
            "tau.state.pending_rpc_count",
            "tau.state.notification_count",
            "tau.state.dialog_count",
            "tau.state.transcript.user_count",
            "tau.state.transcript.assistant_count",
            "tau.state.transcript.tool_count",
            "tau.state.transcript.thinking_count",
            "tau.state.transcript.error_count",
            "tau.state.draft_bucket",
            "tau.state.oldest_pending_rpc_age_ms",
        ])
    } else if family == attributes::FRONTEND_EVENT_LOOP_LAG.name {
        Some(&["tau.heartbeat.visibility", "tau.heartbeat.focused"])
    } else if family == attributes::FRONTEND_LONG_TASK.name {
        Some(&[])
    } else {
        None
    }
}

fn optional_frontend_attributes(family: &str) -> &'static [&'static str] {
    if family == attributes::TAURI_INVOKE.name {
        &["tau.invoke.outcome"]
    } else if family == attributes::OPERATION_CHECKPOINT.name {
        &["pi.rpc.request_id"]
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
            trace_id: None,
            span_id: None,
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

    /// Stage 6's comprehensive end-to-end privacy sweep: every forbidden-
    /// content canary, smuggled in as an extra field under every plausible
    /// "a bug forwarded content here" field name, is rejected outright by
    /// `deny_unknown_fields` regardless of which shape (span/log/metric) or
    /// which field name carries it — the type system rejects the whole
    /// record before any attribute-level check even runs. A clean sibling
    /// record in the same batch still succeeds, proving one bad record
    /// never poisons the rest.
    #[test]
    fn no_forbidden_content_canary_survives_ingest_via_any_unexpected_field() {
        use crate::telemetry::privacy::FORBIDDEN_CONTENT_CANARIES;

        let (telemetry, _directory) = test_telemetry();
        let leak_field_names = [
            "message",
            "prompt",
            "promptText",
            "toolResult",
            "toolArguments",
            "stderr",
            "connectionString",
            "path",
            "clipboard",
        ];

        // Each (canary, field) pair is its own small batch — well under
        // `MAX_RECORDS_PER_CALL` — paired with one clean sibling record per
        // shape, so a poisoned record's rejection is checked against not
        // collaterally rejecting a clean record in the very same call.
        let mut total_rejected = 0;
        let mut total_accepted = 0;
        for (_, canary) in FORBIDDEN_CONTENT_CANARIES {
            for field in leak_field_names {
                let mut poisoned_span =
                    to_value(&record("tauri.invoke", command_attribute("load_workspace")));
                poisoned_span
                    .as_object_mut()
                    .expect("object")
                    .insert(field.to_string(), json!(*canary));

                let mut poisoned_log = to_log_value(&log_record(
                    "frontend.error",
                    frontend_error_attributes("console_error", "Error", "a.ts:1:1"),
                ));
                poisoned_log
                    .as_object_mut()
                    .expect("object")
                    .insert(field.to_string(), json!(*canary));

                let clean_span =
                    to_value(&record("tauri.invoke", command_attribute("load_workspace")));
                let clean_log = to_log_value(&log_record(
                    "frontend.error",
                    frontend_error_attributes("console_error", "Error", "a.ts:1:1"),
                ));

                let outcome = ingest_records(
                    &telemetry,
                    vec![poisoned_span, poisoned_log, clean_span, clean_log],
                );
                assert_eq!(
                    outcome,
                    IngestOutcome {
                        accepted: 2,
                        rejected: 2
                    },
                    "canary {canary:?} smuggled through field {field:?}"
                );
                total_rejected += outcome.rejected;
                total_accepted += outcome.accepted;
            }
        }
        let expected_cases = (FORBIDDEN_CONTENT_CANARIES.len() * leak_field_names.len()) as u32;
        assert_eq!(total_rejected, expected_cases * 2);
        assert_eq!(total_accepted, expected_cases * 2);

        let persisted_traces = telemetry.store.read_records(Signal::Trace);
        let persisted_logs = telemetry.store.read_records(Signal::Log);
        assert_eq!(persisted_traces.len(), expected_cases as usize);
        assert_eq!(persisted_logs.len(), expected_cases as usize);
        for record in persisted_traces.iter().chain(persisted_logs.iter()) {
            let encoded = record.to_string();
            for (name, canary) in FORBIDDEN_CONTENT_CANARIES {
                assert!(
                    !encoded.contains(canary),
                    "a persisted record leaked the {name} canary: {encoded}"
                );
            }
        }
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

    fn metric_record(
        family: &str,
        value: f64,
        attributes: HashMap<String, FrontendAttributeValue>,
    ) -> FrontendMetricRecord {
        FrontendMetricRecord {
            family: family.to_string(),
            value,
            time_unix_nano: "1000000000".to_string(),
            attributes,
        }
    }

    fn to_metric_value(record: &FrontendMetricRecord) -> Value {
        serde_json::to_value(record).expect("serializable record")
    }

    fn heartbeat_attributes() -> HashMap<String, FrontendAttributeValue> {
        HashMap::from([
            string_attribute("tau.heartbeat.visibility", "visible"),
            string_attribute("tau.heartbeat.focused", "true"),
            int_attribute("tau.heartbeat.pending_rpc_count", 1),
            int_attribute("tau.heartbeat.controller_count", 2),
            int_attribute("tau.heartbeat.active_controller_count", 1),
            int_attribute("tau.heartbeat.runtime_count", 1),
            int_attribute("tau.heartbeat.queue_length", 0),
        ])
    }

    fn state_summary_attributes() -> HashMap<String, FrontendAttributeValue> {
        HashMap::from([
            int_attribute("tau.state.controller_count", 2),
            int_attribute("tau.state.runtime_count", 1),
            int_attribute("tau.state.pending_rpc_count", 0),
            int_attribute("tau.state.notification_count", 0),
            int_attribute("tau.state.dialog_count", 0),
            int_attribute("tau.state.transcript.user_count", 3),
            int_attribute("tau.state.transcript.assistant_count", 3),
            int_attribute("tau.state.transcript.tool_count", 0),
            int_attribute("tau.state.transcript.thinking_count", 0),
            int_attribute("tau.state.transcript.error_count", 0),
            string_attribute("tau.state.draft_bucket", "empty"),
            int_attribute("tau.state.oldest_pending_rpc_age_ms", 0),
        ])
    }

    #[test]
    fn ingest_persists_a_controller_lifecycle_transition_log() {
        let (telemetry, _directory) = test_telemetry();
        let attributes = HashMap::from([
            string_attribute("tau.controller.state.before", "idle"),
            string_attribute("tau.controller.state.after", "starting"),
            string_attribute("tau.controller.transition.cause", "controller_start"),
        ]);

        let outcome = ingest_records(
            &telemetry,
            vec![to_log_value(&log_record(
                "controller.lifecycle",
                attributes,
            ))],
        );

        assert_eq!(
            outcome,
            IngestOutcome {
                accepted: 1,
                rejected: 0
            }
        );
        let records = telemetry.store.read_records(Signal::Log);
        assert_eq!(records.len(), 1);
        let log_object = &records[0]["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0];
        assert_eq!(log_object["eventName"], "controller.state_transition");
    }

    #[test]
    fn ingest_rejects_a_controller_lifecycle_record_missing_a_required_attribute() {
        let (telemetry, _directory) = test_telemetry();
        let attributes = HashMap::from([string_attribute("tau.controller.state.before", "idle")]);

        let outcome = ingest_records(
            &telemetry,
            vec![to_log_value(&log_record(
                "controller.lifecycle",
                attributes,
            ))],
        );

        assert_eq!(
            outcome,
            IngestOutcome {
                accepted: 0,
                rejected: 1
            }
        );
    }

    #[test]
    fn ingest_links_a_controller_lifecycle_log_to_the_given_trace_and_span() {
        let (telemetry, _directory) = test_telemetry();
        let mut log = log_record(
            "controller.lifecycle",
            HashMap::from([
                string_attribute("tau.controller.state.before", "idle"),
                string_attribute("tau.controller.state.after", "starting"),
                string_attribute("tau.controller.transition.cause", "controller_start"),
            ]),
        );
        log.trace_id = Some("4bf92f3577b34da6a3ce929d0e0e4736".to_string());
        log.span_id = Some("00f067aa0ba902b7".to_string());

        let outcome = ingest_records(&telemetry, vec![to_log_value(&log)]);

        assert_eq!(outcome.accepted, 1);
        let records = telemetry.store.read_records(Signal::Log);
        let log_object = &records[0]["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0];
        assert_eq!(log_object["traceId"], "4bf92f3577b34da6a3ce929d0e0e4736");
        assert_eq!(log_object["spanId"], "00f067aa0ba902b7");
    }

    #[test]
    fn ingest_rejects_a_log_with_only_a_trace_id_and_no_span_id() {
        let (telemetry, _directory) = test_telemetry();
        let mut log = log_record(
            "controller.lifecycle",
            HashMap::from([
                string_attribute("tau.controller.state.before", "idle"),
                string_attribute("tau.controller.state.after", "starting"),
                string_attribute("tau.controller.transition.cause", "controller_start"),
            ]),
        );
        log.trace_id = Some("4bf92f3577b34da6a3ce929d0e0e4736".to_string());

        let outcome = ingest_records(&telemetry, vec![to_log_value(&log)]);

        assert_eq!(
            outcome,
            IngestOutcome {
                accepted: 0,
                rejected: 1
            }
        );
    }

    #[test]
    fn ingest_rejects_trace_linkage_on_an_unlinked_log_family() {
        let (telemetry, _directory) = test_telemetry();
        let mut log = log_record(
            "frontend.error",
            frontend_error_attributes("window_error", "Error", "main.ts:1:1"),
        );
        log.trace_id = Some("4bf92f3577b34da6a3ce929d0e0e4736".to_string());
        log.span_id = Some("00f067aa0ba902b7".to_string());

        let outcome = ingest_records(&telemetry, vec![to_log_value(&log)]);

        assert_eq!(outcome.accepted, 0);
        assert_eq!(outcome.rejected, 1);
    }

    #[test]
    fn ingest_persists_an_rpc_anomaly_and_increments_its_counter() {
        let (telemetry, _directory) = test_telemetry();
        let attributes = HashMap::from([
            string_attribute("pi.rpc.anomaly.kind", "unmatched_or_duplicate"),
            string_attribute("pi.rpc.request_id", "tau-state-1"),
            string_attribute("tau.runtime.id", "runtime-1"),
        ]);

        let outcome = ingest_records(
            &telemetry,
            vec![to_log_value(&log_record("pi.rpc.anomaly", attributes))],
        );

        assert_eq!(outcome.accepted, 1);
        let logs = telemetry.store.read_records(Signal::Log);
        assert_eq!(
            logs[0]["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0]["eventName"],
            "pi.rpc.response_anomaly"
        );
        let metrics = process_metric_records(&telemetry);
        assert!(metrics.iter().any(|record| {
            record["resourceMetrics"][0]["scopeMetrics"][0]["metrics"][0]["name"]
                == "tau.pi_rpc.unmatched_responses"
        }));
    }

    #[test]
    fn ingest_persists_an_operation_checkpoint_log() {
        let (telemetry, _directory) = test_telemetry();
        let mut log = log_record(
            "operation.checkpoint",
            HashMap::from([
                string_attribute("tau.operation.family", "pi.rpc"),
                string_attribute("tau.operation.name", "prompt"),
                string_attribute("pi.rpc.request_id", "tau-prompt-1"),
                string_attribute("tau.runtime.id", "runtime-1"),
                int_attribute("pi.generation", 1),
            ]),
        );
        log.trace_id = Some("4bf92f3577b34da6a3ce929d0e0e4736".to_string());
        log.span_id = Some("00f067aa0ba902b7".to_string());

        let outcome = ingest_records(&telemetry, vec![to_log_value(&log)]);

        assert_eq!(outcome.accepted, 1);
        let records = telemetry.store.read_records(Signal::Log);
        let log_object = &records[0]["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0];
        assert_eq!(log_object["eventName"], "operation.checkpoint");
        assert_eq!(log_object["traceId"], "4bf92f3577b34da6a3ce929d0e0e4736");
    }

    #[test]
    fn ingest_persists_a_frontend_heartbeat_log_and_its_native_gauges() {
        let (telemetry, _directory) = test_telemetry();

        let outcome = ingest_records(
            &telemetry,
            vec![to_log_value(&log_record(
                "frontend.heartbeat",
                heartbeat_attributes(),
            ))],
        );

        assert_eq!(outcome.accepted, 1);
        assert_eq!(telemetry.store.read_records(Signal::Log).len(), 1);
        telemetry.force_flush_metrics();
        let metrics = telemetry.store.read_records(Signal::Metric);
        assert!(metrics.iter().any(|record| {
            record["resourceMetrics"][0]["scopeMetrics"][0]["metrics"][0]["name"]
                == "tau.telemetry.pending_rpc_count"
        }));
    }

    #[test]
    fn ingest_persists_a_frontend_state_summary_log() {
        let (telemetry, _directory) = test_telemetry();

        let outcome = ingest_records(
            &telemetry,
            vec![to_log_value(&log_record(
                "frontend.state_summary",
                state_summary_attributes(),
            ))],
        );

        assert_eq!(outcome.accepted, 1);
        let records = telemetry.store.read_records(Signal::Log);
        let log_object = &records[0]["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0];
        assert_eq!(log_object["eventName"], "frontend.state_summary");
        let encoded = log_object.to_string();
        // Content-free by construction: nothing here is transcript or draft
        // text, only the reviewed counts/bucket attributes.
        assert!(!encoded.contains("tau-canary"));
    }

    /// Stage 6's state-summary size measurement: with every count at its
    /// largest plausible magnitude, the serialized record still fits
    /// comfortably in a small bound — proof that its size is driven by its
    /// fixed *attribute count* (12), not by how much state it summarizes, so
    /// a workspace with thousands of controllers costs the same few hundred
    /// bytes as one with none.
    #[test]
    fn frontend_state_summary_serializes_to_a_small_bounded_size_regardless_of_counts() {
        let resource_json = json!({ "attributes": [] });
        let mut attributes = state_summary_attributes();
        for (key, value) in &mut attributes {
            if let FrontendAttributeValue::Int(count) = value {
                *count = if key == "tau.state.oldest_pending_rpc_age_ms" {
                    86_400_000
                } else {
                    999_999_999
                };
            }
        }
        let mut log = log_record("frontend.state_summary", attributes);
        log.trace_id = None;
        log.span_id = None;

        let value =
            validated_log_json(&log, &resource_json, "tau").expect("valid state summary record");
        let serialized = value.to_string();

        assert!(
            serialized.len() < 2048,
            "frontend.state_summary serialized to {} bytes at worst-case counts, expected < 2048",
            serialized.len()
        );
    }

    #[test]
    fn ingest_persists_an_event_loop_lag_metric_never_a_log() {
        let (telemetry, _directory) = test_telemetry();
        let attributes = HashMap::from([
            string_attribute("tau.heartbeat.visibility", "hidden"),
            string_attribute("tau.heartbeat.focused", "false"),
        ]);

        let outcome = ingest_records(
            &telemetry,
            vec![to_metric_value(&metric_record(
                "frontend.event_loop_lag",
                42.0,
                attributes,
            ))],
        );

        assert_eq!(
            outcome,
            IngestOutcome {
                accepted: 1,
                rejected: 0
            }
        );
        assert_eq!(telemetry.store.read_records(Signal::Log).len(), 0);
        let metrics = process_metric_records(&telemetry);
        let metric = metrics
            .iter()
            .find(|record| {
                record["resourceMetrics"][0]["scopeMetrics"][0]["metrics"][0]["name"]
                    == "tau.frontend.event_loop_lag"
            })
            .expect("lag histogram");
        assert_eq!(
            metric["resourceMetrics"][0]["scopeMetrics"][0]["metrics"][0]["histogram"]
                ["dataPoints"][0]["sum"],
            42.0
        );
    }

    #[test]
    fn ingest_persists_a_long_task_metric_with_no_attributes() {
        let (telemetry, _directory) = test_telemetry();

        let outcome = ingest_records(
            &telemetry,
            vec![to_metric_value(&metric_record(
                "frontend.long_task",
                90.0,
                HashMap::new(),
            ))],
        );

        assert_eq!(
            outcome,
            IngestOutcome {
                accepted: 1,
                rejected: 0
            }
        );
        let metrics = process_metric_records(&telemetry);
        assert!(metrics.iter().any(|record| {
            record["resourceMetrics"][0]["scopeMetrics"][0]["metrics"][0]["name"]
                == "tau.frontend.long_task.duration"
        }));
    }

    #[test]
    fn ingest_rejects_a_metric_value_beyond_the_hard_bound() {
        let (telemetry, _directory) = test_telemetry();

        let outcome = ingest_records(
            &telemetry,
            vec![to_metric_value(&metric_record(
                "frontend.long_task",
                MAX_METRIC_VALUE_MS + 1.0,
                HashMap::new(),
            ))],
        );

        assert_eq!(
            outcome,
            IngestOutcome {
                accepted: 0,
                rejected: 1
            }
        );
    }

    #[test]
    fn ingest_rejects_a_negative_or_non_finite_metric_value() {
        let (telemetry, _directory) = test_telemetry();

        let outcome = ingest_records(
            &telemetry,
            vec![
                to_metric_value(&metric_record("frontend.long_task", -1.0, HashMap::new())),
                to_metric_value(&metric_record(
                    "frontend.long_task",
                    f64::NAN,
                    HashMap::new(),
                )),
                to_metric_value(&metric_record(
                    "frontend.long_task",
                    f64::INFINITY,
                    HashMap::new(),
                )),
            ],
        );

        assert_eq!(
            outcome,
            IngestOutcome {
                accepted: 0,
                rejected: 3
            }
        );
    }

    #[test]
    fn ingest_rejects_an_event_loop_lag_record_missing_its_required_dimensions() {
        let (telemetry, _directory) = test_telemetry();

        let outcome = ingest_records(
            &telemetry,
            vec![to_metric_value(&metric_record(
                "frontend.event_loop_lag",
                10.0,
                HashMap::new(),
            ))],
        );

        assert_eq!(
            outcome,
            IngestOutcome {
                accepted: 0,
                rejected: 1
            }
        );
    }

    #[test]
    fn ingest_rejects_high_cardinality_context_on_a_metric_record() {
        let (telemetry, _directory) = test_telemetry();
        let attributes = HashMap::from([
            string_attribute("tau.heartbeat.visibility", "visible"),
            string_attribute("tau.heartbeat.focused", "true"),
            string_attribute("tau.session.id", "session-1"),
        ]);

        let outcome = ingest_records(
            &telemetry,
            vec![to_metric_value(&metric_record(
                "frontend.event_loop_lag",
                10.0,
                attributes,
            ))],
        );

        assert_eq!(outcome.accepted, 0);
        assert_eq!(outcome.rejected, 1);
    }

    #[test]
    fn record_span_metrics_derives_a_duration_histogram_from_an_ingested_span() {
        let (telemetry, _directory) = test_telemetry();

        let outcome = ingest_records(
            &telemetry,
            vec![to_value(&record(
                "tauri.invoke",
                command_attribute("load_workspace"),
            ))],
        );

        assert_eq!(outcome.accepted, 1);
        let metrics = process_metric_records(&telemetry);
        assert!(metrics.iter().any(|record| {
            record["resourceMetrics"][0]["scopeMetrics"][0]["metrics"][0]["name"]
                == "tau.invoke.duration"
        }));
    }

    #[test]
    fn record_span_metrics_counts_a_non_success_pi_rpc_outcome_as_a_failure() {
        let (telemetry, _directory) = test_telemetry();
        let attributes = HashMap::from([
            string_attribute("pi.rpc.method", "prompt"),
            string_attribute("pi.rpc.request_id", "tau-prompt-1"),
            string_attribute("pi.rpc.outcome", "timeout"),
            string_attribute("tau.runtime.id", "runtime-1"),
            int_attribute("pi.generation", 1),
        ]);

        let outcome = ingest_records(&telemetry, vec![to_value(&record("pi.rpc", attributes))]);

        assert_eq!(outcome.accepted, 1);
        let metrics = process_metric_records(&telemetry);
        let failures = metrics
            .iter()
            .find(|record| {
                record["resourceMetrics"][0]["scopeMetrics"][0]["metrics"][0]["name"]
                    == "tau.pi_rpc.failures"
            })
            .expect("failure counter");
        assert_eq!(
            failures["resourceMetrics"][0]["scopeMetrics"][0]["metrics"][0]["sum"]["dataPoints"][0]
                ["asInt"],
            "1"
        );
    }

    #[test]
    fn record_span_metrics_counts_abandoned_requests_separately() {
        let (telemetry, _directory) = test_telemetry();
        let attributes = HashMap::from([
            string_attribute("pi.rpc.method", "prompt"),
            string_attribute("pi.rpc.request_id", "tau-prompt-1"),
            string_attribute("pi.rpc.outcome", "abandoned_process_exit"),
            string_attribute("tau.runtime.id", "runtime-1"),
            int_attribute("pi.generation", 1),
        ]);

        let outcome = ingest_records(&telemetry, vec![to_value(&record("pi.rpc", attributes))]);

        assert_eq!(outcome.accepted, 1);
        let metrics = process_metric_records(&telemetry);
        assert!(metrics.iter().any(|record| {
            record["resourceMetrics"][0]["scopeMetrics"][0]["metrics"][0]["name"]
                == "tau.pi_rpc.abandoned"
        }));
    }

    fn process_metric_records(telemetry: &Telemetry) -> Vec<Value> {
        telemetry.force_flush_metrics();
        telemetry.store.read_records(Signal::Metric)
    }

    /// Stage 6's completion criterion in test form: a representative mix of
    /// every signal source (a frontend action → invoke → RPC span chain, a
    /// linked controller-lifecycle transition, native process lifecycle,
    /// app start/exit, and an uncorrelated frontend error) ingested into one
    /// store, then reconstructed three different ways — by trace, by
    /// session, and globally with no session at all — the way a person
    /// investigating an issue from an approximate time actually would.
    #[test]
    fn a_time_window_reconstructs_representative_global_and_session_bound_chronology() {
        let (telemetry, _directory) = test_telemetry();
        const TRACE_ID: &str = "4bf92f3577b34da6a3ce929d0e0e4736";
        const ACTION_SPAN: &str = "aaaaaaaaaaaaaaaa";
        const INVOKE_SPAN: &str = "bbbbbbbbbbbbbbbb";
        const RPC_SPAN: &str = "cccccccccccccccc";

        telemetry.record_app_started();

        // The action span: a real user gesture, carrying session/controller
        // context so it is also session-findable.
        let mut action = record(
            "ui.action",
            HashMap::from([
                string_attribute("tau.action.name", "session.select"),
                string_attribute("tau.session.id", "session-1"),
                string_attribute("tau.controller.id", "controller-1"),
            ]),
        );
        action.trace_id = TRACE_ID.to_string();
        action.span_id = ACTION_SPAN.to_string();

        // Its native `tauri.invoke` child.
        let mut invoke = record("tauri.invoke", command_attribute("set_active_session"));
        invoke.trace_id = TRACE_ID.to_string();
        invoke.span_id = INVOKE_SPAN.to_string();
        invoke.parent_span_id = Some(ACTION_SPAN.to_string());

        // A Pi RPC child sharing the same runtime as the native process
        // lifecycle log below — the process↔RPC correlation.
        let mut rpc = record(
            "pi.rpc",
            HashMap::from([
                string_attribute("pi.rpc.method", "get_state"),
                string_attribute("pi.rpc.request_id", "tau-state-1"),
                string_attribute("pi.rpc.outcome", "success"),
                string_attribute("tau.runtime.id", "runtime-1"),
                int_attribute("pi.generation", 1),
            ]),
        );
        rpc.trace_id = TRACE_ID.to_string();
        rpc.span_id = RPC_SPAN.to_string();
        rpc.parent_span_id = Some(ACTION_SPAN.to_string());

        // The named state transition the action's RPC caused, linked to the
        // action's own span and carrying the same session/controller/
        // runtime context.
        let mut transition = log_record(
            "controller.lifecycle",
            HashMap::from([
                string_attribute("tau.controller.state.before", "idle"),
                string_attribute("tau.controller.state.after", "ready"),
                string_attribute("tau.controller.transition.cause", "get_state_response"),
                string_attribute("tau.session.id", "session-1"),
                string_attribute("tau.controller.id", "controller-1"),
                string_attribute("tau.runtime.id", "runtime-1"),
            ]),
        );
        transition.trace_id = Some(TRACE_ID.to_string());
        transition.span_id = Some(ACTION_SPAN.to_string());

        let outcome = ingest_records(
            &telemetry,
            vec![
                to_value(&action),
                to_value(&invoke),
                to_value(&rpc),
                to_log_value(&transition),
            ],
        );
        assert_eq!(
            outcome,
            IngestOutcome {
                accepted: 4,
                rejected: 0
            }
        );

        // Native process lifecycle, sharing the RPC's runtime id but no
        // trace — process lifetime is not itself a span in this design.
        telemetry.record_process_lifecycle("pi.process.started", "runtime-1", Some(1), None, &[]);

        // An uncorrelated frontend error: global, undetected-issue evidence
        // with no session at all.
        let error = log_record(
            "frontend.error",
            frontend_error_attributes("window_error", "TypeError", "app.ts:1:1"),
        );
        ingest_records(&telemetry, vec![to_log_value(&error)]);

        telemetry.record_app_exited();

        let traces = telemetry.store.read_records(Signal::Trace);
        let logs = telemetry.store.read_records(Signal::Log);

        // 1. By trace: the action → invoke → RPC chain plus the transition it
        //    caused reconstruct as one connected chronology.
        let same_trace_spans: Vec<&Value> = traces
            .iter()
            .filter(|record| {
                record["resourceSpans"][0]["scopeSpans"][0]["spans"][0]["traceId"] == TRACE_ID
            })
            .collect();
        assert_eq!(same_trace_spans.len(), 3, "action, invoke, and rpc spans");
        let child_parents: Vec<&Value> = same_trace_spans
            .iter()
            .map(|record| &record["resourceSpans"][0]["scopeSpans"][0]["spans"][0]["parentSpanId"])
            .filter(|parent| !parent.is_null())
            .collect();
        assert!(
            child_parents.iter().all(|parent| **parent == ACTION_SPAN),
            "both children must resolve back to the action span"
        );
        let linked_transition = logs
            .iter()
            .find(|record| {
                record["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0]["traceId"] == TRACE_ID
            })
            .expect("the transition log links back to the same trace");
        assert_eq!(
            linked_transition["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0]["eventName"],
            "controller.state_transition"
        );

        // 2. By session: the action span and the transition log both carry
        //    `tau.session.id`, surfacing the session-bound slice without
        //    needing the trace id at all.
        let session_bound_logs = logs.iter().filter(|record| {
            record["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0]["attributes"]
                .as_array()
                .is_some_and(|attributes| {
                    attributes.iter().any(|attribute| {
                        attribute["key"] == "tau.session.id"
                            && attribute["value"]["stringValue"] == "session-1"
                    })
                })
        });
        assert_eq!(session_bound_logs.count(), 1, "the linked transition log");

        // 3. Process↔RPC correlation: the RPC span and the native process
        //    lifecycle log share `tau.runtime.id`, without any session.
        let process_log = logs
            .iter()
            .find(|record| {
                record["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0]["eventName"]
                    == "pi.process.started"
            })
            .expect("native process lifecycle log");
        let runtime_id_of = |value: &Value| -> Option<String> {
            value["attributes"]
                .as_array()?
                .iter()
                .find_map(|attribute| {
                    (attribute["key"] == "tau.runtime.id")
                        .then(|| {
                            attribute["value"]["stringValue"]
                                .as_str()
                                .map(str::to_string)
                        })
                        .flatten()
                })
        };
        let rpc_span_object = &traces
            .iter()
            .find(|record| {
                record["resourceSpans"][0]["scopeSpans"][0]["spans"][0]["spanId"] == RPC_SPAN
            })
            .expect("rpc span")["resourceSpans"][0]["scopeSpans"][0]["spans"][0];
        assert_eq!(
            runtime_id_of(&process_log["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0]),
            runtime_id_of(rpc_span_object),
        );

        // 4. Global, undetected-issue evidence needs no session at all:
        //    app.started/app.exited/frontend.error are all present
        //    unconditionally.
        let event_names: Vec<&Value> = logs
            .iter()
            .map(|record| &record["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0]["eventName"])
            .collect();
        assert!(event_names.iter().any(|name| **name == "app.started"));
        assert!(event_names.iter().any(|name| **name == "app.exited"));
        assert!(event_names.iter().any(|name| **name == "frontend.error"));
    }
}
