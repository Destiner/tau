use super::confirm_queue_fixture::Fixture;
use super::*;
use gpui_kit::test::TestWindowExt;
use serde_json::json;

#[test]
fn mounted_setting_write_disables_only_selectors_until_failure_or_timeout() {
    for (model, timeout) in [(true, false), (false, true)] {
        let mut f = Fixture::new();
        f.root.update(|_, _, cx| {
            f.shell.update(cx, |s, cx| {
                let effects = s.sessions.get_mut("a").unwrap().controller.refresh_models();
                s.effects("a", effects, cx);
            })
        });
        for request in f.sent() {
            let data = if request["type"] == "get_available_models" {
                json!({"models":[{"provider":"test", "id":"a", "name":"Test A"}]})
            } else {
                json!({"models":[]})
            };
            f.respond(&request, true, data);
        }
        f.sent();
        f.click("model");
        f.root
            .update(|_, w, _| assert!(w.find("model-menu").visible()));
        // Dispatch the setting from a second owned action while the selector is open.
        f.root.update(|_, _, cx| {
            f.shell.update(cx, |s, cx| {
                s.composer_event(
                    "a",
                    &if model {
                        ComposerEvent::ModelChanged("test\u{1f}a".into())
                    } else {
                        ComposerEvent::EffortChanged("high".into())
                    },
                    cx,
                );
            })
        });
        f.root.draw();
        let requests = f.sent();
        assert_eq!(requests.len(), 1);
        let request = &requests[0];
        assert_eq!(
            request["type"],
            if model {
                "set_model"
            } else {
                "set_thinking_level"
            }
        );
        f.root.update(|_, w, cx| {
            assert!(w.try_find("model-menu").is_none());
            for id in ["model", "effort"] {
                assert_eq!(w.find(id).disabled(), Some(true));
            }
            let composer = f.shell.read(cx).sessions["a"].composer.clone();
            assert!(!composer.read(cx).is_disabled());
            composer
                .read(cx)
                .editor()
                .update(cx, |e, cx| e.focus(w, cx));
        });
        f.root.simulate_input("draft stays editable");
        f.click("model");
        f.click("effort");
        f.root.update(|_, _, cx| {
            f.shell.update(cx, |s, cx| {
                s.composer_event("a", &ComposerEvent::ModelChanged("test\u{1f}a".into()), cx);
                s.composer_event("a", &ComposerEvent::EffortChanged("low".into()), cx);
            })
        });
        assert!(
            f.sent().is_empty(),
            "stale callbacks cannot bypass the controller gate"
        );
        if timeout {
            f.root.update(|_, w, cx| {
                f.shell.update(cx, |s, cx| {
                    let live = s.sessions.get_mut("a").unwrap();
                    let effects = live.controller.tick(60_001);
                    live.dirty = true;
                    s.effects("a", effects, cx);
                    s.sync(w, cx);
                })
            });
            f.root.draw();
        } else {
            f.respond(request, false, json!({}));
        }
        f.root.update(|_, w, cx| {
            for id in ["model", "effort"] {
                assert_ne!(w.find(id).disabled(), Some(true));
            }
            assert_eq!(
                f.shell.read(cx).sessions["a"].controller.state().draft,
                "draft stays editable"
            );
        });
        f.click("model");
        f.root
            .update(|_, w, _| assert!(w.find("model-menu").visible()));
    }
}
