use serde_json::{json, Value};
use tau_session::session::{Controller, Effect, QueueKind, REQUEST_TIMEOUT_MS};

fn request(e: &[Effect], typ: &str) -> Value {
    e.iter()
        .find_map(|e| match e {
            Effect::Send(v) if v["type"] == typ => Some(v.clone()),
            _ => None,
        })
        .unwrap()
}
fn state(name: &str, working: bool) -> Value {
    json!({"sessionId":"a","sessionFile":"/fixture/a.jsonl","sessionName":name,"isStreaming":working,"thinkingLevel":"high"})
}
fn response(c: &mut Controller, r: &Value, d: Value) -> Vec<Effect> {
    c.handle(json!({"type":"response","id":r["id"],"command":r["type"],"success":true,"data":d}))
}
fn ready() -> Controller {
    let mut c = Controller::new("r");
    c.bind_source("a", "/fixture/a.jsonl");
    let bootstrap = c.begin_generation(1);
    let e = response(
        &mut c,
        &request(&bootstrap, "get_state"),
        state("initial", false),
    );
    response(&mut c, &request(&e, "get_messages"), json!({"messages":[]}));
    c
}
#[test]
fn old_barrier_can_prove_durability_without_overwriting_newer_name_or_model() {
    let mut c = ready();
    let b = request(
        &c.handle(
            json!({"type":"message_end","message":{"role":"assistant","content":[],"timestamp":1}}),
        ),
        "get_state",
    );
    let s = request(&c.refresh(), "get_state");
    let mut current = state("newer", false);
    current["model"] = json!({"provider":"fixture","id":"newer"});
    response(&mut c, &s, current);
    let e = response(&mut c, &b, state("older", false));
    assert!(e
        .iter()
        .any(|e| matches!(e,Effect::RegisterSession{id,..} if id=="a")));
    assert_eq!(c.state().identity.name, "newer");
    assert_eq!(c.state().model["id"], "newer");
}
#[test]
fn settled_before_prompt_ack_cannot_become_working_again() {
    let mut c = ready();
    let p = request(&c.submit("fast"), "prompt");
    c.handle(json!({"type":"agent_start"}));
    c.handle(json!({"type":"agent_settled"}));
    response(&mut c, &p, json!({"disposition":"started"}));
    assert!(!c.state().working);
}
#[test]
fn stop_ack_does_not_finalize_a_new_queued_run() {
    let mut c = ready();
    c.handle(json!({"type":"agent_start"}));
    let a = request(&c.abort(), "abort");
    c.handle(json!({"type":"agent_settled"}));
    c.handle(json!({"type":"agent_start"}));
    c.handle(
        json!({"type":"message_start","message":{"role":"assistant","timestamp":2,"content":[]}}),
    );
    c.handle(json!({"type":"message_update","assistantMessageEvent":{"type":"text_delta","contentIndex":0,"delta":"new run"}}));
    response(&mut c, &a, Value::Null);
    assert!(c.state().working);
    assert!(c.state().transcript[0].activity);
    assert!(!c.state().transcript[0].interrupted);
}
#[test]
fn equal_user_timestamps_do_not_deduplicate_distinct_events_or_rekey_history() {
    let mut c = ready();
    for _ in 0..2 {
        c.handle(json!({"type":"message_start","message":{"role":"user","content":"same","timestamp":1}}));
    }
    assert_eq!(c.state().transcript.len(), 2);
    let ids: Vec<_> = c.state().transcript.iter().map(|r| r.id.clone()).collect();
    assert_ne!(ids[0], ids[1]);
    let s = request(&c.refresh(), "get_state");
    let e = response(&mut c, &s, state("initial", false));
    response(
        &mut c,
        &request(&e, "get_messages"),
        json!({"messages":[{"role":"user","content":"same","timestamp":1},{"role":"user","content":"same","timestamp":1}]}),
    );
    assert_eq!(
        c.state()
            .transcript
            .iter()
            .map(|r| r.id.clone())
            .collect::<Vec<_>>(),
        ids
    );
}
#[test]
fn rejected_repeat_during_queue_preparation_does_not_break_the_first_request() {
    let mut c = ready();
    c.handle(json!({"type":"agent_start"}));
    let s = request(
        &c.submit_queued("first", QueueKind::Steer),
        "set_steering_mode",
    );
    let f = request(&response(&mut c, &s, Value::Null), "set_follow_up_mode");
    assert!(c.submit_queued("second", QueueKind::Steer).is_empty());
    let p = request(&response(&mut c, &f, Value::Null), "prompt");
    assert_eq!(p["message"], "first");
}
#[test]
fn accepted_but_no_run_is_bounded_and_transport_loss_fences_late_output() {
    let mut c = ready();
    let p = request(&c.submit("accepted"), "prompt");
    response(&mut c, &p, json!({"disposition":"started"}));
    c.tick(REQUEST_TIMEOUT_MS);
    assert!(!c.state().working);
    assert_eq!(c.state().recovered_drafts, vec!["accepted"]);
    c.transport_failed(1);
    c.ingest(
        "r",
        1,
        json!({"type":"message_start","message":{"role":"user","content":"late","timestamp":3}}),
    );
    assert!(!c.state().transcript.iter().any(|r| r.text == "late"));
}
#[test]
fn empty_path_never_registers_even_after_completed_assistant() {
    let mut c = Controller::new("r");
    let b = c.begin_generation(1);
    let e = response(
        &mut c,
        &request(&b, "get_state"),
        json!({"sessionId":"memory","isStreaming":false}),
    );
    response(&mut c, &request(&e, "get_messages"), json!({"messages":[]}));
    let barrier = request(
        &c.handle(
            json!({"type":"message_end","message":{"role":"assistant","content":[],"timestamp":1}}),
        ),
        "get_state",
    );
    let e = response(
        &mut c,
        &barrier,
        json!({"sessionId":"memory","isStreaming":false}),
    );
    assert!(!e
        .iter()
        .any(|e| matches!(e, Effect::RegisterSession { .. })));
    assert!(!c.state().durable);
}
