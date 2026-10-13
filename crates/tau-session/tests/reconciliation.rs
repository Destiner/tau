use serde_json::{json, Value};
use tau_session::session::{Controller, Effect, ExtensionResponse, Identity};

// The fake port speaks the real response envelope and keeps captured requests,
// so duplicates, delayed predecessor replies and generation fences are observable.
struct Port {
    controller: Controller,
    requests: Vec<Value>,
    registrations: Vec<Identity>,
}
impl Port {
    fn bound(id: &str) -> Self {
        let mut controller = Controller::new("workflow");
        controller.bind_source(id, format!("/fixture/{id}.jsonl"));
        Self {
            controller,
            requests: vec![],
            registrations: vec![],
        }
    }
    fn collect(&mut self, effects: Vec<Effect>) {
        for effect in effects {
            match effect {
                Effect::Send(request) => self.requests.push(request),
                Effect::RegisterSession { id, path, name, .. } => {
                    self.registrations.push(Identity { id, path, name })
                }
                _ => {}
            }
        }
    }
    fn start(&mut self, generation: u64) {
        let effects = self.controller.begin_generation(generation);
        self.collect(effects);
    }
    fn take(&mut self, command: &str) -> Value {
        let index = self
            .requests
            .iter()
            .position(|r| r["type"] == command)
            .unwrap_or_else(|| panic!("missing {command}: {:?}", self.requests));
        self.requests.remove(index)
    }
    fn reply(&mut self, request: &Value, data: Value) {
        let effects = self.controller.ingest("workflow", self.controller.generation(), json!({
            "type":"response", "id":request["id"], "command":request["type"], "success":true, "data":data
        }));
        self.collect(effects);
    }
    fn fail(&mut self, request: &Value) {
        let effects = self
            .controller
            .handle(json!({"type":"response", "id":request["id"],
            "command":request["type"], "success":false, "error":"private raw failure"}));
        self.collect(effects);
    }
    fn event(&mut self, event: Value) {
        let effects = self
            .controller
            .ingest("workflow", self.controller.generation(), event);
        self.collect(effects);
    }
    fn tick(&mut self, now: u64) {
        let effects = self.controller.tick(now);
        self.collect(effects);
    }
    fn metadata(&mut self) {
        while let Some(index) = self.requests.iter().position(|r| {
            matches!(
                r["type"].as_str(),
                Some(
                    "get_available_models"
                        | "get_scoped_models"
                        | "get_commands"
                        | "get_available_thinking_levels"
                )
            )
        }) {
            let request = self.requests.remove(index);
            let data = match request["type"].as_str().unwrap() {
                "get_commands" => {
                    json!({"commands":[{"name":"mock-workflow","source":"extension"}]})
                }
                "get_available_thinking_levels" => json!({"levels":["off"]}),
                _ => json!({"models":[]}),
            };
            self.reply(&request, data);
        }
    }
    fn hydrate(&mut self, id: &str, messages: Value) {
        let state = self.take("get_state");
        self.reply(&state, identity(id));
        // Bootstrap's concurrent initial history read may have been superseded.
        while self.requests.iter().any(|r| r["type"] == "get_messages") {
            let request = self.take("get_messages");
            self.reply(&request, json!({"messages":messages}));
        }
        self.metadata();
    }
    fn ready() -> Self {
        let mut port = Self::bound("a");
        port.start(1);
        port.hydrate("a", json!([]));
        assert!(port.controller.can_release_runtime());
        port
    }
    fn handled(&mut self) {
        let effects = self.controller.submit("/mock-workflow");
        self.collect(effects);
        assert!(self.controller.state().transcript.is_empty());
        let prompt = self.take("prompt");
        self.reply(&prompt, json!({"disposition":"handled"}));
    }
}
fn identity(id: &str) -> Value {
    json!({"sessionId":id,"sessionFile":format!("/fixture/{id}.jsonl"),"sessionName":id,
        "isStreaming":false,"isCompacting":false})
}
fn assistant() -> Value {
    json!({"role":"assistant","timestamp":10,"content":[{"type":"text","text":"Approved plan."}],"stopReason":"stop"})
}

