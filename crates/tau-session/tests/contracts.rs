use serde_json::{json, Value};
use tau_session::session::{
    Controller, Effect, ExtensionResponse, QueueKind, Status, REQUEST_TIMEOUT_MS,
};

fn command(effects: &[Effect], typ: &str) -> Value {
    effects
        .iter()
        .find_map(|e| match e {
            Effect::Send(v) if v["type"] == typ => Some(v.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing {typ}: {effects:?}"))
}
fn reply(c: &mut Controller, request: &Value, data: Value) -> Vec<Effect> {
    c.handle(json!({"type":"response","id":request["id"],"command":request["type"],"success":true,"data":data}))
}
fn identity(id: &str, streaming: bool) -> Value {
    json!({"sessionId":id,"sessionFile":format!("/fixture/{id}.jsonl"),"sessionName":"Work","isStreaming":streaming,
        "model":{"provider":"fixture","id":"alpha","name":"Alpha","reasoning":true},"thinkingLevel":"high",
        "steeringMode":"all","followUpMode":"one-at-a-time"})
}
fn ready() -> Controller {
    let mut c = Controller::new("runtime");
    c.bind_source("a", "/fixture/a.jsonl");
    let bootstrap = c.begin_generation(1);
    let effects = reply(
        &mut c,
        &command(&bootstrap, "get_state"),
        identity("a", false),
    );
    reply(
        &mut c,
        &command(&effects, "get_messages"),
        json!({"messages":[]}),
    );
    reply(
        &mut c,
        &command(&bootstrap, "get_commands"),
        json!({"commands":[{"name":"usage","source":"extension"}]}),
    );
    reply(
        &mut c,
        &command(&bootstrap, "get_available_models"),
        json!({"models":[{"provider":"fixture","id":"alpha"}]}),
    );
    assert!(c.state().ready);
    c
}
fn user(c: &mut Controller, text: &str, timestamp: u64) {
    c.handle(json!({"type":"message_start","message":{"role":"user","content":text,"timestamp":timestamp}}));
}
fn assistant_start(c: &mut Controller) {
    c.handle(json!({"type":"message_start","message":{"role":"assistant","content":[],"timestamp":200,"stopReason":"pending"}}));
}
fn delta(c: &mut Controller, text: &str) {
    c.handle(json!({"type":"message_update","usage":{},"assistantMessageEvent":{"type":"text_delta","contentIndex":0,"delta":text}}));
}
fn assistant_end(c: &mut Controller, text: &str) -> Vec<Effect> {
    c.handle(json!({"type":"message_end","message":{"role":"assistant","timestamp":200,"content":[{"type":"text","text":text}],"stopReason":"stop"}}))
}
#[test]
fn bootstrap_uses_real_rpc_commands_and_namespaces_requests() {
    let mut c = Controller::new("r");
    let e = c.begin_generation(7);
    for typ in [
        "get_state",
        "get_messages",
        "get_available_models",
        "get_commands",
    ] {
        assert!(command(&e, typ)["id"]
            .as_str()
            .unwrap()
            .starts_with("tau:r:g7:"));
    }
    assert_eq!(c.pending_count(), 4);
    assert!(c.begin_generation(7).is_empty());
    c.tick(REQUEST_TIMEOUT_MS);
    assert_eq!(c.state().status, Status::Error);
    assert!(!c.state().history_loading);
}
#[test]
fn runtime_generation_and_unknown_or_mismatched_responses_are_fenced() {
    let mut c = ready();
    let old = command(&c.submit("keep"), "prompt");
    c.begin_generation(2);
    assert!(c
        .ingest("other", 2, json!({"type":"agent_start"}))
        .is_empty());
    assert!(c
        .ingest("runtime", 1, json!({"type":"agent_start"}))
        .is_empty());
    assert!(!c.state().working);
    assert_eq!(c.state().draft, "keep");
    c.handle(json!({"type":"response","id":old["id"],"command":"prompt","success":true}));
    assert!(!c.state().working);
    let before = c.pending_count();
    c.handle(json!({"type":"response","id":"invented","command":"get_state","success":true,"data":identity("evil",true)}));
    assert_eq!(c.pending_count(), before);
    assert_eq!(c.state().identity.id, "a");
}
#[test]
fn rejected_prompt_restores_exact_draft_and_does_not_destroy_new_input() {
    let mut c = ready();
    let p = command(&c.submit("  keep this  "), "prompt");
    assert!(c.submit("again").is_empty());
    c.set_draft("newer");
    c.handle(json!({"type":"response","id":p["id"],"command":"prompt","success":false,"error":"PRIVATE RAW ERROR"}));
    assert_eq!(c.state().draft, "  keep this  \n\nnewer");
    assert!(c.state().transcript.is_empty());
    assert!(!serde_json::to_string(c.state())
        .unwrap()
        .contains("PRIVATE RAW ERROR"));
}
#[test]
fn optimistic_ack_transformed_input_repeated_text_and_hydration_have_stable_ids() {
    let mut c = ready();
    let p = command(&c.submit("original"), "prompt");
    let id = c.state().transcript[0].id.clone();
    reply(&mut c, &p, json!({"disposition":"started"}));
    user(&mut c, "transformed", 100);
    user(&mut c, "transformed", 101);
    assert_eq!(c.state().transcript.len(), 2);
    assert_eq!(c.state().transcript[0].id, id);
    assistant_start(&mut c);
    delta(&mut c, "part");
    let aid = c.state().transcript[2].id.clone();
    delta(&mut c, "ial");
    assistant_end(&mut c, "authoritative final");
    assert_eq!(c.state().transcript[2].id, aid);
    assert_eq!(c.state().transcript[2].text, "authoritative final");
    let s = command(&c.handle(json!({"type":"agent_settled"})), "get_state");
    let m = command(&reply(&mut c, &s, identity("a", false)), "get_messages");
    reply(
        &mut c,
        &m,
        json!({"messages":[{"role":"user","content":"transformed","timestamp":100},
        {"role":"user","content":"transformed","timestamp":101},
        {"role":"assistant","timestamp":200,"content":[{"type":"text","text":"authoritative final"}],"stopReason":"stop"}]}),
    );
    assert_eq!(c.state().transcript[0].id, id);
    assert_eq!(c.state().transcript[2].id, aid);
}
#[test]
fn accepted_event_then_failed_response_never_restores_sent_draft() {
    let mut c = ready();
    let p = command(&c.submit("sent"), "prompt");
    user(&mut c, "sent", 100);
    c.set_draft("newer");
    c.handle(json!({"type":"response","id":p["id"],"command":"prompt","success":false}));
    assert_eq!(c.state().draft, "newer");
    assert_eq!(c.state().transcript.len(), 1);
}
#[test]
fn stop_preserves_partial_reply_and_queue_and_is_bounded() {
    let mut c = ready();
    c.submit("work");
    c.handle(json!({"type":"agent_start"}));
    assistant_start(&mut c);
    delta(&mut c, "partial");
    c.handle(json!({"type":"queue_update","steering":["later"],"followUp":["next"]}));
    let stop = command(&c.abort(), "abort");
    assert!(c.abort().is_empty());
    assert_eq!(c.state().status, Status::Stopping);
    reply(&mut c, &stop, Value::Null);
    let row = c
        .state()
        .transcript
        .iter()
        .find(|r| r.kind == "assistant")
        .unwrap();
    assert_eq!(row.text, "partial");
    assert!(row.interrupted);
    assert!(!row.activity);
    assert_eq!(c.queue_snapshot().steering, vec!["later"]);
    let mut c = ready();
    c.submit("work");
    assistant_start(&mut c);
    delta(&mut c, "kept");
    c.abort();
    c.tick(REQUEST_TIMEOUT_MS);
    assert!(!c.state().stopping);
    assert_eq!(c.state().status, Status::Error);
    assert!(c
        .state()
        .transcript
        .iter()
        .any(|r| r.text == "kept" && !r.activity));
}
#[test]
fn tool_lifecycle_partial_results_and_hydration_keep_one_stable_row() {
    let mut c = ready();
    assistant_start(&mut c);
    let final_message = json!({"role":"assistant","timestamp":200,"content":[{"type":"toolCall","id":"call","name":"bash","arguments":{"command":"ls"}}],"stopReason":"toolUse"});
    c.handle(json!({"type":"message_end","message":final_message}));
    let id = c.state().transcript[0].id.clone();
    c.handle(json!({"type":"tool_execution_start","toolCallId":"call","toolName":"bash","args":{"command":"ls"}}));
    c.handle(json!({"type":"tool_execution_update","toolCallId":"call","partialResult":{"content":[{"type":"text","text":"partial"}]}}));
    assert_eq!(c.state().transcript[0].result, "partial");
    c.handle(json!({"type":"tool_execution_end","toolCallId":"call","result":{"content":[{"type":"text","text":"final"}]},"isError":true}));
    c.handle(json!({"type":"message_end","message":{"role":"toolResult","toolCallId":"call","toolName":"bash","content":[{"type":"text","text":"final"}],"isError":true}}));
    assert_eq!(c.state().transcript.len(), 1);
    assert_eq!(c.state().transcript[0].id, id);
    assert_eq!(c.state().transcript[0].arguments, json!({"command":"ls"}));
    assert!(c.state().transcript[0].is_error);
}
#[test]
fn only_final_assistant_barrier_registers_captured_identity_not_command_or_partial() {
    let mut c = ready();
    let p = command(&c.submit("/usage"), "prompt");
    assert!(c.state().transcript.is_empty());
    let s = command(
        &reply(&mut c, &p, json!({"disposition":"handled"})),
        "get_state",
    );
    let e = reply(&mut c, &s, identity("a", false));
    assert!(!e
        .iter()
        .any(|e| matches!(e, Effect::RegisterSession { .. })));
    assistant_start(&mut c);
    delta(&mut c, "partial");
    assert!(!c.state().durable);
    let b = command(&assistant_end(&mut c, "done"), "get_state");
    assert!(!c.state().durable);
    let e = reply(&mut c, &b, identity("a", true));
    assert!(e.iter().any(|e| matches!(e,Effect::RegisterSession{id,path,name,generation,..} if id=="a" && path=="/fixture/a.jsonl" && name=="Work" && *generation==1)));
    assert!(c.state().durable);
    assert!(reply(&mut c, &b, identity("a", false)).is_empty());
}
#[test]
fn barrier_discovers_successor_registers_predecessor_without_registering_or_selecting_successor() {
    let mut c = ready();
    assistant_start(&mut c);
    let b = command(&assistant_end(&mut c, "done"), "get_state");
    let stale = command(&c.refresh(), "get_state");
    let e = reply(&mut c, &b, identity("b", true));
    assert!(e
        .iter()
        .any(|e| matches!(e,Effect::RegisterSession{id,..} if id=="a")));
    assert!(!e
        .iter()
        .any(|e| matches!(e,Effect::RegisterSession{id,..} if id=="b")));
    assert_eq!(c.state().identity.id, "b");
    assert!(!c.state().durable);
    assert!(c.state().transcript.is_empty());
    assert!(reply(&mut c, &stale, identity("a", false)).is_empty());
    assert_eq!(c.state().identity.id, "b");
}
#[test]
fn late_history_cannot_overwrite_stream_or_newer_history() {
    let mut c = ready();
    let s = command(&c.refresh(), "get_state");
    let m = command(&reply(&mut c, &s, identity("a", true)), "get_messages");
    assistant_start(&mut c);
    delta(&mut c, "live");
    reply(&mut c, &m, json!({"messages":[]}));
    assert!(c.state().transcript.iter().any(|r| r.text == "live"));
    let old = command(&c.refresh(), "get_state");
    let new = command(&c.refresh(), "get_state");
    reply(&mut c, &new, identity("a", true));
    assert!(reply(&mut c, &old, identity("evil", false)).is_empty());
    assert_eq!(c.state().identity.id, "a");
}
#[test]
fn queue_prepares_modes_uses_actual_prompt_and_clear_does_not_erase_new_snapshot() {
    let mut c = ready();
    c.handle(json!({"type":"agent_start"}));
    let s = command(
        &c.submit_queued("later", QueueKind::FollowUp),
        "set_steering_mode",
    );
    let f = command(&reply(&mut c, &s, Value::Null), "set_follow_up_mode");
    let p = command(&reply(&mut c, &f, Value::Null), "prompt");
    assert_eq!(p["streamingBehavior"], "followUp");
    assert_eq!(p["message"], "later");
    reply(&mut c, &p, json!({"disposition":"queued"}));
    assert!(c.state().transcript.is_empty());
    c.handle(json!({"type":"queue_update","steering":[],"followUp":["later"]}));
    let clear = command(&c.clear_queue(), "clear_queue");
    assert!(clear.get("queueVersion").is_none());
    c.handle(json!({"type":"queue_update","steering":[],"followUp":["new"]}));
    reply(&mut c, &clear, json!({"steering":[],"followUp":["later"]}));
    assert_eq!(c.queue_snapshot().follow_up, vec!["new"]);
}
#[test]
fn extensions_are_scoped_time_bounded_and_have_real_response_shapes() {
    let mut c = ready();
    c.handle(json!({"type":"extension_ui_request","id":"ask","method":"confirm","title":"Continue?","timeout":5000}));
    assert!(c.submit("blocked").is_empty());
    let e = c.extension_response("ask", ExtensionResponse::Confirmed(true));
    assert_eq!(
        command(&e, "extension_ui_response"),
        json!({"type":"extension_ui_response","id":"ask","confirmed":true})
    );
    assert!(c
        .extension_response("ask", ExtensionResponse::Cancelled)
        .is_empty());
    c.extension_sent("ask", false);
    assert!(!c
        .extension_response("ask", ExtensionResponse::Cancelled)
        .is_empty());
    c.extension_sent("ask", true);
    c.handle(json!({"type":"extension_ui_request","id":"expires","method":"input","timeout":100}));
    c.tick(100);
    assert!(c.state().extension_dialogs.is_empty());
    c.handle(json!({"type":"extension_ui_request","id":"draft","method":"set_editor_text","text":"prefilled"}));
    assert_eq!(c.state().draft, "prefilled");
    c.begin_generation(2);
    assert!(c
        .extension_response("expires", ExtensionResponse::Value("late".into()))
        .is_empty());
}
#[test]
fn model_and_thinking_settings_use_pi_values_and_refresh_authoritative_state() {
    let mut c = ready();
    let m = command(&c.set_model("fixture", "beta"), "set_model");
    assert_eq!(m["modelId"], "beta");
    let s = command(
        &reply(&mut c, &m, json!({"id":"beta","provider":"fixture"})),
        "get_state",
    );
    let mut d = identity("a", false);
    d["model"]["id"] = json!("beta");
    reply(&mut c, &s, d);
    assert_eq!(c.state().model["id"], "beta");
    let t = command(&c.set_thinking("max"), "set_thinking_level");
    assert_eq!(t["level"], "max");
}
#[test]
fn request_timeout_preserves_uncertain_draft_without_automatically_replaying() {
    let mut c = ready();
    c.submit("uncertain");
    c.tick(REQUEST_TIMEOUT_MS);
    assert_eq!(c.state().recovered_drafts, vec!["uncertain"]);
    assert!(c.state().draft.is_empty());
    assert_eq!(c.pending_count(), 0);
    assert_eq!(c.state().status, Status::Error);
}
