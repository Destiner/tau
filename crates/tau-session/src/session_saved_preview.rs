use super::*;
use std::collections::{HashMap, VecDeque};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::{self, Write};

const MAX_WIRE_ROWS: usize = 100_000;
const MAX_WIRE_BYTES: usize = 32 * 1024 * 1024;

// Hashes are only an index: exact Value equality is the collision proof.
fn fingerprint(message: &Value) -> u64 {
    let mut hash = DefaultHasher::new();
    message.hash(&mut hash);
    hash.finish()
}

#[derive(Default)]
struct WireSize(usize);
impl Write for WireSize {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0 = self.0.saturating_add(bytes.len());
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn wire_size(message: &Value) -> usize {
    let mut size = WireSize::default();
    // Counting serialization needs no second raw copy and cannot fail.
    serde_json::to_writer(&mut size, message).expect("counting JSON writer");
    size.0
}

struct WireRow {
    source: String,
    message: Value,
    fingerprint: u64,
    bytes: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavedPreviewToken {
    runtime: String,
    generation: u64,
    epoch: u64,
    revision: u64,
    preview_revision: u64,
    identity: Identity,
}

#[derive(Default)]
pub(super) struct Admission {
    pub(super) authoritative: bool,
    adopted: Option<(u64, u64, u64)>,
    revision: u64,
    pub(super) occupied: BTreeMap<String, usize>,
    wire_rows: Vec<WireRow>,
    sources: HashMap<String, usize>,
    wire_bytes: usize,
    projected: bool,
    degraded: bool,
    needs_rebase: bool,
    #[cfg(test)]
    operations: usize,
}
impl Admission {
    pub(super) fn track(
        &mut self,
        source: &str,
        message: &Value,
        counts: &BTreeMap<String, usize>,
    ) {
        if !self.projected || self.degraded {
            return;
        }
        self.retain_wire(source, message);
        if !self.degraded {
            // Only this counter changed; never clone the entire occurrence map.
            let base = source.rsplit_once(":occ").map_or(source, |(base, _)| base);
            if let Some(count) = counts.get(base) {
                self.occupied.insert(base.into(), *count);
            }
        }
    }

    fn retain_wire(&mut self, source: &str, message: &Value) {
        if self.degraded {
            return;
        }
        #[cfg(test)]
        {
            self.operations += 1;
        }
        let index = self.sources.get(source).copied();
        self.retain_wire_at(source, message, index);
    }

    fn append_wire(&mut self, source: &str, message: &Value) {
        if self.degraded {
            return;
        }
        #[cfg(test)]
        {
            self.operations += 1;
        }
        // Preview rows are occurrences, even when tool-result keys repeat.
        self.retain_wire_at(source, message, None);
    }

    fn retain_wire_at(&mut self, source: &str, message: &Value, index: Option<usize>) {
        let prior_bytes = index.map_or(0, |i| self.wire_rows[i].bytes);
        let bytes = wire_size(message).saturating_add(source.len());
        let total = self
            .wire_bytes
            .saturating_sub(prior_bytes)
            .saturating_add(bytes);
        if total > MAX_WIRE_BYTES || (index.is_none() && self.wire_rows.len() == MAX_WIRE_ROWS) {
            // Dropping only old references could assign a duplicate the wrong ID.
            // Abandon ALL overlap proofs until the authoritative RPC resets them.
            self.wire_rows = Vec::new();
            self.sources = HashMap::new();
            self.occupied.clear();
            self.wire_bytes = 0;
            self.degraded = true;
            self.needs_rebase = true;
            return;
        }
        let row = WireRow {
            source: source.into(),
            message: message.clone(),
            fingerprint: fingerprint(message),
            bytes,
        };
        if let Some(index) = index {
            self.wire_rows[index] = row;
        } else {
            // A streamed update replaces the first source occurrence, just as
            // the original ledger did; saved duplicates are not collapsed.
            self.sources
                .entry(source.into())
                .or_insert(self.wire_rows.len());
            self.wire_rows.push(row);
        }
        self.wire_bytes = total;
    }

    pub(super) fn reconcile(&mut self, messages: &[Value]) -> Vec<Option<String>> {
        self.reconcile_indexed(messages, fingerprint)
    }

