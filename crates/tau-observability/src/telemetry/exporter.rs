//! Convert SDK logs/spans and frontend spans to the same OTLP JSONL mapping:
//! <https://opentelemetry.io/docs/specs/otel/protocol/file-exporter/>.
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use opentelemetry::logs::AnyValue;
use opentelemetry::trace::{SpanKind, Status};
use opentelemetry::{KeyValue, SpanId};
use opentelemetry_sdk::error::OTelSdkResult;
use opentelemetry_sdk::logs::{LogBatch, LogExporter, SdkLogRecord};
use opentelemetry_sdk::metrics::data::{self, Histogram};
use opentelemetry_sdk::metrics::exporter::PushMetricExporter;
use opentelemetry_sdk::metrics::Temporality;
use opentelemetry_sdk::trace::{SpanData, SpanExporter};
use opentelemetry_sdk::Resource;
use serde_json::{json, Value};

use super::store::{Signal, Store};

pub struct JsonFileLogExporter {
    store: Arc<Store>,
    resource_json: Value,
    /// Optional development target; absent without the feature or endpoint, so ordinary builds
    /// never send.
    #[cfg(feature = "otlp_export")]
    otlp: Option<Arc<super::otlp_export::OtlpTarget>>,
}

impl std::fmt::Debug for JsonFileLogExporter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JsonFileLogExporter")
            .finish_non_exhaustive()
    }
}

impl JsonFileLogExporter {
    /// Capture resource at construction rather than depending on the SDK’s `set_resource` hook.
    pub fn new(store: Arc<Store>, resource: &Resource) -> Self {
        JsonFileLogExporter {
            store,
            resource_json: resource_to_json(resource),
            #[cfg(feature = "otlp_export")]
            otlp: super::otlp_export::OtlpTarget::from_env(
                "OTEL_EXPORTER_OTLP_LOGS_ENDPOINT",
                "/v1/logs",
            )
            .map(Arc::new),
        }
    }
}

impl LogExporter for JsonFileLogExporter {
    async fn export(&self, batch: LogBatch<'_>) -> OTelSdkResult {
        // Gate before optional network export as well as disk persistence; disabling the store
        // alone is insufficient.
        if !self.store.is_enabled() {
            return Ok(());
        }
        for (record, scope) in batch.iter() {
            let value = log_record_to_otlp_json(&self.resource_json, scope.name(), record);
            #[cfg(feature = "otlp_export")]
            if let Some(target) = &self.otlp {
                target.send(&value);
            }
            self.store.append(Signal::Log, value);
        }
        Ok(())
    }
}

pub struct JsonFileSpanExporter {
    store: Arc<Store>,
    resource_json: Value,
    #[cfg(feature = "otlp_export")]
    otlp: Option<Arc<super::otlp_export::OtlpTarget>>,
}

impl std::fmt::Debug for JsonFileSpanExporter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JsonFileSpanExporter")
            .finish_non_exhaustive()
    }
}

impl JsonFileSpanExporter {
    pub fn new(store: Arc<Store>, resource: &Resource) -> Self {
        JsonFileSpanExporter {
            store,
            resource_json: resource_to_json(resource),
            #[cfg(feature = "otlp_export")]
            otlp: super::otlp_export::OtlpTarget::from_env(
                "OTEL_EXPORTER_OTLP_TRACES_ENDPOINT",
                "/v1/traces",
            )
            .map(Arc::new),
        }
    }
}

impl SpanExporter for JsonFileSpanExporter {
    async fn export(&self, batch: Vec<SpanData>) -> OTelSdkResult {
        if !self.store.is_enabled() {
            return Ok(());
        }
        for span in &batch {
            let value = span_data_to_otlp_json(&self.resource_json, span);
            #[cfg(feature = "otlp_export")]
            if let Some(target) = &self.otlp {
                target.send(&value);
            }
            self.store.append(Signal::Trace, value);
        }
        Ok(())
    }
}

/// One self-contained OTLP JSONL record per metric instrument per periodic collection.
pub struct JsonFileMetricExporter {
    store: Arc<Store>,
    resource_json: Value,
    #[cfg(feature = "otlp_export")]
    otlp: Option<Arc<super::otlp_export::OtlpTarget>>,
}

impl std::fmt::Debug for JsonFileMetricExporter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JsonFileMetricExporter")
            .finish_non_exhaustive()
    }
}

