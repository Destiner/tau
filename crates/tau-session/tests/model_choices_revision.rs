use serde_json::{json, Value};
use tau_session::session::{Controller, Effect, State};

fn request(effects: &[Effect], command: &str) -> Value {
    effects
        .iter()
        .find_map(|effect| match effect {
            Effect::Send(value) if value["type"] == command => Some(value.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing {command}"))
}

fn reply(controller: &mut Controller, request: &Value, data: Value) -> Vec<Effect> {
    controller.handle(json!({"type":"response", "id":request["id"],
        "command":request["type"], "success":true, "data":data}))
}

fn model(id: &str) -> Value {
    json!({"provider":"fixture", "id":id, "name":id})
}

fn setup() -> Controller {
    let mut controller = Controller::new("choices");
    controller.bind_source("a", "/fixture/a.jsonl");
    let boot = controller.begin_generation(1);
    let follow = reply(
        &mut controller,
        &request(&boot, "get_state"),
        json!({"sessionId":"a", "sessionFile":"/fixture/a.jsonl",
            "model":model("a"), "thinkingLevel":"high"}),
    );
    reply(
        &mut controller,
        &request(&boot, "get_available_models"),
        json!({"models":[model("a"), model("b")]}),
    );
    reply(
        &mut controller,
        &request(&boot, "get_scoped_models"),
        json!({"models":[model("a")]}),
    );
    reply(
        &mut controller,
        &request(&follow, "get_available_thinking_levels"),
        json!({"levels":["low", "high"]}),
    );
    controller
}

#[test]
fn unchanged_reads_and_non_choice_state_do_not_advance_revision() {
    let mut controller = setup();
    let revision = controller.model_choices_revision();
    let refresh = controller.refresh();
    let follow = reply(
        &mut controller,
        &request(&refresh, "get_state"),
        json!({"sessionId":"a", "sessionFile":"/fixture/a.jsonl",
            "sessionName":"New title", "model":model("a"), "thinkingLevel":"high",
            "isStreaming":true}),
    );
    for (command, data) in [
        (
            "get_available_models",
            json!({"models":[model("a"), model("b")]}),
        ),
        ("get_scoped_models", json!({"models":[model("a")]})),
    ] {
        reply(&mut controller, &request(&refresh, command), data);
        assert_eq!(controller.model_choices_revision(), revision, "{command}");
    }
    reply(
        &mut controller,
        &request(&follow, "get_available_thinking_levels"),
        json!({"levels":["low", "high"]}),
    );
    controller.set_draft("a draft");
    controller.handle(json!({"type":"thinking_level_changed", "level":"high"}));
    assert_eq!(controller.model_choices_revision(), revision);
    let token = controller.begin_model_scope_read();
    assert!(controller.apply_model_scope(&token, Some(json!([]))));
    assert_eq!(controller.model_choices_revision(), revision);
}

#[test]
fn every_choice_input_advances_revision_only_when_changed() {
    let mut controller = setup();
    let mut revision = controller.model_choices_revision();
    let token = controller.begin_model_scope_read();
    assert!(controller.apply_model_scope(&token, Some(json!(["fixture/b"]))));
    assert_eq!(controller.model_choices_revision(), revision + 1);
    revision += 1;
    assert!(controller.apply_model_scope(&token, Some(json!(["fixture/b"]))));
    assert!(!controller.apply_model_scope(&token, Some(json!({}))));
    assert_eq!(controller.model_choices_revision(), revision);
    controller.handle(json!({"type":"thinking_level_changed", "level":"low"}));
    assert_eq!(controller.model_choices_revision(), revision + 1);
    revision += 1;
    let refresh = controller.refresh();
    for (command, data) in [
        (
            "get_available_models",
            json!({"models":[model("b"), model("a")]}),
        ),
        ("get_scoped_models", json!({"models":[model("b")]})),
    ] {
        reply(&mut controller, &request(&refresh, command), data);
        assert_eq!(
            controller.model_choices_revision(),
            revision + 1,
            "{command}"
        );
        revision += 1;
    }
    let follow = reply(
        &mut controller,
        &request(&refresh, "get_state"),
        json!({"sessionId":"a", "sessionFile":"/fixture/a.jsonl",
            "model":model("b"), "thinkingLevel":"low"}),
    );
    assert_eq!(controller.model_choices_revision(), revision + 1);
    revision += 1;
    reply(
        &mut controller,
        &request(&follow, "get_available_thinking_levels"),
        json!({"levels":["off", "high"]}),
    );
    assert_eq!(controller.model_choices_revision(), revision + 1);
    revision += 1;
    let refresh = controller.refresh();
    let mut renamed = model("b");
    renamed["name"] = json!("Readable fallback");
    reply(
        &mut controller,
        &request(&refresh, "get_state"),
        json!({"sessionId":"a", "sessionFile":"/fixture/a.jsonl",
            "model":renamed, "thinkingLevel":"low"}),
    );
    assert_eq!(controller.model_choices_revision(), revision + 1);
}

#[test]
fn repeated_stream_and_background_ingress_never_rebuild_choices() {
    let mut controller = setup();
    let mut shown_revision = None;
    let mut projections = 0;
    let mut sync = |controller: &Controller| {
        let revision = controller.model_choices_revision();
        if shown_revision != Some(revision) {
            shown_revision = Some(revision);
            projections += 1;
            assert_eq!(controller.state().selectable_models().len(), 1);
        }
    };
    sync(&controller);
    controller.handle(json!({"type":"message_start", "message":{
        "role":"assistant", "timestamp":1, "content":[]}}));
    for _ in 0..1000 {
        controller.handle(json!({"type":"message_update", "assistantMessageEvent":{
            "type":"text_delta", "contentIndex":0, "delta":"text"}}));
        sync(&controller);
        controller.handle(
            json!({"type":"tool_execution_update", "toolCallId":"background",
            "partialResult":{"content":[{"type":"text", "text":"output"}]}}),
        );
        sync(&controller);
    }
    assert_eq!(controller.state().transcript[0].text.len(), 4000);
    assert_eq!(projections, 1);
}

#[test]
fn generation_identity_and_stale_scope_fences_preserve_revision() {
    let mut controller = setup();
    let old_token = controller.begin_model_scope_read();
    let refresh = controller.refresh();
    let old_scope = request(&refresh, "get_scoped_models");
    let revision = controller.model_choices_revision();
    let follow = reply(
        &mut controller,
        &request(&refresh, "get_state"),
        json!({"sessionId":"b", "sessionFile":"/fixture/b.jsonl",
            "model":model("a"), "thinkingLevel":"high"}),
    );
    assert!(controller.model_choices_revision() > revision);
    let revision = controller.model_choices_revision();
    assert!(!controller.apply_model_scope(&old_token, Some(json!(["fixture/b"]))));
    reply(&mut controller, &old_scope, json!({"models":[model("b")]}));
    assert_eq!(controller.model_choices_revision(), revision);
    reply(
        &mut controller,
        &request(&follow, "get_available_models"),
        json!({"models":[model("a")]}),
    );
    let token = controller.begin_model_scope_read();
    let revision = controller.model_choices_revision();
    controller.begin_generation(2);
    assert_eq!(controller.model_choices_revision(), revision + 1);
    let revision = controller.model_choices_revision();
    assert!(!controller.apply_model_scope(&token, None));
    reply(&mut controller, &old_scope, json!({"models":[model("b")]}));
    assert_eq!(controller.model_choices_revision(), revision);
    assert!(controller.state().selectable_models().is_empty());
}

#[test]
fn optional_scope_failure_invalidates_once_and_stale_failure_cannot_invalidate() {
    let mut controller = setup();
    let refresh = controller.refresh_models();
    let scope = request(&refresh, "get_scoped_models");
    let revision = controller.model_choices_revision();
    controller.request_failed(scope["id"].as_str().unwrap());
    assert_eq!(controller.model_choices_revision(), revision + 1);
    assert_eq!(controller.state().selectable_models().len(), 2);
    let revision = controller.model_choices_revision();
    controller.request_failed(scope["id"].as_str().unwrap());
    assert_eq!(controller.model_choices_revision(), revision);
    let refresh = controller.refresh_models();
    controller.request_failed(
        request(&refresh, "get_scoped_models")["id"]
            .as_str()
            .unwrap(),
    );
    assert_eq!(controller.model_choices_revision(), revision);
}

#[test]
fn direct_state_edits_and_deserialization_keep_scope_order_first_match_and_fallback() {
    let mut first = model("a");
    first["name"] = json!("First catalogue entry");
    let mut second = model("a");
    second["name"] = json!("Second catalogue entry");
    let mut serialized = serde_json::to_value(State::default()).unwrap();
    serialized["models"] = json!([first, second, model("b")]);
    serialized["scoped_models"] = json!([model("b"), model("a"), model("b"), model("missing")]);
    let mut state: State = serde_json::from_value(serialized).unwrap();
    assert_eq!(
        state.selectable_models(),
        vec![&state.models[2], &state.models[0]]
    );
    state.models.remove(2);
    assert_eq!(state.selectable_models(), vec![&state.models[0]]);
    state.models = vec![model("outside-scope")];
    state.model = model("unavailable-current");
    assert_eq!(state.selectable_models(), vec![&state.models[0]]);
}
