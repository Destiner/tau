use super::*;
use crate::app::tests::{app, project, shell};
use gpui_kit::test::TestWindowExt;
use gpui_kit::{MouseButton, TestApp, TestAppWindow, point, size};
use serde_json::json;
use tau_core::pi::PiEvent;

fn rpc(value: serde_json::Value) -> Event {
    Event::Pi(PiEvent {
        runtime_id: "a".into(),
        generation: 8,
        kind: "rpc",
        line: Some(value.to_string()),
        message: None,
        code: None,
    })
}

fn fixture() -> (
    TestApp,
    TestAppWindow<Root>,
    Entity<Shell>,
    async_channel::Receiver<Command>,
) {
    let mut app = app();
    let (commands, requests) = async_channel::bounded(256);
    let shell = app.new_entity(|_| shell(commands));
    let mut root = app.open_window(|w, cx| Root::new(shell.clone(), w, cx));
    root.simulate_resize(size(px(1000.), px(700.)));
    root.update(|_, w, cx| {
        w.bounds_changed(cx);
        shell.update(cx, |s, cx| {
            s.window = Some(w.window_handle());
            s.workspace = Some(WorkspaceSnapshot {
                projects: vec![project()],
                active_project_path: project().path,
                pi_path: None,
            });
            s.mount(w, cx);
            s.create_live("a".into(), project(), String::new(), String::new(), String::new(), w, cx);
            let attempt = s.sessions["a"].start_attempt;
            s.ingest(Event::Started { runtime: "a".into(), attempt, result: Ok(8) }, cx);
            let mut state = None;
            while let Ok(command) = requests.try_recv() {
                if let Command::Send { request, .. } = command
                    && request["type"] == "get_state"
                {
                    state = Some(request);
                }
            }
            s.ingest(rpc(json!({"type":"response", "id":state.unwrap()["id"],
                "command":"get_state", "success":true,
                "data":{"sessionId":"saved-a", "sessionFile":"/fixture/a.jsonl", "isStreaming":false}})), cx);
            assert!(s.sessions["a"].controller.state().ready);
            s.sync(w, cx);
        });
    });
    root.draw();
    (app, root, shell, requests)
}

fn assert_footer(root: &mut TestAppWindow<Root>) {
    root.update(|_, w, _| {
        let area = w.find("shell-composer-area").bounds();
        let toolbar = w.find("footer-gap").bounds();
        assert_eq!(area.bottom(), w.viewport_size().height);
        assert_eq!(area.bottom() - toolbar.bottom(), px(8.));
        assert_eq!(toolbar.left() - area.left(), px(6.));
        assert_eq!(area.right() - toolbar.right(), px(8.));
        for id in ["model", "effort", "send"] {
            let control = w.find(id).bounds();
            assert!(control.top() >= toolbar.top());
            assert!(control.bottom() <= toolbar.bottom());
        }
    });
}

#[test]
fn layout_empty_production_shell_fills_conversation_and_pads_footer() {
    let (_app, mut root, shell, _requests) = fixture();
    assert_footer(&mut root);
    root.update(|_, w, cx| {
        let area = w.find("shell-composer-area").bounds();
        let editor = w.find("editor").bounds();
        let toolbar = w.find("footer-gap").bounds();
        assert!(w.try_find("shell-transcript-slot").is_none());
        assert_eq!(editor.top(), area.top() + px(6.));
        assert_eq!(editor.bottom(), toolbar.top());
        assert!(
            editor.size.height > w.viewport_size().height * 0.75,
            "{editor:?}"
        );
        assert!(shell.read(cx).sessions["a"].transcript.read(cx).is_empty());
    });
    root.simulate_resize(size(px(900.), px(850.)));
    root.update(|_, w, cx| w.bounds_changed(cx));
    root.draw();
    assert_footer(&mut root);
    root.update(|_, w, _| {
        let editor = w.find("editor").bounds();
        assert!(editor.size.height > w.viewport_size().height * 0.75);
        assert_eq!(editor.bottom(), w.find("footer-gap").bounds().top());
    });
}

