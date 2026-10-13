use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Severity comes from Pi's notifyType, never from message prose or custom data.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NoticeType {
    #[default]
    Info,
    Warning,
    Error,
}
impl NoticeType {
    pub(crate) fn from_notify(value: &Value) -> Self {
        match value.as_str() {
            Some("warning") => Self::Warning,
            Some("error") => Self::Error,
            _ => Self::Info,
        }
    }
}

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
    #[serde(default)]
    pub history_available: bool,
    #[serde(default)]
    pub history_loading: bool,
    pub interrupted: bool,
    pub tool_call_id: Option<String>,
    pub tool_name: Option<String>,
    pub arguments: Value,
    pub result: String,
    pub is_error: bool,
    #[serde(default)]
    pub notice_type: NoticeType,
    #[serde(default)]
    pub error_label: Option<String>,
    pub source: String,
}
/// The exact Pi user envelope; metadata is not inferred from ordinary prose.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkillEnvelope<'a> {
    pub name: &'a str,
    pub location: &'a str,
    pub content: &'a str,
    pub prompt: &'a str,
}

/// Matches Vue's parseSkillBlock without rewriting the source user message.
pub fn parse_skill_envelope(text: &str) -> Option<SkillEnvelope<'_>> {
    let rest = text.strip_prefix("<skill name=\"")?;
    let (name, rest) = rest.split_once("\" location=\"")?;
    let (location, rest) = rest.split_once("\">\n")?;
    if name.is_empty() || location.is_empty() || name.contains('"') || location.contains('"') {
        return None;
    }
    // Vue's non-greedy body can backtrack past a closing tag inside instructions.
    for (end, _) in rest.match_indices("\n</skill>") {
        let content = &rest[..end];
        let trailing = &rest[end + "\n</skill>".len()..];
        let prompt = if matches!(
            trailing,
            "" | "\n" | "\r" | "\r\n" | "\u{2028}" | "\u{2029}"
        ) {
            ""
        } else if let Some(prompt) = trailing.strip_prefix("\n\n").filter(|s| !s.is_empty()) {
            prompt.trim_matches(|c| {
                matches!(c,
                    '\u{0009}'..='\u{000d}' | ' ' | '\u{00a0}' | '\u{1680}' |
                    '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' |
                    '\u{205f}' | '\u{3000}' | '\u{feff}'
                )
            })
        } else {
            continue;
        };
        return Some(SkillEnvelope {
            name,
            location,
            content,
            prompt,
        });
    }
    None
}

