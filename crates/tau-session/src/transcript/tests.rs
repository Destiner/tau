use super::*;
use serde_json::json;

#[test]
fn displayed_custom_output_is_info_without_interpreting_custom_data_or_prose() {
    let message = json!({"role":"custom", "customType":"usage", "display":true,
        "timestamp":123, "content":[{"type":"text", "text":"**Warning**: error"},
        {"type":"image", "data":"not prose"}, {"type":"text", "text":"Tokens: 10"}],
        "details":{"severity":"error", "payload":[1,2,3]}, "notifyType":"warning"});
    let before = message.clone();
    let rows = project(&message, "custom:123");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].kind, "notice");
    assert_eq!(rows[0].notice_type, NoticeType::Info);
    assert_eq!(rows[0].text, "**Warning**: error\nTokens: 10");
    assert_eq!(rows[0].source, "custom:123");
    assert_eq!(message, before);
    let mut hidden = message;
    hidden["display"] = json!(false);
    assert!(project(&hidden, "custom:123").is_empty());
}

#[test]
fn notify_severity_is_wire_owned_and_old_entries_deserialize_as_info() {
    use crate::session::Controller;
    let mut controller = Controller::new("notices");
    let bootstrap = controller.begin_generation(1);
    for (id, value, expected) in [
        ("info", json!("info"), NoticeType::Info),
        ("warning", json!("warning"), NoticeType::Warning),
        ("error", json!("error"), NoticeType::Error),
        ("warn", json!("warn"), NoticeType::Info),
        ("missing", Value::Null, NoticeType::Info),
        ("object", json!({"type":"error"}), NoticeType::Info),
    ] {
        let event = json!({"type":"extension_ui_request", "method":"notify", "id":id,
            "message":"error warning failed", "notifyType":value});
        controller.ingest("notices", 1, event.clone());
        let entry = controller.state().transcript.last().unwrap().clone();
        assert_eq!(entry.notice_type, expected);
        assert_eq!(entry.source, format!("local:notify:{id}"));
        assert_eq!(entry.text, "error warning failed");
        let count = controller.state().transcript.len();
        controller.ingest("notices", 1, event);
        assert_eq!(controller.state().transcript.len(), count);
        assert_eq!(controller.state().transcript.last().unwrap(), &entry);
    }
    let before = controller.state().transcript.clone();
    controller.ingest("notices", 0, json!({"type":"extension_ui_request", "method":"notify", "id":"stale", "message":"stale", "notifyType":"error"}));
    controller.ingest("other", 1, json!({"type":"extension_ui_request", "method":"notify", "id":"other", "message":"other", "notifyType":"error"}));
    controller.handle(
        json!({"type":"extension_ui_request", "method":"notify", "id":"invalid", "message":23}),
    );
    assert_eq!(controller.state().transcript, before);
    let mut legacy = serde_json::to_value(&before[0]).unwrap();
    legacy.as_object_mut().unwrap().remove("notice_type");
    legacy.as_object_mut().unwrap().remove("error_label");
    let decoded: Entry = serde_json::from_value(legacy).unwrap();
    assert_eq!(decoded.notice_type, NoticeType::Info);
    assert_eq!(decoded.error_label, None);
    let request = bootstrap
        .iter()
        .find_map(|effect| match effect {
            crate::session::Effect::Send(request) if request["type"] == "get_messages" => {
                Some(request)
            }
            _ => None,
        })
        .unwrap();
    controller.handle(
        json!({"type":"response", "id":request["id"], "command":"get_messages",
        "success":true, "data":{"messages":[{"role":"assistant", "timestamp":1,
        "content":[{"type":"text", "text":"Ordinary **reply**"}]}]}}),
    );
    let hydrated = &controller.state().transcript;
    assert_eq!(hydrated.len(), before.len() + 1);
    assert_eq!(hydrated[0].kind, "assistant");
    assert_eq!(&hydrated[1..], before.as_slice());
}