    fn reconcile_indexed(
        &mut self,
        messages: &[Value],
        hash: impl Fn(&Value) -> u64,
    ) -> Vec<Option<String>> {
        let mut remaining = std::mem::take(&mut self.wire_rows);
        self.sources = HashMap::new();
        self.occupied.clear();
        self.wire_bytes = 0;
        let mut positions: HashMap<u64, VecDeque<usize>> = HashMap::new();
        for (i, row) in remaining.iter().enumerate() {
            positions.entry(row.fingerprint).or_default().push_back(i);
            #[cfg(test)]
            {
                self.operations += 1;
            }
        }
        let matched = messages
            .iter()
            .map(|message| {
                #[cfg(test)]
                {
                    self.operations += 1;
                }
                let bucket = positions.get_mut(&hash(message))?;
                // Usually one equality and pop_front, including identical duplicates.
                // Only hash collisions scan a bucket, never the entire history.
                let offset = bucket.iter().position(|&i| {
                    #[cfg(test)]
                    {
                        self.operations += 1;
                    }
                    remaining[i].message == *message
                })?;
                #[cfg(test)]
                {
                    self.operations += offset;
                }
                let i = if offset == 0 {
                    bucket.pop_front()?
                } else {
                    bucket.remove(offset)?
                };
                Some(std::mem::take(&mut remaining[i].source))
            })
            .collect();
        self.projected = false;
        self.degraded = false;
        self.needs_rebase = false;
        matched
    }

    pub(super) fn projects_wire_rows(&self) -> bool {
        self.projected
    }

    pub(super) fn adopt_start(&mut self, old: u64, new: u64, epoch: u64) {
        // Only initial startup can straddle a cosmetic read. Restarts cannot.
        self.adopted = (old == 0).then_some((old, new, epoch));
    }
}
impl Controller {
    pub(super) fn finish_saved_preview_track(&mut self) {
        if std::mem::take(&mut self.saved_preview.needs_rebase) {
            self.rebase_saved_preview_provenance();
        }
    }

    pub(super) fn restart_saved_preview_provenance(&mut self) {
        if self.saved_preview.degraded {
            self.rebase_saved_preview_provenance();
        }
    }

    fn rebase_saved_preview_provenance(&mut self) {
        // Sacrifice cosmetic continuity only on overflow. Counter resets cannot
        // alias these visible rows; this never changes runtime/session authority.
        let prefix = format!("preview-unproven:{}", self.next_id());
        for i in 0..self.state.transcript.len() {
            let id = self.next_id();
            let row = &mut self.state.transcript[i];
            row.source = format!("{prefix}:{}", row.source);
            row.id = id;
        }
        if let Some(key) = &mut self.active_assistant {
            *key = format!("{prefix}:{key}");
        }
        self.message_occurrences.clear();
        self.saved_preview.occupied.clear();
    }

    pub fn saved_preview_token(&self) -> Option<SavedPreviewToken> {
        (self.state.identity.saved_candidate()
            && self.revision == 0
            && self.state.transcript.is_empty()
            && !self.saved_preview.authoritative
            && !self.state.working)
            .then(|| SavedPreviewToken {
                runtime: self.runtime_id.clone(),
                generation: self.generation,
                epoch: self.epoch,
                revision: self.revision,
                preview_revision: self.saved_preview.revision,
                identity: self.state.identity.clone(),
            })
    }

    pub fn saved_preview_admitted(&self, token: &SavedPreviewToken) -> bool {
        let same_epoch = token.generation == self.generation && token.epoch == self.epoch;
        let initial_start = self.saved_preview.adopted
            == Some((token.generation, self.generation, token.epoch))
            && self.epoch == token.epoch + 1;
        if !(same_epoch || initial_start)
            || token.runtime != self.runtime_id
            || token.revision != self.revision
            || token.preview_revision != self.saved_preview.revision
            || token.identity.id != self.state.identity.id
            || token.identity.path != self.state.identity.path
            || self.saved_preview.authoritative
            || !self.state.transcript.is_empty()
            || self.state.working
        {
            return false;
        }
        true
    }