impl Entry {
    pub fn skill(&self) -> Option<SkillEnvelope<'_>> {
        (self.kind == "user")
            .then(|| parse_skill_envelope(&self.text))
            .flatten()
    }
    pub(crate) fn new(id: String, kind: &str, text: String, source: String) -> Self {
        Self {
            id,
            revision: 0,
            text_reset_revision: 0,
            kind: kind.into(),
            text,
            activity: false,
            pending: false,
            history_available: false,
            history_loading: false,
            interrupted: false,
            tool_call_id: None,
            tool_name: None,
            arguments: Value::Null,
            result: String::new(),
            is_error: false,
            notice_type: NoticeType::Info,
            error_label: None,
            source,
        }
    }
    /// Vue's toolSummary: prefer the first nonempty string, then compact JSON.
    /// Null also represents missing arguments in the controller's data model.
    pub fn tool_description(&self) -> String {
        if self.arguments.is_null() {
            return self.text.clone();
        }
        if let Some(args) = self.arguments.as_object() {
            for key in ["command", "path", "file_path", "query"] {
                if let Some(value) = args
                    .get(key)
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                {
                    return value.to_owned();
                }
            }
        }
        let json = self.arguments.to_string();
        // JavaScript slices UTF-16 code units; never slice a Rust string by bytes.
        if json.encode_utf16().count() > 220 {
            format!(
                "{}…",
                String::from_utf16_lossy(&json.encode_utf16().take(217).collect::<Vec<_>>())
            )
        } else {
            json
        }
    }
    /// Vue's toolArgumentsText; presentation only, never rewrites the raw value.
    pub fn tool_arguments_text(&self) -> String {
        match self.arguments.as_object() {
            Some(args) if !args.is_empty() => {
                serde_json::to_string_pretty(&self.arguments).unwrap_or_default()
            }
            _ => String::new(),
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
#[cfg(test)]
#[path = "transcript/history_tests.rs"]
pub(crate) mod history_tests;
#[cfg(test)]
#[path = "transcript/skill_tests.rs"]
mod skill_tests;
#[cfg(test)]
#[path = "transcript/tests.rs"]
mod tests;

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
            rows[0].tool_name = Some("bash".into());
            rows[0].result = string(&m["output"]);
        }
        "custom" if m["display"] == true => add("notice", content_text(&m["content"]), key.into()),
        "compactionSummary" => {
            add("compaction", String::new(), key.into());
            rows[0].history_available = true;
        }
        // Branch summaries are model context, not conversation (Vue omits them).
        "branchSummary" => {}
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

#[derive(Clone, Debug)]
pub(crate) struct HistoryLayer {
    pub marker_id: String,
    pub rows: Vec<Entry>,
}

/// Pi's raw active branch, sliced only at real firstKeptEntryId boundaries.
pub(crate) fn history_layers(values: &[Value], leaf: &str) -> Vec<HistoryLayer> {
    use std::collections::{BTreeSet, HashMap, HashSet};
    let by_id: HashMap<_, _> = values
        .iter()
        .filter_map(|v| Some((v["id"].as_str()?, v)))
        .collect();
    let mut path = vec![];
    let mut seen = HashSet::new();
    let mut cursor = leaf;
    while !cursor.is_empty() {
        if !seen.insert(cursor) {
            return vec![];
        }
        let Some(entry) = by_id.get(cursor) else {
            return vec![];
        };
        path.push(*entry);
        cursor = entry["parentId"].as_str().unwrap_or_default();
    }
    path.reverse();
    let Some(latest) = path.iter().rposition(|e| e["type"] == "compaction") else {
        return vec![];
    };
    let index_by_id: HashMap<_, _> = path
        .iter()
        .enumerate()
        .filter_map(|(index, e)| Some((e["id"].as_str()?, index)))
        .collect();
    let Some(start) = path[latest]["firstKeptEntryId"]
        .as_str()
        .and_then(|id| index_by_id.get(id))
        .copied()
        .filter(|&i| i > 0 && i <= latest)
    else {
        return vec![];
    };
    let mut cutoffs = BTreeSet::from([0, start]);
    for entry in &path[..latest] {
        if entry["type"] == "compaction" {
            if let Some(&index) = entry["firstKeptEntryId"]
                .as_str()
                .and_then(|id| index_by_id.get(id))
            {
                if index > 0 && index < start {
                    cutoffs.insert(index);
                }
            }
        }
    }
    let cutoffs: Vec<_> = cutoffs.into_iter().collect();
    let mut layers = vec![];
    for span in cutoffs.windows(2) {
        let prefix = format!("history:{}", string(&path[span[0]]["id"]));
        let mut rows: Vec<Entry> = vec![];
        for entry in &path[span[0]..span[1]] {
            if entry["type"] != "message" {
                continue;
            }
            let key = format!("{prefix}:{}", string(&entry["id"]));
            for (part, mut row) in project(&entry["message"], &key).into_iter().enumerate() {
                if row.kind == "tool" {
                    if let Some(prior) = rows.iter_mut().rev().find(|r| {
                        r.kind == "tool"
                            && r.tool_call_id.is_some()
                            && r.tool_call_id == row.tool_call_id
                    }) {
                        prior.result = row.result;
                        prior.is_error = row.is_error;
                        continue;
                    }
                }
                row.id = format!("{key}:{part}");
                // Tool projection normally uses a runtime-global call source.
                row.source = format!("{prefix}:{}", row.source);
                row.history_available = false;
                rows.push(row);
            }
        }
        if !rows.is_empty() {
            layers.push(HistoryLayer {
                marker_id: format!("{prefix}:compaction"),
                rows,
            });
        }
    }
    layers
}

pub(crate) fn history_prefix(layers: &[HistoryLayer], first: usize) -> Vec<Entry> {
    let mut rows = vec![];
    for (index, layer) in layers.iter().enumerate().skip(first) {
        if index > first || first > 0 {
            let mut marker = Entry::new(
                layer.marker_id.clone(),
                "compaction",
                String::new(),
                layer.marker_id.clone(),
            );
            marker.history_available = index == first && first > 0;
            rows.push(marker);
        }
        rows.extend(layer.rows.iter().cloned());
    }
    rows
}
