use serde_json::{json, Value};
use tau_session::session::{Controller, Effect};

fn request(effects: &[Effect], command: &str) -> Value {
    effects
        .iter()
        .find_map(|e| match e {
            Effect::Send(v) if v["type"] == command => Some(v.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing {command}"))
}

fn reply(c: &mut Controller, r: &Value, data: Value) -> Vec<Effect> {
    c.handle(json!({"type":"response","id":r["id"],"command":r["type"],
        "success":true,"data":data}))
}

fn model(id: &str) -> Value {
    json!({"provider":"fixture","id":id,"name":id})
}

fn catalogue(c: &mut Controller, effects: &[Effect]) {
    reply(
        c,
        &request(effects, "get_available_models"),
        json!({"models":[model("alpha"),model("beta")]}),
    );
}

fn state(c: &mut Controller, effects: &[Effect], id: &str) {
    let follow = reply(
        c,
        &request(effects, "get_state"),
        json!({"model":model(id),"thinkingLevel":"high"}),
    );
    assert!(c.settings_presentation().is_none());
    reply(
        c,
        &request(&follow, "get_available_thinking_levels"),
        json!({"levels":["off","high"]}),
    );
    for effect in &follow {
        if let Effect::Send(r) = effect {
            if r["type"] == "get_messages" {
                reply(c, r, json!({"messages":[]}));
            }
        }
    }
}

fn host(c: &mut Controller, id: &str) {
    let token = c.begin_model_scope_read();
    assert!(c.settings_presentation().is_none());
    assert!(c.apply_model_scope(&token, Some(json!([format!("fixture/{id}")]))));
}

fn setup() -> Controller {
    let mut c = Controller::new("source");
    let boot = c.begin_generation(1);
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
    state(&mut c, &boot, "alpha");
    catalogue(&mut c, &boot);
    reply(
        &mut c,
        &request(&boot, "get_scoped_models"),
        json!({"models":[model("alpha")]}),
    );
    host(&mut c, "alpha");
    assert!(c.state().ready);
    assert!(!c.state().history_loading);
    assert_eq!(c.pending_count(), 0);
    assert_eq!(
        c.settings_presentation().unwrap().models,
        vec![model("alpha")]
    );
    c
}

#[test]
fn host_and_catalogue_cannot_export_previous_runtime_scope_mid_refresh() {
    let mut c = setup();
    let refresh = c.refresh_models();
    assert!(c.settings_presentation().is_none());
    host(&mut c, "beta");
    catalogue(&mut c, &refresh);
    assert!(c.settings_presentation().is_none());
    reply(
        &mut c,
        &request(&refresh, "get_scoped_models"),
        json!({"models":[model("beta")]}),
    );
    assert_eq!(
        c.settings_presentation().unwrap().models,
        vec![model("beta")]
    );
}

#[test]
fn failed_current_host_or_catalogue_read_cannot_reuse_success() {
    for fail_host in [true, false] {
        let mut c = setup();
        let refresh = c.refresh_models();
        let token = c.begin_model_scope_read();
        c.apply_model_scope(
            &token,
            if fail_host {
                None
            } else {
                Some(json!(["fixture/beta"]))
            },
        );
        if fail_host {
            catalogue(&mut c, &refresh);
        } else {
            c.request_failed(
                request(&refresh, "get_available_models")["id"]
                    .as_str()
                    .unwrap(),
            );
        }
        reply(
            &mut c,
            &request(&refresh, "get_scoped_models"),
            json!({"models":[model("beta")]}),
        );
        assert!(c.settings_presentation().is_none());
    }
}

#[test]
fn optional_runtime_rejection_falls_back_only_after_current_host_and_catalogue() {
    let mut c = setup();
    let refresh = c.refresh_models();
    let scope = request(&refresh, "get_scoped_models");
    c.handle(json!({"type":"response","id":scope["id"],"command":scope["type"],"success":false}));
    catalogue(&mut c, &refresh);
    assert!(c.settings_presentation().is_none());
    host(&mut c, "beta");
    assert_eq!(
        c.settings_presentation().unwrap().models,
        vec![model("beta")]
    );
}

#[test]
fn successful_setting_write_waits_for_its_state_and_discovery_reads() {
    for change_model in [false, true] {
        let mut c = setup();
        let write = if change_model {
            c.set_model("fixture", "beta")
        } else {
            c.set_thinking("high")
        };
        assert!(c.settings_presentation().is_none());
        let effects = reply(
            &mut c,
            &request(
                &write,
                if change_model {
                    "set_model"
                } else {
                    "set_thinking_level"
                },
            ),
            json!({}),
        );
        assert!(!c.setting_pending());
        assert!(c.settings_presentation().is_none());
        if change_model {
            catalogue(&mut c, &effects);
            host(&mut c, "beta");
            reply(
                &mut c,
                &request(&effects, "get_scoped_models"),
                json!({"models":[model("beta")]}),
            );
            assert!(c.settings_presentation().is_none());
        }
        state(&mut c, &effects, "beta");
        assert!(c.settings_presentation().is_some());
    }
}

#[test]
fn superseded_responses_host_tokens_and_restart_cannot_restore_eligibility() {
    let mut c = setup();
    let old = c.refresh_models();
    let old_host = c.begin_model_scope_read();
    let current = c.refresh_models();
    host(&mut c, "beta");
    catalogue(&mut c, &current);
    catalogue(&mut c, &old);
    reply(
        &mut c,
        &request(&old, "get_scoped_models"),
        json!({"models":[model("alpha")]}),
    );
    assert!(!c.apply_model_scope(&old_host, Some(json!(["fixture/alpha"]))));
    assert!(c.settings_presentation().is_none());
    reply(
        &mut c,
        &request(&current, "get_scoped_models"),
        json!({"models":[model("beta")]}),
    );
    assert_eq!(
        c.settings_presentation().unwrap().models,
        vec![model("beta")]
    );
    c.request_failed(request(&old, "get_scoped_models")["id"].as_str().unwrap());
    assert!(c.settings_presentation().is_some());
    let boot = c.begin_generation(2);
    assert!(c.settings_presentation().is_none());
    catalogue(&mut c, &current);
    assert!(!c.apply_model_scope(&old_host, Some(json!([]))));
    state(&mut c, &boot, "beta");
    catalogue(&mut c, &boot);
    host(&mut c, "beta");
    assert!(c.settings_presentation().is_none());
    reply(
        &mut c,
        &request(&boot, "get_scoped_models"),
        json!({"models":[model("beta")]}),
    );
    assert_eq!(
        c.settings_presentation().unwrap().models,
        vec![model("beta")]
    );
}

#[test]
fn timed_out_settings_write_requires_fresh_current_discovery() {
    for change_model in [false, true] {
        let mut c = setup();
        c.set_draft("retained draft");
        let write = if change_model {
            c.set_model("fixture", "beta")
        } else {
            c.set_thinking("medium")
        };
        let command = if change_model {
            "set_model"
        } else {
            "set_thinking_level"
        };
        let write = request(&write, command);
        assert!(c.request_write_accepted(1, write["id"].as_str().unwrap()));
        assert!(c.setting_pending());
        assert!(c.settings_presentation().is_none());
        assert!(c.tick(1_000_000).is_empty());
        assert!(c.state().ready);
        assert!(!c.state().history_loading);
        assert!(!c.state().working);
        assert!(!c.setting_pending());
        assert_eq!(c.pending_count(), 0);
        assert_eq!(c.state().model["id"], "alpha");
        assert_eq!(c.state().effort, "high");
        assert!(c.settings_presentation().is_none());
        let token = c.error_token().unwrap();
        assert!(c.acknowledge_error(&token));
        assert!(reply(&mut c, &write, json!({})).is_empty());
        assert!(c.settings_presentation().is_none());
        c.set_draft("still editable");
        assert_eq!(c.state().draft, "still editable");
        let prompt = request(&c.submit("ordinary send remains available"), "prompt");
        assert!(c.settings_presentation().is_none());
        c.handle(json!({"type":"response","id":prompt["id"],"command":"prompt","success":false}));

        let fresh = c.refresh();
        catalogue(&mut c, &fresh);
        host(&mut c, "beta");
        reply(
            &mut c,
            &request(&fresh, "get_scoped_models"),
            json!({"models":[model("beta")]}),
        );
        let efforts = reply(
            &mut c,
            &request(&fresh, "get_state"),
            json!({"model":model("beta"),"thinkingLevel":"medium"}),
        );
        assert!(c.settings_presentation().is_none());
        reply(
            &mut c,
            &request(&efforts, "get_available_thinking_levels"),
            json!({"levels":["low","medium"]}),
        );
        let current = c.settings_presentation().unwrap();
        assert_eq!(current.model["id"], "beta");
        assert_eq!(current.models, vec![model("beta")]);
        assert_eq!(current.effort, "medium");
        assert_eq!(current.efforts, vec!["low", "medium"]);
    }
}

#[test]
fn pre_confirmation_reads_cannot_revalidate_presentation_at_any_write_boundary() {
    for change_model in [false, true] {
        for probe in [true, false] {
            for boundary in ["timeout", "reject", "success"] {
                let mut c = setup();
                if probe {
                    let settlement = c.handle(json!({"type":"agent_settled"}));
                    state(&mut c, &settlement, "alpha");
                }
                assert!(c.state().ready);
                assert!(!c.state().history_loading);
                assert_eq!(c.pending_count(), 0);
                let write = if change_model {
                    c.set_model("fixture", "beta")
                } else {
                    c.set_thinking("medium")
                };
                let write = request(
                    &write,
                    if change_model {
                        "set_model"
                    } else {
                        "set_thinking_level"
                    },
                );
                assert!(c.request_write_accepted(1, write["id"].as_str().unwrap()));
                let reads = if probe {
                    c.tick(3_200)
                } else {
                    assert!(c.tick(3_200).is_empty());
                    c.handle(json!({"type":"session_info_changed"}))
                };
                let reads: Vec<_> = reads
                    .into_iter()
                    .filter_map(|e| match e {
                        Effect::Send(r) => {
                            assert_eq!(r["type"], "get_state");
                            Some(r)
                        }
                        _ => None,
                    })
                    .collect();
                assert!(!reads.is_empty());
                match boundary {
                    "success" => {
                        reply(&mut c, &write, json!({}));
                    }
                    "reject" => {
                        c.handle(json!({"type":"response","id":write["id"],"command":write["type"],"success":false}));
                    }
                    "timeout" => {
                        assert!(c.tick(10_000).is_empty());
                    }
                    _ => unreachable!(),
                }
                for r in reads {
                    let follow = reply(
                        &mut c,
                        &r,
                        json!({"model":model("alpha"),"thinkingLevel":"high"}),
                    );
                    if boundary != "success" {
                        assert!(
                            !follow.is_empty(),
                            "old reads retain reconciliation ownership"
                        );
                    }
                    for effect in follow {
                        if let Effect::Send(r) = effect {
                            let data = match r["type"].as_str().unwrap() {
                                "get_messages" => json!({"messages":[]}),
                                "get_available_thinking_levels" => json!({"levels":["off","high"]}),
                                other => panic!("unexpected old-state follow-up {other}"),
                            };
                            reply(&mut c, &r, data);
                        }
                    }
                    assert!(
                        c.settings_presentation().is_none(),
                        "{boundary} probe={probe} model={change_model}"
                    );
                }
                assert!(c.state().ready);
                c.set_draft("still editable");
                assert_eq!(c.state().draft, "still editable");
                let prompt = request(&c.submit("ordinary send remains available"), "prompt");
                c.handle(
                    json!({"type":"response","id":prompt["id"],"command":"prompt","success":false}),
                );
                assert!(c.settings_presentation().is_none());
                let fresh = c.refresh();
                catalogue(&mut c, &fresh);
                host(&mut c, "beta");
                reply(
                    &mut c,
                    &request(&fresh, "get_scoped_models"),
                    json!({"models":[model("beta")]}),
                );
                let follow = reply(
                    &mut c,
                    &request(&fresh, "get_state"),
                    json!({"model":model("beta"),"thinkingLevel":"medium"}),
                );
                for effect in follow {
                    if let Effect::Send(r) = effect {
                        let data = match r["type"].as_str().unwrap() {
                            "get_messages" => json!({"messages":[]}),
                            "get_available_thinking_levels" => json!({"levels":["low","medium"]}),
                            other => panic!("unexpected fresh-state follow-up {other}"),
                        };
                        reply(&mut c, &r, data);
                    }
                }
                let current = c.settings_presentation().unwrap();
                assert_eq!(current.model["id"], "beta");
                assert_eq!(current.effort, "medium");
                assert_eq!(current.efforts, vec!["low", "medium"]);
                assert_eq!(current.models, vec![model("beta")]);
            }
        }
    }
}

#[test]
fn runtime_transport_failure_or_timeout_is_not_optional_capability_rejection() {
    for timeout in [false, true] {
        let mut c = setup();
        let refresh = c.refresh_models();
        host(&mut c, "beta");
        catalogue(&mut c, &refresh);
        if timeout {
            c.tick(1_000_000);
        } else {
            c.request_failed(
                request(&refresh, "get_scoped_models")["id"]
                    .as_str()
                    .unwrap(),
            );
        }
        assert!(c.settings_presentation().is_none());
    }
}
