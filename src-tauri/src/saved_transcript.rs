//! Read-only, deliberately narrow Pi v3 projection. A miss leaves RPC hydration authoritative.
use crate::{
    models::{ProjectRegistry, TauSessionRegistry},
    profile::{self, SESSION_REGISTRY_FILENAME},
    ssh::SshConnection,
    storage,
};
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    fs::{self, OpenOptions},
    io::Read,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
    thread,
    time::{Duration, Instant},
};

const MAX_BYTES: u64 = 64 * 1024 * 1024;
const MAX_ENTRIES: usize = 100_000;
const MAX_LINE: usize = 8 * 1024 * 1024;
const SSH_DEADLINE: Duration = Duration::from_secs(10);
// A newer navigation supersedes even an SSH read already in progress.
static READ_GENERATION: AtomicU64 = AtomicU64::new(0);
static READ_SLOT: Mutex<()> = Mutex::new(());

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedTranscript {
    /// None means use Pi RPC; Some(empty) is a verified empty session.
    messages: Option<Vec<Value>>,
}

#[tauri::command]
pub async fn read_saved_transcript(
    project_path: String,
    session_id: String,
    session_path: String,
) -> SavedTranscript {
    let generation = READ_GENERATION
        .fetch_add(1, Ordering::AcqRel)
        .wrapping_add(1);
    let messages = tauri::async_runtime::spawn_blocking(move || {
        let current = || READ_GENERATION.load(Ordering::Acquire) == generation;
        // Only the newest navigation may hold the read slot. While an old
        // transport is being reaped, stale waiters exit rather than launching
        // another SSH child or parsing another large local file.
        let _slot = loop {
            if !current() {
                return None;
            }
            match READ_SLOT.try_lock() {
                Ok(slot) => break slot,
                Err(std::sync::TryLockError::Poisoned(_)) => return None,
                Err(std::sync::TryLockError::WouldBlock) => {
                    thread::sleep(Duration::from_millis(10))
                }
            }
        };
        let source = registered_source(&project_path, &session_id, &session_path)?;
        if !current() {
            return None;
        }
        let bytes = match source {
            Source::Local(path) => read_local(&path)?,
            Source::Remote { connection, path } => read_remote(&connection, &path, &current)?,
        };
        if !current() {
            return None;
        }
        let messages = project(&bytes, &session_id)?;
        current().then_some(messages)
    })
    .await
    .ok()
    .flatten();
    SavedTranscript { messages }
}

/// Call when leaving a saved preview without opening another one (e.g. a live session).
#[tauri::command]
pub fn cancel_saved_transcript() {
    READ_GENERATION.fetch_add(1, Ordering::AcqRel);
}

enum Source {
    Local(PathBuf),
    Remote { connection: String, path: String },
}

fn registered_source(project_path: &str, session_id: &str, session_path: &str) -> Option<Source> {
    if session_id.is_empty() || session_path.is_empty() {
        return None;
    }
    let registry: ProjectRegistry = serde_json::from_slice(
        &fs::read(profile::current().ok()?.data_dir().join("projects.json")).ok()?,
    )
    .ok()?;
    let project = registry.projects.iter().find(|p| p.path == project_path)?;
    if let Some(remote) = &project.remote {
        let record = remote
            .sessions
            .iter()
            .find(|s| s.id == session_id && s.path == session_path)?;
        if !Path::new(&record.path).is_absolute() || record.path.contains('\0') {
            return None;
        }
        return Some(Source::Remote {
            connection: remote.connection_string.clone(),
            path: record.path.clone(),
        });
    }
    let directory = storage::default_session_dir(project_path).ok()?;
    let records: TauSessionRegistry =
        serde_json::from_slice(&fs::read(directory.join(SESSION_REGISTRY_FILENAME)).ok()?).ok()?;
    if !records.sessions.iter().any(|s| s.id == session_id) {
        return None;
    }
    let path = PathBuf::from(session_path);
    let resolved = path.canonicalize().ok()?;
    if path.extension()?.to_str()? != "jsonl"
        || resolved.parent()? != directory.canonicalize().ok()?
    {
        return None;
    }
    Some(Source::Local(resolved))
}

