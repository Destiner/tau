use super::*;

fn reply(c: &mut Controller, request: &Value, data: Value) -> Vec<Effect> {
    c.handle(json!({"type":"response","id":request["id"],"command":request["type"],"success":true,"data":data}))
}
fn request(effects: &[Effect], command: &str) -> Value {
    effects
        .iter()
        .find_map(|e| match e {
            Effect::Send(v) if v["type"] == command => Some(v.clone()),
            _ => None,
        })
        .unwrap()
}
fn ready() -> Controller {
    let mut c = Controller::new("workflow");
    c.bind_source("a", "/fixture/a.jsonl");
    let effects = c.begin_generation(1);
    for effect in effects {
        if let Effect::Send(v) = effect {
            let data = match v["type"].as_str().unwrap() {
                "get_state" => {
                    json!({"sessionId":"a","sessionFile":"/fixture/a.jsonl","isStreaming":false})
                }
                "get_messages" => json!({"messages":[]}),
                "get_commands" => json!({"commands":[{"name":"implement","source":"extension"}]}),
                _ => json!({"models":[]}),
            };
            reply(&mut c, &v, data);
        }
    }
    while let Some(v) = c.pending.values().next().map(|p| p.payload.clone()) {
        let data = match v["type"].as_str().unwrap() {
            "get_state" => {
                json!({"sessionId":"a","sessionFile":"/fixture/a.jsonl","isStreaming":false})
            }
            "get_messages" => json!({"messages":[]}),
            "get_commands" => json!({"commands":[{"name":"implement","source":"extension"}]}),
            _ => json!({"models":[],"levels":["off"]}),
        };
        reply(&mut c, &v, data);
    }
    assert!(c.state.ready);
    assert!(!c.state.history_loading);
    c
}
fn admitted(c: &mut Controller) -> Value {
    let effects = c.submit("/implement");
    let v = request(&effects, "prompt");
    // The fake host has accepted the write; Pi has not semantically accepted it.
    assert!(c.request_write_accepted(1, v["id"].as_str().unwrap()));
    v
}

#[test]
fn workflow_preflight_outlives_deadline_without_agent_start() {
    let mut c = ready();
    let prompt = admitted(&mut c);
    for now in [10_001, 20_000, 600_001] {
        let effects = c.tick(now);
        assert!(c.state.error.is_none(), "{now}: {:?}", c.state.error);
        assert!(c.pending.contains_key(prompt["id"].as_str().unwrap()));
        assert!(!c.pending[prompt["id"].as_str().unwrap()].accepted);
        assert!(!c.can_release_runtime());
        assert!(c.state.recovered_drafts.is_empty());
        assert!(c.submit("/implement").is_empty());
        assert!(!effects
            .iter()
            .any(|e| matches!(e, Effect::Send(v) if v["type"] == "prompt")));
    }
    let effects = reply(&mut c, &prompt, json!({"disposition":"handled"}));
    assert_eq!(request(&effects, "get_state")["type"], "get_state");
    assert!(!c.state.working);
    assert!(c.state.draft.is_empty());
    assert!(!c.state.durable);
    assert!(reply(&mut c, &prompt, json!({"disposition":"handled"})).is_empty());
}

#[test]
fn workflow_ui_wait_and_response_have_no_command_ack() {
    let mut c = ready();
    let prompt = admitted(&mut c);
    c.handle(
        json!({"type":"extension_ui_request","id":"dialog","method":"confirm","title":"Continue?"}),
    );
    c.tick(20_000);
    assert!(c.state.error.is_none());
    assert_eq!(c.state.extension_dialogs.len(), 1);
    let effects = c.extension_response("dialog", ExtensionResponse::Confirmed(true));
    assert_eq!(effects.len(), 1);
    let effects = c.extension_sent("dialog", true);
    assert!(!effects
        .iter()
        .any(|e| matches!(e, Effect::Send(v) if v["type"] == "prompt")));
    while let Some(v) = c
        .pending
        .values()
        .find(|p| p.command != "prompt")
        .map(|p| p.payload.clone())
    {
        let data = if v["type"] == "get_state" {
            json!({"sessionId":"a","sessionFile":"/fixture/a.jsonl","isStreaming":false})
        } else {
            json!({"messages":[]})
        };
        reply(&mut c, &v, data);
    }
    c.tick(600_001);
    assert!(c.state.error.is_none());
    assert!(c.pending.contains_key(prompt["id"].as_str().unwrap()));
    reply(&mut c, &prompt, json!({"disposition":"handled"}));
    assert!(c.state.recovered_drafts.is_empty());
}