impl JsonFileMetricExporter {
    pub fn new(store: Arc<Store>, resource: &Resource) -> Self {
        JsonFileMetricExporter {
            store,
            resource_json: resource_to_json(resource),
            #[cfg(feature = "otlp_export")]
            otlp: super::otlp_export::OtlpTarget::from_env(
                "OTEL_EXPORTER_OTLP_METRICS_ENDPOINT",
                "/v1/metrics",
            )
            .map(Arc::new),
        }
    }
}

impl PushMetricExporter for JsonFileMetricExporter {
    async fn export(&self, metrics: &data::ResourceMetrics) -> OTelSdkResult {
        if !self.store.is_enabled() {
            return Ok(());
        }
        for scope_metrics in metrics.scope_metrics() {
            for metric in scope_metrics.metrics() {
                let value =
                    metric_to_otlp_json(&self.resource_json, scope_metrics.scope().name(), metric);
                #[cfg(feature = "otlp_export")]
                if let Some(target) = &self.otlp {
                    target.send(&value);
                }
                self.store.append(Signal::Metric, value);
            }
        }
        Ok(())
    }

    fn force_flush(&self) -> OTelSdkResult {
        Ok(())
    }

    fn shutdown_with_timeout(&self, _timeout: std::time::Duration) -> OTelSdkResult {
        Ok(())
    }

    /// Cumulative points keep each append-only line meaningful without replaying history.
    fn temporality(&self) -> Temporality {
        Temporality::Cumulative
    }
}

fn attributes_json<'a>(attributes: impl Iterator<Item = &'a KeyValue>) -> Vec<Value> {
    attributes
        .map(|key_value| key_value_json(key_value.key.as_str(), &key_value.value))
        .collect()
}

fn histogram_data_points_json(histogram: &Histogram<f64>) -> Vec<Value> {
    histogram
        .data_points()
        .map(|point| {
            json!({
                "attributes": attributes_json(point.attributes()),
                "timeUnixNano": unix_nanos_string(histogram.time()),
                "count": point.count().to_string(),
                "sum": point.sum(),
                "bucketCounts": point.bucket_counts().map(|count| count.to_string()).collect::<Vec<_>>(),
                "explicitBounds": point.bounds().collect::<Vec<_>>(),
            })
        })
        .collect()
}

/// Wrap supported histogram/sum/gauge data in `resourceMetrics`; unknown shapes degrade without
/// panicking.
fn metric_to_otlp_json(resource_json: &Value, scope_name: &str, metric: &data::Metric) -> Value {
    let shape = match metric.data() {
        data::AggregatedMetrics::F64(data::MetricData::Histogram(histogram)) => json!({
            "histogram": {
                "aggregationTemporality": 2,
                "dataPoints": histogram_data_points_json(histogram),
            }
        }),
        data::AggregatedMetrics::U64(data::MetricData::Sum(sum)) => json!({
            "sum": {
                "aggregationTemporality": 2,
                "isMonotonic": sum.is_monotonic(),
                "dataPoints": sum
                    .data_points()
                    .map(|point| json!({
                        "attributes": attributes_json(point.attributes()),
                        "timeUnixNano": unix_nanos_string(sum.time()),
                        "asInt": point.value().to_string(),
                    }))
                    .collect::<Vec<_>>(),
            }
        }),
        data::AggregatedMetrics::U64(data::MetricData::Gauge(gauge)) => json!({
            "gauge": {
                "dataPoints": gauge
                    .data_points()
                    .map(|point| json!({
                        "attributes": attributes_json(point.attributes()),
                        "timeUnixNano": unix_nanos_string(gauge.time()),
                        "asInt": point.value().to_string(),
                    }))
                    .collect::<Vec<_>>(),
            }
        }),
        _ => json!({}),
    };

    let mut metric_object = serde_json::Map::new();
    metric_object.insert("name".into(), Value::String(metric.name().to_string()));
    metric_object.insert("unit".into(), Value::String(metric.unit().to_string()));
    if let Value::Object(fields) = shape {
        metric_object.extend(fields);
    }

    json!({
        "resourceMetrics": [{
            "resource": resource_json,
            "scopeMetrics": [{
                "scope": { "name": scope_name },
                "metrics": [Value::Object(metric_object)],
            }],
        }],
    })
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

pub(super) fn unix_nanos_string(time: SystemTime) -> String {
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

    wrap_log_json(resource_json, scope_name, Value::Object(log_record))
}

pub(super) fn wrap_log_json(resource_json: &Value, scope_name: &str, log_record: Value) -> Value {
    json!({
        "resourceLogs": [{
            "resource": resource_json,
            "scopeLogs": [{
                "scope": { "name": scope_name },
                "logRecords": [log_record],
            }],
        }],
    })
}

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
