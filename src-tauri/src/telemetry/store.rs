//! The bounded segmented OTLP JSON store. This module is signal-agnostic: it
//! knows how to append, rotate, retain, and recover newline-delimited JSON
//! segments, but nothing here understands OTLP shapes. `exporter.rs` builds
//! the actual OTLP-shaped `serde_json::Value` records this store persists.
use std::collections::VecDeque;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::{LOG_SEGMENT_FILE, METRIC_SEGMENT_FILE, TRACE_SEGMENT_FILE};

/// A source of time the store can be given, so retention and rotation can be
/// tested without waiting on the real clock.
pub trait Clock: Send + Sync {
    fn now(&self) -> SystemTime;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Signal {
    Trace,
    Log,
    Metric,
}

impl Signal {
    fn active_filename(self) -> &'static str {
        match self {
            Signal::Trace => TRACE_SEGMENT_FILE,
            Signal::Log => LOG_SEGMENT_FILE,
            Signal::Metric => METRIC_SEGMENT_FILE,
        }
    }

    fn stem(self) -> &'static str {
        match self {
            Signal::Trace => "traces",
            Signal::Log => "logs",
            Signal::Metric => "metrics",
        }
    }

    fn from_stem(stem: &str) -> Option<Self> {
        match stem {
            "traces" => Some(Signal::Trace),
            "logs" => Some(Signal::Log),
            "metrics" => Some(Signal::Metric),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct StoreConfig {
    pub max_age: Duration,
    pub max_total_bytes: u64,
    /// Active segment size that triggers rotation to a closed, timestamped
    /// file. Deliberately smaller than `max_total_bytes` so retention always
    /// has closed segments to delete rather than one unbounded active file.
    pub segment_rotation_bytes: u64,
}

impl StoreConfig {
    pub const fn production() -> Self {
        StoreConfig {
            max_age: Duration::from_secs(7 * 24 * 60 * 60),
            max_total_bytes: 256 * 1024 * 1024,
            segment_rotation_bytes: 1024 * 1024,
        }
    }
}

/// A record that failed to reach disk, kept so a writer failure loses
/// nothing observable rather than silently dropping telemetry. Read back by
/// `fallback_sequences` today; Stage 4+ replays or reports on these.
#[allow(dead_code)]
pub struct FallbackRecord {
    pub signal: Signal,
    pub sequence: u64,
    pub line: String,
}

const FALLBACK_CAPACITY: usize = 256;

struct StoreState {
    next_sequence: u64,
    failed_writes: u64,
    fallback: VecDeque<FallbackRecord>,
}

/// Bounded, segmented, newline-delimited JSON store. One `Store` instance
/// owns a telemetry directory; every append is serialized through a single
/// lock so the monotonic sequence always matches on-disk order.
pub struct Store {
    dir: PathBuf,
    config: StoreConfig,
    clock: Arc<dyn Clock>,
    state: Mutex<StoreState>,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store")
            .field("dir", &self.dir)
            .finish_non_exhaustive()
    }
}

impl Store {
    pub fn new(dir: PathBuf, config: StoreConfig, clock: Arc<dyn Clock>) -> Self {
        let _ = fs::create_dir_all(&dir);
        set_owner_only_dir_permissions(&dir);
        let next_sequence = recover_next_sequence(&dir);
        Store {
            dir,
            config,
            clock,
            state: Mutex::new(StoreState {
                next_sequence,
                failed_writes: 0,
                fallback: VecDeque::new(),
            }),
        }
    }

    /// Stamps `record` with a monotonic store sequence and appends it to
    /// `signal`'s segment. Returns `true` if it reached disk; `false` means
    /// it was retained in the bounded in-memory fallback instead. Never
    /// panics and never fails the caller: telemetry failure must not fail a
    /// product operation.
    pub fn append(&self, signal: Signal, record: serde_json::Value) -> bool {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        self.append_locked(&mut state, signal, record)
    }

    /// Non-blocking counterpart to `append`, for the one caller (the Rust
    /// panic hook) that must never wait on the writer lock: a panic can
    /// occur while this very thread already holds it, mid-`append`, and
    /// nothing may deadlock trying to record telemetry about that panic. If
    /// the lock is genuinely contended (held by another thread, or by this
    /// one), this returns `false` immediately instead of blocking; a
    /// poisoned-but-uncontended lock is still recovered and used, the same
    /// way `append` recovers it.
    pub fn try_append(&self, signal: Signal, record: serde_json::Value) -> bool {
        match self.state.try_lock() {
            Ok(mut state) => self.append_locked(&mut state, signal, record),
            Err(std::sync::TryLockError::Poisoned(poison)) => {
                self.append_locked(&mut poison.into_inner(), signal, record)
            }
            Err(std::sync::TryLockError::WouldBlock) => false,
        }
    }

    fn append_locked(
        &self,
        state: &mut StoreState,
        signal: Signal,
        mut record: serde_json::Value,
    ) -> bool {
        let sequence = state.next_sequence;
        state.next_sequence += 1;
        if let Some(object) = record.as_object_mut() {
            object.insert(
                "tauObservedTimeUnixNano".to_string(),
                serde_json::Value::String(duration_nanos(self.clock.now())),
            );
            object.insert(
                "tauStoreSequence".to_string(),
                serde_json::Value::from(sequence),
            );
        }
        let line = record.to_string();

        if self.write_line(signal, &line, sequence).is_ok() {
            self.enforce_retention();
            return true;
        }

        state.failed_writes += 1;
        if state.fallback.len() == FALLBACK_CAPACITY {
            state.fallback.pop_front();
        }
        state.fallback.push_back(FallbackRecord {
            signal,
            sequence,
            line,
        });
        false
    }

    pub fn failed_writes(&self) -> u64 {
        self.state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .failed_writes
    }

    #[allow(dead_code)]
    pub fn fallback_len(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .fallback
            .len()
    }

    /// Sequences of records currently held in the fallback, oldest first.
    /// Used by tests to confirm eviction order under the bound.
    #[allow(dead_code)]
    pub fn fallback_sequences(&self) -> Vec<u64> {
        self.state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .fallback
            .iter()
            .map(|record| record.sequence)
            .collect()
    }

    /// Reads every recoverable record for `signal`, oldest segment first,
    /// then the active segment. A line that fails to parse as JSON (a
    /// malformed or partial write) is skipped rather than aborting the read.
    /// Exercised by store tests today; later stages read persisted telemetry
    /// through this rather than re-implementing segment traversal.
    #[allow(dead_code)]
    pub fn read_records(&self, signal: Signal) -> Vec<serde_json::Value> {
        let mut paths = self.ordered_rotated_segment_paths(signal);
        paths.push(self.dir.join(signal.active_filename()));

        let mut records = Vec::new();
        for path in paths {
            let Ok(bytes) = fs::read(&path) else { continue };
            let text = String::from_utf8_lossy(&bytes);
            for line in text.lines() {
                if line.trim().is_empty() {
                    continue;
                }
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
                    records.push(value);
                }
            }
        }
        records
    }

    fn ensure_dir(&self) -> io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        set_owner_only_dir_permissions(&self.dir);
        Ok(())
    }

    fn write_line(&self, signal: Signal, line: &str, sequence: u64) -> io::Result<()> {
        self.ensure_dir()?;
        let path = self.dir.join(signal.active_filename());
        recover_active_tail(&path)?;
        let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
        set_owner_only_file_permissions(&path);
        file.write_all(line.as_bytes())?;
        file.write_all(b"\n")?;
        file.sync_all()?;

        if file.metadata()?.len() >= self.config.segment_rotation_bytes {
            // Rotation failing does not undo the write that already landed.
            let _ = self.rotate(signal, sequence);
        }
        Ok(())
    }

    fn rotate(&self, signal: Signal, sequence: u64) -> io::Result<()> {
        let active_path = self.dir.join(signal.active_filename());
        let millis = duration_millis(self.clock.now());
        let rotated_path = self
            .dir
            .join(format!("{}-{millis}-{sequence}.jsonl", signal.stem()));
        fs::rename(&active_path, &rotated_path)
    }

    /// Deletes closed segments older than `max_age`, then deletes the
    /// oldest remaining closed segments (across every signal) until the
    /// directory is within `max_total_bytes`. The active segment for each
    /// signal is never a deletion candidate.
    fn enforce_retention(&self) {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return;
        };
        let now = self.clock.now();
        let mut rotated = Vec::new();
        let mut total_size: u64 = 0;

        for entry in entries.flatten() {
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            if !metadata.is_file() {
                continue;
            }
            total_size = total_size.saturating_add(metadata.len());
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if let Some((_, millis, sequence)) = parse_segment_name(name) {
                rotated.push(RotatedSegment {
                    path,
                    millis,
                    sequence,
                    size: metadata.len(),
                });
            }
        }

        rotated.sort_by_key(|segment| (segment.millis, segment.sequence));

        rotated.retain(|segment| {
            let age = now
                .duration_since(UNIX_EPOCH + Duration::from_millis(segment.millis))
                .unwrap_or(Duration::ZERO);
            if age <= self.config.max_age {
                return true;
            }
            if fs::remove_file(&segment.path).is_ok() {
                total_size = total_size.saturating_sub(segment.size);
            }
            false
        });

        for segment in &rotated {
            if total_size <= self.config.max_total_bytes {
                break;
            }
            if fs::remove_file(&segment.path).is_ok() {
                total_size = total_size.saturating_sub(segment.size);
            }
        }
    }

    #[allow(dead_code)]
    fn ordered_rotated_segment_paths(&self, signal: Signal) -> Vec<PathBuf> {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut rotated: Vec<(u64, u64, PathBuf)> = entries
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                let name = path.file_name()?.to_str()?;
                let (found_signal, millis, sequence) = parse_segment_name(name)?;
                (found_signal == signal).then_some((millis, sequence, path))
            })
            .collect();
        rotated.sort();
        rotated.into_iter().map(|(_, _, path)| path).collect()
    }
}

