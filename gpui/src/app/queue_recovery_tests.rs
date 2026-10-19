use super::*;
use gpui_kit::{MouseButton, base::Root, test::TestWindowExt};

struct QueueRoot(Entity<MessageQueue>);
impl Render for QueueRoot {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(
            div()
                .absolute()
                .left(px(16.))
                .bottom(px(60.))
                .w(px(600.))
                .child(self.0.clone()),
        )
    }
}

fn owned_fixture(s: &mut Shell, w: &mut Window, cx: &mut Context<Shell>) -> Entity<MessageQueue> {
    super::super::tests::create(s, "a", "", w, cx);
    s.active = "a".into();
    let live = s.sessions.get_mut("a").unwrap();
    live.restored_recovered = vec!["  first\n\n  draft  ".into(), "\tsecond\n".into()];
    let queue = live.queue.clone();
    queue.update(cx, |q, cx| {
        q.set_props(
            QueueProps {
                failed: live.restored_recovered.clone(),
                can_recover: true,
                ..Default::default()
            },
            cx,
        )
    });
    queue
}

#[test]
fn restore_uses_owned_text_exactly_and_consumes_only_selected_entry_without_send() {
    super::super::tests::with_shell(64, |s, w, cx, requests| {
        let queue = owned_fixture(s, w, cx);
        let recovery = s.queue_recovery("a", &queue, 1, cx).unwrap();
        assert!(s.restore_queue_draft(recovery, w, cx));
        let live = &s.sessions["a"];
        assert_eq!(live.controller.state().draft, "\tsecond\n");
        assert_eq!(live.shown_draft, "\tsecond\n");
        assert_eq!(
            live.composer.read(cx).editor().read(cx).value().as_ref(),
            "\tsecond\n"
        );
        assert_eq!(live.restored_recovered, ["  first\n\n  draft  "]);
        assert!(s.queue_recovery("a", &queue, 0, cx).is_none());
        while let Ok(command) = requests.try_recv() {
            assert!(!matches!(command, Command::Send { .. }));
        }
    });
}

#[test]
fn controller_owned_restore_preserves_unselected_entries_in_their_original_owner() {
    use serde_json::json;
    use tau_session::session::REQUEST_TIMEOUT_MS;
    super::super::tests::with_shell(64, |s, w, cx, requests| {
        let queue = owned_fixture(s, w, cx);
        let mut controller = Controller::new("a");
        controller.bind_source("fixture", "/fixture/session.jsonl");
        let mut effects: VecDeque<_> = controller.begin_generation(1).into();
        while let Some(effect) = effects.pop_front() {
            let Effect::Send(request) = effect else {
                continue;
            };
            let data = match request["type"].as_str().unwrap_or_default() {
                "get_state" => {
                    json!({"sessionId":"fixture","sessionFile":"/fixture/session.jsonl","isStreaming":false,"steeringMode":"all","followUpMode":"one-at-a-time"})
                }
                "get_messages" => json!({"messages":[]}),
                "get_commands" => json!({"commands":[]}),
                "get_available_models" => json!({"models":[]}),
                "get_scoped_models" => json!({"modelIds":[]}),
                _ => continue,
            };
            effects.extend(controller.handle(json!({"type":"response","id":request["id"],"command":request["type"],"success":true,"data":data})));
        }
        assert!(controller.state().ready);
        for (i, text) in ["  controller first\n", "\tcontroller second  "]
            .into_iter()
            .enumerate()
        {
            assert!(!controller.submit(text).is_empty());
            controller.tick(REQUEST_TIMEOUT_MS * (i as u64 + 1));
        }
        assert_eq!(controller.state().recovered_drafts.len(), 2);
        let live = s.sessions.get_mut("a").unwrap();
        live.controller = controller;
        let failed: Vec<_> = live
            .restored_recovered
            .iter()
            .chain(&live.controller.state().recovered_drafts)
            .cloned()
            .collect();
        queue.update(cx, |q, cx| {
            q.set_props(
                QueueProps {
                    failed,
                    can_recover: true,
                    ..Default::default()
                },
                cx,
            )
        });
        let recovery = s.queue_recovery("a", &queue, 2, cx).unwrap();
        assert!(s.restore_queue_draft(recovery, w, cx));
        let live = &s.sessions["a"];
        assert_eq!(live.controller.state().draft, "  controller first\n");
        assert_eq!(
            live.restored_recovered,
            ["  first\n\n  draft  ", "\tsecond\n"]
        );
        assert_eq!(
            live.controller.state().recovered_drafts,
            ["\tcontroller second  "]
        );
        assert_eq!(live.shown_draft, "  controller first\n");
        assert_eq!(
            live.composer.read(cx).editor().read(cx).value().as_ref(),
            "  controller first\n"
        );
        assert!(
            s.queue_recovery("a", &queue, 2, cx).is_none(),
            "nonempty draft cannot acquire another restore"
        );
        while let Ok(command) = requests.try_recv() {
            assert!(
                !matches!(command, Command::Send { .. }),
                "restore must not Send or Clear"
            );
        }
    });
}

