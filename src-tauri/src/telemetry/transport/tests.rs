use super::*;
use crate::cloud::{error, CloudError};
use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

struct BlockingHttp {
    first: AtomicBool,
    entered: mpsc::Sender<Request>,
    release: Mutex<mpsc::Receiver<()>>,
}
impl Http for BlockingHttp {
    fn send(&self, request: Request) -> Result<Value, CloudError> {
        let _ = self.entered.send(request);
        if self.first.swap(false, Ordering::Relaxed) {
            self.release.lock().unwrap().recv().unwrap();
        }
        Err(error("network"))
    }
}
fn target(endpoint: &str) -> Target {
    Target {
        endpoint: endpoint.into(),
        instance_id: "1".into(),
        token: "test-only".into(),
    }
}
fn append(pipeline: &Pipeline, signal: Signal, value: Value) {
    // Ingestion is intentionally nonblocking: brief writer contention may drop a record.
    for _ in 0..1000 {
        if pipeline.append(signal, value.clone()) {
            return;
        }
        std::thread::yield_now();
    }
    panic!("pipeline stayed busy");
}
#[test]
fn queue_is_bounded_reconfiguration_discards_backlog_and_delivery_never_retries() {
    let (entered, requests) = mpsc::channel();
    let (release, releases) = mpsc::channel();
    let pipeline = Pipeline::new(Arc::new(BlockingHttp {
        first: AtomicBool::new(true),
        entered,
        release: Mutex::new(releases),
    }));
    pipeline.configure(Some(target("https://old.example/otlp")));
    append(
        &pipeline,
        Signal::Log,
        json!({"resourceLogs":[{"resource":{}}]}),
    );
    assert!(requests
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .url
        .starts_with("https://old.example"));
    for _ in 0..256 {
        assert!(pipeline.append(Signal::Log, json!({"resourceLogs":[{}]})));
    }
    assert!(!pipeline.append(Signal::Log, json!({"resourceLogs":[{}]})));
    assert!(!pipeline.append(Signal::Log, json!({"resourceLogs":["x".repeat(65536)]})));
    pipeline.configure(None);
    assert!(!pipeline.is_enabled());
    assert!(!pipeline.append(Signal::Log, json!({"resourceLogs":[{}]})));
    pipeline.configure(Some(target("https://new.example/otlp")));
    append(
        &pipeline,
        Signal::Trace,
        json!({"resourceSpans":[{"resource":{}}]}),
    );
    release.send(()).unwrap();
    let request = requests.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(request.url, "https://new.example/otlp/v1/traces");
    assert_eq!(request.body["resourceSpans"].as_array().unwrap().len(), 1);
    assert!(requests.recv_timeout(Duration::from_millis(30)).is_err());
}
#[test]
fn timestamps_and_metric_generations_cannot_cross_reconfigure() {
    let (entered, _requests) = mpsc::channel();
    let (_release, releases) = mpsc::channel();
    let pipeline = Pipeline::new(Arc::new(BlockingHttp {
        first: AtomicBool::new(false),
        entered,
        release: Mutex::new(releases),
    }));
    pipeline.configure(Some(target("https://example.com")));
    let revision = pipeline.revision();
    assert!(!pipeline.accepts_timestamp("1"));
    assert!(!pipeline.append(
        Signal::Log,
        json!({"resourceLogs":[{"logRecords":[{"timeUnixNano":"1"}]}]})
    ));
    pipeline.configure(Some(target("https://example.com")));
    assert!(!pipeline.append_at(
        Signal::Metric,
        json!({"resourceMetrics":[{}]}),
        Some(revision)
    ));
}
