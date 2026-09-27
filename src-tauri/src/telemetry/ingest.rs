//! Frontend records are revalidated against the attribute catalog before
//! persistence. Ingest itself must not be traced, or sending telemetry would
//! recursively generate more telemetry.
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

/// Reject implausible durations above 24 hours, matching the frontend bound.
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

/// Frontend logs cannot choose their event name or severity; the family
/// determines both. Disjoint wire shapes allow span-first decoding.
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FrontendLogRecord {
    family: String,
    time_unix_nano: String,
    #[serde(default)]
    attributes: HashMap<String, FrontendAttributeValue>,
    /// Linked log trace/span IDs must both be present and valid, or both absent.
    #[serde(default)]
    trace_id: Option<String>,
    #[serde(default)]
    span_id: Option<String>,
}

/// Raw metric values have a disjoint wire shape from logs and spans;
/// unknown fields are rejected.
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FrontendMetricRecord {
    family: String,
    value: f64,
    time_unix_nano: String,
    #[serde(default)]
    attributes: HashMap<String, FrontendAttributeValue>,
}

/// Accept only catalog-compatible scalar attribute shapes, never objects or arrays.
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

/// Decode each record independently so a malformed item does not discard
/// the rest of the batch.
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

/// Decode disjoint span/log/metric shapes independently; reject invalid families.
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

/// Derives metrics from validated, persisted spans rather than accepting
/// a second wire format for durations or RPC outcomes.
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

/// Only validated, persisted heartbeat and health logs may update native gauges.
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

/// Bound raw frontend measurements and select only reviewed metric families.
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

/// Validation restricts this to the two reviewed raw metric families.
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

/// Fixes event name and severity by family, never by untrusted input.
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
    } else if family == attributes::ACTION_MILESTONE.name {
        Some(("action.milestone", Severity::Info))
    } else if family == attributes::FRONTEND_HEARTBEAT.name {
        Some(("frontend.heartbeat", Severity::Info))
    } else if family == attributes::FRONTEND_STATE_SUMMARY.name {
        Some(("frontend.state_summary", Severity::Info))
    } else {
        None
    }
}

fn validated_log_json(
    record: &FrontendLogRecord,
    resource_json: &Value,
    scope_name: &str,
) -> Option<Value> {
    let (event_name, severity) = fixed_log_metadata(&record.family)?;
    validate_frontend_attribute_keys(&record.family, &record.attributes)?;

    let time = canonical_timestamp(&record.time_unix_nano)?;
    // Reject an incomplete or malformed trace/span ID pair, not just the link.
    let linked_context = match (&record.trace_id, &record.span_id) {
        (Some(trace_id), Some(span_id)) => {
            if record.family != attributes::CONTROLLER_LIFECYCLE.name
                && record.family != attributes::OPERATION_CHECKPOINT.name
                && record.family != attributes::ACTION_MILESTONE.name
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

/// Rejects the entire record on validation failure; never persist a
/// partially validated span or unreviewed attributes.
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
    } else if family == attributes::ACTION_MILESTONE.name {
        Some(&[
            "tau.action.name",
            "tau.action.milestone",
            "tau.action.elapsed_ms",
        ])
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
