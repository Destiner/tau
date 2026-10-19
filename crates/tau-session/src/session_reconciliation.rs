use super::*;

const PROBE_DELAYS: [u64; 5] = [150, 450, 1_000, 2_000, 3_200];
const VERIFICATION_DELAYS: [u64; 3] = [250, 750, 1_500];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum ReadPurpose {
    #[default]
    Ordinary,
    Bootstrap,
    Command,
    Probe,
    Settlement,
    Verification,
}

#[derive(Clone, Debug)]
struct ScheduledRead {
    identity: Identity,
    epoch: u64,
    deadline: u64,
    purpose: ReadPurpose,
}

#[derive(Default)]
pub(super) struct Reconciliation {
    pub(super) input_closed: bool,
    scheduled: Vec<ScheduledRead>,
    pub(super) command: bool,
    pub(super) command_incarnation: u64,
    failed_command: bool,
    command_error_revision: Option<u64>,
    pub(super) verifying: bool,
    verification_attempt: usize,
    provisional: bool,
    source: Option<Identity>,
    pub(super) bootstrap_source: Option<Identity>,
    pub(super) messages_serial: u64,
    pub(super) uncertain_write: bool,
    pub(super) uncertain_since: u64,
}

impl Controller {
    /// Storage receipts belong to the host after RegisterSession is emitted.
    pub fn durability_pending(&self) -> bool {
        !self.materialisation.is_empty()
            || self.reconciliation.verifying
            || self.pending.values().any(|p| {
                p.barrier
                    || matches!(
                        p.purpose,
                        ReadPurpose::Settlement | ReadPurpose::Verification
                    )
            })
            || self
                .reconciliation
                .scheduled
                .iter()
                .any(|r| r.purpose == ReadPurpose::Verification)
    }

    /// Drain captured completed output before physical stop. This is not quit approval.
    pub fn prepare_exit(&mut self) -> Vec<Effect> {
        let identities: Vec<_> = self
            .materialisation
            .values()
            .map(|(identity, _)| identity.clone())
            .collect();
        let mut effects = vec![];
        for identity in identities {
            self.handoff_materialisation(&identity, &mut effects);
            self.pending
                .retain(|_, p| !(p.barrier && p.identity.binding_eq(&identity)));
            if identity.binding_eq(&self.state.identity) {
                self.verified();
                self.pending.retain(|_, p| {
                    !matches!(
                        p.purpose,
                        ReadPurpose::Settlement | ReadPurpose::Verification
                    )
                });
            }
        }
        self.pending.retain(|_, p| {
            !(self
                .registered
                .contains(&(p.identity.id.clone(), p.identity.path.clone()))
                && (p.barrier
                    || matches!(
                        p.purpose,
                        ReadPurpose::Settlement | ReadPurpose::Verification
                    )))
        });
        if self.state.durable {
            self.verified();
        }
        effects
    }

    pub(super) fn has_materialisation(&self, identity: &Identity) -> bool {
        self.materialisation
            .contains_key(&(identity.id.clone(), identity.path.clone()))
    }

    pub(super) fn capture_materialisation(&mut self) {
        let identity = self.state.identity.clone();
        let key = (identity.id.clone(), identity.path.clone());
        if identity.saved_candidate() && !self.registered.contains(&key) {
            self.materialisation
                .entry(key)
                .or_insert((identity, self.generation));
        }
    }

    pub(super) fn handoff_materialisation(
        &mut self,
        identity: &Identity,
        effects: &mut Vec<Effect>,
    ) {
        if self.has_materialisation(identity) {
            self.register(identity, effects);
        }
    }

    pub fn reconciliation_pending(&self) -> bool {
        self.reconciliation.command
            || self.reconciliation.verifying
            || self.reconciliation.uncertain_write
            || self.reconciliation.source.is_some()
            || !self.reconciliation.scheduled.is_empty()
            || self
                .pending
                .values()
                .any(|p| p.barrier || p.purpose != ReadPurpose::Ordinary)
    }

    pub fn provisional_replacement(&self) -> bool {
        self.reconciliation.provisional
    }

    pub fn source_recovery(&self) -> Option<&Identity> {
        self.reconciliation.source.as_ref()
    }

