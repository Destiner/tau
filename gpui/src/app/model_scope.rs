use super::*;
use tau_session::session::ModelScopeToken;

impl Shell {
    pub(super) fn sync_model_scopes(&mut self, cx: &mut Context<Self>) {
        let mut reads = Vec::new();
        for (runtime, live) in &mut self.sessions {
            // Readiness implies a connected controller; retained/error sessions do no IO.
            if live.starting || live.controller.generation() == 0 || !live.controller.state().ready
            {
                continue;
            }
            if live.model_scope_read.as_ref() == Some(&live.controller.model_scope_token()) {
                continue;
            }
            let token = live.controller.begin_model_scope_read();
            live.model_scope_read = Some(token.clone());
            reads.push(Command::ReadModelScope {
                runtime: runtime.clone(),
                token,
                connection: live.project.connection_string.clone(),
            });
        }
        for read in reads {
            self.dispatch(read, cx);
        }
    }

    pub(super) fn model_scope_result(
        &mut self,
        runtime: String,
        token: ModelScopeToken,
        patterns: Option<Vec<String>>,
        _cx: &mut Context<Self>,
    ) {
        let Some(live) = self.sessions.get_mut(&runtime) else {
            return;
        };
        if live
            .controller
            .apply_model_scope(&token, patterns.map(|patterns| serde_json::json!(patterns)))
        {
            live.dirty = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::{create, rpc, started, with_shell};
    use serde_json::json;

    fn bootstrap(
        s: &mut Shell,
        w: &mut Window,
        cx: &mut Context<Shell>,
        requests: &async_channel::Receiver<Command>,
        runtime: &str,
    ) -> ModelScopeToken {
        started(s, runtime, 1, cx);
        let mut reads = Vec::new();
        // Complete real bootstrap envelopes through ingress, with no Pi or disk reads.
        for _ in 0..64 {
            let Ok(command) = requests.try_recv() else {
                s.sync(w, cx);
                if let Ok(Command::ReadModelScope { token, .. }) = requests.try_recv() {
                    reads.push(token);
                }
                break;
            };
            match command {
                Command::Send { request, .. } => {
                    let kind = request["type"].as_str().unwrap();
                    let data = match kind {
                        "get_state" => json!({}),
                        "get_available_models" => json!({"models":[
                            {"provider":"fixture","id":"alpha","name":"Alpha"},
                            {"provider":"fixture","id":"beta","name":"Beta"}
                        ]}),
                        "get_messages" => json!({"messages":[]}),
                        "get_commands" => json!({"commands":[]}),
                        "get_available_thinking_levels" => json!({"levels":[]}),
                        "get_scoped_models" => json!(null),
                        _ => panic!("unexpected bootstrap command {kind}"),
                    };
                    s.ingest(
                        rpc(
                            runtime,
                            1,
                            json!({
                                "type":"response", "id":request["id"], "command":kind,
                                "success":kind != "get_scoped_models", "data":data
                            }),
                        ),
                        cx,
                    );
                }
                Command::ReadModelScope { token, .. } => reads.push(token),
                Command::Start { .. } | Command::Save { .. } => {}
                _ => panic!("unexpected bootstrap service command"),
            }
        }
        assert!(s.sessions[runtime].controller.state().ready);
        reads.pop().expect("bootstrap must issue settings fallback")
    }

    #[test]
    fn bootstrap_settings_fallback_reaches_the_actual_composer_without_read_loops() {
        with_shell(64, |s, w, cx, requests| {
            create(s, "scope", "draft", w, cx);
            let token = bootstrap(s, w, cx, &requests, "scope");
            s.ingest(
                Event::ModelScope {
                    runtime: "scope".into(),
                    token: token.clone(),
                    patterns: Some(vec!["fixture/beta".into()]),
                },
                cx,
            );
            s.sync(w, cx);
            let live = &s.sessions["scope"];
            assert_eq!(live.controller.state().selectable_models().len(), 1);
            assert_eq!(live.controller.state().models.len(), 2);
            assert_eq!(live.composer.read(cx).visible_models().len(), 1);
            assert_eq!(
                live.composer.read(cx).visible_models()[0].id,
                "fixture\u{1f}beta"
            );
            assert_eq!(live.controller.model_scope_token(), token);
            for _ in 0..5 {
                s.sync_model_scopes(cx);
            }
            assert!(
                requests.try_recv().is_err(),
                "results/frames must not schedule another read"
            );
        });
    }

    #[test]
    fn refresh_rereads_remote_settings_and_stale_or_failed_completions_are_fenced() {
        with_shell(64, |s, w, cx, requests| {
            create(s, "scope", "draft", w, cx);
            s.sessions
                .get_mut("scope")
                .unwrap()
                .project
                .connection_string = Some("test@host".into());
            let old = bootstrap(s, w, cx, &requests, "scope");
            s.model_scope_result(
                "scope".into(),
                old.clone(),
                Some(vec!["fixture/beta".into()]),
                cx,
            );
            s.sessions
                .get_mut("scope")
                .unwrap()
                .controller
                .refresh_models();
            s.sync_model_scopes(cx);
            let Command::ReadModelScope {
                runtime,
                token,
                connection,
            } = requests.try_recv().unwrap()
            else {
                panic!("model refresh must issue a settings read");
            };
            assert_eq!(connection.as_deref(), Some("test@host"));
            assert_ne!(token, old);
            s.sessions.get_mut("scope").unwrap().dirty = false;
            s.model_scope_result(runtime.clone(), old, None, cx);
            assert!(!s.sessions["scope"].dirty);
            assert_eq!(
                s.sessions["scope"]
                    .controller
                    .state()
                    .selectable_models()
                    .len(),
                1
            );
            s.ingest(
                Event::ModelScope {
                    runtime,
                    token: token.clone(),
                    patterns: None,
                },
                cx,
            );
            assert!(s.sessions["scope"].dirty);
            assert_eq!(
                s.sessions["scope"]
                    .controller
                    .state()
                    .selectable_models()
                    .len(),
                2
            );
            s.sync_model_scopes(cx);
            assert!(requests.try_recv().is_err());
            s.sessions
                .get_mut("scope")
                .unwrap()
                .controller
                .transport_failed(1);
            s.sessions.get_mut("scope").unwrap().dirty = false;
            s.model_scope_result(
                "scope".into(),
                token,
                Some(vec!["fixture/alpha".into()]),
                cx,
            );
            s.sync_model_scopes(cx);
            assert!(!s.sessions["scope"].dirty);
            assert!(requests.try_recv().is_err());
        });
    }

    #[test]
    fn identity_replacement_and_generation_restart_require_fresh_settings_tokens() {
        with_shell(64, |s, w, cx, requests| {
            create(s, "scope", "draft", w, cx);
            let old = bootstrap(s, w, cx, &requests, "scope");
            let effects = s.sessions.get_mut("scope").unwrap().controller.refresh();
            s.effects("scope", effects, cx);
            let mut state_request = None;
            while let Ok(command) = requests.try_recv() {
                if let Command::Send { request, .. } = command
                    && request["type"] == "get_state"
                {
                    state_request = Some(request);
                }
            }
            let state_request = state_request.expect("refresh must request authoritative identity");
            s.ingest(
                rpc(
                    "scope",
                    1,
                    json!({
                        "type":"response", "id":state_request["id"], "command":"get_state",
                        "success":true, "data":{"sessionId":"replacement"}
                    }),
                ),
                cx,
            );
            s.sync_model_scopes(cx);
            let mut replacement = None;
            while let Ok(command) = requests.try_recv() {
                if let Command::ReadModelScope { token, .. } = command {
                    replacement = Some(token);
                }
            }
            let replacement = replacement.expect("identity replacement must reread settings");
            assert_ne!(replacement, old);
            s.sessions.get_mut("scope").unwrap().dirty = false;
            s.model_scope_result("scope".into(), old, Some(vec!["fixture/beta".into()]), cx);
            assert!(!s.sessions["scope"].dirty);
            s.sessions
                .get_mut("scope")
                .unwrap()
                .controller
                .begin_generation(2);
            s.sync_model_scopes(cx);
            assert!(requests.try_recv().is_err(), "restart must await readiness");
            s.model_scope_result("scope".into(), replacement.clone(), None, cx);
            assert!(!s.sessions["scope"].dirty);
            assert_eq!(s.sessions["scope"].controller.generation(), 2);
            assert_ne!(
                s.sessions["scope"].controller.model_scope_token(),
                replacement
            );
        });
    }

    #[test]
    fn retained_sessions_and_failed_dispatch_do_not_flood_reads() {
        with_shell(1, |s, w, cx, requests| {
            s.create_retained(
                "scope".into(),
                crate::app::tests::project(),
                String::new(),
                String::new(),
                "draft".into(),
                w,
                cx,
            );
            s.sync_model_scopes(cx);
            assert!(requests.try_recv().is_err());
            let effects = s
                .sessions
                .get_mut("scope")
                .unwrap()
                .controller
                .begin_generation(1);
            let state = effects
                .iter()
                .find_map(|effect| match effect {
                    Effect::Send(request) if request["type"] == "get_state" => {
                        Some(request.clone())
                    }
                    _ => None,
                })
                .unwrap();
            let live = s.sessions.get_mut("scope").unwrap();
            live.starting = false;
            live.controller.handle(json!({"type":"response","id":state["id"],"command":"get_state","success":true,"data":{}}));
            s.sessions.get_mut("scope").unwrap().dirty = false;
            requests.close();
            s.sync_model_scopes(cx);
            let token = s.sessions["scope"].model_scope_read.clone().unwrap();
            assert!(s.sessions["scope"].dirty);
            s.sessions.get_mut("scope").unwrap().dirty = false;
            for _ in 0..5 {
                s.sync_model_scopes(cx);
            }
            assert!(!s.sessions["scope"].dirty);
            assert_eq!(s.sessions["scope"].controller.model_scope_token(), token);
        });
    }
}
