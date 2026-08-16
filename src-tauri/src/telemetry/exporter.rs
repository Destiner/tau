//! Converts emitted OTel log and span records into the OTLP JSON mapping
//! (<https://opentelemetry.io/docs/specs/otel/protocol/file-exporter/>) and
//! hands the resulting line to the `Store`. This is the only place that
//! understands both the OTel SDK's record types and the on-disk JSON shape;
//! `ingest.rs` reuses its value/envelope helpers for frontend-originated
//! spans so both sources produce identically shaped output.
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use opentelemetry::logs::AnyValue;
use opentelemetry::trace::{SpanKind, Status};
use opentelemetry::SpanId;
use opentelemetry_sdk::error::OTelSdkResult;
use opentelemetry_sdk::logs::{LogBatch, LogExporter, SdkLogRecord};
use opentelemetry_sdk::trace::{SpanData, SpanExporter};
use opentelemetry_sdk::Resource;
use serde_json::{json, Value};

use super::store::{Signal, Store};

pub struct JsonFileLogExporter {
    store: Arc<Store>,
    resource_json: Value,
}

impl std::fmt::Debug for JsonFileLogExporter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JsonFileLogExporter")
            .finish_non_exhaustive()
    }
}

impl JsonFileLogExporter {
    /// `resource` is captured once at construction rather than through
    /// `LogExporter::set_resource`, since Tau builds its resource before the
    /// provider exists and passing it directly avoids depending on the SDK
    /// calling that hook.
    pub fn new(store: Arc<Store>, resource: &Resource) -> Self {
        JsonFileLogExporter {
            store,
            resource_json: resource_to_json(resource),
        }
    }
}

impl LogExporter for JsonFileLogExporter {
    async fn export(&self, batch: LogBatch<'_>) -> OTelSdkResult {
        for (record, scope) in batch.iter() {
            let value = log_record_to_otlp_json(&self.resource_json, scope.name(), record);
            self.store.append(Signal::Log, value);
        }
        Ok(())
    }
}

pub struct JsonFileSpanExporter {
    store: Arc<Store>,
    resource_json: Value,
}

impl std::fmt::Debug for JsonFileSpanExporter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JsonFileSpanExporter")
            .finish_non_exhaustive()
    }
}

impl JsonFileSpanExporter {
    /// See `JsonFileLogExporter::new`: the resource is captured once here for
    /// the same reason.
    pub fn new(store: Arc<Store>, resource: &Resource) -> Self {
        JsonFileSpanExporter {
            store,
            resource_json: resource_to_json(resource),
        }
    }
}

impl SpanExporter for JsonFileSpanExporter {
    async fn export(&self, batch: Vec<SpanData>) -> OTelSdkResult {
        for span in &batch {
            let value = span_data_to_otlp_json(&self.resource_json, span);
            self.store.append(Signal::Trace, value);
        }
        Ok(())
    }
}

pub(super) fn resource_to_json(resource: &Resource) -> Value {
    let attributes: Vec<Value> = resource
        .iter()
        .map(|(key, value)| key_value_json(key.as_str(), value))
        .collect();
    json!({ "attributes": attributes })
}

fn key_value_json(key: &str, value: &opentelemetry::Value) -> Value {
    json!({ "key": key, "value": otel_value_json(value) })
}

pub(super) fn otel_value_json(value: &opentelemetry::Value) -> Value {
    match value {
        opentelemetry::Value::Bool(value) => json!({ "boolValue": value }),
        opentelemetry::Value::I64(value) => json!({ "intValue": value.to_string() }),
        opentelemetry::Value::F64(value) => json!({ "doubleValue": value }),
        opentelemetry::Value::String(value) => json!({ "stringValue": value.as_str() }),
        _ => json!({ "stringValue": "[unsupported telemetry value]" }),
    }
}

pub(super) fn any_value_json(value: &AnyValue) -> Value {
    match value {
        AnyValue::Int(value) => json!({ "intValue": value.to_string() }),
        AnyValue::Double(value) => json!({ "doubleValue": value }),
        AnyValue::String(value) => json!({ "stringValue": value.as_str() }),
        AnyValue::Boolean(value) => json!({ "boolValue": value }),
        _ => json!({ "stringValue": "[unsupported telemetry value]" }),
    }
}

fn unix_nanos_string(time: SystemTime) -> String {
    time.duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos().to_string())
        .unwrap_or_default()
}

