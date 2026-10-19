use serde_json::Value;
use std::{
    ffi::OsStr,
    fs::{self, File},
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};

const MAX_SESSION_LINE_BYTES: usize = 64 * 1024 * 1024;
pub(super) fn discover_local_session(
    directory: &Path,
    supplied: &str,
    session_id: &str,
) -> Result<Option<PathBuf>, String> {
    let supplied = Path::new(supplied);
    if supplied.is_absolute()
        && supplied.extension() == Some(OsStr::new("jsonl"))
        && regular_session_file(supplied)
        && parse_session_file(supplied)?.is_some_and(|parsed| parsed.id == session_id)
    {
        return Ok(Some(supplied.to_path_buf()));
    }
    if !directory.is_dir() {
        return Ok(None);
    }
    for entry in fs::read_dir(directory)
        .map_err(|error| format!("Could not read saved Pi sessions: {error}"))?
    {
        let entry = entry.map_err(|error| format!("Could not read saved Pi sessions: {error}"))?;
        let path = entry.path();
        let name = path.file_name().and_then(OsStr::to_str).unwrap_or_default();
        if (name == format!("{session_id}.jsonl")
            || name.ends_with(&format!("_{session_id}.jsonl")))
            && regular_session_file(&path)
            && parse_session_file(&path)?.is_some_and(|parsed| parsed.id == session_id)
        {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

fn regular_session_file(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_file())
}

pub(super) struct ParsedSession {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) model: String,
    pub(super) first_message: String,
    pub(super) first_agent_message_at: u64,
    pub(super) last_user_message_at: u64,
}

impl ParsedSession {
    pub(super) fn sort_at(&self) -> u64 {
        if self.last_user_message_at > 0 {
            self.last_user_message_at
        } else {
            self.first_agent_message_at
        }
    }
}

pub(super) fn parse_session_file(path: &Path) -> Result<Option<ParsedSession>, String> {
    let file =
        File::open(path).map_err(|error| format!("Could not read a saved Pi session: {error}"))?;
    let mut reader = BufReader::new(file);
    let mut bytes = Vec::new();
    let count = reader
        .read_until(b'\n', &mut bytes)
        .map_err(|error| format!("Could not read a saved Pi session: {error}"))?;
    if count == 0 || bytes.len() > MAX_SESSION_LINE_BYTES {
        return Ok(None);
    }
    let Ok(header) = serde_json::from_slice::<Value>(&bytes) else {
        return Ok(None);
    };
    if header.get("type").and_then(Value::as_str) != Some("session") {
        return Ok(None);
    }
    let Some(id) = header
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
    else {
        return Ok(None);
    };
    let mut parsed = ParsedSession {
        id: id.to_string(),
        name: String::new(),
        model: String::new(),
        first_message: String::new(),
        first_agent_message_at: 0,
        last_user_message_at: 0,
    };
    loop {
        bytes.clear();
        let count = reader
            .read_until(b'\n', &mut bytes)
            .map_err(|error| format!("Could not read a saved Pi session: {error}"))?;
        if count == 0 {
            break;
        }
        if bytes.len() > MAX_SESSION_LINE_BYTES {
            continue;
        }
        let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
            continue;
        };
        match value.get("type").and_then(Value::as_str) {
            Some("session_info") => {
                parsed.name = value
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
            }
            Some("model_change") => {
                if let Some(model_id) = value.get("modelId").and_then(Value::as_str) {
                    parsed.model = model_id.to_string();
                }
            }
            Some("message") => {
                let Some(message) = value.get("message") else {
                    continue;
                };
                let role = message.get("role").and_then(Value::as_str);
                if role == Some("user") {
                    if parsed.first_message.is_empty() {
                        parsed.first_message = content_text(message.get("content"));
                    }
                    if let Some(timestamp) = message.get("timestamp").and_then(Value::as_u64) {
                        parsed.last_user_message_at =
                            parsed.last_user_message_at.max(timestamp_millis(timestamp));
                    }
                } else if role == Some("assistant") && parsed.first_agent_message_at == 0 {
                    parsed.first_agent_message_at = message
                        .get("timestamp")
                        .and_then(Value::as_u64)
                        .map(timestamp_millis)
                        .unwrap_or_default();
                }
            }
            _ => {}
        }
    }
    Ok(Some(parsed))
}