#[test]
fn short_command_reply_reconciles_without_waiting_for_agent_start() {
    let mut c = ready();
    let prompt = admitted(&mut c);
    let effects = reply(&mut c, &prompt, json!({"disposition":"handled"}));
    assert!(c.admission.is_none());
    assert!(!c.state.working);
    assert!(!c.state.durable);
    assert!(c.state.error.is_none());
    assert!(c.reconciliation_pending());
    assert_eq!(request(&effects, "get_state")["type"], "get_state");
    assert!(!c.can_release_runtime());
}

#[test]
fn known_rejection_after_long_preflight_restores_once_without_unsafe_fence() {
    let mut c = ready();
    let prompt = admitted(&mut c);
    c.set_draft("new draft");
    c.tick(20_000);
    c.handle(json!({"type":"response","id":prompt["id"],"command":"prompt","success":false,"error":"private failure"}));
    assert_eq!(c.state.draft, "/implement\n\nnew draft");
    assert!(c.state.recovered_drafts.is_empty());
    assert!(!c.reconciliation.uncertain_write);
    assert!(!c.state.working);
    assert_eq!(
        c.state.error.as_deref(),
        Some("The message was not accepted. Review the draft and try again.")
    );
    assert!(c.request_failed(prompt["id"].as_str().unwrap()).is_empty());
    assert_eq!(c.state.draft, "/implement\n\nnew draft");
}

#[test]
fn transport_close_recovers_unaccepted_command_and_never_replays_it() {
    let mut c = ready();
    let prompt = admitted(&mut c);
    c.set_draft("new draft");
    c.tick(20_000);
    assert!(c.transport_failed(0).is_empty());
    assert!(c.connected);
    c.transport_failed(1);
    assert_eq!(c.state.draft, "/implement\n\nnew draft");
    assert!(c.reconciliation.uncertain_write);
    let token = c.error_token().unwrap();
    assert!(c.acknowledge_error(&token));
    assert!(c.reconciliation.uncertain_write);
    assert!(!c.can_release_runtime());
    let effects = c.begin_generation(2);
    assert!(!effects
        .iter()
        .any(|e| matches!(e, Effect::Send(v) if v["type"] == "prompt")));
    assert!(reply(&mut c, &prompt, json!({"disposition":"handled"})).is_empty());
    assert_eq!(c.state.draft, "/implement\n\nnew draft");
    assert!(!c.request_write_accepted(1, prompt["id"].as_str().unwrap()));
}