#[test]
fn failed_predecessor_barrier_and_adoption_both_orderings_retain_only_a_storage_intent() {
    for fail_first in [false, true] {
        for failure in ["response", "send", "timeout"] {
            let mut p = Port::ready();
            p.event(json!({"type":"message_end","message":assistant()}));
            let barrier = p.take("get_state");
            let fail = |p: &mut Port| match failure {
                "response" => p.fail(&barrier),
                "send" => {
                    let effects = p.controller.request_failed(barrier["id"].as_str().unwrap());
                    p.collect(effects);
                }
                _ => p.tick(tau_session::session::REQUEST_TIMEOUT_MS),
            };
            if fail_first {
                fail(&mut p);
                assert!(p.controller.durability_pending());
                assert!(p.registrations.is_empty());
            }
            let effects = p.controller.refresh();
            p.collect(effects);
            // Choose the new refresh, not a timeout-triggered verification read.
            let index = p
                .requests
                .iter()
                .rposition(|r| r["type"] == "get_state")
                .unwrap();
            let replacement = p.requests.remove(index);
            p.reply(&replacement, identity("b"));
            if !fail_first {
                assert!(p.registrations.is_empty());
                fail(&mut p);
            }
            assert_eq!(
                p.registrations.len(),
                1,
                "{failure}, fail_first={fail_first}"
            );
            assert_eq!(p.registrations[0].id, "a");
            assert_eq!(p.registrations[0].path, "/fixture/a.jsonl");
            assert_eq!(p.controller.state().identity.id, "b");
            assert!(p.controller.state().transcript.is_empty());
            assert!(!p.controller.state().durable);
            assert!(p.controller.prepare_exit().is_empty());
            p.reply(&barrier, identity("a"));
            assert_eq!(p.registrations.len(), 1);
            assert_eq!(p.controller.state().identity.id, "b");
        }
    }
}

#[test]
fn exit_drain_keeps_predecessor_proof_through_restart_and_uses_captured_generation() {
    let mut p = Port::ready();
    p.event(json!({"type":"message_end","message":assistant()}));
    let barrier = p.take("get_state");
    p.fail(&barrier);
    assert!(p.controller.durability_pending());
    p.start(2);
    let state = p.take("get_state");
    let effects = p
        .controller
        .handle(json!({"type":"response","id":state["id"],
        "command":"get_state","success":true,"data":identity("b")}));
    assert!(effects.iter().any(
        |e| matches!(e, Effect::RegisterSession { generation: 1, id, path, .. }
        if id == "a" && path == "/fixture/a.jsonl")
    ));
    p.collect(effects);
    assert_eq!(p.registrations.len(), 1);
    assert_eq!(p.controller.state().identity.id, "b");
    assert!(p.controller.state().transcript.is_empty());
    assert!(!p.controller.state().durable);
    assert!(p.controller.prepare_exit().is_empty());
}

#[test]
fn exit_can_handoff_failed_proof_after_restart_before_any_new_state_read() {
    let mut p = Port::ready();
    p.event(json!({"type":"message_end","message":assistant()}));
    let barrier = p.take("get_state");
    p.fail(&barrier);
    p.tick(30_000);
    assert!(p.controller.durability_pending());
    p.controller.transport_failed(1);
    p.start(2);
    let drain = p.controller.prepare_exit();
    assert!(drain
        .iter()
        .any(|e| matches!(e, Effect::RegisterSession { generation: 1, id, .. } if id == "a")));
    assert!(!p.controller.durability_pending());
    assert!(p.controller.prepare_exit().is_empty());
}

#[test]
fn completed_tool_only_assistant_is_captured_but_streaming_tool_is_not() {
    for completed in [false, true] {
        let mut p = Port::ready();
        let kind = if completed {
            "message_end"
        } else {
            "message_start"
        };
        p.event(json!({"type":kind,"message":{"role":"assistant","stopReason":"toolUse",
            "content":[{"type":"toolCall","id":"call","name":"read","arguments":{"path":"fixture"}}]}}));
        let drain = p.controller.prepare_exit();
        assert_eq!(
            drain
                .iter()
                .any(|e| matches!(e, Effect::RegisterSession { .. })),
            completed
        );
    }
}

#[test]
fn exit_drains_all_captured_identities_even_with_barriers_in_flight() {
    let mut p = Port::ready();
    p.event(json!({"type":"message_end","message":assistant()}));
    let a_barrier = p.take("get_state");
    let effects = p.controller.refresh();
    p.collect(effects);
    p.hydrate("b", json!([]));
    p.event(json!({"type":"message_end","message":assistant()}));
    let b_barrier = p.take("get_state");
    assert!(p.controller.durability_pending());
    let drain = p.controller.prepare_exit();
    assert_eq!(
        drain
            .iter()
            .filter(|e| matches!(e, Effect::RegisterSession { .. }))
            .count(),
        2
    );
    p.collect(drain);
    assert_eq!(
        p.registrations
            .iter()
            .map(|i| i.id.as_str())
            .collect::<Vec<_>>(),
        vec!["a", "b"]
    );
    assert!(!p.controller.durability_pending());
    assert!(p.controller.prepare_exit().is_empty());
    p.reply(&a_barrier, identity("a"));
    p.reply(&b_barrier, identity("b"));
    assert_eq!(p.registrations.len(), 2);
    assert_eq!(p.controller.state().identity.id, "b");
    // Storage acknowledgement is deliberately outside Controller ownership.
}

