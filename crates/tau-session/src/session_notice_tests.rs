use super::*;
use crate::transcript::NoticeType;

#[path = "session_notice_hydration_tests.rs"]
mod hydration_tests;

fn user(text: &str) -> Value {
    json!({"role":"user", "timestamp":1, "content":text})
}
fn hydrate(c: &mut Controller, messages: &[Value]) {
    let Effect::Send(request) = c.send("get_messages", json!({}), false) else {
        panic!()
    };
    c.handle(
        json!({"type":"response", "id":request["id"], "command":"get_messages",
        "success":true, "data":{"messages":messages}}),
    );
}
fn notify(c: &mut Controller, id: &str, level: &str) {
    c.handle(
        json!({"type":"extension_ui_request", "method":"notify", "id":id,
        "notifyType":level, "message":"same error warning prose"}),
    );
}
fn bound() -> Controller {
    let mut c = Controller::new("runtime");
    c.bind_source("a", "/fixture/a.jsonl");
    c.begin_generation(1);
    c
}
fn texts(c: &Controller) -> Vec<&str> {
    c.state
        .transcript
        .iter()
        .map(|row| row.text.as_str())
        .collect()
}

#[test]
fn all_levels_stay_between_same_timestamp_occurrences_on_append_and_hydration() {
    let mut c = bound();
    hydrate(&mut c, &[user("first")]);
    for (id, level) in [("i", "info"), ("w", "warning"), ("e", "error")] {
        notify(&mut c, id, level);
        notify(&mut c, id, level);
    }
    assert_eq!(c.state.transcript.len(), 4);
    let ids: Vec<_> = c
        .state
        .transcript
        .iter()
        .map(|row| row.id.clone())
        .collect();
    c.handle(json!({"type":"message_start", "message":user("second")}));
    for _ in 0..3 {
        hydrate(&mut c, &[user("first"), user("second")]);
        assert_eq!(
            texts(&c),
            [
                "first",
                "same error warning prose",
                "same error warning prose",
                "same error warning prose",
                "second"
            ]
        );
        assert_eq!(
            c.state.transcript[..4]
                .iter()
                .map(|row| row.id.clone())
                .collect::<Vec<_>>(),
            ids
        );
        assert_ne!(c.state.transcript[0].id, c.state.transcript[4].id);
        for (row, level) in c.state.transcript[1..4].iter().zip([
            NoticeType::Info,
            NoticeType::Warning,
            NoticeType::Error,
        ]) {
            assert_eq!(row.notice_type, level);
            assert_eq!(row.anchor, Some(1));
            assert!(matches!(row.origin, EntryOrigin::RuntimeNotice { .. }));
        }
    }
}

#[test]
fn early_notice_claims_first_hydrated_spot_only_once_and_clamps_shorter_history() {
    let mut c = bound();
    notify(&mut c, "early", "info");
    assert_eq!(c.state.transcript[0].anchor, None);
    hydrate(&mut c, &[user("a"), user("b")]);
    assert_eq!(c.state.transcript[2].anchor, Some(2));
    hydrate(&mut c, &[user("a"), user("b"), user("c")]);
    assert_eq!(c.state.transcript[2].anchor, Some(2));
    hydrate(&mut c, &[user("a")]);
    assert_eq!(c.state.transcript[1].anchor, Some(1));
    hydrate(&mut c, &[user("a"), user("b")]);
    assert_eq!(c.state.transcript[1].anchor, Some(1));
}

#[test]
fn stream_updates_do_not_rebuild_or_touch_anchored_notices() {
    let mut c = bound();
    let history: Vec<_> = (0..5000).map(|i| user(&format!("row {i}"))).collect();
    hydrate(&mut c, &history);
    notify(&mut c, "held", "info");
    let before = c.state.transcript[5000].clone();
    let pointers: Vec<_> = c
        .state
        .transcript
        .iter()
        .map(|row| row.text.as_ptr())
        .collect();
    c.handle(json!({"type":"message_start", "message":{"role":"assistant", "timestamp":2, "content":[]}}));
    for _ in 0..100 {
        c.handle(json!({"type":"message_update", "assistantMessageEvent":{"type":"text_delta", "contentIndex":0, "delta":"x"}}));
    }
    assert_eq!(c.state.transcript[5000], before);
    for (row, pointer) in c.state.transcript.iter().zip(pointers) {
        assert_eq!(row.text.as_ptr(), pointer);
    }
}

