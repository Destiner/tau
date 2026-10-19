use serde_json::{json, Value};
use tau_session::session::{Controller, Effect, State};

fn request(effects: &[Effect], command: &str) -> Value {
    effects
        .iter()
        .find_map(|e| match e {
            Effect::Send(v) if v["type"] == command => Some(v.clone()),
            _ => None,
        })
        .unwrap()
}
fn reply(c: &mut Controller, r: &Value, data: Value) -> Vec<Effect> {
    c.handle(json!({"type":"response","id":r["id"],"command":r["type"],"success":true,"data":data}))
}
#[test]
fn concurrent_host_settings_reads_are_latest_wins_and_disconnect_fenced() {
    let mut c = Controller::new("scope");
    c.begin_generation(1);
    let older = c.begin_model_scope_read();
    let newer = c.begin_model_scope_read();
    assert!(c.apply_model_scope(&newer, Some(json!(["openai/*"]))));
    assert!(!c.apply_model_scope(&older, Some(json!(["anthropic/*"]))));
    assert!(!c.apply_model_scope(&older, None));
    c.transport_failed(1);
    assert!(!c.apply_model_scope(&newer, None));
}
#[test]
fn optional_scope_failures_need_exact_correlation() {
    let mut c = Controller::new("scope");
    let boot = c.begin_generation(1);
    let scope = request(&boot, "get_scoped_models");
    let pending = c.pending_count();
    for (id, command) in [
        (json!("unknown"), json!("get_scoped_models")),
        (scope["id"].clone(), json!("get_available_models")),
    ] {
        c.handle(json!({"type":"response","id":id,"command":command,"success":false}));
        assert_eq!(c.pending_count(), pending);
    }
    c.handle(
        json!({"type":"response","id":scope["id"],"command":"get_scoped_models","success":false}),
    );
    assert_eq!(c.pending_count(), pending - 1);
    assert!(c.state().error.is_none());
}
#[test]
fn confirmed_model_setting_refreshes_catalogue_and_scope_and_fences_old_reads() {
    let mut c = Controller::new("scope");
    c.bind_source("a", "/fixture/a.jsonl");
    let boot = c.begin_generation(1);
    reply(
        &mut c,
        &request(&boot, "get_state"),
        json!({"sessionId":"a","sessionFile":"/fixture/a.jsonl"}),
    );
    let token = c.begin_model_scope_read();
    let set = request(&c.set_model("openai", "gpt-5.5"), "set_model");
    let follow = reply(&mut c, &set, json!({"provider":"openai","id":"gpt-5.5"}));
    request(&follow, "get_state");
    request(&follow, "get_available_models");
    request(&follow, "get_scoped_models");
    assert!(!c.apply_model_scope(&token, Some(json!(["sonnet"]))));
}
#[test]
fn current_model_unavailable_in_catalogue_is_display_only_and_old_state_deserializes() {
    let mut state = State::default();
    state.models = vec![json!({"provider":"fixture","id":"available"})];
    state.model = json!({"provider":"fixture","id":"pinned-but-unavailable"});
    assert_eq!(state.selectable_models(), vec![&state.models[0]]);
    let mut old = serde_json::to_value(&state).unwrap();
    old.as_object_mut().unwrap().remove("model_scope");
    old.as_object_mut().unwrap().remove("scoped_models");
    let restored: State = serde_json::from_value(old).unwrap();
    assert_eq!(restored.selectable_models(), vec![&restored.models[0]]);
}
