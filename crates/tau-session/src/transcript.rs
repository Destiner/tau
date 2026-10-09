use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    /// Rendering revision; only changed rows advance it.
    #[serde(default)]
    pub revision: u64,
    /// Last revision that replaced prose rather than appending to it.
    #[serde(default)]
    pub text_reset_revision: u64,
    pub kind: String,
    pub text: String,
    pub activity: bool,
    pub pending: bool,
    pub interrupted: bool,
    pub tool_call_id: Option<String>,
    pub tool_name: Option<String>,
    pub arguments: Value,
    pub result: String,
    pub is_error: bool,
    pub source: String,
}
impl Entry {
    pub(crate) fn new(id: String, kind: &str, text: String, source: String) -> Self {
        Self {
            id,
            revision: 0,
            text_reset_revision: 0,
            kind: kind.into(),
            text,
            activity: false,
            pending: false,
            interrupted: false,
            tool_call_id: None,
            tool_name: None,
            arguments: Value::Null,
            result: String::new(),
            is_error: false,
            source,
        }
    }
    pub(crate) fn touch(&mut self) {
        self.revision += 1;
    }
    pub(crate) fn replace_text(&mut self, text: String) {
        self.touch();
        self.text_reset_revision = self.revision;
        self.text = text;
    }
    pub(crate) fn append_text(&mut self, delta: &str) {
        if !delta.is_empty() {
            self.text.push_str(delta);
            self.touch();
        }
    }
}
pub fn content_text(value: &Value) -> String {
    if let Some(s) = value.as_str() {
        return s.into();
    }
    value
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|v| v["type"] == "text")
                .filter_map(|v| v["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}
pub(crate) fn string(v: &Value) -> String {
    v.as_str().unwrap_or_default().into()
}
pub(crate) fn message_key(m: &Value, fallback: &str) -> String {
    if m["role"] == "toolResult" {
        return format!("tool:{}", string(&m["toolCallId"]));
    }
    format!(
        "{}:{}",
        string(&m["role"]),
        m["timestamp"]
            .as_u64()
            .map(|n| n.to_string())
            .unwrap_or_else(|| fallback.into())
    )
}
pub(crate) fn project(m: &Value, key: &str) -> Vec<Entry> {
    let role = m["role"].as_str().unwrap_or_default();
    let mut rows = vec![];
    let mut add = |kind: &str, text: String, source: String| {
        rows.push(Entry::new(String::new(), kind, text, source));
    };
    match role {
        "user" => add("user", content_text(&m["content"]), key.into()),
        "assistant" => return project_assistant(m, key),
        "toolResult" => {
            add("tool", String::new(), key.into());
            let r = &mut rows[0];
            r.tool_call_id = m["toolCallId"].as_str().map(str::to_owned);
            r.tool_name = m["toolName"].as_str().map(str::to_owned);
            r.result = content_text(&m["content"]);
            r.is_error = m["isError"] == true;
        }
        "bashExecution" => {
            add("tool", string(&m["command"]), key.into());
            rows[0].result = string(&m["output"]);
        }
        "custom" if m["display"] == true => add("notice", content_text(&m["content"]), key.into()),
        "compactionSummary" | "branchSummary" => {
            add("compaction", string(&m["summary"]), key.into())
        }
        _ => {}
    }
    rows
}
fn project_assistant(m: &Value, key: &str) -> Vec<Entry> {
    let mut rows = vec![];
    if let Some(parts) = m["content"].as_array() {
        for (i, p) in parts.iter().enumerate() {
            let kind = match p["type"].as_str() {
                Some("text") => "assistant",
                Some("thinking") => "thinking",
                Some("toolCall") => "tool",
                _ => continue,
            };
            let mut r = Entry::new(
                String::new(),
                kind,
                if kind == "thinking" {
                    string(&p["thinking"])
                } else {
                    string(&p["text"])
                },
                format!("{key}:{i}"),
            );
            if kind == "tool" {
                r.source = format!("tool:{}", string(&p["id"]));
                r.tool_call_id = p["id"].as_str().map(str::to_owned);
                r.tool_name = p["name"].as_str().map(str::to_owned);
                r.arguments = p["arguments"].clone();
                r.interrupted = m["stopReason"] == "aborted" || m["stopReason"] == "error";
            }
            rows.push(r);
        }
    }
    if m["stopReason"] == "error" {
        rows.push(Entry::new(
            String::new(),
            "error",
            "The reply failed. Check the Pi setup and try again.".into(),
            format!("{key}:error"),
        ));
    }
    rows
}