#[test]
fn idle_state_read_does_not_erase_unanswered_extension_write_on_transport_loss() {
    let mut c = ready();
    let prompt = admitted(&mut c);
    let read = request(
        &c.handle(json!({"type":"session_info_changed"})),
        "get_state",
    );
    reply(
        &mut c,
        &read,
        json!({"sessionId":"a","sessionFile":"/fixture/a.jsonl","isStreaming":false}),
    );
    while let Some(read) = c
        .pending
        .values()
        .find(|p| p.command != "prompt")
        .map(|p| p.payload.clone())
    {
        reply(
            &mut c,
            &read,
            json!({"messages":[],"models":[],"commands":[]}),
        );
    }
    let pending = &c.pending[prompt["id"].as_str().unwrap()];
    assert!(pending.write_accepted);
    assert!(pending.extension_command);
    assert!(!pending.accepted);
    assert!(!c.state.working);
    assert!(!c.reconciliation.command);
    assert!(!c.reconciliation.uncertain_write);
    c.transport_failed(1);
    assert!(c.reconciliation.uncertain_write);
    assert_eq!(c.state.draft, "/implement");
    assert!(c.pending.is_empty());
    let token = c.error_token().unwrap();
    assert!(c.acknowledge_error(&token));
    assert!(c.reconciliation.uncertain_write);
    assert!(!c.can_release_runtime());
    let effects = c.begin_generation(2);
    assert!(!effects
        .iter()
        .any(|e| matches!(e, Effect::Send(v) if v["type"] == "prompt")));
    assert!(c.reconciliation.uncertain_write);
    assert!(!c.request_write_accepted(1, prompt["id"].as_str().unwrap()));
    assert!(reply(&mut c, &prompt, json!({"disposition":"handled"})).is_empty());
    assert!(c.reconciliation.uncertain_write);
    assert_eq!(c.state.draft, "/implement");
    while let Some(read) = c.pending.values().next().map(|p| p.payload.clone()) {
        let data = if read["type"] == "get_state" {
            json!({"sessionId":"a","sessionFile":"/fixture/a.jsonl","isStreaming":false})
        } else {
            json!({"messages":[],"models":[],"commands":[],"levels":[]})
        };
        reply(&mut c, &read, data);
    }
    assert!(!c.reconciliation.uncertain_write);
    assert_eq!(c.state.draft, "/implement");
}

#[test]
fn admitted_reads_and_known_prompt_rejection_do_not_become_uncertain_on_exit() {
    for reject_prompt in [false, true] {
        let mut c = ready();
        if reject_prompt {
            let prompt = admitted(&mut c);
            c.handle(json!({"type":"response","id":prompt["id"],"command":"prompt","success":false,"error":"private failure"}));
        }
        let read = request(
            &c.handle(json!({"type":"session_info_changed"})),
            "get_state",
        );
        assert!(c.request_write_accepted(1, read["id"].as_str().unwrap()));
        assert!(!c.state.working);
        assert!(!c.reconciliation.command);
        c.transport_failed(1);
        assert!(!c.reconciliation.uncertain_write);
    }
}

#[test]
fn replacement_retains_command_reply_ownership_without_recovering_predecessor_draft() {
    let mut c = ready();
    let prompt = admitted(&mut c);
    let read = c.send("get_state", json!({}), false);
    let Effect::Send(read) = read else {
        panic!("RPC")
    };
    reply(
        &mut c,
        &read,
        json!({"sessionId":"b","sessionFile":"/fixture/b.jsonl","isStreaming":false}),
    );
    let reads: Vec<_> = c
        .pending
        .values()
        .filter(|p| p.command != "prompt")
        .map(|p| p.payload.clone())
        .collect();
    for read in reads {
        reply(
            &mut c,
            &read,
            json!({"messages":[],"models":[],"commands":[]}),
        );
    }
    c.tick(20_000);
    assert!(c.state.error.is_none());
    assert!(c.state.draft.is_empty());
    assert!(c.state.recovered_drafts.is_empty());
    assert_eq!(c.state.identity.id, "b");
    assert!(c.provisional_replacement());
    c.fail("displayed incident");
    let token = c.error_token().unwrap();
    assert!(c.acknowledge_error(&token));
    assert!(c.provisional_replacement());
    assert!(!c.state.durable);
    assert!(
        !c.request_write_accepted(1, prompt["id"].as_str().unwrap()),
        "old epoch cannot be re-admitted"
    );
    let effects = reply(&mut c, &prompt, json!({"disposition":"handled"}));
    assert_eq!(request(&effects, "get_state")["type"], "get_state");
    assert!(c.provisional_replacement());
    assert!(!c.can_release_runtime());
    assert!(reply(&mut c, &prompt, json!({"disposition":"handled"})).is_empty());
}

