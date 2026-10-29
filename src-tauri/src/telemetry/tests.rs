use super::*;
use crate::cloud::{CloudError, Http, Request, Target};
use serde_json::{json, Value};
use std::sync::mpsc;
use std::time::Duration;

struct Capture(mpsc::Sender<Request>);
impl Http for Capture {
    fn send(&self, request: Request) -> Result<Value, CloudError> {
        let _ = self.0.send(request);
        Ok(json!({}))
    }
}
pub(super) fn fake() -> (Telemetry, mpsc::Receiver<Request>) {
    let (sender, receiver) = mpsc::channel();
    let pipeline = Arc::new(transport::Pipeline::new(Arc::new(Capture(sender))));
    let telemetry = Telemetry::new(pipeline);
    (telemetry, receiver)
}
pub(super) fn target() -> Target {
    Target {
        endpoint: "https://example.com/otlp".into(),
        instance_id: "1".into(),
        token: "test-secret".into(),
    }
}
fn metric_value(receiver: &mpsc::Receiver<Request>, name: &str) -> String {
    loop {
        let request = receiver.recv_timeout(Duration::from_secs(2)).unwrap();
        for resource in request.body["resourceMetrics"]
            .as_array()
            .into_iter()
            .flatten()
        {
            for scope in resource["scopeMetrics"].as_array().into_iter().flatten() {
                for metric in scope["metrics"].as_array().into_iter().flatten() {
                    if metric["name"] == name {
                        return metric["sum"]["dataPoints"][0]["asInt"]
                            .as_str()
                            .unwrap()
                            .into();
                    }
                }
            }
        }
    }
}
#[test]
fn metrics_do_not_record_when_disabled_or_survive_a_target_epoch() {
    let (telemetry, requests) = fake();
    telemetry.record_pi_process_start();
    assert!(telemetry.metric_pipeline.lock().unwrap().is_none());
    telemetry.pipeline.configure(Some(target()));
    telemetry.record_pi_process_start();
    telemetry.force_flush_metrics();
    assert_eq!(metric_value(&requests, "tau.pi_process.starts"), "1");
    telemetry.pipeline.configure(None);
    for _ in 0..10 {
        telemetry.record_pi_process_start();
    }
    // Drain metric collections which the background worker may have already sent.
    while requests.try_recv().is_ok() {}
    telemetry.pipeline.configure(Some(target()));
    telemetry.record_pi_process_start();
    telemetry.force_flush_metrics();
    assert_eq!(metric_value(&requests, "tau.pi_process.starts"), "1");
    telemetry.shutdown();
}
#[test]
fn native_spans_started_before_reconfiguration_are_discarded() {
    let (telemetry, requests) = fake();
    telemetry.pipeline.configure(Some(target()));
    let context =
        TraceContext::parse("00-0123456789abcdef0123456789abcdef-0123456789abcdef-01").unwrap();
    let span = telemetry.start_command_span(&context, "start_pi").unwrap();
    telemetry.pipeline.configure(None);
    telemetry.pipeline.configure(Some(target()));
    drop(span);
    assert!(requests.recv_timeout(Duration::from_millis(30)).is_err());
    telemetry.shutdown();
}
#[test]
fn test_factory_writes_no_telemetry_content_or_markers() {
    let root = tempfile::tempdir().unwrap();
    let telemetry = Telemetry::for_test(root.path().join("telemetry"));
    telemetry.record_app_started();
    telemetry.record_pi_process_start();
    telemetry.record_app_exited();
    telemetry.shutdown();
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}
