use super::confirm_queue_fixture::Fixture;
use gpui_kit::test::TestWindowExt;
use serde_json::{Value, json};

fn assert_busy(f: &mut Fixture, busy: bool) {
    f.root.draw();
    f.root.update(|_, w, cx| {
        let live = &f.shell.read(cx).sessions["a"];
        assert_eq!(live.controller.queue_busy(), busy);
        assert_eq!(live.queue.read(cx).props().busy, busy);
        assert_eq!(
            live.queue.read(cx).props().feedback.as_str(),
            live.controller.queue_feedback().unwrap_or_default()
        );
        assert_eq!(
            w.try_find("feedback").is_some(),
            live.controller.queue_feedback().is_some()
        );
        if busy {
            assert!(
                live.controller.queue_feedback().is_none(),
                "admitted clear retires old feedback"
            );
        }
        assert!(w.find("clear").visible());
        assert_eq!(w.find("clear").disabled() == Some(true), busy);
    });
}

fn clear(f: &mut Fixture) -> Value {
    let geometry = f
        .root
        .update(|_, w, _| ["clear", "steer"].map(|id| w.find(id).bounds()));
    f.click("clear");
    let requests = f.sent();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0]["type"], "clear_queue");
    assert_busy(f, true);
    f.root.update(|_, w, cx| {
        assert_eq!(["clear", "steer"].map(|id| w.find(id).bounds()), geometry);
        let props = f.shell.read(cx).sessions["a"].queue.read(cx).props();
        assert!(
            !props.steering.is_empty(),
            "clear must not optimistically erase items"
        );
    });
    f.click("clear");
    f.click("clear");
    assert!(f.sent().is_empty(), "pending clear must be inert");
    requests.into_iter().next().unwrap()
}

#[test]
fn mounted_queue_clear_pending_success_preserves_newer_update() {
    let mut f = Fixture::new();
    f.queue(&["old steering"], &["old follow up"]);
    let request = clear(&mut f);
    f.queue(&["new steering"], &["new follow up"]);
    assert_busy(&mut f, true);
    f.respond(&request, true, json!({}));
    assert_busy(&mut f, false);
    f.root.update(|_, _, cx| {
        let live = &f.shell.read(cx).sessions["a"];
        assert_eq!(live.queue.read(cx).props().steering, vec!["new steering"]);
        assert_eq!(live.queue.read(cx).props().follow_up, vec!["new follow up"]);
    });
    let retry = clear(&mut f);
    f.respond(&retry, true, json!({}));
    f.root.update(|_, w, cx| {
        let props = f.shell.read(cx).sessions["a"].queue.read(cx).props();
        assert!(!props.busy);
        assert!(props.steering.is_empty() && props.follow_up.is_empty());
        assert!(w.try_find("clear").is_none());
        assert!(w.try_find("feedback").is_none());
    });
}

#[test]
fn mounted_queue_clear_failure_and_timeout_release_button_without_losing_items() {
    for timeout in [false, true] {
        let mut f = Fixture::new();
        f.queue(&["keep steering"], &["keep follow up"]);
        let request = clear(&mut f);
        if timeout {
            f.root.update(|_, w, cx| {
                f.shell.update(cx, |s, cx| {
                    let live = s.sessions.get_mut("a").unwrap();
                    let effects = live.controller.tick(60_001);
                    live.dirty = true;
                    s.effects("a", effects, cx);
                    s.sync(w, cx);
                });
            });
        } else {
            f.respond(&request, false, json!({}));
        }
        assert_busy(&mut f, false);
        f.root.update(|_, _, cx| {
            let live = &f.shell.read(cx).sessions["a"];
            assert!(
                live.controller.state().error.is_none(),
                "queue failure must not open a generic incident"
            );
            assert_eq!(
                live.controller.queue_feedback(),
                Some(if timeout {
                    "Queue clear was not confirmed. Check the queue before retrying."
                } else {
                    "Could not clear the queue. Check the Pi version and retry."
                })
            );
            assert_eq!(live.queue.read(cx).props().steering, vec!["keep steering"]);
            assert_eq!(
                live.queue.read(cx).props().follow_up,
                vec!["keep follow up"]
            );
        });
        f.click("dismiss");
        assert_busy(&mut f, false);
        f.root.update(|_, _, cx| {
            let live = &f.shell.read(cx).sessions["a"];
            assert!(live.controller.queue_feedback().is_none());
            assert_eq!(live.queue.read(cx).props().steering, vec!["keep steering"]);
            assert_eq!(
                live.queue.read(cx).props().follow_up,
                vec!["keep follow up"]
            );
        });
        assert!(f.sent().is_empty(), "Dismiss must not send or clear");
        let retry = clear(&mut f);
        assert_ne!(retry["id"], request["id"]);
        if timeout {
            // A late old acknowledgment must not resolve the new clear or erase updates.
            f.queue(&["new after timeout"], &["new follow up"]);
            f.respond(&request, true, json!({}));
            assert_busy(&mut f, true);
            f.root.update(|_, _, cx| {
                assert_eq!(
                    f.shell.read(cx).sessions["a"]
                        .queue
                        .read(cx)
                        .props()
                        .steering,
                    vec!["new after timeout"]
                );
            });
        }
        f.respond(&retry, false, json!({}));
        assert_busy(&mut f, false);
    }
}

#[test]
fn mounted_queue_preparation_and_prompt_pending_disable_clear_too() {
    let mut f = Fixture::new();
    f.queue(&["existing"], &[]);
    f.ingest(json!({"type":"agent_start"}));
    f.complete_reads(true);
    assert!(f.sent().is_empty());
    f.root.update(|_, w, cx| {
        f.shell.read(cx).sessions["a"]
            .composer
            .read(cx)
            .editor()
            .update(cx, |e, cx| e.focus(w, cx));
    });
    f.root.draw();
    f.root.simulate_input("queue next");
    f.root.simulate_keystroke("enter");
    f.root.draw();
    let mut requests = f.sent();
    assert!(!requests.is_empty());
    for _ in 0..4 {
        assert_busy(&mut f, true);
        f.click("clear");
        assert!(f.sent().is_empty());
        let mut prompt = None;
        for request in requests {
            match request["type"].as_str() {
                Some("set_steering_mode" | "set_follow_up_mode") => {
                    f.respond(&request, true, json!({}));
                }
                Some("prompt") => prompt = Some(request),
                other => panic!("unexpected queue setup command: {other:?}"),
            }
        }
        if let Some(request) = prompt {
            f.respond(&request, true, json!({}));
            assert_busy(&mut f, false);
            return;
        }
        requests = f.sent();
    }
    panic!("queue preparation never produced a prompt");
}
