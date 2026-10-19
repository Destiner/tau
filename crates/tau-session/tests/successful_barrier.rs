use serde_json::{json, Value};
use tau_session::session::{Controller, Effect};

fn request(effects: &[Effect], command: &str) -> Value {
    effects
        .iter()
        .find_map(|effect| match effect {
            Effect::Send(value) if value["type"] == command => Some(value.clone()),
            _ => None,
        })
        .unwrap()
}
fn reply(c: &mut Controller, request: &Value, success: bool, data: Value) -> Vec<Effect> {
    c.handle(
        json!({"type":"response", "id":request["id"], "command":request["type"],
        "success":success, "data":data}),
    )
}
fn state(id: &str, name: &str) -> Value {
    json!({"sessionId":id, "sessionFile":format!("/fixture/{id}.jsonl"),
        "sessionName":name, "model":{"id":name}, "isStreaming":false})
}

#[test]
fn exact_successful_empty_completion_barrier_registers_only_its_captured_source() {
    for replacement in [false, true] {
        for success in [false, true] {
            let mut c = Controller::new("runtime");
            c.bind_source("a", "/fixture/a.jsonl");
            let bootstrap = c.begin_generation(1);
            let reads = reply(
                &mut c,
                &request(&bootstrap, "get_state"),
                true,
                state("a", "initial"),
            );
            reply(
                &mut c,
                &request(&reads, "get_messages"),
                true,
                json!({"messages":[]}),
            );
            let completed = c.handle(json!({"type":"message_end", "message":{
                "role":"assistant", "timestamp":1, "content":[]}}));
            let barrier = request(&completed, "get_state");
            let refresh = c.refresh();
            let id = if replacement { "b" } else { "a" };
            reply(
                &mut c,
                &request(&refresh, "get_state"),
                true,
                state(id, "newer"),
            );
            let effects = reply(&mut c, &barrier, success, state("a", "older"));
            let registrations: Vec<_> = effects
                .iter()
                .filter_map(|effect| match effect {
                    Effect::RegisterSession {
                        id,
                        path,
                        generation,
                        ..
                    } => Some((id.as_str(), path.as_str(), *generation)),
                    _ => None,
                })
                .collect();
            assert_eq!(
                registrations,
                if success {
                    vec![("a", "/fixture/a.jsonl", 1)]
                } else {
                    vec![]
                }
            );
            assert_eq!(c.state().identity.id, id);
            assert_eq!(c.state().identity.name, "newer");
            assert_eq!(c.state().model["id"], "newer");
            assert_eq!(c.state().durable, success && !replacement);
            assert!(c.prepare_exit().is_empty());
        }
    }
}

#[test]
fn aborted_and_failed_empty_assistant_are_not_successful_completion_proof() {
    for reason in ["aborted", "error"] {
        let mut c = Controller::new("runtime");
        c.bind_source("a", "/fixture/a.jsonl");
        c.begin_generation(1);
        let completed = c.handle(json!({"type":"message_end", "message":{
            "role":"assistant", "timestamp":1, "content":[], "stopReason":reason}}));
        let effects = reply(
            &mut c,
            &request(&completed, "get_state"),
            true,
            state("a", "name"),
        );
        assert!(!effects
            .iter()
            .any(|e| matches!(e, Effect::RegisterSession { .. })));
        assert!(!c.state().durable);
        assert!(c.prepare_exit().is_empty());
    }
}
