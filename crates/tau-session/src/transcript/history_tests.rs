use super::*;
use serde_json::json;

pub(crate) fn tree() -> Vec<Value> {
    vec![
        json!({"type":"message","id":"a","parentId":null,"message":{"role":"user","timestamp":1,"content":"oldest"}}),
        json!({"type":"message","id":"b","parentId":"a","message":{"role":"assistant","timestamp":2,"content":[{"type":"text","text":"older"}]}}),
        json!({"type":"compaction","id":"c","parentId":"b","firstKeptEntryId":"b","summary":"/raw/old/path"}),
        json!({"type":"message","id":"d","parentId":"c","message":{"role":"user","timestamp":3,"content":"recent hidden"}}),
        json!({"type":"compaction","id":"e","parentId":"d","firstKeptEntryId":"d","summary":"/raw/new/path"}),
        json!({"type":"message","id":"f","parentId":"e","message":{"role":"user","timestamp":4,"content":"current"}}),
        json!({"type":"message","id":"other","parentId":"a","message":{"role":"user","content":"off branch"}}),
    ]
}

#[test]
fn semantic_context_never_spills_summary_or_file_lists() {
    let raw = "/private/file.rs\n/private/another.rs\n<summary>context only</summary>";
    let rows = project(
        &json!({"role":"compactionSummary","summary":raw,"tokensBefore":999}),
        "compact:1",
    );
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].kind, "compaction");
    assert_eq!(rows[0].text, "");
    assert_eq!(rows[0].source, "compact:1");
    assert!(rows[0].history_available);
    assert!(!rows[0].history_loading);
    assert!(project(&json!({"role":"branchSummary","summary":raw}), "branch:1").is_empty());
    // Ordinary message prose remains ordinary; content shape is not provenance.
    assert_eq!(
        project(
            &json!({"role":"assistant","content":[{"type":"text","text":raw}]}),
            "assistant:1"
        )[0]
        .text,
        raw
    );
}

#[test]
fn layers_reveal_only_active_branch_and_use_real_cutoffs() {
    let layers = history_layers(&tree(), "f");
    assert_eq!(layers.len(), 2);
    assert_eq!(layers[0].rows[0].text, "oldest");
    assert_eq!(layers[1].rows[0].text, "older");
    let first = history_prefix(&layers, 1);
    assert_eq!(
        first.iter().map(|r| r.kind.as_str()).collect::<Vec<_>>(),
        ["compaction", "assistant"]
    );
    assert!(first[0].history_available);
    let all = history_prefix(&layers, 0);
    assert_eq!(
        all.iter().map(|r| r.kind.as_str()).collect::<Vec<_>>(),
        ["user", "compaction", "assistant"]
    );
    assert!(all.iter().all(|r| !r.history_available));
    assert!(all
        .iter()
        .all(|r| !r.text.contains("/raw") && r.text != "off branch"));
    assert_eq!(first[1].id, all[2].id);
    assert_eq!(first[1].source, all[2].source);
}

#[test]
fn startup_metadata_leaf_still_reveals_the_same_active_history() {
    let mut values = tree();
    values.push(json!({"type":"session_info", "id":"name", "parentId":"f", "name":"Saved"}));
    values.push(json!({"type":"thinking_level_change", "id":"setting", "parentId":"name", "thinkingLevel":"medium"}));
    let prior = history_prefix(&history_layers(&values, "setting"), 0);
    let expected = history_prefix(&history_layers(&values, "f"), 0);
    assert_eq!(
        prior
            .iter()
            .map(|r| (&r.id, &r.source, &r.text))
            .collect::<Vec<_>>(),
        expected
            .iter()
            .map(|r| (&r.id, &r.source, &r.text))
            .collect::<Vec<_>>()
    );
}

#[test]
fn unavailable_missing_and_cyclic_paths_do_not_fabricate_history() {
    assert!(history_layers(&tree(), "missing").is_empty());
    let mut values = tree();
    values[0]["parentId"] = json!("f");
    assert!(history_layers(&values, "f").is_empty());
    values = tree();
    values[4]["firstKeptEntryId"] = json!("unknown");
    assert!(history_layers(&values, "f").is_empty());
    values[4]["firstKeptEntryId"] = json!("a");
    assert!(history_layers(&values, "f").is_empty());
}

#[test]
fn historical_tools_keep_arguments_results_and_namespaced_sources() {
    let values = vec![
        json!({"type":"message","id":"a","parentId":null,"message":{"role":"assistant","content":[{"type":"toolCall","id":"call","name":"read","arguments":{"path":"file.rs"}}]}}),
        json!({"type":"message","id":"b","parentId":"a","message":{"role":"toolResult","toolCallId":"call","content":[{"type":"text","text":"contents"}]}}),
        json!({"type":"message","id":"d","parentId":"b","message":{"role":"user","content":"kept"}}),
        json!({"type":"compaction","id":"c","parentId":"d","firstKeptEntryId":"d"}),
    ];
    let layers = history_layers(&values, "c");
    assert_eq!(layers.len(), 1);
    assert_eq!(layers[0].rows.len(), 1);
    let row = &layers[0].rows[0];
    assert_eq!(row.arguments, json!({"path":"file.rs"}));
    assert_eq!(row.result, "contents");
    assert_eq!(row.source, "history:a:tool:call");
    assert_eq!(row.tool_call_id.as_deref(), Some("call"));
}