fn content_text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|part| {
                (part.get("type").and_then(Value::as_str) == Some("text"))
                    .then(|| part.get("text").and_then(Value::as_str))
                    .flatten()
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

pub(super) fn single_line(value: &str) -> String {
    value
        .replace(['\r', '\n', '\t'], " ")
        .chars()
        .take(240)
        .collect()
}

pub(super) fn timestamp_millis(timestamp: u64) -> u64 {
    if timestamp == 0 {
        0
    } else if timestamp < 10_000_000_000 {
        timestamp.saturating_mul(1_000)
    } else {
        timestamp
    }
}

#[cfg(test)]
mod tests {
    use super::super::sessions::list_remote_sessions;
    use super::*;
    use crate::models::{RemoteProjectRecord, RemoteSessionRecord};
    use std::io::Write;
    #[test]
    fn parses_session_identity_and_title() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("session.jsonl");
        let mut file = File::create(&path).expect("session file");
        writeln!(
            file,
            "{{\"type\":\"session\",\"version\":3,\"id\":\"session-1\",\"cwd\":\"/tmp\"}}"
        )
        .expect("header");
        writeln!(
            file,
            "{{\"type\":\"message\",\"message\":{{\"role\":\"user\",\"content\":[{{\"type\":\"text\",\"text\":\"Port Tau\"}}],\"timestamp\":1000}}}}"
        )
        .expect("first message");
        writeln!(
            file,
            "{{\"type\":\"message\",\"message\":{{\"role\":\"user\",\"content\":\"Continue\",\"timestamp\":2000}}}}"
        )
        .expect("second message");
        let parsed = parse_session_file(&path)
            .expect("parsed file")
            .expect("session");
        assert_eq!(parsed.id, "session-1");
        assert_eq!(parsed.first_message, "Port Tau");
        assert_eq!(parsed.last_user_message_at, 2_000_000);
        assert_eq!(parsed.sort_at(), 2_000_000);

        {
            let directory = tempfile::tempdir().expect("temporary directory");
            let path = directory.path().join("session.jsonl");
            let mut file = File::create(&path).expect("session file");
            writeln!(
                file,
                "{{\"type\":\"session\",\"version\":3,\"id\":\"session-1\",\"cwd\":\"/tmp\"}}"
            )
            .expect("header");
            writeln!(
                file,
                "{{\"type\":\"model_change\",\"provider\":\"openai-codex\",\"modelId\":\"gpt-5.5\"}}"
            )
            .expect("first model change");
            writeln!(
                file,
                "{{\"type\":\"model_change\",\"provider\":\"anthropic\",\"modelId\":\"claude-opus-4-6\"}}"
            )
            .expect("second model change");
            let parsed = parse_session_file(&path)
                .expect("parsed file")
                .expect("session");
            assert_eq!(parsed.model, "claude-opus-4-6");
        }
    }
    #[test]
    fn userless_sessions_sort_by_the_first_agent_message() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("session.jsonl");
        let mut file = File::create(&path).expect("session file");
        writeln!(
            file,
            "{{\"type\":\"session\",\"version\":3,\"id\":\"session-1\",\"cwd\":\"/tmp\"}}"
        )
        .expect("header");
        writeln!(
            file,
            "{{\"type\":\"message\",\"message\":{{\"role\":\"assistant\",\"content\":[{{\"type\":\"text\",\"text\":\"Started by an extension\"}}],\"timestamp\":3000}}}}"
        )
        .expect("first agent message");
        writeln!(
            file,
            "{{\"type\":\"message\",\"message\":{{\"role\":\"assistant\",\"content\":[],\"timestamp\":4000}}}}"
        )
        .expect("second agent message");

        let parsed = parse_session_file(&path)
            .expect("parsed file")
            .expect("session");

        assert_eq!(parsed.last_user_message_at, 0);
        assert_eq!(parsed.first_agent_message_at, 3_000_000);
        assert_eq!(parsed.sort_at(), 3_000_000);

        {
            let remote = RemoteProjectRecord {
                connection_string: "ssh build-box".into(),
                working_directory: "/home/timur".into(),
                host: "build-box".into(),
                active_session_id: "newer".into(),
                sessions: vec![
                    RemoteSessionRecord {
                        id: "older".into(),
                        path: "/remote/older.jsonl".into(),
                        name: Some("Older session".into()),
                        archived: false,
                        last_active: 10,
                        sort_at: 10,
                    },
                    RemoteSessionRecord {
                        id: "newer".into(),
                        path: "/remote/newer.jsonl".into(),
                        name: Some("Newer session".into()),
                        archived: false,
                        last_active: 20,
                        sort_at: 20,
                    },
                ],
            };
            let sessions = list_remote_sessions(&remote);
            assert_eq!(sessions[0].id, "newer");
            assert_eq!(sessions[0].last_user_message_at, 20_000);
            assert!(sessions[0].selected);
            assert_eq!(sessions[1].title, "Older session");
        }

        {
            let remote = RemoteProjectRecord {
                connection_string: "ssh build-box".into(),
                working_directory: "/home/timur".into(),
                host: "build-box".into(),
                active_session_id: String::new(),
                sessions: vec![
                    RemoteSessionRecord {
                        id: "user-session".into(),
                        path: "/remote/user.jsonl".into(),
                        name: None,
                        archived: false,
                        last_active: 20,
                        sort_at: 20,
                    },
                    RemoteSessionRecord {
                        id: "agent-session".into(),
                        path: "/remote/agent.jsonl".into(),
                        name: None,
                        archived: false,
                        last_active: 0,
                        sort_at: 30,
                    },
                ],
            };

            let sessions = list_remote_sessions(&remote);

            assert_eq!(sessions[0].id, "agent-session");
            assert_eq!(sessions[0].last_user_message_at, 0);
            assert_eq!(sessions[0].sort_at, 30_000);
        }
    }
}