#[test]
fn exit_never_turns_empty_preview_or_uncompleted_output_into_register() {
    for event in [
        json!({"type":"message_start","message":assistant()}),
        json!({"type":"message_end","message":{"role":"assistant","content":[]}}),
        json!({"type":"message_end","message":{"role":"assistant","content":"partial","stopReason":"error"}}),
    ] {
        let mut p = Port::ready();
        p.event(event);
        let effects = p.controller.prepare_exit();
        assert!(!effects
            .iter()
            .any(|e| matches!(e, Effect::RegisterSession { .. })));
        assert!(p.registrations.is_empty());
    }
    let mut p = Port::bound("a");
    let token = p.controller.saved_preview_token().unwrap();
    assert!(p.controller.apply_saved_preview(&token, &[assistant()]));
    assert!(p.controller.prepare_exit().is_empty());
    assert!(!p.controller.durability_pending());
}

#[test]
fn delayed_successor_after_all_immediate_reads_is_retained_even_when_empty() {
    let mut p = Port::ready();
    p.handled();
    p.hydrate("a", json!([]));
    assert!(!p.controller.provisional_replacement());
    assert!(p.controller.reconciliation_pending());
    for now in [150, 450, 1_000, 2_000] {
        p.tick(now - 1);
        assert!(p.requests.is_empty());
        p.tick(now);
        let state = p.take("get_state");
        p.reply(&state, identity("a"));
        p.metadata();
        assert!(
            p.requests.is_empty(),
            "same-identity probes need no history read"
        );
        assert!(!p.controller.can_release_runtime());
    }
    p.tick(3_199);
    assert!(p.requests.is_empty());
    p.tick(3_200);
    assert!(
        p.controller.reconciliation_pending(),
        "final in-flight read blocks release"
    );
    p.hydrate("b", json!([]));
    assert_eq!(p.controller.state().identity.id, "b");
    assert!(!p.controller.reconciliation_pending());
    assert!(p.controller.provisional_replacement());
    assert!(!p.controller.can_release_runtime());
    assert!(p.controller.resume_reads().is_empty());
    p.tick(30_000);
    assert!(p.requests.is_empty());
    assert!(p.controller.provisional_replacement());
    assert!(
        p.registrations.is_empty(),
        "empty B is not persistence proof"
    );
}

#[test]
fn ordinary_empty_command_session_finishes_reconciliation_without_becoming_provisional() {
    let mut p = Port::ready();
    p.handled();
    p.hydrate("a", json!([]));
    for now in [150, 450, 1_000, 2_000, 3_200] {
        p.tick(now);
        let request = p.take("get_state");
        p.reply(&request, identity("a"));
        p.metadata();
    }
    assert!(!p.controller.reconciliation_pending());
    assert!(!p.controller.provisional_replacement());
    assert!(p.controller.can_release_runtime());
}

#[test]
fn command_state_and_message_failures_retry_only_reads_on_reselection() {
    for fail_messages in [false, true] {
        let mut p = Port::ready();
        p.handled();
        let request = p.take("get_state");
        if fail_messages {
            p.reply(&request, identity("a"));
            let messages = p.take("get_messages");
            p.fail(&messages);
            p.metadata();
        } else {
            p.fail(&request);
        }
        assert!(p.controller.reconciliation_pending());
        assert!(!p.controller.can_release_runtime());
        assert!(p.controller.state().draft.is_empty());
        let effects = p.controller.resume_reads();
        assert_eq!(effects.len(), 1);
        p.collect(effects);
        assert!(
            p.controller.resume_reads().is_empty(),
            "retry is deduplicated"
        );
        p.hydrate("a", json!([]));
        assert!(p.controller.resume_reads().is_empty());
        assert!(p
            .requests
            .iter()
            .all(|r| r["type"] != "prompt" && r["type"] != "extension_ui_response"));
        assert!(p.controller.state().draft.is_empty());
    }
}