struct RotatedSegment {
    path: PathBuf,
    millis: u64,
    sequence: u64,
    size: u64,
}

fn recover_next_sequence(dir: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_name()?.to_str()?;
            let is_active =
                [TRACE_SEGMENT_FILE, LOG_SEGMENT_FILE, METRIC_SEGMENT_FILE].contains(&name);
            if !is_active && parse_segment_name(name).is_none() {
                return None;
            }
            fs::read_to_string(path).ok()
        })
        .flat_map(|contents| {
            contents
                .lines()
                .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
                .filter_map(|record| record["tauStoreSequence"].as_u64())
                .collect::<Vec<_>>()
        })
        .max()
        .map_or(0, |sequence| sequence.saturating_add(1))
}

/// Repairs an interrupted final append before writing the next record. A
/// complete JSON value without its newline is preserved; malformed trailing
/// bytes are truncated back to the last complete line.
fn recover_active_tail(path: &Path) -> io::Result<()> {
    let Ok(bytes) = fs::read(path) else {
        return Ok(());
    };
    if bytes.is_empty() || bytes.last() == Some(&b'\n') {
        return Ok(());
    }

    let tail_start = bytes
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |position| position + 1);
    if serde_json::from_slice::<serde_json::Value>(&bytes[tail_start..]).is_ok() {
        let mut file = OpenOptions::new().append(true).open(path)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
    } else {
        let file = OpenOptions::new().write(true).open(path)?;
        file.set_len(tail_start as u64)?;
        file.sync_all()?;
    }
    Ok(())
}