fn log_record_to_otlp_json(
    resource_json: &Value,
    scope_name: &str,
    record: &SdkLogRecord,
) -> Value {
    let mut log_record = serde_json::Map::new();
    if let Some(timestamp) = record.timestamp() {
        log_record.insert(
            "timeUnixNano".into(),
            Value::String(unix_nanos_string(timestamp)),
        );
    }
    if let Some(observed) = record.observed_timestamp() {
        log_record.insert(
            "observedTimeUnixNano".into(),
            Value::String(unix_nanos_string(observed)),
        );
    }
    if let Some(severity_number) = record.severity_number() {
        log_record.insert("severityNumber".into(), Value::from(severity_number as i32));
        log_record.insert(
            "severityText".into(),
            Value::String(severity_number.name().to_string()),
        );
    }
    if let Some(event_name) = record.event_name() {
        log_record.insert("eventName".into(), Value::String(event_name.to_string()));
    }
    if let Some(context) = record.trace_context() {
        log_record.insert(
            "traceId".into(),
            Value::String(context.trace_id.to_string()),
        );
        log_record.insert("spanId".into(), Value::String(context.span_id.to_string()));
        if let Some(flags) = context.trace_flags {
            log_record.insert("flags".into(), Value::from(flags.to_u8()));
        }
    }
    if let Some(body) = record.body() {
        log_record.insert("body".into(), any_value_json(body));
    }
    let attributes: Vec<Value> = record
        .attributes_iter()
        .map(|(key, value)| json!({ "key": key.as_str(), "value": any_value_json(value) }))
        .collect();
    if !attributes.is_empty() {
        log_record.insert("attributes".into(), Value::Array(attributes));
    }

    json!({
        "resourceLogs": [{
            "resource": resource_json,
            "scopeLogs": [{
                "scope": { "name": scope_name },
                "logRecords": [Value::Object(log_record)],
            }],
        }],
    })
}

/// Wraps one already-built OTLP span object in the `resourceSpans` envelope.
/// Shared by native spans (below) and `ingest.rs`'s frontend-originated
/// spans, so both sources produce identically shaped `traces.jsonl` lines.
pub(super) fn wrap_span_json(resource_json: &Value, scope_name: &str, span_object: Value) -> Value {
    json!({
        "resourceSpans": [{
            "resource": resource_json,
            "scopeSpans": [{
                "scope": { "name": scope_name },
                "spans": [span_object],
            }],
        }],
    })
}

fn span_data_to_otlp_json(resource_json: &Value, span: &SpanData) -> Value {
    let mut span_object = serde_json::Map::new();
    span_object.insert(
        "traceId".into(),
        Value::String(span.span_context.trace_id().to_string()),
    );
    span_object.insert(
        "spanId".into(),
        Value::String(span.span_context.span_id().to_string()),
    );
    if span.parent_span_id != SpanId::INVALID {
        span_object.insert(
            "parentSpanId".into(),
            Value::String(span.parent_span_id.to_string()),
        );
    }
    span_object.insert("name".into(), Value::String(span.name.to_string()));
    span_object.insert("kind".into(), Value::from(span_kind_code(&span.span_kind)));
    span_object.insert(
        "startTimeUnixNano".into(),
        Value::String(unix_nanos_string(span.start_time)),
    );
    span_object.insert(
        "endTimeUnixNano".into(),
        Value::String(unix_nanos_string(span.end_time)),
    );
    let attributes: Vec<Value> = span
        .attributes
        .iter()
        .map(|attribute| key_value_json(attribute.key.as_str(), &attribute.value))
        .collect();
    if !attributes.is_empty() {
        span_object.insert("attributes".into(), Value::Array(attributes));
    }
    span_object.insert(
        "status".into(),
        json!({ "code": status_code(&span.status) }),
    );

    wrap_span_json(
        resource_json,
        span.instrumentation_scope.name(),
        Value::Object(span_object),
    )
}

fn span_kind_code(kind: &SpanKind) -> i32 {
    match kind {
        SpanKind::Internal => 1,
        SpanKind::Server => 2,
        SpanKind::Client => 3,
        SpanKind::Producer => 4,
        SpanKind::Consumer => 5,
    }
}

