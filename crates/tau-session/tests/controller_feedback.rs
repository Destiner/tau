use serde_json::{json, Value};
use tau_session::session::{Controller, Effect, ExtensionResponse, REQUEST_TIMEOUT_MS};

// Real RPC envelopes with explicit host Written completion; no invented Pi acknowledgment.
struct RpcFake {
    c: Controller,
}
impl RpcFake {
    fn ready() -> Self {
        let mut c = Controller::new("fake");
        c.bind_source("a", "/fixture/a");
        let mut work = c.begin_generation(1);
        while let Some(effect) = work.pop() {
            if let Effect::Send(request) = effect {
                let data = match request["type"].as_str().unwrap() {
                    "get_state" => identity("a"),
                    "get_messages" => json!({"messages":[]}),
                    "get_available_models" => json!({"models":[]}),
                    "get_scoped_models" => json!({"models":[]}),
                    "get_commands" => json!({"commands":[]}),
                    "get_available_thinking_levels" => json!({"levels":[]}),
                    other => panic!("unexpected bootstrap {other}"),
                };
                work.extend(reply(&mut c, &request, true, data));
            }
        }
        assert!(c.state().ready);
        assert_eq!(c.pending_count(), 0);
        Self { c }
    }
    fn ask(&mut self, id: &str, timeout: u64) {
        self.c.ingest(
            "fake",
            self.c.generation(),
            json!({
                "type":"extension_ui_request", "id":id, "method":"input", "timeout":timeout
            }),
        );
    }
    fn answer(&mut self, id: &str, response: ExtensionResponse) -> Value {
        command(
            &self.c.extension_response(id, response),
            "extension_ui_response",
        )
    }
    fn written(&mut self, id: &str, success: bool) -> Vec<Effect> {
        self.c.extension_sent(id, success)
    }
}
fn identity(id: &str) -> Value {
    json!({"sessionId":id,"sessionFile":format!("/fixture/{id}"),"isStreaming":false})
}
fn command(effects: &[Effect], typ: &str) -> Value {
    effects
        .iter()
        .find_map(|effect| match effect {
            Effect::Send(v) if v["type"] == typ => Some(v.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing {typ}: {effects:?}"))
}
fn reply(c: &mut Controller, request: &Value, success: bool, data: Value) -> Vec<Effect> {
    c.ingest(
        "fake",
        c.generation(),
        json!({"type":"response", "id":request["id"],
        "command":request["type"],"success":success,"data":data}),
    )
}
fn failed_answer(f: &mut RpcFake, id: &str) {
    f.ask(id, 100);
    f.answer(id, ExtensionResponse::Value("answer".into()));
    f.written(id, false);
}

#[test]
fn extension_failure_clears_only_on_admitted_retry_and_written_success() {
    let mut f = RpcFake::ready();
    failed_answer(&mut f, "ask");
    assert!(f.c.state().error.is_some());
    assert!(f
        .c
        .extension_response("missing", ExtensionResponse::Cancelled)
        .is_empty());
    assert!(f
        .c
        .extension_response("ask", ExtensionResponse::Confirmed(true))
        .is_empty());
    assert!(f.c.state().error.is_some());
    assert_eq!(
        f.answer("ask", ExtensionResponse::Cancelled)["cancelled"],
        true
    );
    assert!(f.c.state().error.is_none());
    assert!(f.c.state().extension_dialogs[0].submitting);
    assert!(f
        .c
        .extension_response("ask", ExtensionResponse::Cancelled)
        .is_empty());
    let effects = f.written("ask", true);
    assert!(!command(&effects, "get_state").is_null());
    assert!(f.c.state().extension_dialogs.is_empty());
    assert!(f.c.state().error.is_none());
    assert!(f.written("ask", false).is_empty());
    assert!(f.c.state().error.is_none());
}

#[test]
fn expiration_clears_failure_but_accepted_cancel_survives_deadline_and_agent_end() {
    let mut f = RpcFake::ready();
    failed_answer(&mut f, "expired");
    assert!(f.c.tick(100).is_empty());
    assert!(f.c.state().extension_dialogs.is_empty());
    assert!(f.c.state().error.is_none());
    f.ask("pending", 100);
    f.answer("pending", ExtensionResponse::Cancelled);
    f.c.tick(200);
    f.c.handle(json!({"type":"agent_settled"}));
    assert!(f.c.state().extension_dialogs[0].submitting);
    assert!(f.written("missing", true).is_empty());
    assert!(f.c.state().extension_dialogs[0].submitting);
    f.written("pending", false);
    assert!(!f.c.state().extension_dialogs[0].submitting);
    f.c.tick(200);
    assert!(f.c.state().extension_dialogs.is_empty());
    assert!(f.c.state().error.is_none());
    f.ask("late", 10);
    f.answer("late", ExtensionResponse::Value("exact".into()));
    f.c.tick(210);
    assert!(f.c.state().extension_dialogs[0].submitting);
    f.written("late", true);
    assert!(f.c.state().extension_dialogs.is_empty());
}

#[test]
fn unrelated_incident_survives_retry_success_expiration_and_replacement() {
    for operation in ["retry", "expire", "identity", "generation"] {
        let mut f = RpcFake::ready();
        failed_answer(&mut f, "ask");
        f.c.handle(json!({"type":"extension_error"}));
        let unrelated = f.c.state().error.clone();
        match operation {
            "retry" => {
                f.answer("ask", ExtensionResponse::Cancelled);
                f.written("ask", true);
            }
            "expire" => {
                f.c.tick(100);
            }
            "identity" => {
                let request = command(&f.c.refresh(), "get_state");
                // Refresh is an explicit global dismissal. Create a newer incident after it.
                f.c.handle(json!({"type":"extension_error"}));
                reply(&mut f.c, &request, true, identity("b"));
            }
            "generation" => {
                f.c.begin_generation(2);
            }
            _ => unreachable!(),
        }
        assert_eq!(f.c.state().error, unrelated, "{operation}");
    }
}

#[test]
fn owned_failure_is_retired_on_identity_generation_and_connection_replacement() {
    for operation in ["identity", "generation", "connection"] {
        let mut f = RpcFake::ready();
        f.ask("ask", 100);
        let request = command(
            &f.c.handle(json!({"type":"session_info_changed"})),
            "get_state",
        );
        f.answer("ask", ExtensionResponse::Cancelled);
        f.written("ask", false);
        match operation {
            "identity" => {
                reply(&mut f.c, &request, true, identity("b"));
            }
            "generation" => {
                f.c.begin_generation(2);
            }
            "connection" => {
                f.c.transport_failed(1);
            }
            _ => unreachable!(),
        }
        assert!(f.c.state().extension_dialogs.is_empty());
        assert_ne!(
            f.c.state().error.as_deref(),
            Some("The extension response could not be sent. Try again.")
        );
        assert!(f.written("ask", true).is_empty());
        f.c.ingest(
            "fake",
            0,
            json!({"type":"extension_ui_request","id":"old","method":"input"}),
        );
        assert!(f.c.state().extension_dialogs.is_empty());
    }
}

#[test]
fn queue_failure_is_local_owned_dismissible_and_timeout_does_not_resend() {
    let mut f = RpcFake::ready();
    f.c.handle(json!({"type":"extension_error"}));
    let unrelated = f.c.state().error.clone();
    f.c.handle(json!({"type":"queue_update","steering":["old"],"followUp":[]}));
    let request = command(&f.c.clear_queue(), "clear_queue");
    f.c.request_failed("missing");
    assert!(f.c.queue_feedback().is_none());
    f.c.request_failed(request["id"].as_str().unwrap());
    assert!(f.c.queue_feedback().is_some());
    assert_eq!(f.c.state().error, unrelated);
    assert_eq!(f.c.queue_snapshot().steering, ["old"]);
    f.c.dismiss_queue_feedback();
    assert!(f.c.queue_feedback().is_none());
    assert_eq!(f.c.state().error, unrelated);
    let retry = command(&f.c.clear_queue(), "clear_queue");
    reply(&mut f.c, &request, false, json!({}));
    assert!(f.c.queue_feedback().is_none());
    assert!(f.c.tick(REQUEST_TIMEOUT_MS).is_empty());
    assert!(!f.c.queue_busy());
    assert!(f.c.queue_feedback().unwrap().contains("not confirmed"));
    assert_eq!(f.c.state().error, unrelated);
    reply(&mut f.c, &retry, true, json!({}));
    assert_eq!(f.c.queue_snapshot().steering, ["old"]);
    assert!(f.c.queue_feedback().is_some());
    let current = command(&f.c.clear_queue(), "clear_queue");
    assert!(f.c.queue_feedback().is_none());
    f.c.handle(json!({"type":"queue_update","steering":["new"],"followUp":[]}));
    reply(&mut f.c, &current, true, json!({}));
    assert_eq!(f.c.queue_snapshot().steering, ["new"]);
    assert!(f.c.queue_feedback().is_none());
    let genuine = command(&f.c.clear_queue(), "clear_queue");
    reply(&mut f.c, &genuine, true, json!({}));
    assert!(f.c.queue_snapshot().steering.is_empty());
}

#[test]
fn queue_feedback_reset_and_old_identity_request_fences() {
    let mut f = RpcFake::ready();
    let clear = command(&f.c.clear_queue(), "clear_queue");
    reply(&mut f.c, &clear, false, json!({}));
    assert!(f.c.queue_feedback().is_some());
    let state = command(
        &f.c.handle(json!({"type":"session_info_changed"})),
        "get_state",
    );
    reply(&mut f.c, &state, true, identity("b"));
    assert!(f.c.queue_feedback().is_none());
    reply(&mut f.c, &clear, false, json!({}));
    f.c.request_failed(clear["id"].as_str().unwrap());
    assert!(f.c.queue_feedback().is_none());
    let current = command(&f.c.clear_queue(), "clear_queue");
    reply(&mut f.c, &current, false, json!({}));
    f.c.begin_generation(2);
    assert!(f.c.queue_feedback().is_none());
    f.c.ingest(
        "fake",
        1,
        json!({"type":"response","id":current["id"],"command":"clear_queue","success":false}),
    );
    assert!(f.c.queue_feedback().is_none());
}

#[test]
fn individual_restore_preserves_duplicates_whitespace_order_and_newer_draft() {
    let mut f = RpcFake::ready();
    f.c.handle(
        json!({"type":"queue_update","steering":["same",""," \n ","same"],"followUp":["tail"]}),
    );
    f.c.begin_generation(2);
    let original = f.c.state().recovered_drafts.clone();
    for (index, expected) in [(99, "same"), (1, ""), (2, "wrong")] {
        assert!(!f.c.restore_recovered_draft(index, expected));
        assert_eq!(f.c.state().recovered_drafts, original);
    }
    f.c.set_draft(" ");
    assert!(!f.c.restore_recovered_draft(3, "same"));
    assert_eq!(f.c.state().recovered_drafts, original);
    f.c.set_draft("");
    assert!(f.c.restore_recovered_draft(3, "same"));
    assert_eq!(f.c.state().draft, "same");
    assert_eq!(f.c.state().recovered_drafts, ["same", "", " \n ", "tail"]);
    assert!(!f.c.restore_recovered_draft(0, "same"));
    f.c.set_draft("");
    assert!(f.c.restore_recovered_draft(2, " \n "));
    assert_eq!(f.c.state().draft, " \n ");
    assert_eq!(f.c.state().recovered_drafts, ["same", "", "tail"]);
}

#[test]
fn state_queue_feedback_is_backward_compatible_and_dismiss_is_narrow() {
    let f = RpcFake::ready();
    let mut old = serde_json::to_value(f.c.state()).unwrap();
    old.as_object_mut().unwrap().remove("queue_feedback");
    let state: tau_session::session::State = serde_json::from_value(old).unwrap();
    assert!(state.queue_feedback.is_none());
}
