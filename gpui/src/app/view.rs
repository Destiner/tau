use super::*;
use gpui_kit::base::Button;

impl Shell {
    pub(super) fn restore_input_focus(&self, cx: &mut Context<Self>) {
        let owner = cx.entity().downgrade();
        let target = self.window;
        cx.defer(move |cx| {
            if let Some(target) = target {
                let _ = target.update(cx, |_, window, _| {
                    window.on_next_frame(move |window, cx| {
                        let Some(owner) = owner.upgrade() else {
                            return;
                        };
                        let shell = owner.read(cx);
                        if shell.window != Some(target)
                            || shell.lifecycle.is_some()
                            || shell.confirmation.is_some()
                        {
                            return;
                        }
                        let Some(live) = shell.sessions.get(&shell.active) else {
                            return;
                        };
                        if let Some((_, dialog)) = &live.extension {
                            let dialog = dialog.clone();
                            dialog.update(cx, |dialog, cx| dialog.focus(window, cx));
                        } else if !live.composer.read(cx).is_disabled() {
                            use gpui_kit::Focusable;
                            let focus = live.composer.read(cx).editor().read(cx).focus_handle(cx);
                            focus.focus(window, cx);
                        }
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
        let projects = self
            .workspace
            .as_ref()
            .map(|w| {
                w.projects
                    .iter()
                    .map(|p| {
                        let mut sessions: Vec<_> =
                            p.sessions
                                .iter()
                                .filter(|s| !s.archived)
                                .map(|s| {
                                    let live = self.sessions.iter().find(|(_, l)| {
                                        l.project.path == p.path
                                            && (l.controller.state().identity.id == s.id
                                                || l.source_id == s.id)
                                    });
                                    SessionItem {
                                        id: live
                                            .map(|(k, _)| k.clone())
                                            .unwrap_or_else(|| s.id.clone()),
                                        title: live
                                            .map(|(_, l)| title(l))
                                            .unwrap_or_else(|| s.title.clone()),
                                        time: s.last_active.clone(),
                                        status: live.and_then(|(_, l)| {
                                            l.controller.state().working.then_some(
                                                components::chrome::SessionStatus::Working,
                                            )
                                        }),
                                        unread: false,
                                        can_archive: self.can_archive(&s.id),
                                    }
                                })
                                .collect();
                        for (key, live) in &self.sessions {
                            if live.project.path == p.path
                                && !p.sessions.iter().any(|s| {
                                    s.id == live.controller.state().identity.id
                                        || s.id == live.source_id
                                })
                            {
                                sessions.push(SessionItem {
                                    id: key.clone(),
                                    title: title(live),
                                    time: String::new(),
                                    status: Some(if live.controller.state().working {
                                        components::chrome::SessionStatus::Working
                                    } else {
                                        components::chrome::SessionStatus::Draft
                                    }),
                                    unread: false,
                                    can_archive: false,
                                });
                            }
                        }
                        ProjectItem {
                            id: p.path.clone(),
                            title: p.name.clone(),
                            location: p.working_directory.clone(),
                            collapsed: p.collapsed,
                            sessions,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        if let Some(sidebar) = &self.sidebar {
            let mut props = sidebar_props();
            props.active = self.active.clone();
            props.archived = self.session_actions.archived;
            if props.archived {
                props.sessions = self.archived_items();
            }
            sidebar.update(cx, |s, cx| {
                s.set_props(props, cx);
                s.set_projects(projects, cx);
                if changed {
                    s.set_theme(dark, window, cx);
                }
            });
        }
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
        let keys: Vec<_> = self.sessions.keys().cloned().collect();
        for key in keys {
            let live = self.sessions.get_mut(&key).unwrap();
            if !live.dirty && !changed {
                continue;
            }
            live.dirty = false;
            let state = live.controller.state();
            if live.projection_dirty {
                live.projection
                    .sync(&state.transcript, &live.transcript, cx);
                live.projection_dirty = false;
            }
            if state.draft != live.shown_draft {
                let draft = state.draft.clone();
                live.shown_draft = draft.clone();
                let editor = live.composer.read(cx).editor();
                editor.update(cx, |e, cx| e.set_value(&draft, window, cx));
            }
            // Models/settings mutate only on protocol reads, not frame rendering.
            let choice_key = serde_json::to_string(&(
                &state.models,
                &state.thinking_levels,
                &state.model,
                &state.effort,
            ))
            .unwrap_or_default();
            if choice_key != live.choices {
                live.choices = choice_key;
                let models = state
                    .models
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
                        label: level.clone(),
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
                    c.set_effort(&state.effort, cx);
                });
            }
            let status = (
                !state.ready || live.starting || state.stopping || self.exit_revision.is_some(),
                state.working,
            );
            let status_changed = live.ui_status != Some(status);
            live.ui_status = Some(status);
            live.composer.update(cx, |c, cx| {
                if status_changed {
                    if state.error.is_some() && state.working {
                        c.set_status(status.0, false, cx);
                    }
                    c.set_status(status.0, status.1, cx);
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
                        feedback: String::new(),
                        busy: false,
                        can_recover: false,
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
            if request.as_ref().map(|r| &r.id) != live.extension.as_ref().map(|(id, _)| id) {
                live.extension = None;
                live.extension_subscription = None;
                if let Some(request) = request.filter(|_| key == self.active) {
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
                                    .map(|s| DialogOption {
                                        id: s.into(),
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
                        NativeDialog::new(props, dark, window, cx)
                            .embedded()
                            .keyboard_submission()
                    });
                    if let Some(prefill) = request.request["prefill"]
                        .as_str()
                        .or_else(|| request.request["value"].as_str())
                        .or_else(|| request.request["text"].as_str())
                    {
                        dialog
                            .read(cx)
                            .editor()
                            .update(cx, |e, cx| e.set_value(prefill, window, cx));
                    }
                    let id = request.id.clone();
                    let event_key = key.clone();
                    let confirm = method == "confirm";
                    let subscription =
                        cx.subscribe(&dialog, move |s, _, event: &DialogEvent, cx| {
                            if let Some(live) = s.sessions.get_mut(&event_key) {
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
                                s.effects(&event_key, effects, cx);
                                s.refresh_window(cx);
                            }
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
                    if changed {
                        d.set_theme(dark, window, cx);
                    }
                });
            }
        }
        if let Some(request) = self.lifecycle {
            if self.confirmation.is_none() && self.exit_revision.is_none() {
                let busy = self.is_busy();
                if !busy && self.loaded && !self.picker_busy && !self.save_failed {
                    self.begin_exit(request, matches!(request.intent(), Intent::Quit), cx);
                } else {
                    let props = DialogProps {
                        kind: DialogKind::Quit,
                        title: "Close Tau?".into(),
                        description: "Active work will stop. Drafts must be saved before closing."
                            .into(),
                        placeholder: String::new(),
                        options: vec![],
                    };
                    let dialog = cx.new(|cx| NativeDialog::new(props, dark, window, cx));
                    self.subscriptions.push(cx.subscribe(
                        &dialog,
                        move |s, _, event: &DialogEvent, cx| {
                            if s.lifecycle != Some(request) {
                                return;
                            }
                            match event {
                                DialogEvent::Cancelled => {
                                    cx.global::<NativeHost>()
                                        .lifecycle()
                                        .resolve(request, false, cx);
                                }
                                DialogEvent::Submitted(_) => {
                                    if s.picker_busy {
                                        s.error = Some(
                                            "Finish or cancel the folder picker before closing."
                                                .into(),
                                        );
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
                dialog.update(cx, |d, cx| d.set_busy(self.exit_revision.is_some(), cx));
            }
        }
        cx.notify();
    }
    pub(super) fn select(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
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
            self.sessions.get_mut(&key).unwrap().dirty = true;
            if let Some((project, session, _)) = self.saved_session(&key)
                && !session.archived
            {
                self.dispatch(
                    Command::Select {
                        project: project.path,
                        session: session.id,
                    },
                    cx,
                );
            }
            self.active = key.clone();
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
                self.dispatch(
                    Command::Select {
                        project: project.path.clone(),
                        session: session.id.clone(),
                    },
                    cx,
                );
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
        if self.session_action(&action, window, cx) {
            return;
        }
        match action {
            Action::Select(id) => self.select(&id, window, cx),
            Action::NewSession => self.new_session(None, window, cx),
            Action::NewSessionIn(project) => self.new_session(Some(project), window, cx),
            Action::ProjectToggle(project) => {
                let collapsed = self
                    .workspace
                    .as_ref()
                    .and_then(|w| w.projects.iter().find(|p| p.path == project))
                    .is_some_and(|p| !p.collapsed);
                self.dispatch(Command::Collapse { project, collapsed }, cx);
            }
            Action::ReorderProjects(projects) => self.dispatch(Command::Reorder(projects), cx),
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
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use gpui_kit::TestSupportExt;
        let t = support::tokens(self.dark);
        let mut body = div()
            .capture_key_down(cx.listener(|s, event: &gpui_kit::KeyDownEvent, _, _| {
                if event.keystroke.key == "enter"
                    && !event.keystroke.modifiers.shift
                    && let Some(live) = s.sessions.get_mut(&s.active)
                {
                    live.follow_up = event.keystroke.modifiers.platform;
                }
            }))
            .flex()
            .size_full()
            .bg(rgb(t.canvas))
            .text_color(rgb(t.text));
        if let Some(sidebar) = &self.sidebar {
            body = body.child(sidebar.clone());
        }
        let mut main = div().flex().flex_col().flex_1().min_w_0().h_full();
        if let Some(header) = &self.header {
            main = main.child(header.clone());
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
            let empty = live.transcript.read(cx).is_empty();
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
            if live.starting {
                main = main.child(div().px(px(24.)).child("Starting Pi…"));
            }
            if live.controller.state().status == Status::Error {
                let key = self.active.clone();
                main = main.child(
                    div()
                        .px(px(24.))
                        .child(live.controller.state().error.clone().unwrap_or_else(|| {
                            "Pi could not complete this operation. Try again.".into()
                        }))
                        .child(
                            Button::new("retry-pi")
                                .child(if live.controller.state().ready {
                                    "Refresh"
                                } else {
                                    "Reconnect"
                                })
                                .on_click(cx.listener(move |s, _, _, cx| {
                                    if let Some(live) = s.sessions.get_mut(&key) {
                                        if live.starting {
                                            return;
                                        }
                                        if live.controller.state().ready {
                                            live.dirty = true;
                                            let effects = live.controller.refresh();
                                            s.effects(&key, effects, cx);
                                            s.refresh_window(cx);
                                            return;
                                        }
                                        live.starting = true;
                                        live.startup_events.clear();
                                        live.dirty = true;
                                        live.start_attempt += 1;
                                        live.start_deadline =
                                            s.epoch.elapsed().as_millis() as u64 + 10_000;
                                        let attempt = live.start_attempt;
                                        let project = live.project.clone();
                                        let path =
                                            if live.controller.state().identity.path.is_empty() {
                                                live.source_path.clone()
                                            } else {
                                                live.controller.state().identity.path.clone()
                                            };
                                        s.dispatch(
                                            Command::Start {
                                                runtime: key.clone(),
                                                attempt,
                                                project,
                                                source: (!path.is_empty()).then_some(path),
                                            },
                                            cx,
                                        );
                                        s.refresh_window(cx);
                                    }
                                })),
                        ),
                );
            }
            main = main.child(live.queue.clone());
            if let Some((_, dialog)) = &live.extension {
                main = main.child(dialog.clone());
            } else {
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
        } else {
            main = main.child(div().flex_1()).child(div().p(px(24.)).child(
                if self.workspace.is_none() && self.error.is_none() {
                    "Loading workspace…"
                } else {
                    "Select a session or import a project."
                },
            ));
        }
        if let Some(error) = &self.error {
            main = main.child(
                div().p(px(12.)).child(error.clone()).child(
                    Button::new("dismiss-error")
                        .child("Dismiss")
                        .on_click(cx.listener(|s, _, _, cx| {
                            s.error = None;
                            cx.notify();
                        })),
                ),
            );
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
        if let Some(dialog) = &self.confirmation {
            body = body.child(dialog.clone());
        }
        body
    }
}

#[cfg(test)]
#[path = "layout_tests.rs"]
mod layout_tests;