fn drag_text(root: &mut TestAppWindow<Root>, bounds: gpui_kit::Bounds<gpui_kit::Pixels>) -> String {
    let start = point(bounds.left(), bounds.center().y);
    let end = point(bounds.right() + px(4.), bounds.center().y);
    root.simulate_mouse_down(start, MouseButton::Left);
    root.simulate_event(gpui_kit::MouseMoveEvent {
        position: end,
        pressed_button: Some(MouseButton::Left),
        modifiers: Default::default(),
    });
    root.simulate_mouse_up(end, MouseButton::Left);
    root.update(|_, w, cx| gpui_kit::base::TextSelection::selected_text(w, cx))
}

#[test]
fn layout_editor_send_and_assistant_stream_paint_in_production_shell() {
    let (_app, mut root, shell, requests) = fixture();
    let (composer, transcript, editor) = root.read(|_, cx| {
        let live = &shell.read(cx).sessions["a"];
        (
            live.composer.clone(),
            live.transcript.clone(),
            live.composer.read(cx).editor(),
        )
    });
    root.update(|_, w, cx| editor.update(cx, |e, cx| e.focus(w, cx)));
    root.simulate_input("optimistic");
    root.simulate_keystrokes("enter");
    root.draw();
    assert!(std::iter::from_fn(|| requests.try_recv().ok()).any(
        |command| matches!(command, Command::Send { request, .. } if request["type"] == "prompt")
    ));
    assert_footer(&mut root);
    let bubble = root.update(|_, w, _| {
        let slot = w.find("shell-transcript-slot").bounds();
        let area = w.find("shell-composer-area").bounds();
        assert!(slot.bottom() <= area.top());
        assert!(slot.size.height > px(400.), "{slot:?}");
        assert_eq!(w.find("editor").bounds().size.height, px(83.));
        let bubble = w
            .painted_quads()
            .into_iter()
            .find(|q| {
                q.background == rgb(0xebeef0).into()
                    && slot.contains(&q.bounds.map(|p| px(p.as_f32() / w.scale_factor())).center())
            })
            .expect("optimistic user bubble must be painted inside the real transcript viewport");
        let bounds = bubble.bounds.map(|p| px(p.as_f32() / w.scale_factor()));
        gpui_kit::Bounds::from_corners(
            point(bounds.left() + px(13.), bounds.top() + px(9.)),
            point(bounds.right() - px(13.), bounds.bottom() - px(9.)),
        )
    });
    assert_eq!(
        drag_text(&mut root, bubble).trim(),
        "optimistic",
        "painted TextView glyph hit testing, not just a row count"
    );
    root.update(|_, w, cx| {
        shell.update(cx, |s, cx| {
            s.ingest(rpc(json!({"type":"message_start", "message":{"role":"assistant", "timestamp":1,"content":[]}})), cx);
            s.ingest(rpc(json!({"type":"message_update", "assistantMessageEvent":{"type":"text_delta", "contentIndex":0,"delta":"[streaming](https://example.com)"}})), cx);
            s.sync(w, cx);
        });
    });
    root.draw();
    root.update(|_, w, cx| {
        assert_eq!(
            gpui_kit::base::TextSelection::selected_text(w, cx).trim(),
            "optimistic",
            "streaming must preserve the retained user's selection"
        );
    });
    let ink = root.update(|_, w, _| {
        let slot = w.find("shell-transcript-slot").bounds();
        let underline = w
            .painted_underlines()
            .into_iter()
            .find(|u| slot.contains(&u.bounds.map(|p| px(p.as_f32() / w.scale_factor())).center()))
            .expect("assistant TextView must paint its streamed link in the actual viewport");
        let bounds = underline.bounds.map(|p| px(p.as_f32() / w.scale_factor()));
        gpui_kit::Bounds::from_corners(
            point(bounds.left(), bounds.top() - px(12.)),
            point(bounds.right(), bounds.top()),
        )
    });
    assert_eq!(drag_text(&mut root, ink).trim(), "streaming");
    root.read(|_, cx| {
        let live = &shell.read(cx).sessions["a"];
        assert_eq!(live.composer, composer);
        assert_eq!(live.transcript, transcript);
        assert_eq!(live.composer.read(cx).editor(), editor);
        assert!(live.transcript.read(cx).is_following_tail());
    });
    assert_footer(&mut root);
}
