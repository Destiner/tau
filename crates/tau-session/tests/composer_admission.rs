use serde_json::{json, Value};
use tau_session::session::{Controller, Effect, QueueKind, REQUEST_TIMEOUT_MS};

fn request(effects: &[Effect], command: &str) -> Value {
    effects
        .iter()
        .find_map(|effect| match effect {
            Effect::Send(v) if v["type"] == command => Some(v.clone()),
            _ => None,
        })
        .unwrap()
}

fn respond(c: &mut Controller, r: &Value, success: bool, data: Value) -> Vec<Effect> {
    c.handle(json!({"type":"response", "id":r["id"], "command":r["type"], "success":success, "data":data}))
}

fn ready() -> Controller {
    let mut c = Controller::new("r");
    c.bind_source("a", "/fixture/a.jsonl");
    let state = request(&c.begin_generation(1), "get_state");
    let effects = respond(
        &mut c,
        &state,
        true,
        json!({"sessionId":"a", "sessionFile":"/fixture/a.jsonl", "isStreaming":false}),
    );
    respond(
        &mut c,
        &request(&effects, "get_messages"),
        true,
        json!({"messages":[]}),
    );
    c
}

#[test]
fn pending_model_and_effort_writes_block_both_controls_until_success_failure_or_timeout() {
    for model_first in [false, true] {
        for outcome in ["success", "failure", "timeout"] {
            let mut c = ready();
            c.set_draft("editable draft");
            let effects = if model_first {
                c.set_model("fixture", "one")
            } else {
                c.set_thinking("high")
            };
            let pending = request(
                &effects,
                if model_first {
                    "set_model"
                } else {
                    "set_thinking_level"
                },
            );
            assert!(c.setting_pending());
            assert!(c.set_model("fixture", "two").is_empty());
            assert!(c.set_thinking("low").is_empty());
            c.set_draft("still editable");
            assert_eq!(c.state().draft, "still editable");
            match outcome {
                "timeout" => {
                    c.tick(REQUEST_TIMEOUT_MS);
                }
                _ => {
                    respond(&mut c, &pending, outcome == "success", json!({}));
                }
            }
            assert!(!c.setting_pending());
            assert!(!c.set_thinking("low").is_empty());
        }
    }
}

#[test]
fn settings_recheck_active_work_and_stopping_before_writing() {
    let mut c = ready();
    c.handle(json!({"type":"agent_start"}));
    for stopping in [false, true] {
        if stopping {
            assert!(!c.abort().is_empty());
        }
        assert!(c.set_model("fixture", "one").is_empty());
        assert!(c.set_thinking("high").is_empty());
    }
}

#[test]
fn compaction_blocks_stop_and_all_prompt_admission_and_restores_working_lifecycle() {
    for working in [false, true] {
        let mut c = ready();
        if working {
            c.handle(json!({"type":"agent_start"}));
        }
        c.set_draft("preserve me");
        c.handle(json!({"type":"compaction_start"}));
        assert!(c.abort().is_empty());
        assert!(c.submit("preserve me").is_empty());
        for kind in [QueueKind::Steer, QueueKind::FollowUp] {
            assert!(c.submit_queued("preserve me", kind).is_empty());
        }
        assert_eq!(c.state().draft, "preserve me");
        c.handle(json!({"type":"compaction_end"}));
        assert_eq!(c.state().working, working);
        assert!(!c.submit_queued("preserve me", QueueKind::Steer).is_empty());
    }
}

#[test]
fn mode_acknowledgments_never_resume_a_gesture_crossing_compaction_even_after_end() {
    for second_mode in [false, true] {
        for end_before_ack in [false, true] {
            for kind in [QueueKind::Steer, QueueKind::FollowUp] {
                let mut c = ready();
                c.handle(json!({"type":"agent_start"}));
                c.set_draft("queued draft");
                let mut pending =
                    request(&c.submit_queued("queued draft", kind), "set_steering_mode");
                if second_mode {
                    pending = request(
                        &respond(&mut c, &pending, true, json!({})),
                        "set_follow_up_mode",
                    );
                }
                c.handle(json!({"type":"compaction_start"}));
                if end_before_ack {
                    c.handle(json!({"type":"compaction_end"}));
                }
                assert!(respond(&mut c, &pending, true, json!({})).is_empty());
                assert_eq!(c.state().draft, "queued draft");
                assert!(!c.submission_pending());
                if !end_before_ack {
                    c.handle(json!({"type":"compaction_end"}));
                }
                assert!(!c.submit_queued("new gesture", kind).is_empty());
            }
        }
    }
}
