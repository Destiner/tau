use super::*;

fn reply(controller: &mut Controller, effect: &Effect, data: Value) -> Vec<Effect> {
    let Effect::Send(request) = effect else {
        panic!("expected RPC request")
    };
    controller.handle(json!({"type":"response","id":request["id"],"command":request["type"],"success":true,"data":data}))
}
fn hydrated() -> Controller {
    let mut controller = Controller::new("history");
    controller.begin_generation(1);
    let request = controller.send("get_messages", json!({}), false);
    reply(
        &mut controller,
        &request,
        json!({"messages":[
            {"role":"compactionSummary","timestamp":5,"summary":"/context/file.rs"},
            {"role":"user","timestamp":3,"content":"recent hidden"},
            {"role":"user","timestamp":4,"content":"current"}
        ]}),
    );
    controller
}

#[test]
fn history_is_session_local_incremental_and_survives_tail_hydration() {
    let mut controller = hydrated();
    let marker = controller.state.transcript[0].id.clone();
    let tail_ids: Vec<_> = controller
        .state
        .transcript
        .iter()
        .map(|r| r.id.clone())
        .collect();
    let effects = controller.load_history(&marker);
    assert_eq!(effects.len(), 1);
    assert!(controller.state.transcript[0].history_loading);
    assert!(controller.load_history(&marker).is_empty());
    reply(
        &mut controller,
        &effects[0],
        json!({"entries":crate::transcript::history_tests::tree(),"leafId":"f"}),
    );
    assert_eq!(controller.state.transcript[0].kind, "compaction");
    assert!(controller.state.transcript[0].history_available);
    assert_eq!(controller.state.transcript[1].text, "older");
    assert_eq!(controller.state.transcript[2].id, tail_ids[0]);
    assert!(!controller.state.transcript[2].history_available);
    let prefix_ids: Vec<_> = controller.state.transcript[..2]
        .iter()
        .map(|r| r.id.clone())
        .collect();
    let request = controller.send("get_messages", json!({}), false);
    reply(
        &mut controller,
        &request,
        json!({"messages":[
            {"role":"compactionSummary","timestamp":5,"summary":"not prose"},
            {"role":"user","timestamp":3,"content":"recent hidden"},
            {"role":"user","timestamp":4,"content":"current"}
        ]}),
    );
    assert_eq!(
        controller.state.transcript[..2]
            .iter()
            .map(|r| r.id.clone())
            .collect::<Vec<_>>(),
        prefix_ids
    );
    assert!(controller.load_history(&prefix_ids[0]).is_empty());
    assert_eq!(controller.state.transcript[0].text, "oldest");
    assert_eq!(controller.state.transcript[2].id, prefix_ids[1]);
    assert!(controller
        .state
        .transcript
        .iter()
        .all(|r| !r.history_available));
}

#[test]
fn old_history_responses_are_fenced_by_compaction_and_generation() {
    for replacement in [false, true] {
        let mut controller = hydrated();
        let marker = controller.state.transcript[0].id.clone();
        let request = controller.load_history(&marker).remove(0);
        if replacement {
            controller.begin_generation(2);
        } else {
            controller.handle(json!({"type":"compaction_end","result":{}}));
        }
        let before = controller.state.transcript.clone();
        reply(
            &mut controller,
            &request,
            json!({"entries":crate::transcript::history_tests::tree(),"leafId":"f"}),
        );
        assert_eq!(controller.state.transcript, before);
        assert!(controller.history_layers.is_empty());
    }
}

#[test]
fn history_failure_clears_busy_and_leaves_retry_available() {
    let mut controller = hydrated();
    let marker = controller.state.transcript[0].id.clone();
    let effect = controller.load_history(&marker).remove(0);
    let Effect::Send(request) = effect else {
        panic!()
    };
    controller.handle(json!({"type":"response","id":request["id"],"command":"get_entries","success":false,"error":"private raw error"}));
    assert!(!controller.state.transcript[0].history_loading);
    assert!(controller.state.transcript[0].history_available);
    assert!(controller
        .state
        .transcript
        .iter()
        .all(|r| !r.text.contains("private raw error")));
    assert_eq!(controller.load_history(&marker).len(), 1);
}

#[test]
fn compaction_reconciles_during_a_continuing_stream_without_reviving_old_context() {
    let mut controller = hydrated();
    controller.state.transcript.push(Entry::new(
        "local".into(),
        "notice",
        "obsolete".into(),
        "local:1".into(),
    ));
    controller.handle(json!({"type":"agent_start"}));
    controller.handle(json!({"type":"compaction_start"}));
    let effects = controller.handle(json!({"type":"compaction_end","result":{}}));
    let request = effects
        .iter()
        .find(|e| matches!(e, Effect::Send(v) if v["type"] == "get_messages"))
        .unwrap();
    controller.handle(
        json!({"type":"message_start","message":{"role":"assistant","timestamp":99,"content":[]}}),
    );
    controller.handle(json!({"type":"message_update","assistantMessageEvent":{"type":"text_delta","contentIndex":0,"delta":"fresh output"}}));
    reply(
        &mut controller,
        request,
        json!({"messages":[{"role":"compactionSummary","timestamp":98,"summary":"/private/context"}]}),
    );
    assert_eq!(
        controller
            .state
            .transcript
            .iter()
            .map(|r| (r.kind.as_str(), r.text.as_str()))
            .collect::<Vec<_>>(),
        [("compaction", ""), ("assistant", "fresh output")]
    );
    assert!(controller.state.working);
    assert!(controller.compaction_snapshot.is_none());
}

#[test]
fn empty_hidden_history_disables_only_the_boundary() {
    let mut controller = hydrated();
    let marker = controller.state.transcript[0].id.clone();
    let request = controller.load_history(&marker).remove(0);
    reply(&mut controller, &request, json!({"entries":[],"leafId":""}));
    assert!(!controller.state.transcript[0].history_available);
    assert!(!controller.state.transcript[0].history_loading);
    assert_eq!(controller.state.transcript[1].text, "recent hidden");
}
