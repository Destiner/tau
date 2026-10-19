use super::*;

fn ledger(messages: &[Value]) -> Admission {
    let mut ledger = Admission {
        projected: true,
        ..Admission::default()
    };
    for (i, message) in messages.iter().enumerate() {
        ledger.retain_wire(&format!("saved-{i}"), message);
    }
    ledger
}

fn realistic(i: usize) -> Value {
    json!({"role":"user", "timestamp":i, "content":format!(
        "Inspect the session response {i}, preserve its source binding and report the result."
    )})
}

#[test]
fn thirty_thousand_and_one_hundred_thousand_exact_occurrences_have_linear_index_work() {
    for n in [30_000, 100_000] {
        for duplicates in [false, true] {
            let messages: Vec<_> = (0..n)
                .map(|i| realistic(if duplicates { 0 } else { i }))
                .collect();
            let mut admission = ledger(&messages);
            assert!(!admission.degraded);
            assert_eq!(admission.sources.len(), n);
            assert_eq!(admission.wire_rows.len(), n);
            assert!(admission.wire_bytes <= MAX_WIRE_BYTES);
            let matched = admission.reconcile(&messages);
            for (i, source) in matched.iter().enumerate() {
                assert_eq!(source.as_deref(), Some(format!("saved-{i}").as_str()));
            }
            // One source lookup per admission, one queue insertion, one hydration
            // lookup and one equality per message. No timing/frame assumptions.
            assert_eq!(admission.operations, 4 * n);
            assert!(admission.wire_rows.is_empty());
            assert!(admission.sources.is_empty());
            assert!(!admission.projects_wire_rows());
        }
    }
}

#[test]
fn large_reordered_and_unmatched_histories_do_not_scan_the_ledger() {
    let n = 30_000;
    let messages: Vec<_> = (0..n).map(realistic).collect();
    let mut admission = ledger(&messages);
    let reversed: Vec<_> = messages.iter().rev().cloned().collect();
    let matched = admission.reconcile(&reversed);
    for (i, source) in matched.iter().enumerate() {
        assert_eq!(
            source.as_deref(),
            Some(format!("saved-{}", n - i - 1).as_str())
        );
    }
    assert_eq!(admission.operations, 4 * n);

    let mut admission = ledger(&messages);
    let unrelated: Vec<_> = (n..2 * n).map(realistic).collect();
    assert!(admission.reconcile(&unrelated).iter().all(Option::is_none));
    assert!(admission.operations <= 4 * n);
    assert!(admission.operations >= 3 * n);
}

#[test]
fn forced_hash_collisions_never_prove_identity_and_duplicates_keep_occurrence_order() {
    let a = json!({"role":"user", "timestamp":1, "content":"same prose"});
    let b = json!({"role":"user", "timestamp":1,
        "content":[{"type":"text", "text":"same prose"}]});
    let c = json!({"role":"user", "timestamp":1, "content":"different"});
    let absent = json!({"role":"user", "timestamp":1, "content":"absent"});
    let mut admission = ledger(&[a.clone(), b.clone(), a.clone(), c.clone()]);
    for row in &mut admission.wire_rows {
        row.fingerprint = 7;
    }
    assert_eq!(
        admission.reconcile_indexed(&[absent, c, a.clone(), b, a], |_| 7),
        vec![
            None,
            Some("saved-3".into()),
            Some("saved-0".into()),
            Some("saved-1".into()),
            Some("saved-2".into()),
        ]
    );
}

#[test]
fn repeated_saved_source_keys_remain_independent_wire_occurrences() {
    let a = json!({"role":"toolResult", "toolCallId":"same", "content":"first"});
    let b = json!({"role":"toolResult", "toolCallId":"same", "content":"second"});
    let updated = json!({"role":"toolResult", "toolCallId":"same", "content":"updated"});
    let mut admission = Admission {
        projected: true,
        ..Admission::default()
    };
    admission.append_wire("tool:same", &a);
    admission.append_wire("tool:same", &b);
    assert_eq!(admission.wire_rows.len(), 2);
    assert_eq!(admission.sources.len(), 1);
    admission.track("tool:same", &updated, &BTreeMap::new());
    assert_eq!(admission.wire_rows.len(), 2);
    assert_eq!(
        admission.reconcile(&[a, b, updated]),
        vec![None, Some("tool:same".into()), Some("tool:same".into()),]
    );
}

