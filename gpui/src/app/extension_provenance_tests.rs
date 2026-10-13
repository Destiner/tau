use super::confirm_queue_fixture::Fixture;
use super::*;
use crate::app::tests::rpc;
use gpui_kit::test::TestWindowExt;
use serde_json::{Value, json};

fn fail_answer(f: &mut Fixture) -> Value {
    f.dialog("input");
    f.click("cancel");
    let requests = f.sent();
    assert_eq!(requests.len(), 1);
    let request = requests[0].clone();
    assert_eq!(request["type"], "extension_ui_response");
    assert_eq!(request["id"], "question");
    f.root.update(|_, w, cx| {
        f.shell.update(cx, |s, cx| {
            s.ingest(
                Event::Written {
                    runtime: "a".into(),
                    generation: 8,
                    request: request.clone(),
                    success: false,
                },
                cx,
            );
            s.sync(w, cx);
            assert!(Shell::extension_failure_owned(&s.sessions["a"]));
            assert!(s.fallback.is_none());
        });
    });
    f.root.draw();
    f.root.update(|_, w, _| {
        assert!(w.try_find("dialog-error").is_some());
    });
    request
}

#[test]
fn owned_failure_stays_local_and_admitted_retry_preserves_other_work() {
    let mut f = Fixture::new();
    f.queue(&["steering"], &["follow up"]);
    f.root.update(|_, _, cx| {
        f.shell.update(cx, |s, _| {
            s.sessions
                .get_mut("a")
                .unwrap()
                .controller
                .set_draft("ordinary draft");
        });
    });
    let first = fail_answer(&mut f);
    f.click("cancel");
    let requests = f.sent();
    assert_eq!(requests, [first]);
    f.root.update(|_, w, cx| {
        assert!(w.try_find("dialog-error").is_none());
        let s = f.shell.read(cx);
        let live = &s.sessions["a"];
        assert!(live.controller.state().extension_dialogs[0].submitting);
        assert!(live.controller.state().error.is_none());
        assert!(!Shell::extension_failure_owned(live));
        assert!(s.fallback.is_none());
        assert_eq!(live.controller.state().draft, "ordinary draft");
        assert_eq!(live.controller.state().queue.steering, ["steering"]);
        assert_eq!(live.controller.state().queue.follow_up, ["follow up"]);
    });
}

#[test]
fn unrelated_incident_with_equal_cached_copy_is_not_hidden_by_local_failure() {
    let mut f = Fixture::new();
    fail_answer(&mut f);
    f.root.update(|_, w, cx| {
        f.shell.update(cx, |s, cx| {
            // A genuine unrelated controller incident invalidates delivery provenance.
            s.ingest(rpc("a", 8, json!({"type":"extension_error"})), cx);
            let live = s.sessions.get_mut("a").unwrap();
            let incident = live.controller.state().error.clone().unwrap();
            // Force the copy collision independently of today's reviewed wording.
            // Only Shell's presentation cache changes; no controller owner is fabricated.
            live.extension_error.as_mut().unwrap().2 = incident.clone();
            assert!(!live.controller.extension_delivery_error_owned("question"));
            assert!(!Shell::extension_failure_owned(live));
            s.sync(w, cx);
            assert_eq!(
                s.sessions["a"].controller.state().error.as_ref(),
                Some(&incident)
            );
            assert!(s.fallback.as_ref().unwrap().1.read(cx).is_open());
        });
    });
    f.root.draw();
    f.root.update(|_, w, _| {
        assert!(w.try_find("dialog").is_some());
    });
    assert!(f.sent().is_empty());
}
