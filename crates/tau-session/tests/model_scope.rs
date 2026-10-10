use serde_json::{json, Value};
use tau_session::session::{Controller, Effect, REQUEST_TIMEOUT_MS};

fn request(effects: &[Effect], command: &str) -> Value {
    effects
        .iter()
        .find_map(|effect| match effect {
            Effect::Send(v) if v["type"] == command => Some(v.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing {command}"))
}
fn reply(c: &mut Controller, request: &Value, data: Value) -> Vec<Effect> {
    let wire = serde_json::to_string(&json!({"type":"response", "id":request["id"],
        "command":request["type"], "success":true, "data":data}))
    .unwrap();
    c.ingest(
        "scope",
        c.generation(),
        serde_json::from_str(&wire).unwrap(),
    )
}
fn model(provider: &str, id: &str) -> Value {
    json!({"provider":provider,"id":id,"name":id,"reasoning":false})
}
fn catalog() -> Vec<Value> {
    vec![
        model("anthropic", "claude-opus-4-8"),
        model("anthropic", "claude-sonnet-4-5"),
        model("anthropic", "claude-sonnet-4-5-20250929"),
        model("openai", "gpt-5.5"),
        model("openrouter", "deepseek/deepseek-v4-pro"),
    ]
}
fn ids(c: &Controller) -> Vec<String> {
    c.state()
        .selectable_models()
        .iter()
        .map(|m| {
            format!(
                "{}/{}",
                m["provider"].as_str().unwrap(),
                m["id"].as_str().unwrap()
            )
        })
        .collect()
}
fn setup() -> (Controller, Vec<Effect>) {
    let mut c = Controller::new("scope");
    c.bind_source("a", "/fixture/a.jsonl");
    let boot = c.begin_generation(1);
    let follow = reply(
        &mut c,
        &request(&boot, "get_state"),
        json!({"sessionId":"a", "sessionFile":"/fixture/a.jsonl", "model":model("openai", "gpt-5.5")}),
    );
    reply(
        &mut c,
        &request(&follow, "get_messages"),
        json!({"messages":[]}),
    );
    reply(
        &mut c,
        &request(&follow, "get_available_thinking_levels"),
        json!({"levels":[]}),
    );
    reply(
        &mut c,
        &request(&boot, "get_messages"),
        json!({"messages":[]}),
    );
    reply(
        &mut c,
        &request(&boot, "get_commands"),
        json!({"commands":[]}),
    );
    reply(
        &mut c,
        &request(&boot, "get_available_models"),
        json!({"models":catalog()}),
    );
    (c, boot)
}
#[test]
fn real_json_scope_filters_full_catalogue_in_scope_order_not_current_model() {
    let (mut c, boot) = setup();
    let request = request(&boot, "get_scoped_models");
    assert_eq!(request["type"], "get_scoped_models");
    reply(
        &mut c,
        &request,
        json!({"models":[
            {"model":model("anthropic", "claude-sonnet-4-5"),"thinkingLevel":"high"},
            {"model":model("anthropic", "claude-opus-4-8")},
            {"model":model("anthropic", "claude-sonnet-4-5")},
            {"model":model("missing", "unavailable")}
        ]}),
    );
    assert_eq!(c.state().models, catalog());
    assert_eq!(
        ids(&c),
        ["anthropic/claude-sonnet-4-5", "anthropic/claude-opus-4-8"]
    );
    assert_eq!(c.state().model, model("openai", "gpt-5.5"));
    assert!(c.state().error.is_none());
}
#[test]
fn vue_settings_pattern_vectors_and_conservative_fallbacks() {
    let (mut c, _) = setup();
    let cases = [
        (
            json!([]),
            catalog()
                .iter()
                .map(|m| {
                    format!(
                        "{}/{}",
                        m["provider"].as_str().unwrap(),
                        m["id"].as_str().unwrap()
                    )
                })
                .collect::<Vec<_>>(),
        ),
        (
            json!(["openai/gpt-5.5", "anthropic/claude-opus-4-8"]),
            vec!["openai/gpt-5.5".into(), "anthropic/claude-opus-4-8".into()],
        ),
        (
            json!(["gpt-5.5", "openai/gpt-5.5"]),
            vec!["openai/gpt-5.5".into()],
        ),
        (
            json!(["openai/gpt-5.5:high"]),
            vec!["openai/gpt-5.5".into()],
        ),
        (
            json!(["sonnet"]),
            vec!["anthropic/claude-sonnet-4-5".into()],
        ),
        (
            json!(["*sonnet*:medium"]),
            vec![
                "anthropic/claude-sonnet-4-5".into(),
                "anthropic/claude-sonnet-4-5-20250929".into(),
            ],
        ),
        (
            json!(["openai/*", "openrouter/*"]),
            vec!["openai/gpt-5.5".into()],
        ),
        (
            json!(["openrouter/**"]),
            vec!["openrouter/deepseek/deepseek-v4-pro".into()],
        ),
        (json!(["openai/gpt-?.[45]"]), vec!["openai/gpt-5.5".into()]),
        (
            json!([null, 123, "  OPENAI / GPT-5.5  "]),
            vec!["openai/gpt-5.5".into()],
        ),
    ];
    for (patterns, expected) in cases {
        assert!(c.apply_model_scope(&c.model_scope_token(), Some(patterns.clone())));
        assert_eq!(ids(&c), expected, "{patterns}");
    }
    c.apply_model_scope(&c.model_scope_token(), Some(json!(["mistral/*", "  "])));
    assert_eq!(c.state().selectable_models().len(), catalog().len());
    c.apply_model_scope(&c.model_scope_token(), Some(json!(["gpt-5.5"])));
    assert!(!c.apply_model_scope(&c.model_scope_token(), Some(json!({}))));
    assert_eq!(ids(&c), ["openai/gpt-5.5"]);
    c.apply_model_scope(&c.model_scope_token(), None);
    assert_eq!(c.state().selectable_models().len(), catalog().len());
}
#[test]
fn unsupported_timeout_and_malformed_scope_are_optional_and_use_settings() {
    for data in [
        json!(null),
        json!({}),
        json!({"models":[]}),
        json!({"models":[{"id":"broken"}]}),
        json!({"models":[model("openai", "gpt-5.5"), null]}),
        json!({"models":[model("missing", "unavailable")]}),
    ] {
        let (mut c, boot) = setup();
        c.apply_model_scope(&c.model_scope_token(), Some(json!(["sonnet"])));
        reply(&mut c, &request(&boot, "get_scoped_models"), data);
        assert_eq!(ids(&c), ["anthropic/claude-sonnet-4-5"]);
        assert!(c.state().error.is_none());
    }
    for failure in ["unsupported", "timeout", "transport"] {
        let (mut c, boot) = setup();
        c.apply_model_scope(&c.model_scope_token(), Some(json!(["sonnet"])));
        let probe = request(&boot, "get_scoped_models");
        match failure {
            "unsupported" => {
                c.handle(json!({"type":"response","id":probe["id"],"command":"get_scoped_models","success":false,"error":"Unknown command"}));
            }
            "timeout" => {
                c.tick(REQUEST_TIMEOUT_MS);
            }
            _ => {
                c.request_failed(probe["id"].as_str().unwrap());
            }
        }
        assert!(c.state().ready, "{failure}");
        assert!(c.state().error.is_none(), "{failure}");
        assert_eq!(ids(&c), ["anthropic/claude-sonnet-4-5"]);
        reply(
            &mut c,
            &probe,
            json!({"models":[model("openai", "gpt-5.5")]}),
        );
        assert_eq!(ids(&c), ["anthropic/claude-sonnet-4-5"]);
    }
}
#[test]
fn superseded_refresh_response_and_failure_cannot_overwrite_scope() {
    let (mut c, boot) = setup();
    let token = c.model_scope_token();
    let refresh = c.refresh();
    assert!(!c.apply_model_scope(&token, Some(json!(["sonnet"]))));
    let latest = request(&refresh, "get_scoped_models");
    reply(
        &mut c,
        &latest,
        json!({"models":[model("openai", "gpt-5.5")]}),
    );
    let stale = request(&boot, "get_scoped_models");
    reply(
        &mut c,
        &stale,
        json!({"models":[model("anthropic", "claude-opus-4-8")]}),
    );
    assert_eq!(ids(&c), ["openai/gpt-5.5"]);
    c.request_failed(stale["id"].as_str().unwrap());
    assert_eq!(ids(&c), ["openai/gpt-5.5"]);
}
#[test]
fn generation_and_session_replacement_clear_scope_and_fence_async_reads() {
    let (mut c, boot) = setup();
    let old_token = c.model_scope_token();
    c.apply_model_scope(&old_token, Some(json!(["sonnet"])));
    let refresh = c.refresh();
    let follow = reply(
        &mut c,
        &request(&refresh, "get_state"),
        json!({"sessionId":"b","sessionFile":"/fixture/b.jsonl"}),
    );
    assert!(c.state().models.is_empty());
    assert!(!c.apply_model_scope(&old_token, Some(json!(["sonnet"]))));
    reply(
        &mut c,
        &request(&boot, "get_scoped_models"),
        json!({"models":[model("anthropic", "claude-sonnet-4-5")]}),
    );
    reply(
        &mut c,
        &request(&follow, "get_available_models"),
        json!({"models":catalog()}),
    );
    assert_eq!(c.state().selectable_models().len(), catalog().len());
    let token = c.model_scope_token();
    let old_probe = request(&follow, "get_scoped_models");
    let next = c.begin_generation(2);
    assert!(!c.apply_model_scope(&token, Some(json!(["sonnet"]))));
    assert!(c.state().models.is_empty());
    reply(
        &mut c,
        &old_probe,
        json!({"models":[model("anthropic", "claude-sonnet-4-5")]}),
    );
    reply(
        &mut c,
        &request(&next, "get_available_models"),
        json!({"models":catalog()}),
    );
    assert_eq!(c.state().selectable_models().len(), catalog().len());
}
#[test]
fn scope_can_arrive_before_catalogue_and_invalid_catalogue_entries_are_removed() {
    let mut c = Controller::new("scope");
    let boot = c.begin_generation(1);
    reply(
        &mut c,
        &request(&boot, "get_scoped_models"),
        json!({"models":[model("openai", "gpt-5.5")]}),
    );
    assert!(ids(&c).is_empty());
    reply(
        &mut c,
        &request(&boot, "get_available_models"),
        json!({"models":[null, {"provider":"","id":"bad"},model("openai", "gpt-5.5"),model("anthropic", "claude-opus-4-8")]}),
    );
    assert_eq!(c.state().models.len(), 2);
    assert_eq!(ids(&c), ["openai/gpt-5.5"]);
}
