//! Memory-only, bounded, best-effort OTLP delivery. Reconfiguration drops pending work.
#[cfg(test)]
mod tests;
use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex};

use serde_json::Value;

use crate::cloud::{validate_otlp_response, Http, Request, Target};

#[derive(Clone, Copy)]
pub enum Signal {
    Log,
    Trace,
    Metric,
}
impl Signal {
    pub fn path(self) -> &'static str {
        match self {
            Self::Log => "logs",
            Self::Trace => "traces",
            Self::Metric => "metrics",
        }
    }
    fn key(self) -> &'static str {
        match self {
            Self::Log => "resourceLogs",
            Self::Trace => "resourceSpans",
            Self::Metric => "resourceMetrics",
        }
    }
}

#[derive(Default)]
struct Pending {
    target: Option<Target>,
    records: VecDeque<(Signal, Value)>,
    revision: u64,
    since: u128,
    stopped: bool,
}
struct Shared {
    pending: Mutex<Pending>,
    ready: Condvar,
}
pub struct Pipeline {
    shared: Arc<Shared>,
}
impl Pipeline {
    pub fn new(http: Arc<dyn Http>) -> Self {
        let shared = Arc::new(Shared {
            pending: Mutex::new(Pending::default()),
            ready: Condvar::new(),
        });
        let worker = shared.clone();
        // Failure to start a diagnostic worker must not break product operations.
        let started =
            std::thread::Builder::new()
                .name("tau-cloud-export".into())
                .spawn(move || loop {
                    let (target, revision, batch) = {
                        let Ok(mut pending) = worker.pending.lock() else {
                            return;
                        };
                        while pending.records.is_empty() && !pending.stopped {
                            let Ok(next) = worker.ready.wait(pending) else {
                                return;
                            };
                            pending = next;
                        }
                        if pending.stopped {
                            return;
                        }
                        let Some(target) = pending.target.clone() else {
                            continue;
                        };
                        let revision = pending.revision;
                        let count = pending.records.len().min(32);
                        let batch = pending.records.drain(..count).collect::<Vec<_>>();
                        (target, revision, batch)
                    };
                    for signal in [Signal::Log, Signal::Trace, Signal::Metric] {
                        let records = batch
                            .iter()
                            .filter(|(s, _)| s.path() == signal.path())
                            .flat_map(|(_, value)| {
                                value[signal.key()].as_array().cloned().unwrap_or_default()
                            })
                            .collect::<Vec<_>>();
                        if records.is_empty() {
                            continue;
                        }
                        if !worker.pending.lock().is_ok_and(|p| {
                            !p.stopped && p.revision == revision && p.target.is_some()
                        }) {
                            break;
                        }
                        let body = serde_json::json!({signal.key(): records});
                        let _ = http
                            .send(Request::otlp(&target, signal.path(), body))
                            .and_then(|response| validate_otlp_response(&response));
                    }
                })
                .is_ok();
        if !started {
            if let Ok(mut pending) = shared.pending.lock() {
                pending.stopped = true;
            }
        }
        Self { shared }
    }
    pub fn configure(&self, target: Option<Target>) {
        if let Ok(mut pending) = self.shared.pending.lock() {
            pending.revision = pending.revision.wrapping_add(1);
            pending.since = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            pending.records.clear();
            pending.target = if pending.stopped { None } else { target };
        }
    }
    pub fn is_enabled(&self) -> bool {
        self.shared
            .pending
            .lock()
            .is_ok_and(|p| p.target.is_some() && !p.stopped)
    }
    pub fn revision(&self) -> u64 {
        self.shared.pending.lock().map_or(u64::MAX, |p| p.revision)
    }
    pub fn accepts_timestamp(&self, value: &str) -> bool {
        let Ok(timestamp) = value.parse::<u128>() else {
            return false;
        };
        self.shared
            .pending
            .lock()
            .is_ok_and(|p| p.target.is_some() && !p.stopped && timestamp >= p.since)
    }
    pub fn append(&self, signal: Signal, value: Value) -> bool {
        self.append_at(signal, value, None)
    }
    pub fn append_at(&self, signal: Signal, value: Value, revision: Option<u64>) -> bool {
        // Keep both record count and individual payload size bounded.
        if serde_json::to_vec(&value).map_or(true, |v| v.len() > 64 * 1024) {
            return false;
        }
        let Ok(mut pending) = self.shared.pending.try_lock() else {
            return false;
        };
        if pending.target.is_none()
            || pending.stopped
            || pending.records.len() >= 256
            || revision.is_some_and(|r| r != pending.revision)
        {
            return false;
        }
        if !matches!(signal, Signal::Metric) && has_stale_timestamp(&value, pending.since) {
            return false;
        }
        pending.records.push_back((signal, value));
        self.shared.ready.notify_one();
        true
    }
    pub fn try_append(&self, signal: Signal, value: Value) -> bool {
        self.append(signal, value)
    }
}
fn has_stale_timestamp(value: &Value, since: u128) -> bool {
    match value {
        Value::Object(fields) => fields.iter().any(|(key, value)| {
            if key == "startTimeUnixNano" || key == "timeUnixNano" {
                value
                    .as_str()
                    .and_then(|s| s.parse::<u128>().ok())
                    .is_none_or(|time| time < since)
            } else {
                has_stale_timestamp(value, since)
            }
        }),
        Value::Array(values) => values.iter().any(|v| has_stale_timestamp(v, since)),
        _ => false,
    }
}
impl Drop for Pipeline {
    fn drop(&mut self) {
        if let Ok(mut pending) = self.shared.pending.lock() {
            pending.stopped = true;
            pending.target = None;
            pending.records.clear();
            self.shared.ready.notify_all();
        }
    }
}