#[test]
fn failed_barrier_is_verified_by_bounded_state_then_assistant_hydration() {
    let mut p = Port::ready();
    p.event(json!({"type":"message_end","message":assistant()}));
    let barrier = p.take("get_state");
    p.fail(&barrier);
    assert!(!p.controller.state().durable);
    assert!(p.registrations.is_empty());
    p.tick(249);
    assert!(p.requests.is_empty());
    p.tick(250);
    p.hydrate("a", json!([assistant()]));
    assert!(p.controller.state().durable);
    assert_eq!(p.registrations.len(), 1);
    assert_eq!(p.registrations[0].id, "a");
    assert!(!p.controller.reconciliation_pending());
    p.tick(10_000);
    assert!(p.requests.is_empty());
}

#[test]
fn verification_has_three_relative_retries_and_never_registers_empty_history() {
    let mut p = Port::ready();
    p.event(json!({"type":"agent_settled"}));
    p.hydrate("a", json!([]));
    for now in [250, 1_000, 2_500] {
        p.tick(now);
        // Probe deadlines can coincide with verification; reply to every read.
        while p.requests.iter().any(|r| r["type"] == "get_state") {
            p.hydrate("a", json!([]));
        }
    }
    p.tick(3_200);
    while p.requests.iter().any(|r| r["type"] == "get_state") {
        p.hydrate("a", json!([]));
    }
    assert!(!p.controller.reconciliation_pending());
    assert!(p.registrations.is_empty());
    p.tick(30_000);
    assert!(p.requests.is_empty());
}

#[test]
fn verification_transport_failures_are_bounded_and_do_not_mask_other_request_errors() {
    let mut p = Port::ready();
    p.event(json!({"type":"message_end","message":assistant()}));
    let barrier = p.take("get_state");
    let effects = p.controller.request_failed(barrier["id"].as_str().unwrap());
    p.collect(effects);
    for now in [250, 1_000, 2_500] {
        p.tick(now);
        let state = p.take("get_state");
        p.fail(&state);
    }
    assert!(!p.controller.reconciliation_pending());
    assert!(p.controller.state().error.is_none());
    let effects = p.controller.refresh();
    p.collect(effects);
    let state = p.take("get_state");
    p.fail(&state);
    assert!(
        p.controller.state().error.is_some(),
        "ordinary read failures stay visible"
    );
}

#[test]
fn predecessor_barrier_survives_successor_adoption_and_cannot_overwrite_b() {
    let mut p = Port::ready();
    p.event(json!({"type":"message_end","message":assistant()}));
    let barrier = p.take("get_state");
    let effects = p.controller.refresh();
    p.collect(effects);
    p.hydrate("b", json!([]));
    assert!(p.controller.provisional_replacement());
    assert!(p.registrations.is_empty());
    p.reply(&barrier, identity("b"));
    assert_eq!(p.registrations.len(), 1);
    assert_eq!(p.registrations[0].id, "a");
    assert_eq!(p.controller.state().identity.id, "b");
    assert!(!p.controller.state().durable);
    assert!(p.controller.state().transcript.is_empty());
    p.reply(&barrier, identity("a"));
    assert_eq!(p.registrations.len(), 1);
    assert_eq!(p.controller.state().identity.id, "b");
}

#[test]
fn stale_bootstrap_source_signal_is_typed_exact_and_persists_until_storage_ack() {
    let mut p = Port::bound("saved");
    p.start(1);
    let state = p.take("get_state");
    let mut reported = identity("fresh");
    reported["sessionFile"] = json!("/fixture/saved.jsonl");
    p.reply(&state, reported.clone());
    let source = p.controller.source_recovery().unwrap().clone();
    assert_eq!(source.id, "saved");
    assert_eq!(source.path, "/fixture/saved.jsonl");
    assert!(
        !p.controller.provisional_replacement(),
        "unsaved bootstrap recovery is not a workflow successor"
    );
    assert!(p.controller.reconciliation_pending());
    assert!(
        p.controller.state().error.is_none(),
        "recovery is not error-text classification"
    );
    assert!(!p.controller.acknowledge_source_recovery(&Identity {
        name: "wrong".into(),
        ..source.clone()
    }));
    // A failed archive is deliberately no acknowledgement. Restart must not lose the signal.
    p.start(2);
    let state = p.take("get_state");
    p.reply(&state, reported);
    assert_eq!(p.controller.source_recovery(), Some(&source));
    assert!(p.controller.acknowledge_source_recovery(&source));
    assert!(p.controller.source_recovery().is_none());
    assert!(!p.controller.acknowledge_source_recovery(&source));
    assert!(p.registrations.is_empty());
}