#[test]
fn revealed_history_is_not_counted_again_and_replacement_clears_notices() {
    let mut c = bound();
    hydrate(&mut c, &[user("tail")]);
    notify(&mut c, "before", "info");
    let mut history = project(&user("older"), "history:older");
    history[0].id = "history-row".into();
    c.history_layers = vec![HistoryLayer {
        marker_id: "boundary".into(),
        rows: history,
    }];
    c.apply_history_prefix();
    notify(&mut c, "after", "warning");
    assert_eq!(c.state.transcript.last().unwrap().anchor, Some(1));
    hydrate(&mut c, &[user("tail"), user("new")]);
    assert_eq!(
        texts(&c),
        [
            "older",
            "tail",
            "same error warning prose",
            "same error warning prose",
            "new"
        ]
    );
    c.bind_source("b", "/fixture/b.jsonl");
    c.begin_generation(2);
    c.ingest(
        "runtime",
        1,
        json!({"type":"extension_ui_request", "method":"notify", "id":"stale", "message":"stale"}),
    );
    hydrate(&mut c, &[user("replacement")]);
    assert_eq!(texts(&c), ["replacement"]);
}

#[test]
fn preview_overlap_keeps_pi_ids_separate_from_runtime_notice_ownership() {
    let mut c = Controller::new("runtime");
    c.bind_source("a", "/fixture/a.jsonl");
    let token = c.saved_preview_token().unwrap();
    c.begin_generation(1);
    let messages = vec![user("same"), user("same")];
    assert!(c.apply_saved_preview(&token, &messages));
    let ids: Vec<_> = c
        .state
        .transcript
        .iter()
        .map(|row| row.id.clone())
        .collect();
    notify(&mut c, "preview-notice", "info");
    hydrate(&mut c, &messages);
    assert_eq!(
        c.state.transcript[..2]
            .iter()
            .map(|row| row.id.clone())
            .collect::<Vec<_>>(),
        ids
    );
    assert_ne!(ids[0], ids[1]);
    assert_eq!(c.state.transcript[2].anchor, Some(2));
    hydrate(&mut c, &[user("same"), user("same"), user("later")]);
    assert_eq!(c.state.transcript[2].anchor, Some(2));
    assert!(!c.apply_saved_preview(&token, &messages));
}

#[test]
fn typed_abort_preserves_partial_output_without_failure_but_real_error_while_stopping_surfaces() {
    for stop in ["aborted", "error"] {
        let mut c = bound();
        hydrate(&mut c, &[]);
        c.state.stopping = true;
        let message = json!({"role":"assistant", "timestamp":1, "stopReason":stop,
            "errorMessage":"aborted", "content":[{"type":"text", "text":"partial"},
                {"type":"toolCall", "id":"call", "name":"bash", "arguments":{}}]});
        c.handle(json!({"type":"message_start", "message":message}));
        c.handle(json!({"type":"message_end", "message":message}));
        assert_eq!(c.state.transcript[0].text, "partial");
        assert!(c.state.transcript[1].interrupted);
        assert_eq!(
            c.state
                .transcript
                .iter()
                .filter(|row| row.kind == "error")
                .count(),
            usize::from(stop == "error")
        );
        hydrate(&mut c, &[message]);
        assert_eq!(
            c.state
                .transcript
                .iter()
                .filter(|row| row.kind == "error")
                .count(),
            usize::from(stop == "error")
        );
    }
}

#[test]
fn custom_pi_notice_is_not_a_runtime_notice_and_legacy_entry_fields_default() {
    let mut c = bound();
    let custom = json!({"role":"custom", "timestamp":1, "display":true, "content":"info", "data":{"notifyType":"error"}});
    hydrate(&mut c, std::slice::from_ref(&custom));
    assert_eq!(c.state.transcript[0].origin, EntryOrigin::Pi);
    assert_eq!(c.state.transcript[0].notice_type, NoticeType::Info);
    notify(&mut c, "local", "warning");
    hydrate(&mut c, &[custom, user("next")]);
    assert_eq!(texts(&c), ["info", "same error warning prose", "next"]);
    let mut value = serde_json::to_value(&c.state.transcript[0]).unwrap();
    value.as_object_mut().unwrap().remove("origin");
    value.as_object_mut().unwrap().remove("anchor");
    let restored: Entry = serde_json::from_value(value).unwrap();
    assert_eq!(restored.origin, EntryOrigin::Pi);
    assert_eq!(restored.anchor, None);
}
