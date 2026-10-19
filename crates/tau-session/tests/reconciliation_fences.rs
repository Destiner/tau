use serde_json::{json, Value};
use tau_session::session::{Controller, Effect, REQUEST_TIMEOUT_MS};

fn request(effects: &[Effect], command: &str) -> Value {
    effects
        .iter()
        .find_map(|e| match e {
            Effect::Send(v) if v["type"] == command => Some(v.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing {command}: {effects:?}"))
}
fn reply(controller: &mut Controller, request: &Value, data: Value) -> Vec<Effect> {
    controller.handle(json!({"type":"response","id":request["id"],"command":request["type"],"success":true,"data":data}))
}
fn identity() -> Value {
    json!({"sessionId":"a","sessionFile":"/fixture/a.jsonl","isStreaming":false})
}
fn ready() -> Controller {
    let mut controller = Controller::new("r");
    controller.bind_source("a", "/fixture/a.jsonl");
    let bootstrap = controller.begin_generation(1);
    let state = request(&bootstrap, "get_state");
    let hydration = reply(&mut controller, &state, identity());
    for effects in [&bootstrap, &hydration] {
        for effect in effects {
            if let Effect::Send(request) = effect {
                let data = match request["type"].as_str().unwrap() {
                    "get_state" => continue,
                    "get_messages" => json!({"messages":[]}),
                    "get_commands" => {
                        json!({"commands":[{"name":"workflow","source":"extension"}]})
                    }
                    "get_available_thinking_levels" => json!({"levels":[]}),
                    _ => json!({"models":[]}),
                };
                reply(&mut controller, request, data);
            }
        }
    }
    assert!(controller.can_release_runtime());
    controller
}

#[test]
fn obsolete_same_identity_command_success_never_rearms_completed_or_inflight_hydration() {
    for stale_before_hydration in [false, true] {
        let mut c = ready();
        let prompt = request(&c.submit("/workflow"), "prompt");
        let command = request(
            &reply(&mut c, &prompt, json!({"disposition":"handled"})),
            "get_state",
        );
        let probe = request(&c.tick(150), "get_state");
        let effects = reply(&mut c, &probe, identity());
        let hydration = request(&effects, "get_messages");
        for effect in effects {
            if let Effect::Send(v) = effect {
                if v["type"] != "get_messages" {
                    reply(&mut c, &v, json!({"levels":[]}));
                }
            }
        }
        if stale_before_hydration {
            assert!(reply(&mut c, &command, identity()).is_empty());
            assert!(c.reconciliation_pending());
            assert!(c.state().error.is_none());
        }
        reply(&mut c, &hydration, json!({"messages":[]}));
        assert!(c.resume_reads().is_empty());
        if !stale_before_hydration {
            assert!(reply(&mut c, &command, identity()).is_empty());
        }
        assert!(c.state().error.is_none());
        assert!(c.resume_reads().is_empty());
        // Finish remaining probes to distinguish their release fence from a rearmed command.
        for now in [450, 1_000, 2_000, 3_200] {
            let probe = request(&c.tick(now), "get_state");
            let effects = reply(&mut c, &probe, identity());
            for effect in effects {
                if let Effect::Send(v) = effect {
                    reply(&mut c, &v, json!({"levels":[]}));
                }
            }
        }
        c.tick(3_200 + REQUEST_TIMEOUT_MS);
        assert!(!c.reconciliation_pending());
        assert!(c.state().error.is_none());
        assert!(c.can_release_runtime());
    }
}

#[test]
fn failed_obsolete_command_read_cannot_own_a_newer_command_incarnation() {
    for failure_before_completion in [false, true] {
        let mut c = ready();
        let prompt = request(&c.submit("/workflow"), "prompt");
        let old = request(
            &reply(&mut c, &prompt, json!({"disposition":"handled"})),
            "get_state",
        );
        let prompt = request(&c.submit("/workflow"), "prompt");
        let current = request(
            &reply(&mut c, &prompt, json!({"disposition":"handled"})),
            "get_state",
        );
        let fail = |c: &mut Controller| {
            c.handle(
                json!({"type":"response","id":old["id"],"command":"get_state","success":false}),
            );
        };
        if failure_before_completion {
            fail(&mut c);
            assert!(c.state().error.is_none());
        }
        let hydration = request(&reply(&mut c, &current, identity()), "get_messages");
        reply(&mut c, &hydration, json!({"messages":[]}));
        if !failure_before_completion {
            fail(&mut c);
        }
        assert!(c.state().error.is_none());
        assert!(c.resume_reads().is_empty());
    }
}

#[test]
fn physical_stop_disconnects_even_after_new_work_and_preserves_storage_proof() {
    let mut c = ready();
    let message = json!({"role":"assistant","content":[{"type":"text","text":"completed"}],"stopReason":"stop"});
    c.handle(json!({"type":"message_end","message":message}));
    c.set_draft("new draft");
    c.handle(json!({"type":"extension_ui_request","id":"new","method":"input","title":"Value?"}));
    assert!(!c.release_generation(1));
    c.backend_stopped(1);
    assert!(!c.state().ready);
    assert!(!c.state().working);
    assert_eq!(c.state().draft, "new draft");
    assert!(!c.state().transcript.is_empty());
    assert!(c.durability_pending());
    let before = c.state().transcript.len();
    assert!(c.ingest("r", 1, json!({"type":"agent_start"})).is_empty());
    assert!(!c.state().working);
    assert_eq!(c.state().transcript.len(), before);
    assert!(c
        .prepare_exit()
        .iter()
        .any(|e| matches!(e, Effect::RegisterSession { id, generation: 1, .. } if id == "a")));
    assert!(!c.durability_pending());
    assert!(c.begin_generation(1).is_empty());
    assert!(!c.begin_generation(2).is_empty());
}

#[test]
fn new_completion_after_exit_drain_reopens_durability_until_second_handoff() {
    let mut c = ready();
    assert!(c.prepare_exit().is_empty());
    assert!(!c.durability_pending());
    // Models a queued record arriving while the host Save is in flight.
    c.handle(json!({"type":"message_end","message":{"role":"assistant","content":[{"type":"text","text":"done"}],"stopReason":"stop"}}));
    assert!(c.durability_pending());
    let drain = c.prepare_exit();
    assert_eq!(
        drain
            .iter()
            .filter(|e| matches!(e, Effect::RegisterSession { .. }))
            .count(),
        1
    );
    assert!(!c.durability_pending());
    assert!(c.prepare_exit().is_empty());
}

#[test]
fn last_probe_transport_timeout_ends_its_fence_without_masking_ordinary_errors() {
    let mut c = ready();
    let prompt = request(&c.submit("/workflow"), "prompt");
    let effects = reply(&mut c, &prompt, json!({"disposition":"handled"}));
    let state = request(&effects, "get_state");
    let effects = reply(&mut c, &state, identity());
    reply(
        &mut c,
        &request(&effects, "get_messages"),
        json!({"messages":[]}),
    );
    for now in [150, 450, 1_000, 2_000] {
        let state = request(&c.tick(now), "get_state");
        let effects = reply(&mut c, &state, identity());
        for effect in effects {
            if let Effect::Send(request) = effect {
                reply(&mut c, &request, json!({"levels":[]}));
            }
        }
    }
    let last = request(&c.tick(3_200), "get_state");
    assert!(c.reconciliation_pending());
    assert!(!c.release_generation(1));
    c.tick(3_200 + REQUEST_TIMEOUT_MS);
    assert!(!c.reconciliation_pending());
    assert!(c.state().error.is_none());
    assert!(reply(
        &mut c,
        &last,
        json!({"sessionId":"evil","sessionFile":"/fixture/evil.jsonl"})
    )
    .is_empty());
    assert_eq!(c.state().identity.id, "a");
    assert!(c.can_release_runtime());
}

#[test]
fn command_read_send_failure_reselection_is_deduplicated_and_preserves_new_draft() {
    let mut c = ready();
    let prompt = request(&c.submit("/workflow"), "prompt");
    let effects = reply(&mut c, &prompt, json!({"disposition":"handled"}));
    let state = request(&effects, "get_state");
    c.set_draft("new text");
    c.request_failed(state["id"].as_str().unwrap());
    assert!(c.reconciliation_pending());
    let retry = c.resume_reads();
    assert_eq!(retry.len(), 1);
    assert_eq!(request(&retry, "get_state")["type"], "get_state");
    assert!(c.resume_reads().is_empty());
    let state = request(&retry, "get_state");
    let effects = reply(&mut c, &state, identity());
    reply(
        &mut c,
        &request(&effects, "get_messages"),
        json!({"messages":[]}),
    );
    assert!(c.state().error.is_none());
    assert_eq!(c.state().draft, "new text");
    assert!(!c.state().working);
}

#[test]
fn uncertain_write_cannot_be_released_until_new_authoritative_idle_reads() {
    let mut c = ready();
    let prompt = request(&c.submit("ordinary work"), "prompt");
    c.tick(REQUEST_TIMEOUT_MS);
    assert!(c.reconciliation_pending());
    assert!(!c.can_release_runtime());
    assert!(!c.release_generation(1));
    assert!(reply(&mut c, &prompt, json!({"disposition":"started"})).is_empty());
    let state = request(&c.refresh(), "get_state");
    let hydration = reply(&mut c, &state, identity());
    reply(
        &mut c,
        &request(&hydration, "get_messages"),
        json!({"messages":[]}),
    );
    assert!(!c.reconciliation_pending());
    assert!(!c.state().working);
}

#[test]
fn pending_extension_transport_receipt_queue_and_compaction_block_release() {
    let mut c = ready();
    c.handle(
        json!({"type":"extension_ui_request","id":"dialog","method":"confirm","title":"Continue?"}),
    );
    assert!(!c.can_release_runtime());
    assert!(!c.release_generation(1));
    c.transport_failed(1);
    assert!(!c.release_generation(1));
    let mut c = ready();
    c.handle(json!({"type":"queue_update","steering":["later"],"followUp":[]}));
    assert!(!c.can_release_runtime());
    let mut c = ready();
    c.handle(json!({"type":"compaction_start"}));
    assert!(!c.can_release_runtime());
}

#[test]
fn provisional_b_is_cleared_by_even_a_superseded_successful_b_barrier() {
    let mut c = ready();
    let state = request(&c.refresh(), "get_state");
    let effects = reply(
        &mut c,
        &state,
        json!({"sessionId":"b","sessionFile":"/fixture/b.jsonl","isStreaming":false}),
    );
    reply(
        &mut c,
        &request(&effects, "get_messages"),
        json!({"messages":[]}),
    );
    assert!(c.provisional_replacement());
    let barrier = request(&c.handle(json!({"type":"message_end","message":{"role":"assistant","content":[{"type":"text","text":"done"}]}})), "get_state");
    let newer = request(&c.refresh(), "get_state");
    reply(
        &mut c,
        &newer,
        json!({"sessionId":"b","sessionFile":"/fixture/b.jsonl","isStreaming":false}),
    );
    let effects = reply(
        &mut c,
        &barrier,
        json!({"sessionId":"b","sessionFile":"/fixture/b.jsonl"}),
    );
    assert!(effects
        .iter()
        .any(|e| matches!(e, Effect::RegisterSession { id, .. } if id == "b")));
    assert!(c.state().durable);
    assert!(!c.provisional_replacement());
}
