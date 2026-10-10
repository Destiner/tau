use std::sync::Arc;
use std::time::SystemTime;

use super::{attributes, ingest, Telemetry};

#[derive(Clone, Copy)]
pub enum Outcome {
    Success,
    Error,
}

/// A catalog entry, not a free-form operation label.
#[derive(Clone, Copy)]
pub struct Operation {
    family: &'static str,
    key: &'static str,
    name: &'static str,
}

impl Operation {
    pub fn command(name: &'static str) -> Option<Self> {
        attributes::TAURI_INVOKE_COMMANDS
            .contains(&name)
            .then_some(Self {
                family: attributes::TAURI_INVOKE.name,
                key: "tau.invoke.command",
                name,
            })
    }

    pub fn action(name: &'static str) -> Option<Self> {
        attributes::UI_ACTION_NAMES.contains(&name).then_some(Self {
            family: attributes::UI_ACTION.name,
            key: "tau.action.name",
            name,
        })
    }
}

pub struct OperationTimer {
    telemetry: Arc<Telemetry>,
    operation: Operation,
    start: SystemTime,
    enabled: bool,
    epoch: u64,
}

impl Telemetry {
    pub fn start_operation(self: &Arc<Self>, operation: Operation) -> OperationTimer {
        OperationTimer {
            telemetry: Arc::clone(self),
            operation,
            start: SystemTime::now(),
            enabled: self.is_enabled(),
            epoch: self.admin_epoch.load(std::sync::atomic::Ordering::Relaxed),
        }
    }
}

impl OperationTimer {
    /// Only confirmed outcomes are recorded; dropping a timer fabricates no success.
    pub fn finish(self, outcome: Outcome) {
        let _guard = self
            .telemetry
            .admin_lock
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if !self.enabled
            || !self.telemetry.is_enabled()
            || self.epoch
                != self
                    .telemetry
                    .admin_epoch
                    .load(std::sync::atomic::Ordering::Relaxed)
        {
            return;
        }
        let mut attrs = serde_json::Map::new();
        attrs.insert(self.operation.key.into(), self.operation.name.into());
        if self.operation.family == attributes::TAURI_INVOKE.name {
            attrs.insert(
                "tau.invoke.outcome".into(),
                match outcome {
                    Outcome::Success => "success",
                    Outcome::Error => "error",
                }
                .into(),
            );
        }
        let trace = uuid::Uuid::new_v4().simple().to_string();
        let span = uuid::Uuid::new_v4().simple().to_string()[..16].to_string();
        let record = serde_json::json!({
            "family": self.operation.family,
            "traceId": trace,
            "spanId": span,
            "sampled": true,
            "startTimeUnixNano": super::exporter::unix_nanos_string(self.start),
            "endTimeUnixNano": super::exporter::unix_nanos_string(SystemTime::now()),
            "attributes": attrs,
        });
        ingest::ingest_records(&self.telemetry, vec![record]);
    }
}

#[derive(Clone, Copy)]
pub enum Lifecycle {
    Started,
    Exited,
}

impl Telemetry {
    pub fn record_lifecycle(&self, lifecycle: Lifecycle) {
        match lifecycle {
            Lifecycle::Started => self.record_app_started(),
            Lifecycle::Exited => self.record_app_exited(),
        }
    }
}