/// Parses a rotated segment's file name (`{signal}-{millis}-{sequence}.jsonl`)
/// back into its parts. Active segment file names (no dashes) return `None`.
fn parse_segment_name(name: &str) -> Option<(Signal, u64, u64)> {
    let stem = name.strip_suffix(".jsonl")?;
    let (signal_and_millis, sequence) = stem.rsplit_once('-')?;
    let sequence: u64 = sequence.parse().ok()?;
    let (signal_name, millis) = signal_and_millis.rsplit_once('-')?;
    let millis: u64 = millis.parse().ok()?;
    let signal = Signal::from_stem(signal_name)?;
    Some((signal, millis, sequence))
}

fn duration_nanos(time: SystemTime) -> String {
    time.duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos().to_string())
        .unwrap_or_default()
}

fn duration_millis(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(unix)]
fn set_owner_only_dir_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o700));
}
#[cfg(not(unix))]
fn set_owner_only_dir_permissions(_path: &Path) {}

#[cfg(unix)]
fn set_owner_only_file_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
}
#[cfg(not(unix))]
fn set_owner_only_file_permissions(_path: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Mutex as StdMutex;

    struct TestClock(StdMutex<SystemTime>);

    impl TestClock {
        fn new(start: SystemTime) -> Arc<Self> {
            Arc::new(TestClock(StdMutex::new(start)))
        }

        fn advance(&self, duration: Duration) {
            let mut time = self.0.lock().expect("test clock lock");
            *time += duration;
        }
    }

    impl Clock for TestClock {
        fn now(&self) -> SystemTime {
            *self.0.lock().expect("test clock lock")
        }
    }

    fn tiny_config() -> StoreConfig {
        StoreConfig {
            max_age: Duration::from_secs(7 * 24 * 60 * 60),
            max_total_bytes: 32 * 1024 * 1024,
            segment_rotation_bytes: u64::MAX,
        }
    }

    #[test]
    fn appends_are_ordered_and_sequenced() {
        let directory = tempfile::tempdir().expect("temp dir");
        let store = Store::new(
            directory.path().to_path_buf(),
            tiny_config(),
            TestClock::new(UNIX_EPOCH),
        );

        for index in 0..3 {
            assert!(store.append(Signal::Log, json!({ "n": index })));
        }

        let records = store.read_records(Signal::Log);
        let sequences: Vec<u64> = records
            .iter()
            .map(|record| record["tauStoreSequence"].as_u64().expect("sequence"))
            .collect();
        assert_eq!(sequences, vec![0, 1, 2]);
        assert!(records
            .iter()
            .all(|record| record["tauObservedTimeUnixNano"].is_string()));
        let values: Vec<u64> = records
            .iter()
            .map(|record| record["n"].as_u64().expect("n"))
            .collect();
        assert_eq!(values, vec![0, 1, 2]);
    }

    #[test]
    fn concurrent_appends_keep_disk_order_and_unique_sequences() {
        let directory = tempfile::tempdir().expect("temp dir");
        let store = Arc::new(Store::new(
            directory.path().to_path_buf(),
            tiny_config(),
            TestClock::new(UNIX_EPOCH),
        ));
        let threads: Vec<_> = (0..8)
            .map(|thread| {
                let store = Arc::clone(&store);
                std::thread::spawn(move || {
                    for record in 0..10 {
                        store.append(Signal::Log, json!({ "thread": thread, "record": record }));
                    }
                })
            })
            .collect();
        for thread in threads {
            thread.join().expect("append thread");
        }

        let sequences: Vec<u64> = store
            .read_records(Signal::Log)
            .iter()
            .map(|record| record["tauStoreSequence"].as_u64().expect("sequence"))
            .collect();
        assert_eq!(sequences, (0..80).collect::<Vec<_>>());
    }

    /// Stage 6: the concurrency test above deliberately never rotates
    /// (`tiny_config`'s `segment_rotation_bytes: u64::MAX`), so it never
    /// proves ordering survives a rotation happening *during* concurrent
    /// writes. This one forces frequent rotation while 8 threads append, so
    /// most writes land in a freshly-rotated segment, and asserts every
    /// sequence is still contiguous, unique, and disk-ordered once read back
    /// across however many rotated files resulted.
    #[test]
    fn concurrent_appends_survive_rotation_and_remain_ordered_and_unique() {
        let directory = tempfile::tempdir().expect("temp dir");
        let config = StoreConfig {
            segment_rotation_bytes: 40,
            ..tiny_config()
        };
        let store = Arc::new(Store::new(
            directory.path().to_path_buf(),
            config,
            TestClock::new(UNIX_EPOCH),
        ));
        let threads: Vec<_> = (0..8)
            .map(|thread| {
                let store = Arc::clone(&store);
                std::thread::spawn(move || {
                    for record in 0..15 {
                        store.append(Signal::Log, json!({ "thread": thread, "record": record }));
                    }
                })
            })
            .collect();
        for thread in threads {
            thread.join().expect("append thread");
        }

        let rotated_segments = fs::read_dir(directory.path())
            .expect("read dir")
            .flatten()
            .filter(|entry| parse_segment_name(entry.file_name().to_str().unwrap_or("")).is_some())
            .count();
        assert!(
            rotated_segments > 1,
            "expected multiple rotated segments under concurrent writes, got {rotated_segments}"
        );

        let records = store.read_records(Signal::Log);
        assert_eq!(records.len(), 120);
        let sequences: Vec<u64> = records
            .iter()
            .map(|record| record["tauStoreSequence"].as_u64().expect("sequence"))
            .collect();
        assert_eq!(sequences, (0..120).collect::<Vec<_>>());
        let mut unique_sequences = sequences.clone();
        unique_sequences.dedup();
        assert_eq!(
            unique_sequences.len(),
            120,
            "every sequence number must be unique across rotated segments"
        );
    }

    #[test]
    fn signals_are_stored_in_separate_segments() {
        let directory = tempfile::tempdir().expect("temp dir");
        let store = Store::new(
            directory.path().to_path_buf(),
            tiny_config(),
            TestClock::new(UNIX_EPOCH),
        );

        store.append(Signal::Log, json!({ "marker": "log" }));
        store.append(Signal::Trace, json!({ "marker": "trace" }));

        let logs = store.read_records(Signal::Log);
        let traces = store.read_records(Signal::Trace);
        assert_eq!(logs.len(), 1);
        assert_eq!(traces.len(), 1);
        assert_eq!(logs[0]["marker"], "log");
        assert_eq!(traces[0]["marker"], "trace");
        assert!(directory.path().join(LOG_SEGMENT_FILE).is_file());
        assert!(directory.path().join(TRACE_SEGMENT_FILE).is_file());
        assert!(!directory.path().join(METRIC_SEGMENT_FILE).exists());
    }

    #[test]
    fn recovers_earlier_records_despite_a_malformed_final_line() {
        let directory = tempfile::tempdir().expect("temp dir");
        let store = Store::new(
            directory.path().to_path_buf(),
            tiny_config(),
            TestClock::new(UNIX_EPOCH),
        );

        store.append(Signal::Log, json!({ "n": 0 }));
        store.append(Signal::Log, json!({ "n": 1 }));

        let path = directory.path().join(LOG_SEGMENT_FILE);
        let mut file = OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("open segment");
        file.write_all(b"{\"resourceLogs\":[{\"resource\"")
            .expect("write partial line");

        let records = store.read_records(Signal::Log);
        let values: Vec<u64> = records
            .iter()
            .map(|record| record["n"].as_u64().expect("n"))
            .collect();
        assert_eq!(values, vec![0, 1]);

        assert!(store.append(Signal::Log, json!({ "n": 2 })));
        let recovered_values: Vec<u64> = store
            .read_records(Signal::Log)
            .iter()
            .map(|record| record["n"].as_u64().expect("n"))
            .collect();
        assert_eq!(recovered_values, vec![0, 1, 2]);
    }

    #[test]
    fn preserves_a_complete_final_record_that_only_lacks_a_newline() {
        let directory = tempfile::tempdir().expect("temp dir");
        let path = directory.path().join(LOG_SEGMENT_FILE);
        fs::write(&path, r#"{"n":0,"tauStoreSequence":0}"#).expect("record without newline");
        let store = Store::new(
            directory.path().to_path_buf(),
            tiny_config(),
            TestClock::new(UNIX_EPOCH),
        );

        store.append(Signal::Log, json!({ "n": 1 }));
        let records = store.read_records(Signal::Log);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0]["n"], 0);
        assert_eq!(records[1]["tauStoreSequence"], 1);
    }

    #[test]
    fn sequence_continues_after_store_reopens() {
        let directory = tempfile::tempdir().expect("temp dir");
        {
            let store = Store::new(
                directory.path().to_path_buf(),
                tiny_config(),
                TestClock::new(UNIX_EPOCH),
            );
            store.append(Signal::Log, json!({ "n": 0 }));
            store.append(Signal::Trace, json!({ "n": 1 }));
        }

        let reopened = Store::new(
            directory.path().to_path_buf(),
            tiny_config(),
            TestClock::new(UNIX_EPOCH),
        );
        reopened.append(Signal::Log, json!({ "n": 2 }));
        assert_eq!(reopened.read_records(Signal::Log)[1]["tauStoreSequence"], 2);
    }

    #[test]
    fn rotation_moves_the_active_segment_once_it_exceeds_the_configured_size() {
        let directory = tempfile::tempdir().expect("temp dir");
        let config = StoreConfig {
            segment_rotation_bytes: 10,
            ..tiny_config()
        };
        let store = Store::new(
            directory.path().to_path_buf(),
            config,
            TestClock::new(UNIX_EPOCH),
        );

        for index in 0..5 {
            store.append(Signal::Log, json!({ "n": index }));
        }

        let rotated_exists = fs::read_dir(directory.path())
            .expect("read dir")
            .flatten()
            .any(|entry| parse_segment_name(entry.file_name().to_str().unwrap_or("")).is_some());
        assert!(rotated_exists, "expected at least one rotated segment");

        let records = store.read_records(Signal::Log);
        assert_eq!(records.len(), 5);
        let values: Vec<u64> = records
            .iter()
            .map(|record| record["n"].as_u64().expect("n"))
            .collect();
        assert_eq!(values, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn retention_deletes_the_oldest_rotated_segments_first_by_age() {
        let directory = tempfile::tempdir().expect("temp dir");
        let config = StoreConfig {
            max_age: Duration::from_secs(1),
            segment_rotation_bytes: 1,
            ..tiny_config()
        };
        let clock = TestClock::new(UNIX_EPOCH);
        let store = Store::new(
            directory.path().to_path_buf(),
            config,
            Arc::clone(&clock) as Arc<dyn Clock>,
        );

        store.append(Signal::Log, json!({ "n": 0 })); // rotates immediately (segment cap is 1 byte)
        clock.advance(Duration::from_secs(2));
        store.append(Signal::Log, json!({ "n": 1 })); // old rotated segment is now stale

        let records = store.read_records(Signal::Log);
        let values: Vec<u64> = records
            .iter()
            .map(|record| record["n"].as_u64().expect("n"))
            .collect();
        assert_eq!(
            values,
            vec![1],
            "the aged-out segment should have been deleted"
        );
    }

    #[test]
    fn retention_deletes_the_oldest_rotated_segments_first_by_size() {
        // One record's on-disk line, including the stamped sequence, so the
        // byte budget below can target "room for one segment, not two"
        // without hardcoding a JSON encoding length.
        let one_segment_bytes = json!({
            "n": 0u64,
            "tauObservedTimeUnixNano": "1000000",
            "tauStoreSequence": 0u64
        })
        .to_string()
        .len() as u64
            + 1;

        let directory = tempfile::tempdir().expect("temp dir");
        let config = StoreConfig {
            max_total_bytes: one_segment_bytes + one_segment_bytes / 2,
            segment_rotation_bytes: 1,
            ..tiny_config()
        };
        let clock = TestClock::new(UNIX_EPOCH);
        let store = Store::new(
            directory.path().to_path_buf(),
            config,
            Arc::clone(&clock) as Arc<dyn Clock>,
        );

        for index in 0..3 {
            clock.advance(Duration::from_millis(1));
            store.append(Signal::Log, json!({ "n": index }));
        }

        let records = store.read_records(Signal::Log);
        let values: Vec<u64> = records
            .iter()
            .map(|record| record["n"].as_u64().expect("n"))
            .collect();
        assert_eq!(
            values,
            vec![2],
            "only the newest segment should remain within budget"
        );
    }

    #[test]
    fn write_failures_are_retained_in_a_bounded_in_memory_fallback() {
        let directory = tempfile::tempdir().expect("temp dir");
        let blocked_path = directory.path().join("not-a-directory");
        fs::write(&blocked_path, b"file").expect("blocking file");
        let store = Store::new(blocked_path, tiny_config(), TestClock::new(UNIX_EPOCH));

        for index in 0..(FALLBACK_CAPACITY + 1) as u64 {
            let wrote_to_disk = store.append(Signal::Log, json!({ "n": index }));
            assert!(!wrote_to_disk);
        }

        assert_eq!(store.failed_writes(), (FALLBACK_CAPACITY + 1) as u64);
        assert_eq!(store.fallback_len(), FALLBACK_CAPACITY);
        assert_eq!(store.fallback_sequences().first().copied(), Some(1));
    }

    #[test]
    #[cfg(unix)]
    fn existing_directories_and_files_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().expect("temp dir");
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o755)).expect("chmod");
        let store = Store::new(
            directory.path().to_path_buf(),
            tiny_config(),
            TestClock::new(UNIX_EPOCH),
        );
        store.append(Signal::Log, json!({ "n": 0 }));

        let dir_mode = fs::metadata(directory.path())
            .expect("directory metadata")
            .permissions()
            .mode()
            & 0o777;
        let file_mode = fs::metadata(directory.path().join(LOG_SEGMENT_FILE))
            .expect("file metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(dir_mode, 0o700);
        assert_eq!(file_mode, 0o600);
    }

    #[test]
    fn try_append_persists_like_append_when_the_lock_is_free() {
        let directory = tempfile::tempdir().expect("temp dir");
        let store = Store::new(
            directory.path().to_path_buf(),
            tiny_config(),
            TestClock::new(UNIX_EPOCH),
        );

        assert!(store.try_append(Signal::Log, json!({ "n": 0 })));

        let records = store.read_records(Signal::Log);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0]["n"], 0);
    }

    /// The one guarantee the panic hook depends on: `try_append` must
    /// return immediately, never wait, when the writer lock is already
    /// held — including by this same thread, which is exactly what
    /// happens if a panic occurs mid-`append` and the installed panic hook
    /// tries to record telemetry about it through the normal blocking path.
    #[test]
    fn try_append_never_blocks_when_the_lock_is_already_held() {
        let directory = tempfile::tempdir().expect("temp dir");
        let store = Arc::new(Store::new(
            directory.path().to_path_buf(),
            tiny_config(),
            TestClock::new(UNIX_EPOCH),
        ));
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<()>();
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let holder = Arc::clone(&store);
        let handle = std::thread::spawn(move || {
            let _guard = holder.state.lock().expect("lock");
            ready_tx.send(()).expect("signal ready");
            release_rx.recv().expect("wait for release");
        });
        ready_rx.recv().expect("wait for the lock to be held");

        let started = std::time::Instant::now();
        let wrote = store.try_append(Signal::Log, json!({ "n": 0 }));
        let elapsed = started.elapsed();

        release_tx.send(()).expect("release the lock");
        handle.join().expect("holder thread");

        assert!(!wrote, "try_append must not write while the lock is held");
        assert!(
            elapsed < Duration::from_millis(200),
            "try_append blocked for {elapsed:?} instead of returning immediately"
        );
        assert!(store.read_records(Signal::Log).is_empty());
    }

    #[test]
    fn store_writes_to_its_configured_directory() {
        let directory = tempfile::tempdir().expect("temp dir");
        let store = Store::new(
            directory.path().to_path_buf(),
            tiny_config(),
            TestClock::new(UNIX_EPOCH),
        );
        store.append(Signal::Log, json!({ "n": 0 }));
        assert!(directory.path().join(LOG_SEGMENT_FILE).is_file());
    }
}