#[test]
fn genuine_path_changing_bootstrap_and_later_same_path_changes_are_not_source_recovery() {
    let mut p = Port::bound("saved");
    p.start(1);
    p.hydrate("b", json!([]));
    assert!(p.controller.provisional_replacement());
    assert!(p.controller.source_recovery().is_none());
    let effects = p.controller.refresh();
    p.collect(effects);
    let state = p.take("get_state");
    let mut reported = identity("c");
    reported["sessionFile"] = json!("/fixture/b.jsonl");
    p.reply(&state, reported);
    assert!(p.controller.source_recovery().is_none());
}

#[test]
fn extension_delivery_receipt_not_submission_starts_reconciliation_and_probes() {
    let mut p = Port::ready();
    p.event(json!({"type":"extension_ui_request","id":"continue","method":"confirm","title":"Implement?"}));
    let effects = p
        .controller
        .extension_response("continue", ExtensionResponse::Confirmed(true));
    p.collect(effects);
    p.take("extension_ui_response");
    assert!(!p.controller.can_release_runtime());
    p.tick(150);
    assert!(p.requests.is_empty());
    let effects = p.controller.extension_sent("continue", true);
    p.collect(effects);
    p.hydrate("a", json!([]));
    p.tick(299);
    assert!(p.requests.is_empty());
    p.tick(300);
    p.hydrate("b", json!([]));
    assert!(p.controller.provisional_replacement());
    assert!(p.controller.resume_reads().is_empty());
}

#[test]
fn cached_transcript_and_newer_draft_survive_acknowledged_release_and_restart() {
    let mut p = Port::ready();
    let effects = p.controller.refresh();
    p.collect(effects);
    p.hydrate("a", json!([assistant()]));
    p.controller.set_draft("newer draft");
    let transcript = p.controller.state().transcript.clone();
    assert!(!p.controller.release_generation(2));
    assert!(p.controller.release_generation(1));
    assert!(!p.controller.state().ready);
    assert!(!p.controller.state().working);
    assert_eq!(p.controller.state().draft, "newer draft");
    assert_eq!(p.controller.state().transcript, transcript);
    p.start(2);
    assert_eq!(p.controller.state().transcript, transcript);
    assert_eq!(p.controller.state().draft, "newer draft");
    assert!(p
        .controller
        .ingest("workflow", 1, json!({"type":"agent_start"}))
        .is_empty());
    p.hydrate("a", json!([assistant()]));
    assert_eq!(p.controller.state().draft, "newer draft");
}

#[test]
fn command_completion_remains_correlated_across_replacement_without_replaying_prompt() {
    let mut p = Port::ready();
    let effects = p.controller.submit("/mock-workflow");
    p.collect(effects);
    let prompt = p.take("prompt");
    p.event(json!({"type":"agent_start"}));
    p.hydrate("b", json!([]));
    p.tick(20_000);
    assert!(
        p.controller.submission_pending(),
        "accepted command is not a timed read"
    );
    p.reply(&prompt, json!({}));
    p.hydrate("b", json!([]));
    assert!(!p.controller.submission_pending());
    assert!(p.controller.state().draft.is_empty());
    assert!(p.controller.provisional_replacement());
    assert!(p.requests.iter().all(|r| r["type"] != "prompt"));
}

#[test]
fn persisted_assistant_hydration_promotes_provisional_successor_without_user_optimism() {
    let mut p = Port::ready();
    p.handled();
    p.hydrate("b", json!([]));
    assert!(p.controller.provisional_replacement());
    p.event(json!({"type":"agent_settled"}));
    p.hydrate("b", json!([assistant()]));
    assert!(!p.controller.provisional_replacement());
    assert!(p.controller.state().durable);
    assert_eq!(p.registrations.len(), 1);
    assert_eq!(p.registrations[0].id, "b");
    assert!(p
        .controller
        .state()
        .transcript
        .iter()
        .all(|row| row.kind != "user"));
}

#[test]
fn outstanding_probe_and_old_generation_responses_cannot_rebind_successor() {
    let mut p = Port::ready();
    p.handled();
    p.hydrate("a", json!([]));
    p.tick(150);
    let stale = p.take("get_state");
    p.tick(450);
    p.hydrate("b", json!([]));
    p.reply(&stale, identity("a"));
    assert_eq!(p.controller.state().identity.id, "b");
    p.start(2);
    assert!(p
        .controller
        .ingest(
            "workflow",
            1,
            json!({"type":"response","id":stale["id"],
        "command":"get_state","success":true,"data":identity("a")})
        )
        .is_empty());
    assert_eq!(p.controller.state().identity.id, "b");
}
