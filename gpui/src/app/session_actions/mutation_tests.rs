use super::super::tests::{app, rpc, shell};
use super::*;
use serde_json::json;

#[test]
fn same_runtime_identity_replacement_cancels_open_header_without_committing() {
    let mut app = app();
    let (commands, requests) = async_channel::bounded(128);
    let shell = app.new_entity(|_| shell(commands));
    let mut root = app.open_window(|w, cx| Root::new(shell.clone(), w, cx));
    root.update(|_, w, cx| {
        shell.update(cx, |s, cx| {
            super::tests::fixture(s, w, cx);
            s.restored = true;
            s.mount(w, cx);
            s.sync(w, cx);
            s.header
                .as_ref()
                .unwrap()
                .update(cx, |h, cx| h.begin_rename(w, cx));
        })
    });
    root.draw();
    root.simulate_input("Old editor draft");
    root.update(|_, w, cx| shell.update(cx, |s, cx| {
        assert!(s.header.as_ref().unwrap().read(cx).renaming());
        while requests.try_recv().is_ok() {}
        s.ingest(rpc("live", 1, json!({"type":"session_info_changed"})), cx);
        let read = std::iter::from_fn(|| requests.try_recv().ok()).find_map(|c| match c {
            Command::Send { request, .. } if request["type"] == "get_state" => Some(request),
            _ => None,
        }).unwrap();
        s.ingest(rpc("live", 1, json!({"type":"response","id":read["id"],"command":"get_state","success":true,"data":{"sessionId":"replacement","sessionFile":"/replacement","sessionName":"Replacement"}})), cx);
        let p = &mut s.workspace.as_mut().unwrap().projects[0];
        p.sessions.push(serde_json::from_value(json!({"id":"replacement","path":"/replacement","title":"Replacement","lastActive":"now","lastUserMessageAt":0,"sortAt":0,"archived":false,"selected":true})).unwrap());
        assert_eq!(s.active, "live");
        assert!(s.sessions["live"].controller.state().ready);
        assert!(s.can_rename());
        s.sync(w, cx);
        assert!(!s.header.as_ref().unwrap().read(cx).renaming());
    }));
    root.simulate_keystroke("enter");
    root.draw();
    root.read(|_, cx| {
        assert_eq!(
            shell.read(cx).sessions["live"]
                .controller
                .state()
                .identity
                .name,
            "Replacement"
        )
    });
    assert!(
        !std::iter::from_fn(|| requests.try_recv().ok()).any(
            |c| matches!(c, Command::Send {request,..} if request["type"] == "set_session_name")
        )
    );
}

#[test]
fn actual_header_oversized_input_secondary_enter_sends_normalized_bounded_name() {
    for key in ["secondary-enter", "enter"] {
        let mut app = app();
        let (commands, requests) = async_channel::bounded(128);
        let shell = app.new_entity(|_| shell(commands));
        let mut root = app.open_window(|w, cx| Root::new(shell.clone(), w, cx));
        root.update(|_, w, cx| {
            shell.update(cx, |s, cx| {
                super::tests::fixture(s, w, cx);
                s.restored = true;
                s.mount(w, cx);
                s.sync(w, cx);
                s.header
                    .as_ref()
                    .unwrap()
                    .update(cx, |h, cx| h.begin_rename(w, cx));
            })
        });
        root.draw();
        let pasted = format!("  hello   {} tail", "😀".repeat(200));
        root.simulate_input(&pasted);
        root.read(|_, cx| {
            let h = shell.read(cx).header.as_ref().unwrap().read(cx);
            assert_eq!(h.editor().read(cx).value().as_ref(), pasted.as_str());
        });
        root.simulate_keystroke(key);
        root.draw();
        let name = std::iter::from_fn(|| requests.try_recv().ok())
            .find_map(|c| match c {
                Command::Send { request, .. } if request["type"] == "set_session_name" => {
                    Some(request["name"].as_str().unwrap().to_owned())
                }
                _ => None,
            })
            .unwrap();
        assert_eq!(name, tau_session::session::normalize_session_name(&pasted));
        assert!(name.encode_utf16().count() <= 240);
    }
}
