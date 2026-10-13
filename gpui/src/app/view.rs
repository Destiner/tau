use super::*;
use gpui_kit::base::{Button, ElementExt};

impl Shell {
    pub(super) fn project_actions_admitted(&self, cx: &App) -> bool {
        self.loaded
            && self.lifecycle.is_none()
            && self.confirmation.is_none()
            && self.error.is_none()
            && !self.picker_busy
            && !self.remote.is_open()
            && !self.preview_open()
            && !self
                .fallback
                .as_ref()
                .is_some_and(|(_, d)| d.read(cx).is_open())
            && self.empty_disposals.is_empty()
            && self.register_retries.is_empty()
    }

    pub(super) fn sync_chrome_admission(&self, cx: &mut Context<Self>) {
        let actions = self.project_actions_admitted(cx);
        let prompt = self
            .sessions
            .get(&self.active)
            .is_some_and(|live| !live.controller.state().extension_dialogs.is_empty());
        let palette = self.palette.is_open(cx);
        let version = actions && !prompt && !self.admin_reporter_open(cx);
        if let Some(sidebar) = &self.sidebar {
            sidebar.update(cx, |sidebar, cx| {
                sidebar.set_actions_interactive(actions && !palette, cx);
                sidebar.set_remove_project_available(false, cx);
                if version && palette {
                    sidebar.preserve_version_under_palette(cx);
                } else {
                    sidebar.set_version_interactive(version, cx);
                }
            });
        }
        if let Some(header) = &self.header {
            header.update(cx, |header, cx| {
                header.set_action_admission(
                    self.selected_project().is_some(),
                    actions && !palette,
                    actions && !prompt,
                    cx,
                )
            });
        }
    }

    fn extension_failure_owned(live: &LiveSession) -> bool {
        live.extension_error
            .as_ref()
            .is_some_and(|(generation, id, _)| {
                *generation == live.controller.generation()
                    && live
                        .controller
                        .state()
                        .extension_dialogs
                        .first()
                        .is_some_and(|r| &r.id == id)
                    && live.controller.extension_delivery_error_owned(id)
            })
    }

    pub(super) fn retry_pi(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(live) = self.sessions.get_mut(key) else {
            return;
        };
        if live.starting {
            return;
        }
        live.dirty = true;
        if live.controller.state().ready {
            let effects = live.controller.refresh();
            self.effects(key, effects, cx);
        } else {
            live.starting = true;
            live.startup_events.clear();
            live.start_attempt += 1;
            live.start_deadline = self.epoch.elapsed().as_millis() as u64 + 10_000;
            let attempt = live.start_attempt;
            let project = live.project.clone();
            let path = if live.controller.state().identity.path.is_empty() {
                live.source_path.clone()
            } else {
                live.controller.state().identity.path.clone()
            };
            self.dispatch(
                Command::Start {
                    runtime: key.into(),
                    attempt,
                    project,
                    source: (!path.is_empty()).then_some(path),
                },
                cx,
            );
        }
        self.refresh_window(cx);
    }

    fn sync_fallback(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let session = self.sessions.get(&self.active).filter(|live| {
            live.controller.state().status == Status::Error
                && live.controller.state().error.is_some()
                && !Self::extension_failure_owned(live)
        });
        let incident = if let Some(error) = &self.error {
            Some((format!("app:{error}"), error.clone(), None))
        } else {
            session.map(|live| {
                let error =
                    live.controller.state().error.clone().unwrap_or_else(|| {
                        "Pi could not complete this operation. Try again.".into()
                    });
                (
                    format!(
                        "session:{}:{}:{error}",
                        self.active,
                        live.controller.generation()
                    ),
                    error,
                    Some((
                        self.active.clone(),
                        live.controller.generation(),
                        live.controller.state().ready,
                    )),
                )
            })
        };
        let Some((id, message, session)) = incident else {
            self.fallback = None;
            return;
        };
        if self.lifecycle.is_some()
            || self.confirmation.is_some()
            || self.remote.is_open()
            || self.preview_open()
        {
            self.fallback = None;
            return;
        }
        if self
            .fallback
            .as_ref()
            .is_some_and(|(current, _)| current == &id)
        {
            return;
        }
        let initialize = !self.loaded && session.is_none();
        let action = if initialize {
            "Try Again"
        } else if let Some((_, _, ready)) = &session {
            if *ready { "Refresh" } else { "Reconnect" }
        } else {
            ""
        };
        let owned_message = message.clone();
        let dialog = cx.new(|cx| {
            NativeDialog::new(
                DialogProps {
                    kind: DialogKind::Feedback,
                    title: if initialize {
                        "Workspace unavailable"
                    } else {
                        "Operation failed"
                    }
                    .into(),
                    description: message,
                    placeholder: String::new(),
                    options: vec![],
                },
                self.dark,
                window,
                cx,
            )
        });
        dialog.update(cx, |d, cx| {
            d.set_action_label(action, cx);
            d.set_dismissible(!initialize, cx);
        });
        let incident_id = id.clone();
        self.subscriptions.push(cx.subscribe(
            &dialog,
            move |s, source, event: &DialogEvent, cx| {
                if !s
                    .fallback
                    .as_ref()
                    .is_some_and(|(current, dialog)| current == &incident_id && dialog == &source)
                {
                    return;
                }
                if let Some((key, generation, _)) = &session {
                    if s.active != *key
                        || !s.sessions.get(key).is_some_and(|live| {
                            live.controller.generation() == *generation
                                && live.controller.state().error.as_ref() == Some(&owned_message)
                        })
                    {
                        return;
                    }
                    if matches!(event, DialogEvent::Submitted(_)) {
                        s.retry_pi(key, cx);
                    }
                    source.update(cx, |d, cx| {
                        d.set_busy(matches!(event, DialogEvent::Submitted(_)), cx)
                    });
                    s.refresh_window(cx);
                    if matches!(event, DialogEvent::Cancelled) {
                        s.restore_input_focus(cx);
                    }
                    return;
                } else {
                    if s.error.as_ref() != Some(&owned_message) || (initialize && s.loaded) {
                        return;
                    }
                    if initialize && matches!(event, DialogEvent::Submitted(_)) {
                        s.dispatch(Command::Load { project: None }, cx);
                    }
                }
                s.error = None;
                s.fallback = None;
                s.refresh_window(cx);
                s.restore_input_focus(cx);
            },
        ));
        self.fallback = Some((id, dialog));
    }