    pub fn acknowledge_source_recovery(&mut self, source: &Identity) -> bool {
        if self.reconciliation.source.as_ref() != Some(source) {
            return false;
        }
        self.reconciliation.source = None;
        true
    }

    pub fn can_release_runtime(&self) -> bool {
        self.connected
            && self.state.ready
            && !self.state.working
            && !self.state.stopping
            && !self.state.compacting
            && !self.state.history_loading
            && self.state.extension_dialogs.is_empty()
            && self.state.queue.steering.is_empty()
            && self.state.queue.follow_up.is_empty()
            && self.admission.is_none()
            && self.queue_submission.is_none()
            && self.pending.is_empty()
            && self.startup_deadline.is_none()
            && self.stop_deadline.is_none()
            && !self.reconciliation_pending()
            && !self.durability_pending()
            && !self.provisional_replacement()
    }

    /// Call only after the host has acknowledged cleanup of this generation.
    pub fn release_generation(&mut self, generation: u64) -> bool {
        if generation != self.generation || !self.can_release_runtime() {
            return false;
        }
        self.connected = false;
        self.state.ready = false;
        self.state.working = false;
        self.state.stopping = false;
        self.state.compacting = false;
        self.state.retry = None;
        self.state.status = Status::Starting;
        true
    }

    /// Selection retries reconciliation, never the user operation that caused it.
    pub fn resume_reads(&mut self) -> Vec<Effect> {
        if !self.connected || !self.reconciliation.failed_command {
            return vec![];
        }
        if self
            .pending
            .values()
            .any(|p| p.purpose == ReadPurpose::Command)
        {
            return vec![];
        }
        self.start_command_reconciliation()
    }

    pub(super) fn send_read(&mut self, command: &str, purpose: ReadPurpose) -> Effect {
        let effect = self.send(command, json!({}), false);
        if let Effect::Send(v) = &effect {
            if let Some(p) = self.pending.get_mut(v["id"].as_str().unwrap_or_default()) {
                p.purpose = purpose;
            }
        }
        effect
    }

    pub(super) fn start_command_reconciliation(&mut self) -> Vec<Effect> {
        self.reconciliation.command_incarnation += 1;
        self.reconciliation.command = true;
        self.reconciliation.failed_command = false;
        self.watch_replacement();
        vec![self.send_read("get_state", ReadPurpose::Command)]
    }

    pub(super) fn watch_replacement(&mut self) {
        self.reconciliation
            .scheduled
            .retain(|r| r.purpose != ReadPurpose::Probe);
        for delay in PROBE_DELAYS {
            self.schedule_read(ReadPurpose::Probe, delay);
        }
    }

    fn schedule_read(&mut self, purpose: ReadPurpose, delay: u64) {
        self.reconciliation.scheduled.push(ScheduledRead {
            identity: self.state.identity.clone(),
            epoch: self.epoch,
            deadline: self.now.saturating_add(delay),
            purpose,
        });
    }

    pub(super) fn start_verification(&mut self) {
        if !self.state.durable && !self.reconciliation.verifying {
            self.reconciliation.verifying = true;
            self.reconciliation.verification_attempt = 0;
        }
    }

    pub(super) fn retry_verification(&mut self) {
        if !self.reconciliation.verifying || self.state.durable {
            return;
        }
        if self
            .reconciliation
            .scheduled
            .iter()
            .any(|r| r.purpose == ReadPurpose::Verification)
            || self.pending.values().any(|p| {
                matches!(
                    p.purpose,
                    ReadPurpose::Settlement | ReadPurpose::Verification
                )
            })
        {
            return;
        }
        let attempt = self.reconciliation.verification_attempt;
        if let Some(delay) = VERIFICATION_DELAYS.get(attempt) {
            self.reconciliation.verification_attempt += 1;
            self.schedule_read(ReadPurpose::Verification, *delay);
        } else {
            self.reconciliation.verifying = false;
        }
    }

    pub(super) fn verified(&mut self) {
        self.reconciliation.verifying = false;
        self.reconciliation.provisional = false;
        self.reconciliation
            .scheduled
            .retain(|r| r.purpose != ReadPurpose::Verification);
    }

