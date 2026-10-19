use super::*;

#[test]
fn indexed_hydration_preserves_tool_call_and_result_as_one_row() {
    let mut c = bound();
    let messages = vec![
        json!({"role":"assistant", "timestamp":1, "content":[{"type":"toolCall", "id":"call", "name":"read", "arguments":{"path":"fixture"}}]}),
        json!({"role":"toolResult", "toolCallId":"call", "toolName":"read", "isError":true, "content":"result"}),
    ];
    hydrate(&mut c, &messages);
    assert_eq!(c.state.transcript.len(), 1);
    let id = c.state.transcript[0].id.clone();
    notify(&mut c, "held", "info");
    hydrate(&mut c, &messages);
    let tool = &c.state.transcript[0];
    assert_eq!(tool.id, id);
    assert_eq!(tool.arguments, json!({"path":"fixture"}));
    assert_eq!(tool.result, "result");
    assert!(tool.is_error);
    assert_eq!(c.state.transcript[1].anchor, Some(1));
}

#[test]
fn successful_compaction_drops_old_notices_but_keeps_new_live_notice() {
    let mut c = bound();
    hydrate(&mut c, &[user("old-a"), user("old-b"), user("old-c")]);
    notify(&mut c, "old-notice", "info");
    let effects = c.handle(json!({"type":"compaction_end", "result":{}}));
    assert!(!c
        .state
        .transcript
        .iter()
        .any(|row| matches!(row.origin, EntryOrigin::RuntimeNotice { .. })));
    notify(&mut c, "live-notice", "error");
    let request = effects
        .into_iter()
        .find_map(|effect| match effect {
            Effect::Send(request) if request["type"] == "get_messages" => Some(request),
            _ => None,
        })
        .unwrap();
    c.handle(
        json!({"type":"response", "id":request["id"], "command":"get_messages",
        "success":true, "data":{"messages":[user("kept")]}}),
    );
    assert_eq!(texts(&c), ["kept", "same error warning prose"]);
    assert_eq!(c.state.transcript[1].anchor, Some(1));
    c.handle(json!({"type":"message_start", "message":user("later")}));
    hydrate(&mut c, &[user("kept"), user("later")]);
    assert_eq!(texts(&c), ["kept", "same error warning prose", "later"]);
    assert!(matches!(&c.state.transcript[1].origin,
        EntryOrigin::RuntimeNotice { request_id } if request_id == "live-notice"));
}

#[test]
fn compaction_translates_interleaved_live_notices_before_later_hydration() {
    for suffix_in_snapshot in [false, true] {
        let mut c = bound();
        hydrate(&mut c, &[user("old-a"), user("old-b"), user("old-c")]);
        check_interleaved_compaction(&mut c, suffix_in_snapshot, Some(3));
    }
}

#[test]
fn compaction_resolves_unknown_interleaved_live_notice_anchors() {
    for suffix_in_snapshot in [false, true] {
        let mut c = bound();
        check_interleaved_compaction(&mut c, suffix_in_snapshot, None);
    }
}

fn check_interleaved_compaction(
    c: &mut Controller,
    suffix_in_snapshot: bool,
    initial_anchor: Option<usize>,
) {
    let effects = c.handle(json!({"type":"compaction_end", "result":{}}));
    let request = effects
        .into_iter()
        .find_map(|effect| match effect {
            Effect::Send(request) if request["type"] == "get_messages" => Some(request),
            _ => None,
        })
        .unwrap();
    let a = json!({"role":"user", "timestamp":2, "content":"live-a"});
    let b = json!({"role":"user", "timestamp":3, "content":"live-b"});
    notify(c, "before-a-1", "info");
    notify(c, "before-a-2", "warning");
    assert_eq!(c.state.transcript.last().unwrap().anchor, initial_anchor);
    c.handle(json!({"type":"message_start", "message":a}));
    notify(c, "between", "error");
    c.handle(json!({"type":"message_start", "message":b}));
    notify(c, "after-b", "info");
    let ids: Vec<_> = c
        .state
        .transcript
        .iter()
        .filter(|row| matches!(row.origin, EntryOrigin::RuntimeNotice { .. }))
        .map(|row| row.id.clone())
        .collect();
    let mut messages = vec![user("kept")];
    if suffix_in_snapshot {
        messages.extend([a.clone(), b.clone()]);
    }
    c.handle(
        json!({"type":"response", "id":request["id"], "command":"get_messages",
        "success":true, "data":{"messages":messages}}),
    );
    let expected = [
        "kept",
        "same error warning prose",
        "same error warning prose",
        "live-a",
        "same error warning prose",
        "live-b",
        "same error warning prose",
    ];
    assert_eq!(texts(c), expected);
    let notices: Vec<_> = c
        .state
        .transcript
        .iter()
        .filter(|row| matches!(row.origin, EntryOrigin::RuntimeNotice { .. }))
        .collect();
    assert_eq!(
        notices.iter().map(|row| row.anchor).collect::<Vec<_>>(),
        [Some(1), Some(1), Some(2), Some(3)]
    );
    assert_eq!(
        notices.iter().map(|row| row.id.clone()).collect::<Vec<_>>(),
        ids
    );
    let later = json!({"role":"user", "timestamp":4, "content":"later"});
    c.handle(json!({"type":"message_start", "message":later}));
    for _ in 0..2 {
        hydrate(c, &[user("kept"), a.clone(), b.clone(), later.clone()]);
        assert_eq!(
            texts(c),
            expected.into_iter().chain(["later"]).collect::<Vec<_>>()
        );
        let notices: Vec<_> = c
            .state
            .transcript
            .iter()
            .filter(|row| matches!(row.origin, EntryOrigin::RuntimeNotice { .. }))
            .collect();
        assert_eq!(
            notices.iter().map(|row| row.anchor).collect::<Vec<_>>(),
            [Some(1), Some(1), Some(2), Some(3)]
        );
        assert_eq!(
            notices.iter().map(|row| row.id.clone()).collect::<Vec<_>>(),
            ids
        );
    }
}
