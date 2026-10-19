//! Bounded segmented OTLP JSONL storage; `exporter.rs` owns the OTLP shape.
use std::collections::VecDeque;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
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
    /// Rotate below the total byte limit so retention can evict closed segments.
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

/// Retain failed writes in bounded memory instead of silently losing them.
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

/// Serialize appends so sequence numbers reflect disk order.
pub struct Store {
    dir: PathBuf,
    config: StoreConfig,
    clock: Arc<dyn Clock>,
    state: Mutex<StoreState>,
    /// Gate every signal before touching disk; disabled telemetry creates no directory.
    enabled: AtomicBool,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store")
            .field("dir", &self.dir)
            .finish_non_exhaustive()
    }
}

impl Store {
    pub fn with_enabled(
        dir: PathBuf,
        config: StoreConfig,
        clock: Arc<dyn Clock>,
        enabled: bool,
    ) -> Self {
        let next_sequence = if enabled {
            prepare_dir(&dir);
            recover_next_sequence(&dir)
        } else {
            0
        };
        Store {
            dir,
            config,
            clock,
            state: Mutex::new(StoreState {
                next_sequence,
                failed_writes: 0,
                fallback: VecDeque::new(),
            }),
            enabled: AtomicBool::new(enabled),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    /// Enabling a previously disabled store initializes disk and resumes sequence numbering.
    pub fn set_enabled(&self, enabled: bool) {
        if self.enabled.swap(enabled, Ordering::Relaxed) == enabled {
            return;
        }
        if !enabled {
            return;
        }
        prepare_dir(&self.dir);
        let recovered = recover_next_sequence(&self.dir);
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        state.next_sequence = state.next_sequence.max(recovered);
    }

    /// Stamp sequence and append; on failure retain in bounded fallback without failing product
    /// work.
    pub fn append(&self, signal: Signal, record: serde_json::Value) -> bool {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        self.append_locked(&mut state, signal, record)
    }

    /// Never wait for the writer lock from the panic hook: panic can occur while this thread
    /// holds it.
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
        // Dropped, not counted as a failed write: nothing was meant to
        // reach disk, so this is not a writer problem to report.
        if !self.is_enabled() {
            return true;
        }
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

    /// Skip malformed or partial JSONL lines, preserving other recoverable records.
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

    /// Evict expired then oldest closed segments across signals; never delete active segments.
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

fn prepare_dir(dir: &Path) {
    let _ = fs::create_dir_all(dir);
    set_owner_only_dir_permissions(dir);
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

/// Preserve complete final JSON without a newline; truncate incomplete trailing bytes.
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