    /// Cosmetic projection only: no requests, registration or durability proof.
    pub fn apply_saved_preview(&mut self, token: &SavedPreviewToken, messages: &[Value]) -> bool {
        if !self.saved_preview_admitted(token) || messages.is_empty() {
            return false;
        }
        let mut occurrences = BTreeMap::new();
        let mut ledger = Admission {
            projected: true,
            ..Admission::default()
        };
        let mut rows = vec![];
        for (i, message) in messages.iter().enumerate() {
            let key = occurrence_key(message, &format!("history-{i}"), &mut occurrences);
            rows.extend(project(message, &key));
            ledger.append_wire(&key, message);
        }
        if rows.is_empty() {
            return false;
        }
        ledger.adopted = self.saved_preview.adopted;
        ledger.revision = self.saved_preview.revision;
        self.saved_preview = ledger;
        self.merge_rows(rows, false);
        // Reserve projection keys, not runtime provenance or durability authority.
        if !self.saved_preview.degraded {
            self.saved_preview.occupied = occurrences.clone();
            self.message_occurrences = occurrences;
        }
        self.finish_saved_preview_track();
        self.saved_preview.revision += 1;
        // Loading, status, settings and drafts remain owned by the runtime.
        !self.state.transcript.is_empty()
    }
}

#[cfg(test)]
#[path = "session_saved_preview_perf_tests.rs"]
mod perf_tests;

#[cfg(test)]
mod tests {
    use super::*;
    fn bound() -> Controller {
        let mut c = Controller::new("runtime");
        c.bind_source("saved", "/saved.jsonl");
        c.set_draft("keep draft");
        c
    }
    fn messages() -> Vec<Value> {
        vec![json!({"role":"user", "timestamp":1, "content":"saved text"})]
    }
    #[test]
    fn initial_start_adopts_preview_without_proving_durability() {
        let mut c = bound();
        let token = c.saved_preview_token().unwrap();
        c.begin_generation(1);
        let status = c.state.status;
        c.state.model = json!({"id":"preserve-model"});
        c.state.effort = "high".into();
        c.state.recovered_drafts = vec!["recovery".into()];
        assert!(c.apply_saved_preview(&token, &messages()));
        assert_eq!(c.state.status, status);
        assert_eq!(c.state.model, json!({"id":"preserve-model"}));
        assert_eq!(c.state.effort, "high");
        assert_eq!(c.state.recovered_drafts, vec!["recovery"]);
        assert!(c.state.history_loading);
        assert_eq!(c.state.draft, "keep draft");
        assert!(!c.state.ready);
        assert!(!c.state.durable);
        assert!(!c.apply_saved_preview(&token, &messages()));
        let source = c.state.transcript[0].source.clone();
        let id = c.state.transcript[0].id.clone();
        let request = c.latest["get_messages"].clone();
        let effects = c.handle(json!({"type":"response", "id":request,
            "command":"get_messages", "success":true, "data":{"messages":messages()}}));
        assert!(!effects
            .iter()
            .any(|e| matches!(e, Effect::RegisterSession { .. })));
        assert_eq!(c.state.transcript[0].source, source);
        assert_eq!(c.state.transcript[0].id, id);
    }
    #[test]
    fn occupied_preview_keys_survive_initial_start_and_reconcile_exact_wire_occurrences() {
        for before_start in [false, true] {
            for identical_text in [false, true] {
                for hydration in ["both", "new", "empty"] {
                    let mut c = bound();
                    let old = json!({"role":"user", "timestamp":1, "content":"old"});
                    let new = if identical_text {
                        // Same displayed prose, distinct wire content and provenance.
                        json!({"role":"user", "timestamp":1,
                            "content":[{"type":"text", "text":"old"}]})
                    } else {
                        json!({"role":"user", "timestamp":1, "content":"new"})
                    };
                    let token = c.saved_preview_token().unwrap();
                    if !before_start {
                        c.begin_generation(1);
                    }
                    assert!(c.apply_saved_preview(&token, std::slice::from_ref(&old)));
                    assert!(!c.apply_saved_preview(&token, std::slice::from_ref(&old)));
                    if before_start {
                        c.begin_generation(1);
                    }
                    let old_id = c.state.transcript[0].id.clone();
                    let old_source = c.state.transcript[0].source.clone();
                    c.handle(json!({"type":"message_start", "message":new}));
                    assert_eq!(c.state.transcript.len(), 2);
                    let new_id = c.state.transcript[1].id.clone();
                    assert_ne!(old_id, new_id);
                    assert_eq!(c.state.transcript[0].source, old_source);
                    assert_ne!(old_source, c.state.transcript[1].source);
                    let stale = c.latest["get_messages"].clone();
                    c.handle(json!({"type":"response", "id":stale,
                        "command":"get_messages", "success":true, "data":{"messages":[old.clone()]}}));
                    assert_eq!(c.state.transcript.len(), 2);
                    assert_eq!(c.state.transcript[0].id, old_id);
                    assert_eq!(c.state.transcript[1].id, new_id);
                    assert!(!c.saved_preview.authoritative);
                    assert_eq!(c.state.transcript[0].text, "old");
                    assert!(!c.state.ready);
                    assert!(!c.state.durable);
                    assert_eq!(c.state.draft, "keep draft");
                    let messages = match hydration {
                        "both" => vec![old, new],
                        "new" => vec![new],
                        _ => vec![],
                    };
                    let request = c.send("get_messages", json!({}), false);
                    let Effect::Send(request) = request else {
                        unreachable!()
                    };
                    let effects = c.handle(json!({"type":"response", "id":request["id"],
                        "command":"get_messages", "success":true, "data":{"messages":messages}}));
                    assert!(!effects
                        .iter()
                        .any(|e| matches!(e, Effect::RegisterSession { .. })));
                    let ids: Vec<_> = c.state.transcript.iter().map(|r| r.id.clone()).collect();
                    assert_eq!(
                        ids,
                        match hydration {
                            "both" => vec![old_id, new_id],
                            "new" => vec![new_id],
                            _ => vec![],
                        }
                    );
                    let sources: HashSet<_> =
                        c.state.transcript.iter().map(|r| &r.source).collect();
                    assert_eq!(sources.len(), c.state.transcript.len());
                    // Authoritative empty is a reset, not a retained cosmetic reservation.
                    if hydration == "empty" {
                        c.handle(json!({"type":"message_start", "message":{
                            "role":"user", "timestamp":1, "content":"after empty"}}));
                        assert_eq!(c.state.transcript[0].source, "user:1:occ0");
                    }
                }
            }
        }
    }