fn read_local(path: &Path) -> Option<Vec<u8>> {
    #[cfg(unix)]
    use std::os::unix::fs::OpenOptionsExt;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NONBLOCK);
    let file = options.open(path).ok()?;
    let before = file.metadata().ok()?;
    if !before.is_file() || before.len() > MAX_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1).read_to_end(&mut bytes).ok()?;
    let after = fs::metadata(path).ok()?;
    if bytes.len() as u64 > MAX_BYTES
        || before.len() != bytes.len() as u64
        || before.len() != after.len()
        || before.modified().ok()? != after.modified().ok()?
        || !same_file(&before, &after)
    {
        return None;
    }
    Some(bytes)
}

#[cfg(unix)]
fn same_file(before: &fs::Metadata, after: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    before.dev() == after.dev() && before.ino() == after.ino()
}

#[cfg(not(unix))]
fn same_file(_before: &fs::Metadata, _after: &fs::Metadata) -> bool {
    false
}

fn read_remote(connection: &str, path: &str, current: &impl Fn() -> bool) -> Option<Vec<u8>> {
    // Python supplies a single-file fstat consistency check and byte ceiling. No remote file writes.
    let script = "import os,sys,stat\np=sys.argv[1]\ntry:\n fd=os.open(p,os.O_RDONLY|os.O_NONBLOCK); f=os.fdopen(fd,'rb'); a=os.fstat(f.fileno())\n if not stat.S_ISREG(a.st_mode) or a.st_size>67108864: sys.exit(1)\n b=f.read(67108865); z=os.fstat(f.fileno()); f.close()\n if len(b)>67108864 or len(b)!=a.st_size or (a.st_dev,a.st_ino,a.st_size,a.st_mtime_ns)!=(z.st_dev,z.st_ino,z.st_size,z.st_mtime_ns): sys.exit(1)\n sys.stdout.buffer.write(b)\nexcept (OSError,ValueError): sys.exit(1)";
    let command = format!(
        "python3 -c {} {}",
        shell_words::quote(script),
        shell_words::quote(path)
    );
    let mut child = SshConnection::parse(connection)
        .ok()?
        .command(&command)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let stdout = child.stdout.take()?;
    let reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout.take(MAX_BYTES + 1).read_to_end(&mut bytes);
        result
            .ok()
            .filter(|_| bytes.len() as u64 <= MAX_BYTES)
            .map(|_| bytes)
    });
    let deadline = Instant::now() + SSH_DEADLINE;
    let status = loop {
        if !current() {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    // A remote descendant may inherit stdout after SSH exits. Never wait indefinitely for it.
    let bytes = if status.is_some() && current() {
        while !reader.is_finished() && Instant::now() < deadline && current() {
            thread::sleep(Duration::from_millis(20));
        }
        reader
            .is_finished()
            .then(|| reader.join().ok().flatten())
            .flatten()?
    } else {
        return None;
    };
    status.filter(|s| s.success() && current()).map(|_| bytes)
}

fn timestamp(entry: &Value) -> Option<i64> {
    // Pi constructs synthetic timestamps using new Date(ISO string).getTime().
    let text = entry.get("timestamp")?.as_str()?;
    // RFC3339 UTC only; other offsets fall back to Pi rather than inventing a date.
    let (date, time) = text.split_once('T')?;
    let time = time.strip_suffix('Z')?;
    let mut date = date.split('-').map(|n| n.parse::<i64>().ok());
    let (year, month, day) = (date.next()??, date.next()??, date.next()??);
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        _ => return None,
    };
    if date.next().is_some() || !(1..=days).contains(&day) {
        return None;
    }
    let (clock, fraction) = time.split_once('.').unwrap_or((time, ""));
    let mut clock = clock.split(':').map(|n| n.parse::<i64>().ok());
    let (hour, minute, second) = (clock.next()??, clock.next()??, clock.next()??);
    if clock.next().is_some()
        || hour > 23
        || minute > 59
        || second > 59
        || fraction.len() > 9
        || !fraction.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let millis = format!("{fraction:0<3}").get(..3)?.parse::<i64>().ok()?;
    // Civil date to Unix days, Gregorian calendar (Howard Hinnant).
    let y = year - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = month + if month > 2 { -3 } else { 9 };
    let doy = (153 * m + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(
        ((era * 146097 + doe - 719468) * 86400 + hour * 3600 + minute * 60 + second) * 1000
            + millis,
    )
}

fn project(bytes: &[u8], session_id: &str) -> Option<Vec<Value>> {
    if bytes.is_empty() || !bytes.ends_with(b"\n") {
        return None;
    }
    let mut lines = bytes.split(|b| *b == b'\n').filter(|l| !l.is_empty());
    let header: Value = serde_json::from_slice(lines.next()?).ok()?;
    if header.get("type")?.as_str()? != "session"
        || header.get("version")?.as_u64()? != 3
        || header.get("id")?.as_str()? != session_id
    {
        return None;
    }
    let mut entries = Vec::<Value>::new();
    let mut ids = HashMap::<String, usize>::new();
    for line in lines {
        if line.len() > MAX_LINE || entries.len() >= MAX_ENTRIES {
            return None;
        }
        let entry: Value = serde_json::from_slice(line).ok()?;
        let kind = entry.get("type")?.as_str()?;
        if !matches!(
            kind,
            "message"
                | "custom_message"
                | "branch_summary"
                | "compaction"
                | "context_edit"
                | "custom"
                | "label"
                | "session_info"
                | "model_change"
                | "thinking_level_change"
                | "usage"
        ) {
            return None;
        }
        let id = entry.get("id")?.as_str()?;
        if id.is_empty() || ids.contains_key(id) {
            return None;
        }
        let parent = entry.get("parentId")?;
        if !parent.is_null() && !ids.contains_key(parent.as_str()?) {
            return None;
        }
        ids.insert(id.to_owned(), entries.len());
        entries.push(entry);
    }
    let mut path = Vec::new();
    let mut current = entries.last();
    while let Some(entry) = current {
        path.push(entry);
        current = match entry.get("parentId")?.as_str() {
            Some(id) => Some(&entries[*ids.get(id)?]),
            None => None,
        };
    }
    path.reverse();
    let latest = path.iter().rposition(|e| e["type"] == "compaction");
    let context = if let Some(index) = latest {
        let kept = path[index].get("firstKeptEntryId")?.as_str();
        let retained = if kept.is_none_or(|id| id == path[index]["id"]) {
            Vec::new()
        } else {
            let kept = kept?;
            let first = path[..index].iter().position(|e| e["id"] == kept)?;
            path[first..index]
                .iter()
                .copied()
                .filter(|e| !(e["type"] == "message" && e["message"]["role"] == "system"))
                .collect()
        };
        let mut result = vec![path[index]];
        result.extend(retained);
        result.extend_from_slice(&path[index + 1..]);
        result
    } else {
        path
    };
    let mut edits = HashMap::new();
    let context_ids: HashSet<&str> = context.iter().filter_map(|e| e["id"].as_str()).collect();
    for entry in &context {
        if entry["type"] == "context_edit" {
            let target = entry.get("targetId")?.as_str()?;
            if !context_ids.contains(target) {
                return None;
            }
            let replacement = entry.get("replacement")?;
            if !replacement.is_null() && replacement.get("content").is_none() {
                return None;
            }
            edits.insert(target, replacement);
        }
    }
    let mut messages = Vec::new();
    for (index, entry) in context.iter().enumerate() {
        let kind = entry["type"].as_str()?;
        let message = match kind {
            "message" => {
                let mut message = entry.get("message")?.clone();
                let role = message.get("role")?.as_str()?;
                if !matches!(
                    role,
                    "system"
                        | "user"
                        | "assistant"
                        | "toolResult"
                        | "custom"
                        | "bashExecution"
                        | "branchSummary"
                        | "compactionSummary"
                ) {
                    return None;
                }
                if message.get("content").is_none_or(Value::is_null) {
                    if role == "system" {
                        message["content"] = json!("");
                    } else if matches!(role, "user" | "assistant" | "toolResult") {
                        message["content"] = json!([]);
                    }
                }
                Some(message)
            }
            "custom_message" => Some(
                json!({"role":"custom", "customType":entry.get("customType")?, "content":entry.get("content").filter(|v| !v.is_null()).cloned().unwrap_or(json!([])), "display":entry.get("display")?, "details":entry.get("details").cloned().unwrap_or(Value::Null), "timestamp":timestamp(entry)?}),
            ),
            "branch_summary" => Some(
                json!({"role":"branchSummary", "summary":entry.get("summary")?, "fromId":entry.get("fromId")?, "timestamp":timestamp(entry)?}),
            ),
            "compaction" if index == 0 => {
                if let Some(system) = entry.get("systemMessage") {
                    if system.get("role")?.as_str()? != "system" {
                        return None;
                    }
                    messages.push(system.clone());
                }
                Some(
                    json!({"role":"compactionSummary", "summary":entry.get("summary")?, "tokensBefore":entry.get("tokensBefore")?, "timestamp":timestamp(entry)?}),
                )
            }
            "compaction" => None,
            _ => None,
        };
        if let Some(mut message) = message {
            if let Some(replacement) = edits.get(entry["id"].as_str()?) {
                if replacement.is_null() {
                    continue;
                }
                if matches!(
                    message["role"].as_str()?,
                    "user" | "assistant" | "toolResult" | "custom"
                ) {
                    let content = replacement.get("content")?.clone();
                    message["content"] =
                        if matches!(message["role"].as_str()?, "assistant" | "toolResult")
                            && content.is_string()
                        {
                            json!([{"type":"text", "text":content}])
                        } else {
                            content
                        };
                }
            }
            messages.push(message);
        }
    }
    Some(messages)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    fn fixture(entries: Vec<Value>) -> Vec<u8> {
        let mut data = format!("{}\n", json!({"type":"session","version":3,"id":"s"})).into_bytes();
        for entry in entries {
            data.extend_from_slice(format!("{entry}\n").as_bytes());
        }
        data
    }
    fn entry(id: &str, parent: Option<&str>, kind: &str) -> Value {
        json!({"type":kind,"id":id,"parentId":parent,"timestamp":"2026-01-01T00:00:00Z"})
    }
    #[test]
    fn empty_and_invalid_sources() {
        assert_eq!(project(&fixture(vec![]), "s"), Some(vec![]));
        assert!(project(&fixture(vec![]), "wrong").is_none());
        let mut truncated = fixture(vec![]);
        truncated.pop();
        assert!(project(&truncated, "s").is_none());
        let mut legacy = fixture(vec![]);
        legacy.splice(
            0..legacy.iter().position(|b| *b == b'\n').unwrap(),
            b"{\"type\":\"session\",\"version\":2,\"id\":\"s\"}"
                .iter()
                .copied(),
        );
        assert!(project(&legacy, "s").is_none());
    }
    #[test]
    fn branches_compaction_and_edits() {
        let mut a = entry("a", None, "message");
        a["message"] = json!({"role":"user","content":"old"});
        let mut b = entry("b", Some("a"), "message");
        b["message"] = json!({"role":"assistant","content":null});
        let mut abandoned = entry("lost", Some("a"), "message");
        abandoned["message"] = json!({"role":"user","content":"lost"});
        let mut c = entry("c", Some("b"), "compaction");
        c["firstKeptEntryId"] = json!("b");
        c["summary"] = json!("recap");
        c["tokensBefore"] = json!(100);
        c["systemMessage"] = json!({"role":"system","content":"checkpoint"});
        let mut edit = entry("edit", Some("c"), "context_edit");
        edit["targetId"] = json!("b");
        edit["replacement"] = json!({"content":"replacement"});
        let result = project(&fixture(vec![a, b, abandoned, c, edit]), "s").unwrap();
        assert_eq!(result.len(), 3);
        assert_eq!(result[0]["content"], "checkpoint");
        assert_eq!(result[1]["summary"], "recap");
        assert_eq!(
            result[2]["content"],
            json!([{"type":"text","text":"replacement"}])
        );
    }
    #[test]
    fn summaries_roots_and_retain_none() {
        let mut old = entry("old", None, "message");
        old["message"] = json!({"role":"user","content":"discarded"});
        let mut root = entry("root", None, "branch_summary");
        root["summary"] = json!("other root");
        root["fromId"] = json!("old");
        let mut compact = entry("compact", Some("root"), "compaction");
        compact["summary"] = json!("saved");
        compact["tokensBefore"] = json!(12);
        compact["firstKeptEntryId"] = Value::Null;
        let mut tail = entry("tail", Some("compact"), "custom_message");
        tail["customType"] = json!("note");
        tail["content"] = json!("text");
        tail["display"] = json!(true);
        let result = project(&fixture(vec![old, root, compact, tail]), "s").unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0]["role"], "compactionSummary");
        assert_eq!(result[1]["timestamp"], 1767225600000_i64);
        assert_eq!(result[1]["content"], "text");
        // Pi writes the compaction's own ID for retain-none, not null.
        let mut compact = entry("compact", Some("root"), "compaction");
        compact["summary"] = json!("saved");
        compact["tokensBefore"] = json!(12);
        compact["firstKeptEntryId"] = json!("compact");
        let mut old = entry("old", None, "message");
        old["message"] = json!({"role":"user","content":"discarded"});
        let mut root = entry("root", Some("old"), "message");
        root["message"] = json!({"role":"user","content":"also discarded"});
        let result = project(&fixture(vec![old, root, compact]), "s").unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0]["summary"], "saved");
    }
    #[test]
    fn older_compaction_inside_retained_range_does_not_emit_two_summaries() {
        let mut first = entry("first", None, "message");
        first["message"] = json!({"role":"system","content":"old system"});
        let mut older = entry("older", Some("first"), "compaction");
        older["firstKeptEntryId"] = Value::Null;
        older["summary"] = json!("older");
        older["tokensBefore"] = json!(1);
        let mut user = entry("user", Some("older"), "message");
        user["message"] = json!({"role":"user","content":"keep"});
        let mut recent = entry("recent", Some("user"), "compaction");
        recent["firstKeptEntryId"] = json!("older");
        recent["summary"] = json!("recent");
        recent["tokensBefore"] = json!(2);
        let result = project(&fixture(vec![first, older, user, recent]), "s").unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0]["summary"], "recent");
        assert_eq!(result[1]["content"], "keep");
    }
    #[test]
    fn bounded_local_and_simulated_ssh_agree_without_touching_source() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("quotes ' ; spaces.jsonl");
        let mut message = entry("one", None, "message");
        message["message"] = json!({"role":"toolResult","content":null});
        let bytes = fixture(vec![message]);
        fs::write(&path, &bytes).unwrap();
        let ssh = dir.path().join("ssh");
        fs::write(
            &ssh,
            "#!/bin/sh\nfor word do command=$word; done\nexec /bin/sh -c \"$command\"\n",
        )
        .unwrap();
        let mut permissions = fs::metadata(&ssh).unwrap().permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&ssh, permissions).unwrap();
        assert_eq!(read_local(&path).unwrap(), bytes);
        assert_eq!(
            read_remote(
                &format!("{} fake-host", ssh.display()),
                path.to_str().unwrap(),
                &|| true
            ),
            Some(bytes.clone())
        );
        assert_eq!(project(&bytes, "s").unwrap()[0]["content"], json!([]));
        assert_eq!(fs::read(&path).unwrap(), bytes);
        fs::remove_file(&path).unwrap();
        assert!(read_local(&path).is_none());
        let oversized = File::create(&path).unwrap();
        oversized.set_len(MAX_BYTES + 1).unwrap();
        assert!(read_local(&path).is_none());
    }
    #[cfg(unix)]
    #[test]
    fn fifo_is_rejected_without_waiting_for_a_writer_locally_and_over_ssh() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let fifo = dir.path().join("session.jsonl");
        assert!(std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success());
        let start = Instant::now();
        assert!(read_local(&fifo).is_none());
        assert!(start.elapsed() < Duration::from_secs(2));
        let ssh = dir.path().join("ssh");
        fs::write(
            &ssh,
            "#!/bin/sh\nfor word do command=$word; done\nexec /bin/sh -c \"$command\"\n",
        )
        .unwrap();
        let mut permissions = fs::metadata(&ssh).unwrap().permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&ssh, permissions).unwrap();
        let start = Instant::now();
        assert!(read_remote(
            &format!("{} host", ssh.display()),
            fifo.to_str().unwrap(),
            &|| true
        )
        .is_none());
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn newer_read_interrupts_slow_ssh_instead_of_blocking_it() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let ssh = dir.path().join("ssh");
        fs::write(&ssh, "#!/bin/sh\nsleep 5\n").unwrap();
        let mut permissions = fs::metadata(&ssh).unwrap().permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&ssh, permissions).unwrap();
        let generation = AtomicU64::new(1);
        let started = Instant::now();
        let result = thread::scope(|scope| {
            let read = scope.spawn(|| {
                read_remote(&format!("{} host", ssh.display()), "/unused", &|| {
                    generation.load(Ordering::Acquire) == 1
                })
            });
            thread::sleep(Duration::from_millis(100));
            generation.store(2, Ordering::Release);
            read.join().unwrap()
        });
        assert!(result.is_none());
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    // Opt-in real Pi canary; invoked by scripts/pi-contract.ts, not default cargo test.
    #[test]
    #[ignore]
    fn real_pi_saved_projection_parity() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("parity.jsonl");
        let mut old = entry("old", None, "message");
        old["message"] = json!({"role":"user","content":"discarded"});
        let mut compact = entry("compact", Some("old"), "compaction");
        compact["firstKeptEntryId"] = json!("compact");
        compact["summary"] = json!("summary");
        compact["tokensBefore"] = json!(42);
        let mut tail = entry("tail", Some("compact"), "message");
        tail["message"] = json!({"role":"user","content":"kept"});
        let bytes = fixture(vec![old, compact, tail]);
        fs::write(&path, &bytes).unwrap();
        let pi = std::env::var("TAU_PI_PATH").unwrap_or_else(|_| "pi".into());
        let mut child = std::process::Command::new(pi)
            .args([
                "--mode",
                "rpc",
                "--offline",
                "--no-tools",
                "--no-extensions",
                "--no-skills",
                "--no-prompt-templates",
                "--no-context-files",
                "--no-themes",
                "--session",
            ])
            .arg(&path)
            .env("PI_CODING_AGENT_DIR", dir.path().join("config"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("start real Pi for saved transcript parity");
        child
            .stdin
            .take()
            .unwrap()
            .write_all(b"{\"id\":\"parity\",\"type\":\"get_messages\"}\n")
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let status = loop {
            if let Ok(Some(status)) = child.try_wait() {
                break status;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("Pi parity canary timed out");
            }
            thread::sleep(Duration::from_millis(20));
        };
        assert!(status.success(), "Pi parity canary exited unsuccessfully");
        let mut stdout = String::new();
        child
            .stdout
            .take()
            .unwrap()
            .read_to_string(&mut stdout)
            .unwrap();
        let response = stdout
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .find(|value| value["type"] == "response" && value["id"] == "parity")
            .expect("Pi get_messages response");
        assert_eq!(response["success"], true, "Pi rejected parity fixture");
        assert_eq!(
            response["data"]["messages"],
            json!(project(&bytes, "s").unwrap())
        );
    }

    #[test]
    fn malformed_graph_and_unknown_semantics_fail_closed() {
        assert!(project(&fixture(vec![entry("a", Some("missing"), "message")]), "s").is_none());
        assert!(project(&fixture(vec![entry("a", None, "future")]), "s").is_none());
        assert!(project(
            &fixture(vec![
                entry("a", None, "custom"),
                entry("a", Some("a"), "custom")
            ]),
            "s"
        )
        .is_none());
    }
}