fn status_code(status: &Status) -> i32 {
    match status {
        Status::Unset => 0,
        Status::Ok => 1,
        Status::Error { .. } => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opentelemetry::logs::{LogRecord as _, Logger as _, LoggerProvider as _, Severity};
    use opentelemetry_sdk::logs::SdkLoggerProvider;
    use opentelemetry_sdk::Resource;

    #[test]
    fn resource_attributes_round_trip_into_json() {
        let resource = Resource::builder_empty()
            .with_attributes([opentelemetry::KeyValue::new("service.name", "tau")])
            .build();
        let json = resource_to_json(&resource);
        let attributes = json["attributes"].as_array().expect("attributes array");
        assert!(attributes
            .iter()
            .any(|entry| entry["key"] == "service.name" && entry["value"]["stringValue"] == "tau"));
    }

    #[test]
    fn log_record_conversion_carries_event_name_body_and_severity() {
        let provider = SdkLoggerProvider::builder().build();
        let logger = provider.logger("test");
        let mut record = logger.create_log_record();
        record.set_event_name("app.started");
        record.set_severity_number(Severity::Info);
        record.set_body(AnyValue::String("app.started".into()));

        let resource_json = json!({ "attributes": [] });
        // `record` here is the trait object created by `create_log_record`,
        // which for `SdkLogger` is concretely `SdkLogRecord`.
        let sdk_record: SdkLogRecord = record;
        let value = log_record_to_otlp_json(&resource_json, "tau", &sdk_record);

        let log_record = &value["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0];
        assert_eq!(log_record["eventName"], "app.started");
        assert_eq!(log_record["body"]["stringValue"], "app.started");
        assert_eq!(log_record["severityNumber"], 9);
        assert_eq!(log_record["severityText"], "INFO");
        assert_eq!(
            value["resourceLogs"][0]["scopeLogs"][0]["scope"]["name"],
            "tau"
        );
    }

    #[test]
    fn otel_value_kinds_map_to_the_otlp_json_shape() {
        assert_eq!(
            otel_value_json(&opentelemetry::Value::Bool(true)),
            json!({ "boolValue": true })
        );
        assert_eq!(
            otel_value_json(&opentelemetry::Value::I64(7)),
            json!({ "intValue": "7" })
        );
        assert_eq!(
            otel_value_json(&opentelemetry::Value::String("x".into())),
            json!({ "stringValue": "x" })
        );
    }

    #[test]
    fn any_value_kinds_map_to_the_otlp_json_shape() {
        assert_eq!(
            any_value_json(&AnyValue::Int(7)),
            json!({ "intValue": "7" })
        );
        assert_eq!(
            any_value_json(&AnyValue::Boolean(false)),
            json!({ "boolValue": false })
        );
        assert_eq!(
            any_value_json(&AnyValue::String("x".into())),
            json!({ "stringValue": "x" })
        );
    }

    #[test]
    fn unsupported_values_fail_closed_without_rendering_content() {
        let canary = super::super::privacy::FORBIDDEN_CONTENT_CANARIES[0].1;
        let value = AnyValue::ListAny(Box::new(vec![AnyValue::String(canary.into())]));
        let encoded = any_value_json(&value).to_string();
        assert_eq!(
            encoded,
            r#"{"stringValue":"[unsupported telemetry value]"}"#
        );
        assert!(!encoded.contains(canary));
    }

    fn sample_span_data(parent_span_id: opentelemetry::SpanId) -> SpanData {
        use opentelemetry::trace::{SpanContext, TraceFlags, TraceState};
        use opentelemetry::{InstrumentationScope, KeyValue, TraceId};
        use std::time::Duration;

        let start = UNIX_EPOCH + Duration::from_secs(1);
        SpanData {
            span_context: SpanContext::new(
                TraceId::from_hex("4bf92f3577b34da6a3ce929d0e0e4736").expect("trace id"),
                SpanId::from_hex("00f067aa0ba902b7").expect("span id"),
                TraceFlags::SAMPLED,
                false,
                TraceState::default(),
            ),
            parent_span_id,
            parent_span_is_remote: parent_span_id != SpanId::INVALID,
            span_kind: SpanKind::Internal,
            name: "tauri.invoke".into(),
            start_time: start,
            end_time: start + Duration::from_millis(5),
            attributes: vec![KeyValue::new("tau.invoke.command", "load_workspace")],
            dropped_attributes_count: 0,
            events: Default::default(),
            links: Default::default(),
            status: Status::Unset,
            instrumentation_scope: InstrumentationScope::builder("tau").build(),
        }
    }

    #[test]
    fn span_conversion_produces_the_otlp_trace_json_shape() {
        let span = sample_span_data(SpanId::from_hex("cafebabecafebabe").expect("parent span id"));
        let resource_json = json!({ "attributes": [] });
        let value = span_data_to_otlp_json(&resource_json, &span);

        let span_object = &value["resourceSpans"][0]["scopeSpans"][0]["spans"][0];
        assert_eq!(span_object["traceId"], "4bf92f3577b34da6a3ce929d0e0e4736");
        assert_eq!(span_object["spanId"], "00f067aa0ba902b7");
        assert_eq!(span_object["parentSpanId"], "cafebabecafebabe");
        assert_eq!(span_object["name"], "tauri.invoke");
        assert_eq!(span_object["kind"], 1);
        assert_eq!(span_object["status"]["code"], 0);
        assert_eq!(
            span_object["attributes"][0],
            json!({ "key": "tau.invoke.command", "value": { "stringValue": "load_workspace" } })
        );
        assert_eq!(
            value["resourceSpans"][0]["scopeSpans"][0]["scope"]["name"],
            "tau"
        );
    }

    #[test]
    fn span_conversion_omits_parent_span_id_for_a_root_span() {
        let span = sample_span_data(SpanId::INVALID);
        let resource_json = json!({ "attributes": [] });
        let value = span_data_to_otlp_json(&resource_json, &span);
        let span_object = &value["resourceSpans"][0]["scopeSpans"][0]["spans"][0];
        assert!(span_object.get("parentSpanId").is_none());
    }

    #[test]
    fn span_kind_and_status_map_to_otlp_integer_codes() {
        assert_eq!(span_kind_code(&SpanKind::Internal), 1);
        assert_eq!(span_kind_code(&SpanKind::Server), 2);
        assert_eq!(status_code(&Status::Unset), 0);
        assert_eq!(status_code(&Status::Ok), 1);
        assert_eq!(status_code(&Status::error("unused description")), 2);
    }
}