    pub(super) fn restore_input_focus(&self, cx: &mut Context<Self>) {
        self.restore_input_focus_from(None, cx);
    }
    pub(super) fn restore_input_focus_from(
        &self,
        origin: Option<gpui_kit::FocusHandle>,
        cx: &mut Context<Self>,
    ) {
        let binding = self.palette_binding();
        let owner = cx.entity().downgrade();
        let target = self.window;
        cx.defer(move |cx| {
            if let Some(target) = target {
                let _ = target.update(cx, |_, window, _| {
                    // GPUI delivers frame callbacks before drawing. The second callback
                    // sees the dispatch tree painted by the first frame.
                    window.on_next_frame(move |window, _| {
                        window.on_next_frame(move |window, cx| {
                            let Some(owner) = owner.upgrade() else {
                                return;
                            };
                            let shell = owner.read(cx);
                            if shell.window != Some(target)
                                || shell.lifecycle.is_some()
                                || shell.confirmation.is_some()
                                || shell.error.is_some()
                                || shell
                                    .fallback
                                    .as_ref()
                                    .is_some_and(|(_, d)| d.read(cx).is_open())
                                || shell.picker_busy
                                || shell.palette.is_open(cx)
                                || shell.preview_open()
                                || shell
                                    .sidebar
                                    .as_ref()
                                    .is_some_and(|s| s.read(cx).version_open())
                            {
                                return;
                            }
                            if shell.remote.is_open() {
                                owner.update(cx, |s, cx| s.focus_remote(window, cx));
                                return;
                            }
                            if shell.admin_reporter_open(cx) {
                                owner.update(cx, |s, cx| s.open_admin_reporter(window, cx));
                                return;
                            }
                            if let Some((_, dialog)) = shell
                                .sessions
                                .get(&shell.active)
                                .and_then(|live| live.extension.as_ref())
                            {
                                let dialog = dialog.clone();
                                dialog.update(cx, |dialog, cx| dialog.focus(window, cx));
                                return;
                            }
                            if binding == shell.palette_binding()
                                && let Some(focus) = origin
                                && shell.shell_focus.as_ref().is_some_and(|root| {
                                    root != &focus && root.contains(&focus, window)
                                })
                            {
                                focus.focus(window, cx);
                                return;
                            }
                            if let Some(live) = shell.sessions.get(&shell.active)
                                && live.controller.state().ready
                                && !live.starting
                                && !live.controller.state().stopping
                                && !live.composer.read(cx).is_disabled()
                            {
                                use gpui_kit::Focusable;
                                let focus =
                                    live.composer.read(cx).editor().read(cx).focus_handle(cx);
                                // contains(self) is true even for an unmounted handle.
                                if shell
                                    .shell_focus
                                    .as_ref()
                                    .is_some_and(|root| root.contains(&focus, window))
                                {
                                    focus.focus(window, cx);
                                    return;
                                }
                            }
                            if let Some(focus) = shell.shell_focus.clone() {
                                focus.focus(window, cx);
                            }
                        });
                    });
                });
            }
        });
    }
    pub(super) fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dark = self.theme_dark(window);
        Theme::global_mut(cx).appearance = if dark {
            ThemeAppearance::Dark
        } else {
            ThemeAppearance::Light
        };
        let changed = self.dark != dark;
        self.dark = dark;
        if changed {
            support::install_scrollbar_theme(dark, cx);
        }
        self.sync_preview(cx);
        self.sync_preview_theme(window, cx);
        self.sync_model_scopes(cx);
        if self.loaded && !self.restored {
            self.restored = true;
            let drafts = self.preferences.drafts.clone();
            for draft in drafts {
                if let Some(project) = self
                    .workspace
                    .as_ref()
                    .and_then(|w| w.projects.iter().find(|p| p.path == draft.project))
                    .cloned()
                {
                    let key = draft.key.clone();
                    self.create_retained(
                        draft.key,
                        project,
                        draft.source_id,
                        draft.source_path,
                        draft.text,
                        window,
                        cx,
                    );
                    if let Some(live) = self.sessions.get_mut(&key) {
                        live.persisted_recovered = draft.recovered.len();
                        live.restored_recovered = draft.recovered;
                    }
                }
            }
            let selected = self
                .workspace
                .as_ref()
                .and_then(|w| {
                    w.projects
                        .iter()
                        .flat_map(|p| p.sessions.iter().map(move |s| (p, s)))
                        .find(|(p, s)| {
                            self.requested_session
                                .as_ref()
                                .map(|wanted| wanted == &s.id || wanted == &s.path)
                                .unwrap_or(
                                    p.path == w.active_project_path && s.selected && !s.archived,
                                )
                        })
                })
                .map(|(_, s)| s.id.clone());
            let selected = if self.requested_session.is_none()
                && self.sessions.contains_key(&self.preferences.selected_draft)
            {
                Some(self.preferences.selected_draft.clone())
            } else {
                selected
            };
            if let Some(id) = selected {
                self.select(&id, window, cx);
            } else if self.requested_session.is_some() {
                self.error = Some("The requested session is not registered in this workspace. Select a saved session or import its project.".into());
            }
        }
        self.sync_chrome_admission(cx);
        self.sync_sidebar(window, cx);
        if let Some(header) = &self.header {
            let heading = self
                .sessions
                .get(&self.active)
                .map(title)
                .unwrap_or_else(|| "Tau".into());
            header.update(cx, |h, cx| {
                h.set_session(self.active.clone(), heading, window, cx);
                h.set_renamable(self.can_rename(), cx);
                if changed {
                    h.set_theme(dark, window, cx);
                }
            });
        }
        if self
            .sessions
            .get(&self.active)
            .is_some_and(|live| !live.controller.state().extension_dialogs.is_empty())
            && self.palette.is_open(cx)
            && let Some(palette) = &self.palette.view
        {
            palette.update(cx, |p, cx| p.dismiss(cx));
        }
        let keys: Vec<_> = self.sessions.keys().cloned().collect();
        let mut focus_ready_composer = false;
        for key in keys {
            let can_recover = self.can_recover_queue(&key, cx);
            let can_answer = self.loaded
                && self.lifecycle.is_none()
                && self.confirmation.is_none()
                && !self.picker_busy
                && !self.remote.is_open()
                && !self.preview_open()
                && !self.admin_reporter_open(cx)
                && !self.palette.is_open(cx)
                && !self
                    .fallback
                    .as_ref()
                    .is_some_and(|(_, d)| d.read(cx).is_open())
                && self.empty_disposals.is_empty()
                && self.register_retries.is_empty();
            let draft_blocked = !self.project_actions_admitted(cx) || self.palette.is_open(cx);
            let live = self.sessions.get_mut(&key).unwrap();
            if key == self.active
                && can_answer
                && live.extension.is_none()
                && !live.controller.state().extension_dialogs.is_empty()
            {
                live.dirty = true;
            }
            let state = live.controller.state();
            let retry_focus =
                live.composer.read(cx).retry_has_focus(window) && state.retry.is_none();
            live.composer.update(cx, |c, cx| {
                c.set_admission(live.controller.setting_pending(), state.compacting, cx);
                c.set_readiness(
                    state.ready && !live.starting,
                    state.stopping,
                    live.controller.submission_pending(),
                    live.controller.queue_busy(),
                    !state.extension_dialogs.is_empty(),
                    cx,
                );
                c.set_retry(state.retry.as_ref().map(Into::into), cx);
                if c.is_disabled() != draft_blocked {
                    c.set_status(draft_blocked, state.working, cx);
                }
            });
            if retry_focus && key == self.active {
                focus_ready_composer = true;
            }
            let copy_paths = live.project.connection_string.is_some();
            live.transcript.update(cx, |t, cx| {
                t.set_copy_paths(copy_paths, cx);
                let disabled = state
                    .transcript
                    .iter()
                    .filter(|entry| {
                        entry.kind == "error"
                            || (entry.kind == "notice"
                                && super::file_preview::notice_file_base(
                                    entry,
                                    &live.project.working_directory,
                                )
                                .is_none())
                    })
                    .map(|entry| entry.id.clone())
                    .collect();
                t.set_disabled_file_sources(disabled, cx);
            });
            live.queue.update(cx, |q, cx| {
                q.set_copy_paths(copy_paths, cx);
                if q.props().can_recover != can_recover {
                    let mut props = q.props().clone();
                    props.can_recover = can_recover;
                    q.set_props(props, cx);
                }
            });
            if let Some((_, dialog)) = &live.extension {
                dialog.update(cx, |d, cx| {
                    d.set_copy_paths(copy_paths, cx);
                    d.set_readonly(!can_answer, cx);
                });
            }
            if !live.dirty && !changed {
                continue;
            }
            live.dirty = false;
            let state = live.controller.state();
            live.transcript
                .update(cx, |t, cx| t.set_compacting(state.compacting, cx));
            if live.projection_dirty {
                live.projection
                    .sync(&state.transcript, &live.transcript, cx);
                live.projection_dirty = false;
            }
            live.transcript.update(cx, |t, cx| {
                t.set_working_indicator(projection::working_indicator(state), cx);
            });
            if state.draft != live.shown_draft {
                let draft = state.draft.clone();
                live.shown_draft = draft.clone();
                let editor = live.composer.read(cx).editor();
                editor.update(cx, |e, cx| e.set_value(&draft, window, cx));
            }
            let commands_revision = live.controller.commands_revision();
            if live.command_choices != Some(commands_revision) {
                live.command_choices = Some(commands_revision);
                let commands = state
                    .commands
                    .iter()
                    .filter_map(|command| serde_json::from_value(command.clone()).ok())
                    .collect();
                live.composer
                    .update(cx, |composer, cx| composer.set_commands(commands, cx));
            }
            // Check the mutation token before projecting or serializing any model data.
            let choice_revision = live.controller.model_choices_revision();
            if live.choices != Some(choice_revision) {
                live.choices = Some(choice_revision);
                let selectable_models = state.selectable_models();
                let models = selectable_models
                    .iter()
                    .filter_map(|m| {
                        let provider = m["provider"].as_str()?;
                        let id = m["id"].as_str()?;
                        Some(Choice {
                            id: format!("{provider}\u{1f}{id}"),
                            label: m["name"].as_str().unwrap_or(id).into(),
                            element_id: format!("model-{provider}-{id}"),
                        })
                    })
                    .collect();
                let efforts = state
                    .thinking_levels
                    .iter()
                    .map(|level| Choice {
                        id: level.clone(),
                        label: components::composer::effort_label(level).into(),
                        element_id: format!("effort-{level}"),
                    })
                    .collect();
                let model = format!(
                    "{}\u{1f}{}",
                    state.model["provider"].as_str().unwrap_or_default(),
                    state.model["id"].as_str().unwrap_or_default()
                );
                live.composer.update(cx, |c, cx| {
                    c.set_choices(models, efforts, cx);
                    c.set_model(&model, cx);
                    c.set_model_fallback(
                        state.model["name"]
                            .as_str()
                            .or(state.model["id"].as_str())
                            .unwrap_or("Select Model"),
                        cx,
                    );
                    c.set_effort(&state.effort, cx);
                });
            }
            let status = (
                !state.ready || live.starting || state.stopping || self.exit_revision.is_some(),
                state.working,
            );
            let status_changed = live.ui_status != Some(status);
            focus_ready_composer |= status_changed
                && !status.0
                && key == self.active
                && self
                    .shell_focus
                    .as_ref()
                    .is_some_and(|focus| focus.is_focused(window));
            live.ui_status = Some(status);
            live.composer.update(cx, |c, cx| {
                c.set_admission(live.controller.setting_pending(), state.compacting, cx);
                if status_changed {
                    if state.error.is_some() && state.working {
                        c.set_status(draft_blocked, false, cx);
                    }
                    c.set_status(draft_blocked, status.1, cx);
                }
                if changed {
                    c.set_theme(dark, window, cx);
                }
            });
            live.queue.update(cx, |q, cx| {
                q.set_props(
                    QueueProps {
                        steering: state.queue.steering.clone(),
                        follow_up: state.queue.follow_up.clone(),
                        failed: live
                            .restored_recovered
                            .iter()
                            .chain(&state.recovered_drafts)
                            .cloned()
                            .collect(),
                        feedback: live.controller.queue_feedback().unwrap_or_default().into(),
                        busy: live.controller.queue_busy(),
                        can_recover,
                    },
                    cx,
                );
                if changed {
                    q.set_theme(dark, window, cx);
                }
            });
            if changed {
                live.transcript
                    .update(cx, |t, cx| t.set_theme(dark, window, cx));
            }
            let request = state.extension_dialogs.first().cloned();
            let generation = live.controller.generation();
            if request.as_ref().map(|r| &r.id) != live.extension.as_ref().map(|(id, _)| id)
                || live.extension_generation != Some(generation)
            {
                live.extension = None;
                live.extension_subscription = None;
                live.extension_error = None;
                live.extension_generation = Some(generation);
                if let Some(request) = request.filter(|_| key == self.active && can_answer) {
                    let method = request.request["method"].as_str().unwrap_or("input");
                    let select = matches!(method, "select" | "confirm");
                    let options = if method == "confirm" {
                        vec![
                            DialogOption {
                                id: "yes".into(),
                                label: "Yes".into(),
                            },
                            DialogOption {
                                id: "no".into(),
                                label: "No".into(),
                            },
                        ]
                    } else {
                        request.request["options"]
                            .as_array()
                            .map(|a| {
                                a.iter()
                                    .filter_map(|v| v.as_str())
                                    .enumerate()
                                    .map(|(index, s)| DialogOption {
                                        id: format!("extension-option-{index}"),
                                        label: s.into(),
                                    })
                                    .collect()
                            })
                            .unwrap_or_default()
                    };
                    let props = DialogProps {
                        kind: if select {
                            DialogKind::ExtensionSelect
                        } else {
                            DialogKind::ExtensionInput
                        },
                        title: request.request["title"]
                            .as_str()
                            .unwrap_or("Extension Prompt")
                            .into(),
                        description: request.request["message"]
                            .as_str()
                            .unwrap_or_default()
                            .into(),
                        placeholder: request.request["placeholder"]
                            .as_str()
                            .unwrap_or_default()
                            .into(),
                        options,
                    };
                    let dialog = cx.new(|cx| {
                        let dialog = if method == "editor" {
                            NativeDialog::new_editor(props, dark, window, cx)
                        } else {
                            NativeDialog::new(props, dark, window, cx)
                        }
                        .embedded()
                        .keyboard_submission();
                        if method == "confirm" {
                            dialog.confirm_shortcuts()
                        } else {
                            dialog
                        }
                    });
                    if let Some(prefill) = request.request["prefill"]
                        .as_str()
                        .or_else(|| request.request["value"].as_str())
                        .or_else(|| request.request["text"].as_str())
                    {
                        dialog.update(cx, |d, cx| d.set_value(prefill, window, cx));
                    }
                    let id = request.id.clone();
                    let event_key = key.clone();
                    let confirm = method == "confirm";
                    let subscription =
                        cx.subscribe(&dialog, move |s, source, event: &DialogEvent, cx| {
                            if s.active != event_key
                                || s.lifecycle.is_some()
                                || s.confirmation.is_some()
                                || s.picker_busy
                                || s.remote.is_open()
                                || s.preview_open()
                                || s.admin_reporter_open(cx)
                                || s.palette.is_open(cx)
                                || s.fallback
                                    .as_ref()
                                    .is_some_and(|(_, d)| d.read(cx).is_open())
                            {
                                s.refresh_window(cx);
                                return;
                            }
                            if let Some(live) = s.sessions.get_mut(&event_key) {
                                if live.controller.generation() != generation
                                    || !live.extension.as_ref().is_some_and(|(current, dialog)| {
                                        current == &id && dialog == &source
                                    })
                                    || !live
                                        .controller
                                        .state()
                                        .extension_dialogs
                                        .first()
                                        .is_some_and(|request| request.id == id)
                                {
                                    return;
                                }
                                live.dirty = true;
                                let response = match event {
                                    DialogEvent::Cancelled => ExtensionResponse::Cancelled,
                                    DialogEvent::Submitted(value) if confirm => {
                                        ExtensionResponse::Confirmed(
                                            value.eq_ignore_ascii_case("yes"),
                                        )
                                    }
                                    DialogEvent::Submitted(value) => {
                                        ExtensionResponse::Value(value.clone())
                                    }
                                };
                                let effects = live.controller.extension_response(&id, response);
                                // Keep failure provenance until delivery completes; retry intent
                                // alone must not promote its generic controller duplicate.
                                s.effects(&event_key, effects, cx);
                                s.refresh_window(cx);
                            }
                        });
                    let link_key = key.clone();
                    let link_id = request.id.clone();
                    let link_dialog = dialog.downgrade();
                    let owner = cx.entity().downgrade();
                    let context_key = link_key.clone();
                    let context_id = link_id.clone();
                    let context_dialog = link_dialog.clone();
                    let context_owner = owner.clone();
                    dialog.update(cx, |d, cx| {
                        d.set_copy_paths(copy_paths, cx);
                        d.set_context_handler(
                            move |target, event, window, cx| {
                                let Some(entity) = context_dialog.upgrade() else {
                                    return;
                                };
                                let _ = context_owner.update(cx, |shell, cx| {
                                    if shell.sessions.get(&context_key).is_some_and(|live| {
                                        live.controller.generation() == generation
                                    }) {
                                        shell.open_link_menu(
                                            &context_key,
                                            super::file_preview::LinkSource::Dialog {
                                                id: context_id.clone(),
                                                entity,
                                            },
                                            target,
                                            event.position,
                                            window,
                                            cx,
                                        );
                                    }
                                });
                            },
                            cx,
                        );
                        d.set_anchored_link_handler(move |target, origin, anchor, window, cx| {
                            let Some(source) = link_dialog.upgrade() else {
                                return;
                            };
                            let _ = owner.update(cx, |s, cx| {
                                if s.sessions.get(&link_key).is_some_and(|live| {
                                    live.controller.generation() == generation
                                        && live.extension.as_ref().is_some_and(|(id, dialog)| {
                                            id == &link_id && dialog == &source
                                        })
                                        && live
                                            .controller
                                            .state()
                                            .extension_dialogs
                                            .first()
                                            .is_some_and(|r| r.id == link_id)
                                }) {
                                    s.open_source_link(
                                        &link_key,
                                        super::file_preview::LinkSource::Dialog {
                                            id: link_id.clone(),
                                            entity: source.clone(),
                                        },
                                        target,
                                        origin,
                                        anchor,
                                        window,
                                        cx,
                                    );
                                }
                            });
                        })
                    });
                    live.extension_subscription = Some(subscription);
                    live.extension = Some((request.id, dialog));
                }
            }
            if let Some((_, dialog)) = &live.extension {
                let busy = live
                    .controller
                    .state()
                    .extension_dialogs
                    .first()
                    .is_some_and(|d| d.submitting);
                dialog.update(cx, |d, cx| {
                    d.set_busy(busy, cx);
                    d.set_readonly(!can_answer, cx);
                    d.set_error(
                        live.extension_error
                            .as_ref()
                            .filter(|_| !busy)
                            .map(|(_, _, error)| error.as_str())
                            .unwrap_or_default(),
                        cx,
                    );
                    if changed {
                        d.set_theme(dark, window, cx);
                    }
                });
            }
            let prompt = live.extension.as_ref().map(|(_, dialog)| dialog.clone());
            live.transcript
                .update(cx, |t, cx| t.set_extension_prompt(prompt, cx));
        }
        if let Some(request) = self.lifecycle {
            if self.confirmation.is_none() && self.exit_revision.is_none() {
                let busy = self.is_busy();
                if !busy && self.loaded && !self.picker_busy && !self.save_failed {
                    self.begin_exit(request, matches!(request.intent(), Intent::Quit), cx);
                } else {
                    let quitting = matches!(request.intent(), Intent::Quit);
                    let active = self
                        .sessions
                        .values()
                        .filter(|live| {
                            live.starting
                                || live.controller.state().working
                                || live.controller.state().stopping
                        })
                        .count();
                    let props = DialogProps {
                        kind: DialogKind::Quit,
                        title: if quitting { "Quit Tau?" } else { "Close Tau?" }.into(),
                        description: match active {
                            0 => String::new(),
                            1 => "1 session is still in progress.".into(),
                            n => format!("{n} sessions are still in progress."),
                        },
                        placeholder: String::new(),
                        options: vec![],
                    };
                    let dialog = cx.new(|cx| NativeDialog::new(props, dark, window, cx));
                    dialog.update(cx, |d, cx| {
                        d.set_action_label(if quitting { "Quit" } else { "Close" }, cx)
                    });
                    self.subscriptions.push(cx.subscribe(
                        &dialog,
                        move |s, source, event: &DialogEvent, cx| {
                            if s.lifecycle != Some(request)
                                || s.exit_revision.is_some()
                                || s.confirmation.as_ref() != Some(&source)
                            {
                                return;
                            }
                            match event {
                                DialogEvent::Cancelled => {
                                    cx.global::<NativeHost>()
                                        .lifecycle()
                                        .resolve(request, false, cx);
                                }
                                DialogEvent::Submitted(_) => {
                                    if s.picker_busy || s.remote.is_busy() {
                                        s.quit_error = Some((request,
                                        if s.remote.is_busy() {
                                            "Wait for the remote project operation before closing."
                                        } else {
                                            "Finish or cancel the folder picker before closing."
                                        }
                                        .into(),
                                    ));
                                    } else {
                                        s.begin_exit(request, true, cx);
                                    }
                                }
                            }
                            s.refresh_window(cx);
                        },
                    ));
                    self.confirmation = Some(dialog);
                }
            }
            if let Some(dialog) = &self.confirmation {
                dialog.update(cx, |d, cx| {
                    d.set_busy(
                        self.exit_revision
                            .is_some_and(|(_, owner)| owner == request),
                        cx,
                    );
                    d.set_error(
                        self.quit_error
                            .as_ref()
                            .filter(|(owner, _)| *owner == request)
                            .map(|(_, error)| error.as_str())
                            .unwrap_or_default(),
                        cx,
                    );
                });
            }
        }
        let loading_finished = self.sync_session_loading(cx);
        self.sync_remote(window, cx);
        self.sync_admin(window, cx);
        self.sync_palette(window, cx);
        self.sync_fallback(window, cx);
        self.sync_chrome_admission(cx);
        if (focus_ready_composer || loading_finished)
            && !self
                .header
                .as_ref()
                .is_some_and(|header| header.read(cx).renaming())
        {
            self.restore_input_focus(cx);
        }
        cx.notify();
    }
    pub(super) fn select(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if !self.project_actions_admitted(cx) {
            return;
        }
        if let Some(previous) = self.sessions.get(&self.active) {
            previous
                .transcript
                .update(cx, |t, cx| t.clear_selection(cx));
        }
        if let Some(key) = self
            .sessions
            .iter()
            .find(|(key, live)| {
                key.as_str() == id
                    || live.source_id == id
                    || live.controller.state().identity.id == id
            })
            .map(|(key, _)| key.clone())
        {
            self.discard_empty_before_switch(&key, cx);
            self.sessions.get_mut(&key).unwrap().dirty = true;
            match self.saved_session(&key) {
                Some((project, session, _)) if !session.archived => {
                    self.select_workspace(project.path, session.id, cx);
                }
                None => {
                    let project = self.sessions[&key].project.path.clone();
                    self.select_workspace(project, String::new(), cx);
                }
                _ => {}
            }
            self.active = key.clone();
            self.mark_sidebar_read(&key);
            self.start_live(&key, cx);
            self.persist(false, cx);
            self.sync(window, cx);
            return;
        }
        if let Some((project, session)) = self.workspace.as_ref().and_then(|w| {
            w.projects.iter().find_map(|p| {
                p.sessions
                    .iter()
                    .find(|s| s.id == id)
                    .map(|s| (p.clone(), s.clone()))
            })
        }) {
            if !session.archived {
                self.select_workspace(project.path.clone(), session.id.clone(), cx);
            }
            self.create_live(
                uuid::Uuid::new_v4().to_string(),
                project,
                session.id,
                session.path,
                String::new(),
                window,
                cx,
            );
        }
    }
    pub(super) fn action(&mut self, action: Action, window: &mut Window, cx: &mut Context<Self>) {
        if !self.project_actions_admitted(cx) || self.preview_open() {
            return;
        }
        if self.session_action(&action, window, cx) {
            return;
        }
        if self.handle_read_state(&action) {
            self.sync(window, cx);
            return;
        }
        match action {
            Action::Palette => {
                self.toggle_palette(window, cx);
                self.sync_chrome_admission(cx);
            }
            Action::CheckUpdates => {
                if self.admin_reporter_open(cx) {
                    return;
                }
                if let Some(sidebar) = &self.sidebar {
                    sidebar.update(cx, |s, cx| s.open_version_status(window, cx));
                }
            }
            Action::OpenRemoteProject => self.open_remote(window, cx),
            Action::Select(id) => self.select(&id, window, cx),
            Action::NewSession => self.new_session(None, window, cx),
            Action::NewSessionIn(project) => self.new_session(Some(project), window, cx),
            Action::ProjectToggle(project) => {
                let collapsed = self
                    .workspace
                    .as_ref()
                    .and_then(|w| w.projects.iter().find(|p| p.path == project))
                    .is_some_and(|p| !p.collapsed);
                self.mutate_workspace(
                    workspace_mutations::Mutation::Collapse { project, collapsed },
                    cx,
                );
                self.sync(window, cx);
            }
            Action::ReorderProjects(projects) => {
                self.mutate_workspace(workspace_mutations::Mutation::Order(projects), cx);
                self.sync(window, cx);
            }
            Action::OpenLocalProject => {
                if let Some(picker) = self.picker.clone() {
                    let weak = cx.entity().downgrade();
                    if let Some(target) = window.window_handle().downcast::<Root>() {
                        self.picker_busy = true;
                        cx.defer(move |cx| {
                            let callback_owner = weak.clone();
                            let result = target.update(cx, |_, window, cx| {
                                picker.select(PathOperation::ProjectFolder, window, cx, move |_, outcome, _, cx| {
                                    let _ = callback_owner.update(cx, |s, cx| {
                                        s.picker_busy = false;
                                        match outcome { PathOutcome::Selected(path) => s.dispatch(Command::Import(path.to_string_lossy().into_owned()), cx), PathOutcome::Cancelled => {}, PathOutcome::Failed(_) => { s.error = Some("The folder picker failed. Try importing again.".into()); cx.notify(); } }
                                    });
                                })
                            });
                            if !matches!(result, Ok(crate::native::PathStart::Started)) {
                                let _ = weak.update(cx, |s, cx| { s.picker_busy = false; s.error = Some("Finish the current folder picker before opening another.".into()); cx.notify(); });
                            }
                        });
                    }
                }
            }
            Action::FocusComposer => {
                if let Some(live) = self.sessions.get(&self.active) {
                    use gpui_kit::Focusable;
                    live.composer
                        .read(cx)
                        .editor()
                        .read(cx)
                        .focus_handle(cx)
                        .focus(window, cx);
                }
            }
            _ => {
                self.error = Some("This action is not available in the native app yet.".into());
                cx.notify();
            }
        }
    }
}
fn title(live: &LiveSession) -> String {
    let name = &live.controller.state().identity.name;
    if !name.is_empty() {
        name.clone()
    } else {
        live.project
            .sessions
            .iter()
            .find(|s| s.id == live.source_id)
            .map(|s| s.title.clone())
            .unwrap_or_else(|| "New Session".into())
    }
}
impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use gpui_kit::TestSupportExt;
        self.sync_preview(cx);
        self.sync_preview_theme(window, cx);
        self.sync_chrome_admission(cx);
        let t = support::tokens(self.dark);
        let mut body = Self::palette_keymap(div().key_context("TauShell"), cx)
            .when_some(self.shell_focus.as_ref(), |body, focus| {
                body.track_focus(focus)
            })
            .capture_key_down(cx.listener(Self::admin_key))
            .flex()
            .size_full()
            .bg(rgb(t.canvas))
            .text_color(rgb(t.text));
        if let Some(first_run) = self.first_run_view(window, cx) {
            body = body.child(first_run);
        } else {
            if let Some(sidebar) = &self.sidebar {
                body = body.child(sidebar.clone());
            }
            let mut main = div().flex().flex_col().flex_1().min_w_0().h_full();
            if let Some(header) = &self.header {
                let composer = self
                    .sessions
                    .get(&self.active)
                    .map(|live| live.composer.clone());
                main = main.child(
                    div()
                        .w_full()
                        .flex_shrink_0()
                        .on_prepaint(move |bounds, _, cx| {
                            if let Some(composer) = &composer {
                                composer.update(cx, |composer, cx| {
                                    composer.set_command_top_boundary(bounds.bottom(), cx);
                                });
                            }
                        })
                        .child(header.clone()),
                );
            }
            if let Some(error) = self.session_action_error() {
                main = main.child(
                    div().px(px(24.)).child(error.to_owned()).child(
                        Button::new("retry-session-action")
                            .child("Retry")
                            .on_click(cx.listener(|s, _, window, cx| {
                                s.retry_session_action(window, cx);
                            })),
                    ),
                );
            }
            if let Some(live) = self.sessions.get(&self.active) {
                if self.active_session_loading()
                    && live.controller.state().extension_dialogs.is_empty()
                {
                    main = main.child(
                        div()
                            .id("session-loading")
                            .test_support()
                            .flex_1()
                            .min_h_0()
                            .flex()
                            .flex_col()
                            .items_center()
                            .justify_center()
                            .pb(px(24.))
                            .text_size(px(12.))
                            .text_color(rgb(t.muted))
                            .when(self.loading_indicator_visible(), |loading| {
                                loading.child(
                                    div()
                                        .id("session-loading-content")
                                        .test_support()
                                        .flex()
                                        .flex_col()
                                        .items_center()
                                        .gap(px(10.))
                                        .child(components::transcript::working_spinner(
                                            "Loading session".into(),
                                            t.muted,
                                            cx.reduce_motion(),
                                        ))
                                        .child(
                                            div()
                                                .id("session-loading-label")
                                                .test_support()
                                                .child("Loading"),
                                        ),
                                )
                            }),
                    );
                } else {
                    let empty = live.transcript.read(cx).is_empty()
                        && live.transcript.read(cx).working_indicator().is_none()
                        && live.controller.state().extension_dialogs.is_empty()
                        && !live.controller.state().compacting;
                    live.composer.update(cx, |composer, cx| {
                        composer.set_fill_height(empty, cx);
                    });
                    if !empty {
                        main = main.child(
                            div()
                                .id("shell-transcript-slot")
                                .test_support()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .min_h_0()
                                .child(live.transcript.clone()),
                        );
                    }
                    if live.controller.state().extension_dialogs.is_empty() {
                        main = main.child(live.queue.clone());
                        main = main.child(
                            div()
                                .id("shell-composer-area")
                                .test_support()
                                .flex()
                                .flex_col()
                                .min_h_0()
                                .flex_shrink_0()
                                .pt(px(6.))
                                .pr(px(8.))
                                .pb(px(8.))
                                .pl(px(6.))
                                .when(empty, |area| area.flex_1())
                                .when(!empty, |area| area.border_t_1().border_color(rgb(t.border)))
                                .child(live.composer.clone()),
                        );
                    }
                }
            } else {
                main = main.child(div().flex_1()).child(div().p(px(24.)).child(
                    if self.workspace.is_none() && self.error.is_none() {
                        "Loading workspace…"
                    } else {
                        "Select a session or import a project."
                    },
                ));
            }
            if !self.register_retries.is_empty() || self.save_failed {
                main = main.child(Button::new("retry-storage").child("Retry Save").on_click(
                    cx.listener(|s, _, _, cx| {
                        let retries = std::mem::take(&mut s.register_retries);
                        for command in retries {
                            s.dispatch(command, cx);
                        }
                        if s.save_failed {
                            s.persist(false, cx);
                        }
                        s.refresh_window(cx);
                    }),
                ));
            }
            body = body.child(main);
        }
        body = body
            .when_some(self.preview_view(), |body, preview| body.child(preview))
            .when_some(self.preview_menu_view(), |body, menu| body.child(menu))
            .when_some(self.palette.view.clone(), |body, palette| {
                body.child(palette)
            });
        if self.confirmation.is_none() {
            body = body.when_some(self.remote.view.clone(), |body, remote| body.child(remote));
        }
        if let Some(dialog) = &self.confirmation {
            body = body.child(dialog.clone());
        } else if let Some((_, dialog)) = &self.fallback
            && dialog.read(cx).is_open()
        {
            body = body.child(dialog.clone());
        }
        body.on_action(cx.listener(Self::copy_link)).when_some(
            self.preview_feedback().zip(self.preview_anchor()),
            |body, (message, anchor)| {
                body.child(
                    div()
                        .id("file-link-feedback")
                        .test_support()
                        .absolute()
                        .left(anchor.x)
                        .top(anchor.y + px(8.))
                        .px(px(8.))
                        .py(px(4.))
                        .rounded(px(4.))
                        .bg(rgb(if self.dark { 0x252a34 } else { 0xf0f1f3 }))
                        .text_color(rgb(if self.dark { 0xe4e7ed } else { 0x252a34 }))
                        .text_size(px(11.))
                        .child(message),
                )
            },
        )
    }
}

#[cfg(test)]
#[path = "shell_integration_tests.rs"]
mod shell_integration_tests;

#[cfg(test)]
#[path = "layout_tests.rs"]
mod layout_tests;

#[cfg(test)]
#[path = "confirm_queue_fixture.rs"]
pub(super) mod confirm_queue_fixture;
#[cfg(test)]
#[path = "confirm_shortcut_tests.rs"]
mod confirm_shortcut_tests;
#[cfg(test)]
#[path = "extension_provenance_tests.rs"]
mod extension_provenance_tests;
#[cfg(test)]
#[path = "queue_pending_tests.rs"]
mod queue_pending_tests;
#[cfg(test)]
#[path = "settings_pending_tests.rs"]
mod settings_pending_tests;
