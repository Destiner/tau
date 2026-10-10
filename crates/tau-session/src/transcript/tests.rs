use super::*;
use serde_json::json;

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