#[test]
fn same_wire_object_order_and_malformed_prefix_use_exact_value_equality() {
    let a: Value =
        serde_json::from_str(r#"{"role":"user","timestamp":1,"content":"same"}"#).unwrap();
    let reordered: Value =
        serde_json::from_str(r#"{"content":"same","timestamp":1,"role":"user"}"#).unwrap();
    assert_eq!(a, reordered);
    assert_eq!(fingerprint(&a), fingerprint(&reordered));
    let mut admission = ledger(&[Value::Null, json!({"role":23}), a.clone(), a.clone()]);
    assert_eq!(
        admission.reconcile(&[json!({"role":false}), reordered, a]),
        vec![None, Some("saved-2".into()), Some("saved-3".into()),]
    );
}

#[test]
fn updating_one_stream_source_replaces_one_raw_value_and_one_counter() {
    let messages: Vec<_> = (0..30_000).map(realistic).collect();
    let mut admission = ledger(&messages);
    let mut counts: BTreeMap<_, _> = (0..30_000).map(|i| (format!("user:{i}"), 1)).collect();
    let start = json!({"role":"assistant", "timestamp":30001, "content":[]});
    let end = json!({"role":"assistant", "timestamp":30001,
        "content":[{"type":"text", "text":"completed response"}]});
    counts.insert("assistant:30001".into(), 1);
    admission.track("assistant:30001:occ0", &start, &counts);
    let start_bytes = admission.wire_bytes;
    admission.track("assistant:30001:occ0", &end, &counts);
    assert_eq!(admission.wire_rows.len(), 30_001);
    assert_eq!(admission.sources.len(), 30_001);
    assert_eq!(admission.occupied.len(), 1);
    assert_eq!(admission.occupied["assistant:30001"], 1);
    assert_eq!(admission.operations, 30_002);
    assert!(admission.wire_bytes > start_bytes);
    assert_eq!(
        admission.reconcile(&[end, start]),
        vec![Some("assistant:30001:occ0".into()), None,]
    );
}

#[test]
fn count_overflow_discards_all_proofs_not_just_old_duplicate_references() {
    let message = realistic(0);
    let mut admission = Admission {
        projected: true,
        ..Admission::default()
    };
    for i in 0..MAX_WIRE_ROWS {
        admission.retain_wire(&format!("saved-{i}"), &message);
    }
    assert!(!admission.degraded);
    admission.retain_wire("excess", &message);
    assert!(admission.degraded);
    assert!(admission.needs_rebase);
    assert!(admission.projects_wire_rows());
    assert!(admission.wire_rows.is_empty());
    assert!(admission.sources.is_empty());
    assert!(admission.occupied.is_empty());
    assert_eq!(admission.wire_bytes, 0);
    let operations = admission.operations;
    let counts = BTreeMap::new();
    for _ in 0..100_000 {
        admission.track("unbounded-stream", &message, &counts);
    }
    assert_eq!(admission.operations, operations);
    assert_eq!(
        admission.reconcile(&[message.clone(), message]),
        vec![None, None]
    );
    assert!(!admission.degraded);
    assert!(!admission.needs_rebase);
}

#[test]
fn byte_overflow_and_replacement_growth_are_bounded_without_false_overlap() {
    let mut admission = ledger(&[realistic(0)]);
    // A single oversized wire must not be cloned into the ledger.
    let oversized = json!({"role":"user", "content":"x".repeat(MAX_WIRE_BYTES)});
    admission.track("saved-0", &oversized, &BTreeMap::new());
    assert!(admission.degraded);
    assert!(admission.wire_rows.is_empty());
    assert!(admission.sources.is_empty());
    assert_eq!(admission.wire_bytes, 0);
    assert_eq!(
        admission.reconcile(&[realistic(0), oversized]),
        vec![None, None]
    );
}

#[test]
fn controller_overflow_rebases_visible_rows_and_rpc_resets_cosmetic_fallback() {
    let mut c = Controller::new("runtime");
    c.bind_source("saved", "/saved.jsonl");
    c.begin_generation(1);
    let token = c.saved_preview_token().unwrap();
    let old = json!({"role":"user", "timestamp":1, "content":"same"});
    assert!(c.apply_saved_preview(&token, std::slice::from_ref(&old)));
    let old_id = c.state.transcript[0].id.clone();
    // Exercise the actual byte-budget branch without allocating huge UI content.
    c.saved_preview.wire_bytes = MAX_WIRE_BYTES;
    c.handle(json!({"type":"message_start", "message":old.clone()}));
    assert!(c.saved_preview.degraded);
    assert!(!c.saved_preview.needs_rebase);
    assert_ne!(c.state.transcript[0].id, old_id);
    assert!(c.state.transcript[0]
        .source
        .starts_with("preview-unproven:"));
    assert_eq!(c.state.transcript.len(), 2);
    // Continued identical starts must not reuse the just-reset counter sources.
    c.handle(json!({"type":"message_start", "message":old.clone()}));
    c.handle(json!({"type":"message_start", "message":old.clone()}));
    assert_eq!(c.state.transcript.len(), 4);
    let unique_sources: HashSet<_> = c.state.transcript.iter().map(|r| &r.source).collect();
    assert_eq!(unique_sources.len(), 4);
    let first_fallback_ids: Vec<_> = c.state.transcript.iter().map(|r| r.id.clone()).collect();
    c.begin_generation(2);
    assert!(c.message_occurrences.is_empty());
    assert!(c
        .state
        .transcript
        .iter()
        .all(|r| r.source.starts_with("preview-unproven:")));
    assert!(c
        .state
        .transcript
        .iter()
        .all(|r| !first_fallback_ids.contains(&r.id)));
    let visible_ids: Vec<_> = c.state.transcript.iter().map(|r| r.id.clone()).collect();
    let request = c.latest["get_messages"].clone();
    let effects = c.handle(json!({"type":"response", "id":request,
        "command":"get_messages", "success":true, "data":{"messages":[old]}}));
    assert!(c.saved_preview.authoritative);
    assert!(!c.saved_preview.degraded);
    assert!(!c.saved_preview.projects_wire_rows());
    assert_eq!(c.state.transcript.len(), 1);
    assert!(!visible_ids.contains(&c.state.transcript[0].id));
    assert!(!effects
        .iter()
        .any(|effect| matches!(effect, Effect::RegisterSession { .. })));
    assert!(!c.state.durable);
}

#[test]
fn controller_reordered_same_timestamp_prose_preserves_exact_duplicate_ids() {
    let mut c = Controller::new("runtime");
    c.bind_source("saved", "/saved.jsonl");
    c.begin_generation(1);
    let token = c.saved_preview_token().unwrap();
    let a = json!({"role":"user", "timestamp":1, "content":"same"});
    let b = json!({"role":"user", "timestamp":1,
        "content":[{"type":"text", "text":"same"}]});
    assert!(c.apply_saved_preview(&token, &[a.clone(), b.clone(), a.clone()]));
    let ids: Vec<_> = c.state.transcript.iter().map(|r| r.id.clone()).collect();
    let request = c.latest["get_messages"].clone();
    c.handle(
        json!({"type":"response", "id":request, "command":"get_messages",
        "success":true, "data":{"messages":[b, a.clone(), a]}}),
    );
    assert_eq!(
        c.state
            .transcript
            .iter()
            .map(|r| r.id.clone())
            .collect::<Vec<_>>(),
        vec![ids[1].clone(), ids[0].clone(), ids[2].clone()]
    );
}

#[test]
fn overflow_rebase_preserves_active_assistant_part_source_continuity() {
    let mut c = Controller::new("runtime");
    c.bind_source("saved", "/saved.jsonl");
    c.begin_generation(1);
    let token = c.saved_preview_token().unwrap();
    assert!(c.apply_saved_preview(&token, &[realistic(0)]));
    c.saved_preview.wire_bytes = MAX_WIRE_BYTES;
    c.handle(json!({"type":"message_start", "message":{
        "role":"assistant", "timestamp":1,
        "content":[{"type":"text", "text":"partial"}]}}));
    let key = c.active_assistant.clone().unwrap();
    assert!(key.starts_with("preview-unproven:"));
    assert_eq!(c.state.transcript[1].source, format!("{key}:0"));
    let id = c.state.transcript[1].id.clone();
    c.handle(json!({"type":"message_end", "message":{
        "role":"assistant", "timestamp":1,
        "content":[{"type":"text", "text":"complete"}]}}));
    assert_eq!(c.state.transcript.len(), 2);
    assert_eq!(c.state.transcript[1].id, id);
    assert_eq!(c.state.transcript[1].text, "complete");
}

#[test]
fn malformed_nonprojecting_prefix_does_not_shift_duplicate_visible_identity() {
    let mut c = Controller::new("runtime");
    c.bind_source("saved", "/saved.jsonl");
    c.begin_generation(1);
    let token = c.saved_preview_token().unwrap();
    let message = realistic(1);
    assert!(c.apply_saved_preview(&token, &[Value::Null, message.clone(), message.clone()]));
    let ids: Vec<_> = c.state.transcript.iter().map(|r| r.id.clone()).collect();
    assert_eq!(ids.len(), 2);
    let request = c.latest["get_messages"].clone();
    c.handle(
        json!({"type":"response", "id":request, "command":"get_messages",
        "success":true, "data":{"messages":[message.clone(), message]}}),
    );
    assert_eq!(
        c.state
            .transcript
            .iter()
            .map(|r| r.id.clone())
            .collect::<Vec<_>>(),
        ids
    );
}