    pub(super) fn reconciliation_failed(&mut self, p: &Pending) -> bool {
        match p.purpose {
            ReadPurpose::Command => {
                if !self.reconciliation.command
                    || p.command_incarnation != self.reconciliation.command_incarnation
                {
                    return true;
                }
                self.reconciliation.command = true;
                self.reconciliation.failed_command = true;
                self.state.history_loading = false;
                self.fail("The command completed, but its session could not be refreshed. Select it again to retry the reads.");
                self.reconciliation.command_error_revision = Some(self.error_revision);
                true
            }
            ReadPurpose::Probe => {
                if p.command == "get_messages" {
                    self.state.history_loading = false;
                }
                true
            }
            ReadPurpose::Settlement | ReadPurpose::Verification => {
                self.state.history_loading = false;
                self.retry_verification();
                true
            }
            _ if p.barrier => {
                self.start_verification();
                self.retry_verification();
                true
            }
            _ => false,
        }
    }

    pub(super) fn tick_reconciliation(&mut self) -> Vec<Effect> {
        let scheduled = std::mem::take(&mut self.reconciliation.scheduled);
        let mut effects = vec![];
        for read in scheduled {
            if !self.connected
                || read.epoch != self.epoch
                || !read.identity.binding_eq(&self.state.identity)
            {
                continue;
            }
            if read.deadline > self.now {
                self.reconciliation.scheduled.push(read);
                continue;
            }
            if self.state.working
                || self.state.compacting
                || !self.state.extension_dialogs.is_empty()
            {
                if read.purpose == ReadPurpose::Verification {
                    self.reconciliation.verifying = false;
                }
                continue;
            }
            effects.push(self.send_read("get_state", read.purpose));
        }
        effects
    }

    pub(super) fn reset_reconciliation_generation(&mut self) {
        let source = self.reconciliation.source.take();
        let provisional = self.reconciliation.provisional;
        let uncertain_write = self.reconciliation.uncertain_write;
        let uncertain_since = self.reconciliation.uncertain_since;
        self.reconciliation = Reconciliation::default();
        self.reconciliation.uncertain_write = uncertain_write;
        self.reconciliation.uncertain_since = uncertain_since;
        self.reconciliation.source = source;
        self.reconciliation.provisional = provisional;
        self.reconciliation.bootstrap_source = self
            .state
            .identity
            .saved_candidate()
            .then(|| self.state.identity.clone());
    }

    pub(super) fn replacement_reconciliation(
        &mut self,
        bootstrap: bool,
        previous: &Identity,
        current: &Identity,
    ) {
        let missing_source = bootstrap
            && self
                .reconciliation
                .bootstrap_source
                .as_ref()
                .is_some_and(|source| {
                    source.binding_eq(previous)
                        && source.path == current.path
                        && !current.id.is_empty()
                        && source.id != current.id
                });
        if missing_source && self.reconciliation.source.is_none() {
            self.reconciliation.source = Some(previous.clone());
        }
        self.reconciliation.provisional = previous.saved_candidate() && !missing_source;
        self.reconciliation.scheduled.clear();
        self.reconciliation.verifying = false;
        self.reconciliation.verification_attempt = 0;
        self.reconciliation.messages_serial = 0;
        self.reconciliation.bootstrap_source = None;
    }

    pub(super) fn reconciled_messages(
        &mut self,
        p: &Pending,
        messages: &[Value],
        effects: &mut Vec<Effect>,
    ) {
        if p.purpose == ReadPurpose::Command
            && p.command_incarnation == self.reconciliation.command_incarnation
            && self.reconciliation.command
        {
            self.reconciliation.command = false;
            self.reconciliation.failed_command = false;
            if self.reconciliation.command_error_revision.take() == Some(self.error_revision) {
                self.state.error = None;
            }
        }
        if !self.state.working
            && self.state_serial > self.reconciliation.uncertain_since
            && p.serial > self.reconciliation.uncertain_since
        {
            self.reconciliation.uncertain_write = false;
        }
        let verifies = matches!(
            p.purpose,
            ReadPurpose::Settlement | ReadPurpose::Verification | ReadPurpose::Command
        ) || self.reconciliation.verifying
            || self.reconciliation.provisional;
        if verifies && !self.state.working && messages.iter().any(meaningful_completed_assistant) {
            let identity = self.state.identity.clone();
            self.register(&identity, effects);
            self.verified();
        } else if self.reconciliation.verifying {
            self.retry_verification();
        }
    }
}
