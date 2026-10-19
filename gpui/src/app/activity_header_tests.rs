use super::*;

#[test]
fn real_shell_heredoc_header_stays_compact_and_keeps_exact_arguments() {
    let (_app, mut root, shell, _requests) = fixture();
    root.simulate_resize(size(px(740.), px(700.)));
    root.update(|_, w, cx| w.bounds_changed(cx));
    let command = format!(
        "python <<'PY'\nprint({:?})\nPY",
        "long-unbroken-value".repeat(90)
    );
    root.update(|_, w, cx| shell.update(cx, |s, cx| {
        s.ingest(rpc(json!({"type":"tool_execution_start", "toolCallId":"heredoc", "toolName":"bash", "args":{"command":command}})), cx);
        s.sync(w, cx);
    }));
    root.draw();
    let header = root.update(|_, w, cx| {
        let header = w.find("a-activity-0").bounds();
        let slot = w.find("shell-transcript-slot").bounds();
        assert_eq!(header.size.height, px(18.5));
        assert!(header.left() >= slot.left() && header.right() <= slot.right());
        let entry = &shell.read(cx).sessions["a"].controller.state().transcript[0];
        let arguments: serde_json::Value =
            serde_json::from_str(&entry.tool_arguments_text()).unwrap();
        assert_eq!(arguments["command"], command);
        header
    });
    root.simulate_mouse_move(header.center());
    root.simulate_click(header.center(), MouseButton::Left);
    root.draw();
    root.update(|_, w, _| {
        assert!(w.find("Arguments-body").visible());
        assert_eq!(w.find("a-activity-0").bounds().size.height, px(18.5));
    });
}