#[test]
fn assistant_error_and_ordinary_markdown_keep_their_real_provenance() {
    let message = json!({"role":"assistant", "content":[{"type":"text", "text":"**warning** error"}],
        "stopReason":"error", "errorMessage":"raw secret details"});
    let rows = project(&message, "assistant:1");
    assert_eq!(rows[0].kind, "assistant");
    assert_eq!(rows[0].text, "**warning** error");
    assert_eq!(rows[0].source, "assistant:1:0");
    assert_eq!(rows[1].kind, "error");
    assert_eq!(rows[1].source, "assistant:1:error");
    assert!(!rows[1].text.contains("raw secret"));
    let mut stopped = message;
    stopped["stopReason"] = json!("aborted");
    assert_eq!(project(&stopped, "assistant:1").len(), 1);
}

#[test]
fn history_arguments_use_vue_precedence_and_preserve_sources() {
    let message = json!({"role":"assistant", "content":[
        {"type":"thinking", "thinking":"Plan"},
        {"type":"toolCall", "id":"read", "name":"read", "arguments":{"path":"gpui/APP.md", "offset":1}},
        {"type":"toolCall", "id":"bash", "name":"bash", "arguments":{"command":"printf 'hello\\n'", "path":"ignored"}},
        {"type":"toolCall", "id":"edit", "name":"edit", "arguments":{"file_path":"file.rs", "oldText":"raw  \n", "newText":"replacement"}},
        {"type":"toolCall", "id":"search", "name":"search", "arguments":{"command":"", "path":1, "query":"needle"}}
    ]});
    let rows = project(&message, "assistant:1");
    assert_eq!(rows[1].tool_description(), "gpui/APP.md");
    assert_eq!(rows[2].tool_description(), "printf 'hello\\n'");
    assert_eq!(rows[3].tool_description(), "file.rs");
    assert_eq!(rows[4].tool_description(), "needle");
    for (row, part) in rows[1..]
        .iter()
        .zip(message["content"].as_array().unwrap()[1..].iter())
    {
        assert_eq!(row.arguments, part["arguments"]);
        assert_eq!(
            serde_json::from_str::<Value>(&row.tool_arguments_text()).unwrap(),
            row.arguments
        );
    }
    assert_eq!(rows[1].source, "tool:read");
}

#[test]
fn missing_malformed_and_unknown_arguments_have_safe_vue_fallbacks() {
    let mut entry = Entry::new("tool".into(), "tool", String::new(), "tool:call".into());
    for (args, expected) in [
        (Value::Null, ""),
        (json!({}), "{}"),
        (json!(false), "false"),
        (json!([1, "two"]), "[1,\"two\"]"),
        (json!("{partial"), "\"{partial\""),
        (json!({"custom":true}), "{\"custom\":true}"),
    ] {
        entry.arguments = args.clone();
        assert_eq!(entry.tool_description(), expected);
        assert_eq!(entry.arguments, args);
        assert_eq!(
            entry.tool_arguments_text().is_empty(),
            !args.as_object().is_some_and(|args| !args.is_empty())
        );
    }
}

#[test]
fn json_fallback_truncates_only_the_header_in_utf16_units() {
    let mut entry = Entry::new("tool".into(), "tool", String::new(), "tool:call".into());
    entry.arguments = json!({"unknown":"é".repeat(300)});
    let raw = entry.arguments.clone();
    let header = entry.tool_description();
    assert_eq!(header.encode_utf16().count(), 218);
    assert!(header.ends_with('…'));
    assert_eq!(entry.arguments, raw);
    assert_eq!(
        serde_json::from_str::<Value>(&entry.tool_arguments_text()).unwrap(),
        raw
    );
    entry.arguments = json!({"path":"x".repeat(300)});
    assert_eq!(entry.tool_description().len(), 300);
}

#[test]
fn recorded_bash_execution_keeps_the_command_and_result() {
    let rows = project(
        &json!({"role":"bashExecution", "command":"pwd", "output":"/raw/path\n"}),
        "bash:1",
    );
    assert_eq!(rows[0].tool_name.as_deref(), Some("bash"));
    assert_eq!(rows[0].tool_description(), "pwd");
    assert_eq!(rows[0].tool_arguments_text(), "");
    assert_eq!(rows[0].result, "/raw/path\n");
}
