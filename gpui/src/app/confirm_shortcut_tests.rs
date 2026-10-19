use super::confirm_queue_fixture::{Fixture, secondary};
use gpui_kit::test::TestWindowExt;
use serde_json::json;

#[test]
fn production_confirm_modifiers_send_fixed_booleans_independent_of_cursor() {
    for shift in [false, true] {
        let mut f = Fixture::new();
        f.dialog("confirm");
        if !shift {
            // Focus No before invoking the affirmative shortcut.
            f.root.simulate_keystroke("tab");
        }
        f.root.simulate_keystroke(secondary(shift));
        f.root.draw();
        let responses = f.sent();
        assert_eq!(responses.len(), 1);
        assert_eq!(
            responses[0],
            json!({"type":"extension_ui_response",
            "id":"question", "confirmed":!shift})
        );
        f.root.update(|_, _, cx| {
            let live = &f.shell.read(cx).sessions["a"];
            assert!(live.controller.state().extension_dialogs[0].submitting);
            assert!(live.extension.as_ref().unwrap().1.read(cx).is_busy());
        });
        f.root.simulate_keystroke(secondary(!shift));
        f.root.simulate_keystroke("enter");
        f.click(if shift { "confirm" } else { "reject" });
        assert!(
            f.sent().is_empty(),
            "pending responses must not be duplicated"
        );
    }
}

#[test]
fn production_select_modifiers_still_submit_the_highlighted_string() {
    for key in ["enter", secondary(false), secondary(true)] {
        let mut f = Fixture::new();
        f.dialog("select");
        f.root.simulate_keystroke("down");
        f.root.simulate_keystroke(key);
        f.root.draw();
        assert_eq!(
            f.sent(),
            vec![json!({"type":"extension_ui_response",
            "id":"question", "value":"Second"})]
        );
    }
}

#[test]
fn production_confirm_plain_enter_and_click_keep_selected_answer() {
    for click in [false, true] {
        let mut f = Fixture::new();
        f.dialog("confirm");
        if click {
            f.click("reject");
        } else {
            f.press("tab");
            f.root.update(|_, w, _| {
                assert_eq!(w.find("reject").focused(), Some(true));
            });
            f.press("enter");
        }
        assert_eq!(
            f.sent(),
            vec![json!({"type":"extension_ui_response",
            "id":"question", "confirmed":false})]
        );
    }
}