    #[test]
    fn nonprojecting_preview_rolls_back_reservations_and_admission_revision() {
        let mut c = bound();
        let token = c.saved_preview_token().unwrap();
        assert!(!c.apply_saved_preview(
            &token,
            &[json!({"role":"assistant", "timestamp":1, "content":[]})]
        ));
        assert!(c.saved_preview_admitted(&token));
        assert!(c.message_occurrences.is_empty());
        assert!(!c.saved_preview.projects_wire_rows());
        assert!(c.apply_saved_preview(&token, &messages()));
        c.bind_source("other", "/other.jsonl");
        assert!(!c.saved_preview.projects_wire_rows());
        assert!(c.message_occurrences.is_empty());
        assert!(!c.saved_preview_admitted(&token));
    }

    #[test]
    fn authoritative_empty_stream_identity_and_restart_defeat_preview() {
        let mut c = bound();
        let token = c.saved_preview_token().unwrap();
        let mut other = Controller::new("other-runtime");
        other.bind_source("saved", "/saved.jsonl");
        assert!(!other.apply_saved_preview(&token, &messages()));
        c.begin_generation(1);
        let token = c.saved_preview_token().unwrap();
        let request = c.latest["get_messages"].clone();
        c.handle(
            json!({"type":"response", "id":request, "command":"get_messages",
            "success":true, "data":{"messages":[]}}),
        );
        assert!(!c.apply_saved_preview(&token, &messages()));
        let mut c = bound();
        let token = c.saved_preview_token().unwrap();
        c.begin_generation(1);
        c.handle(json!({"type":"message_start", "message":messages()[0]}));
        assert!(!c.apply_saved_preview(&token, &messages()));
        let mut c = bound();
        let token = c.saved_preview_token().unwrap();
        c.bind_source("other", "/other.jsonl");
        c.bind_source("saved", "/saved.jsonl");
        assert!(!c.apply_saved_preview(&token, &messages()));
        let mut c = bound();
        let token = c.saved_preview_token().unwrap();
        c.begin_generation(1);
        c.begin_generation(2);
        assert!(!c.apply_saved_preview(&token, &messages()));
    }
}
