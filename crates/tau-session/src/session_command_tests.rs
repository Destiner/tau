use super::*;

fn reply(controller: &mut Controller, effect: &Effect, data: Value) -> Vec<Effect> {
    let Effect::Send(request) = effect else {
        panic!("expected RPC")
    };
    controller.handle(json!({"type":"response", "id":request["id"],
        "command":request["type"], "success":true, "data":data}))
}
fn catalog() -> Value {
    json!({"commands":[{"name":"review", "source":"prompt", "description":"Review changes"}]})
}
#[test]
fn command_revision_changes_only_with_authoritative_catalog_and_reset() {
    let mut c = Controller::new("a");
    c.begin_generation(1);
    let read = c.send("get_commands", json!({}), false);
    reply(&mut c, &read, catalog());
    let revision = c.commands_revision();
    assert!(revision > 0);
    let read = c.send("get_commands", json!({}), false);
    reply(&mut c, &read, catalog());
    assert_eq!(c.commands_revision(), revision);
    c.handle(json!({"type":"message_start", "message":{"role":"assistant", "content":[]}}));
    for _ in 0..1000 {
        c.handle(json!({"type":"message_update", "assistantMessageEvent":{"type":"text_delta", "delta":"x"}}));
    }
    assert_eq!(c.commands_revision(), revision);
    c.begin_generation(2);
    assert!(c.state.commands.is_empty());
    assert_eq!(c.commands_revision(), revision + 1);
    reply(&mut c, &read, catalog());
    assert!(
        c.state.commands.is_empty(),
        "old generation cannot restore options"
    );
    assert_eq!(c.commands_revision(), revision + 1);
}
#[test]
fn identity_replacement_clears_options_and_fences_old_catalog_reply() {
    let mut c = Controller::new("a");
    c.bind_source("a", "/a.jsonl");
    c.begin_generation(1);
    let read = c.send("get_commands", json!({}), false);
    reply(&mut c, &read, catalog());
    let revision = c.commands_revision();
    let stale = c.send("get_commands", json!({}), false);
    let state = c.send("get_state", json!({}), false);
    let effects = reply(
        &mut c,
        &state,
        json!({"sessionId":"b", "sessionFile":"/b.jsonl"}),
    );
    assert!(c.state.commands.is_empty());
    assert_eq!(c.commands_revision(), revision + 1);
    reply(&mut c, &stale, catalog());
    assert!(c.state.commands.is_empty());
    let read = effects
        .iter()
        .find(|e| matches!(e, Effect::Send(v) if v["type"] == "get_commands"))
        .unwrap();
    reply(&mut c, read, catalog());
    assert_eq!(c.state.commands.len(), 1);
    assert_eq!(c.commands_revision(), revision + 2);
}
