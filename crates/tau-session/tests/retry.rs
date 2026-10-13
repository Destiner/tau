use serde_json::{json, Value};
use tau_session::session::{Controller, Effect, QueueKind, RetryReason};

fn request(effects: &[Effect], command: &str) -> Value {
    effects
        .iter()
        .find_map(|e| match e {
            Effect::Send(v) if v["type"] == command => Some(v.clone()),
            _ => None,
        })
        .unwrap()
}
fn respond(c: &mut Controller, request: &Value, data: Value) -> Vec<Effect> {
    c.handle(json!({"type":"response", "id":request["id"], "command":request["type"], "success":true, "data":data}))
}
fn ready() -> Controller {
    let mut c = Controller::new("r");
    c.bind_source("a", "/fixture/a.jsonl");
    let state = request(&c.begin_generation(1), "get_state");
    respond(
        &mut c,
        &state,
        json!({"sessionId":"a", "sessionFile":"/fixture/a.jsonl", "isStreaming":false}),
    );
    c
}
fn start() -> Value {
    json!({"type":"auto_retry_start", "attempt":2, "maxAttempts":3, "errorMessage":"503 provider-private-data"})
}

#[test]
fn retry_uses_reviewed_copy_and_never_serializes_raw_provider_content() {
    for (raw, reason) in [
        ("429 private prompt", RetryReason::RateLimit),
        ("rate-limit", RetryReason::RateLimit),
        ("503 private credential", RetryReason::Failed),
        ("503 overloaded private prompt", RetryReason::Unavailable),
        ("network timeout private prompt", RetryReason::Network),
        ("401 429 api_key=private", RetryReason::Failed),
        ("insufficient_quota 429", RetryReason::Failed),
        ("model unavailable 503", RetryReason::Failed),
        ("unknown private prompt", RetryReason::Failed),
        ("1429", RetryReason::Failed),
    ] {
        let mut c = ready();
        c.handle(
            json!({"type":"auto_retry_start", "attempt":2, "maxAttempts":3, "errorMessage":raw}),
        );
        let retry = c.state().retry.as_ref().unwrap();
        assert_eq!(retry.reason, reason);
        assert_eq!(retry.attempt, Some(2));
        assert_eq!(retry.max_attempts, Some(3));
        let serialized = serde_json::to_string(retry).unwrap();
        assert!(!serialized.contains("private"));
        assert!(!serialized.contains(raw));
        assert!(c.state().working);
    }
}

#[test]
fn retry_provenance_is_runtime_generation_fenced_and_not_summarization() {
    let mut c = ready();
    for (runtime, generation) in [("other", 1), ("r", 0), ("r", 2)] {
        c.ingest(runtime, generation, start());
        assert!(c.state().retry.is_none());
    }
    c.ingest(
        "r",
        1,
        json!({"type":"summarization_retry_scheduled", "errorMessage":"429"}),
    );
    assert!(c.state().retry.is_none());
    c.ingest("r", 1, start());
    assert!(c.state().retry.is_some());
    c.ingest("other", 1, json!({"type":"auto_retry_end"}));
    assert!(c.state().retry.is_some());
    c.ingest(
        "r",
        1,
        json!({"type":"auto_retry_end", "success":false, "finalError":"private"}),
    );
    assert!(c.state().retry.is_none());
    assert!(c.state().working, "retry-end is not run-settled");
}

#[test]
fn retry_clears_on_settle_transport_generation_and_identity_without_losing_draft() {
    for transition in ["settle", "transport", "generation", "identity", "abort"] {
        let mut c = ready();
        c.set_draft("next draft");
        c.handle(start());
        match transition {
            "settle" => {
                c.handle(json!({"type":"agent_settled"}));
            }
            "transport" => {
                c.transport_failed(1);
                c.ingest("r", 1, start());
            }
            "generation" => {
                c.begin_generation(2);
                c.ingest("r", 1, start());
            }
            "identity" => {
                let state = request(&c.refresh(), "get_state");
                respond(
                    &mut c,
                    &state,
                    json!({"sessionId":"b", "sessionFile":"/fixture/b.jsonl", "isStreaming":true}),
                );
            }
            "abort" => {
                let abort = request(&c.abort(), "abort");
                assert!(
                    c.state().retry.is_some(),
                    "Stop request is not a completed abort"
                );
                respond(&mut c, &abort, json!({}));
            }
            _ => unreachable!(),
        }
        assert!(c.state().retry.is_none(), "{transition}");
        assert_eq!(c.state().draft, "next draft");
    }
}

#[test]
fn retry_event_fences_old_idle_state_and_does_not_release_submission_ownership() {
    let mut c = ready();
    let state = request(&c.refresh(), "get_state");
    let prompt = request(&c.submit("first"), "prompt");
    c.handle(start());
    respond(
        &mut c,
        &state,
        json!({"sessionId":"a", "sessionFile":"/fixture/a.jsonl", "isStreaming":false}),
    );
    assert!(c.state().retry.is_some());
    assert!(c.state().working);
    c.handle(json!({"type":"auto_retry_end"}));
    assert!(c.submission_pending());
    assert!(c.submit_queued("second", QueueKind::Steer).is_empty());
    // Accepted ordinary work remains owned until the actual prompt acknowledgment.
    c.handle(json!({"type":"agent_start"}));
    assert!(c.submission_pending());
    respond(&mut c, &prompt, json!({"disposition":"started"}));
    assert!(!c.submission_pending());
    assert!(!c.submit_queued("second", QueueKind::Steer).is_empty());
}