#[test]
fn deferred_restore_rejects_newer_editor_draft_and_even_type_then_clear() {
    for clear_again in [false, true] {
        let mut app = super::super::tests::app();
        let (commands, _requests) = async_channel::bounded(64);
        let shell = app.new_entity(|_| super::super::tests::shell(commands));
        let mut window = app.open_window(|w, cx| Root::new(shell.clone(), w, cx));
        window.update(|_, w, cx| {
            shell.update(cx, |s, cx| {
                let queue = owned_fixture(s, w, cx);
                let recovery = s.queue_recovery("a", &queue, 0, cx).unwrap();
                let editor = s.sessions["a"].composer.read(cx).editor();
                editor.update(cx, |e, cx| e.replace_all("newer draft", w, cx));
                if clear_again {
                    editor.update(cx, |e, cx| e.replace_all("", w, cx));
                }
                cx.defer_in(w, move |s, w, cx| {
                    assert!(!s.restore_queue_draft(recovery, w, cx));
                    assert_eq!(s.sessions["a"].restored_recovered.len(), 2);
                    assert_eq!(
                        s.sessions["a"]
                            .composer
                            .read(cx)
                            .editor()
                            .read(cx)
                            .value()
                            .as_ref(),
                        if clear_again { "" } else { "newer draft" }
                    );
                });
            })
        });
        app.run_until_parked();
    }
}

#[test]
fn stale_ui_index_changed_ownership_session_switch_and_whitespace_draft_are_rejected() {
    super::super::tests::with_shell(64, |s, w, cx, _| {
        let queue = owned_fixture(s, w, cx);
        let recovery = s.queue_recovery("a", &queue, 1, cx).unwrap();
        s.sessions
            .get_mut("a")
            .unwrap()
            .restored_recovered
            .remove(0);
        assert!(!s.restore_queue_draft(recovery, w, cx));
        assert!(
            s.queue_recovery("a", &queue, 0, cx).is_none(),
            "stale UI cannot supply recovery text"
        );
        let queue = owned_fixture(s, w, cx);
        let recovery = s.queue_recovery("a", &queue, 0, cx).unwrap();
        s.active = "b".into();
        assert!(!s.restore_queue_draft(recovery, w, cx));
        s.active = "a".into();
        s.sessions.get_mut("a").unwrap().controller.set_draft("  ");
        assert!(!s.can_recover_queue("a", cx));
    });
}

#[test]
fn root_restore_button_reaches_shell_helper_and_never_dispatches_a_prompt() {
    let mut app = super::super::tests::app();
    let (commands, requests) = async_channel::bounded(64);
    let shell = app.new_entity(|_| super::super::tests::shell(commands));
    let mut window = app.open_window(|w, cx| {
        let queue = shell.update(cx, |s, cx| owned_fixture(s, w, cx));
        let host = cx.new(|_| QueueRoot(queue));
        Root::new(host, w, cx)
    });
    let subscription = window.update(|_, w, cx| {
        shell.update(cx, |s, cx| {
            let queue = s.sessions["a"].queue.clone();
            cx.subscribe_in(&queue, w, |s, source, event: &QueueEvent, w, cx| {
                if let QueueEvent::Recover(index) = event
                    && let Some(recovery) = s.queue_recovery("a", source, *index, cx)
                {
                    s.restore_queue_draft(recovery, w, cx);
                }
            })
        })
    });
    window.draw();
    let point = window.update(|_, w, _| w.find("recover").bounds().center());
    window.simulate_click(point, MouseButton::Left);
    window.draw();
    let point = window.update(|_, w, _| w.find("restore").bounds().center());
    window.simulate_click(point, MouseButton::Left);
    app.run_until_parked();
    window.update(|_, _, cx| {
        let s = shell.read(cx);
        assert_eq!(
            s.sessions["a"].controller.state().draft,
            "  first\n\n  draft  "
        );
        assert_eq!(s.sessions["a"].restored_recovered, ["\tsecond\n"]);
    });
    while let Ok(command) = requests.try_recv() {
        assert!(!matches!(command, Command::Send { .. }));
    }
    drop(subscription);
}