#[test]
fn unowned_extension_and_ordinary_prompt_keep_uncertain_timeout_policy() {
    for (text, admit) in [("/implement", false), ("ordinary prompt", true)] {
        let mut c = ready();
        let prompt = request(&c.submit(text), "prompt");
        if admit {
            assert!(c.request_write_accepted(1, prompt["id"].as_str().unwrap()));
        }
        let effects = c.tick(10_001);
        assert_eq!(
            c.state.error.as_deref(),
            Some("Message confirmation was lost. Check the transcript before resending.")
        );
        assert_eq!(c.state.recovered_drafts, vec![text]);
        assert!(c.state.draft.is_empty());
        assert!(c.reconciliation.uncertain_write);
        assert!(c.state.transcript.is_empty());
        assert!(!effects
            .iter()
            .any(|e| matches!(e, Effect::Send(v) if v["type"] == "prompt")));
        let token = c.error_token().unwrap();
        assert!(c.acknowledge_error(&token));
        assert!(c.reconciliation.uncertain_write);
        assert!(!c.can_release_runtime());
        assert!(reply(&mut c, &prompt, json!({"disposition":"started"})).is_empty());
        assert_eq!(c.state.recovered_drafts, vec![text]);
    }
}

#[test]
fn write_admission_does_not_disable_stop_read_or_queue_deadlines() {
    let mut c = ready();
    let prompt = admitted(&mut c);
    assert!(c.submit_queued("queued", QueueKind::Steer).is_empty());
    let abort = request(&c.abort(), "abort");
    assert!(c.request_write_accepted(1, abort["id"].as_str().unwrap()));
    c.tick(10_001);
    assert!(c.pending.contains_key(prompt["id"].as_str().unwrap()));
    assert!(!c.state.stopping);
    assert_eq!(
        c.state.error.as_deref(),
        Some("Stop was not confirmed. Pi may still be working. Try Stop again.")
    );
    let mut c = ready();
    let read = c.send("get_state", json!({}), false);
    let Effect::Send(read) = read else {
        panic!("RPC")
    };
    assert!(c.request_write_accepted(1, read["id"].as_str().unwrap()));
    c.tick(10_001);
    assert!(!c.pending.contains_key(read["id"].as_str().unwrap()));
    assert!(c.state.error.is_some());
}

#[test]
fn error_acknowledgement_fences_repeated_text_generation_epoch_and_runtime() {
    let mut c = ready();
    c.fail("same incident copy");
    let old = c.error_token().unwrap();
    c.fail("same incident copy");
    assert!(!c.acknowledge_error(&old));
    let current = c.error_token().unwrap();
    let mut other = ready();
    other.runtime_id = "other".into();
    other.fail("same incident copy");
    assert!(!other.acknowledge_error(&current));
    c.bind_source("b", "/fixture/b.jsonl");
    assert!(!c.acknowledge_error(&current));
    let token = c.error_token().unwrap();
    c.begin_generation(2);
    assert!(!c.acknowledge_error(&token));
    let token = c.error_token().unwrap();
    c.reconciliation.uncertain_write = true;
    assert!(c.acknowledge_error(&token));
    assert!(c.error_token().is_none());
    assert!(!c.acknowledge_error(&token));
    assert!(c.reconciliation.uncertain_write);
}

#[test]
fn extension_error_event_and_handled_reply_do_not_recover_or_resubmit_command() {
    let mut c = ready();
    let prompt = admitted(&mut c);
    c.tick(20_000);
    c.handle(json!({"type":"extension_error","event":"command","error":"private failure"}));
    let token = c.error_token().unwrap();
    let effects = reply(&mut c, &prompt, json!({"disposition":"handled"}));
    assert!(c.state.draft.is_empty());
    assert!(c.state.recovered_drafts.is_empty());
    assert!(c.state.transcript.is_empty());
    assert!(c.acknowledge_error(&token));
    assert!(c.reconciliation_pending());
    assert!(!c.can_release_runtime());
    assert!(!effects
        .iter()
        .any(|e| matches!(e, Effect::Send(v) if v["type"] == "prompt")));
}
