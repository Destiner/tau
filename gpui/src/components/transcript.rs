//! Session-local transcript. Sources outlive the bounded presentation cache.
use super::{
    activity::{ActivityContent, ActivityEvent, ActivityRow, ActivitySource},
    dialogs::NativeDialog,
    markdown::{self, MarkdownPresentation},
    support,
};
use gpui_kit::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, FollowMode, IntoElement,
    ListAlignment, ListState, Render, ScrollHandle, Subscription, TestSupportExt, WeakEntity,
    Window,
    base::{
        Button as BaseButton, Scrollbar, ScrollbarMode, SelectionFormat, TextSelectionDomain,
        TextSelectionDomainRange, TextViewState,
    },
    div, list,
    prelude::*,
    px, rgb, size,
};
use std::collections::{HashMap, HashSet, VecDeque};

#[path = "transcript/notice.rs"]
mod notice;
pub use notice::{Notice, NoticeSeverity};

#[derive(Clone, Debug)]
pub enum TranscriptContent {
    User(String),
    PendingUser(String),
    Compaction { available: bool, loading: bool },
    Assistant(String),
    Notice(Notice),
    Activity(ActivityContent),
}
impl TranscriptContent {
    fn inline_only(&self) -> bool {
        matches!(self, Self::Notice(notice) if notice.inline)
    }
    fn is_activity(&self) -> bool {
        matches!(self, Self::Activity(_))
    }
    fn text(&self) -> Option<&str> {
        match self {
            Self::User(text) | Self::PendingUser(text) | Self::Assistant(text) => Some(text),
            Self::Notice(notice) => Some(&notice.text),
            Self::Activity(_) | Self::Compaction { .. } => None,
        }
    }
}
#[derive(Clone, Debug)]
pub struct TranscriptEntry {
    pub id: String,
    pub content: TranscriptContent,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TranscriptError {
    DuplicateId(String),
}
#[derive(Clone, Debug)]
pub enum TranscriptEvent {
    /// The owner must validate schemes and route external/local-file destinations.
    OpenLink(String),
    LinkRequested {
        key: u64,
        target: markdown::LinkTarget,
        origin: Option<FocusHandle>,
        anchor: gpui_kit::Point<gpui_kit::Pixels>,
    },
    LinkContextRequested {
        key: u64,
        target: markdown::LinkTarget,
        event: gpui_kit::MouseDownEvent,
    },
    /// The owner routes this to the originating controller's load_history.
    LoadHistory(String),
}
struct Row {
    entry: TranscriptEntry,
    key: u64,
    link_revision: u64,
    expanded: bool,
    details_scroll: ScrollHandle,
    activity_source: Option<Entity<ActivitySource>>,
    activity_subscription: Option<Subscription>,
    loading_started: Option<std::time::Instant>,
    history_requested: bool,
}
#[derive(Clone)]
enum View {
    Prose(Entity<TextViewState>, Entity<MarkdownPresentation>),
    Activity(Entity<ActivityRow>),
    Boundary {
        id: String,
        available: bool,
        loading: bool,
        show_loading: bool,
    },
}
struct Mounted {
    view: View,
    _subscriptions: Vec<Subscription>,
}
const CACHE_LIMIT: usize = 128;

pub type LinkContextHandler = dyn Fn(u64, markdown::LinkTarget, &gpui_kit::MouseDownEvent, &mut Window, &mut App)
    + Send
    + Sync;

pub struct Transcript {
    id: String,
    dark: bool,
    focus: FocusHandle,
    rows: Vec<Row>,
    by_id: HashMap<String, usize>,
    by_key: HashMap<u64, usize>,
    next_key: u64,
    list: ListState,
    selection: TextSelectionDomain,
    cache: HashMap<u64, Mounted>,
    recent: VecDeque<u64>,
    working_indicator: Option<String>,
    copy_paths: bool,
    context_handler: Option<std::sync::Arc<LinkContextHandler>>,
    disabled_file_sources: HashSet<String>,
    extension_prompt: Option<Entity<NativeDialog>>,
    prompt_height: gpui_kit::Pixels,
    prompt_subscription: Option<Subscription>,
    compacting: bool,
    previous_compactions: Option<HashSet<String>>,
    awaiting_compaction: bool,
    compaction_pulse: Option<(u64, std::time::Instant)>,
}
impl EventEmitter<TranscriptEvent> for Transcript {}
impl Focusable for Transcript {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
impl Transcript {
    pub fn new(id: impl Into<String>, dark: bool, cx: &mut Context<Self>) -> Self {
        let owner = cx.entity().downgrade();
        let copy_owner = owner.clone();
        let selection = TextSelectionDomain::new(
            move |key, cx| {
                let owner = owner.upgrade()?;
                let this = owner.read(cx);
                this.prose_index(key).map(|ix| ix as u64)
            },
            move |range, cx| {
                copy_owner
                    .upgrade()
                    .map(|owner| owner.read(cx).copy_selection(range))
                    .unwrap_or_default()
            },
        );
        let list = ListState::new(0, ListAlignment::Top, px(80.))
            .with_estimated_item_size(size(px(0.), px(80.)));
        list.set_follow_mode(FollowMode::Tail);
        Self {
            id: id.into(),
            dark,
            focus: cx.focus_handle(),
            rows: vec![],
            by_id: HashMap::new(),
            by_key: HashMap::new(),
            next_key: 0,
            list,
            selection,
            cache: HashMap::new(),
            recent: VecDeque::new(),
            working_indicator: None,
            copy_paths: false,
            context_handler: None,
            disabled_file_sources: HashSet::new(),
            extension_prompt: None,
            prompt_height: px(0.),
            prompt_subscription: None,
            compacting: false,
            previous_compactions: None,
            awaiting_compaction: false,
            compaction_pulse: None,
        }
    }
    pub fn set_context_handler(
        &mut self,
        handler: impl Fn(u64, markdown::LinkTarget, &gpui_kit::MouseDownEvent, &mut Window, &mut App)
        + Send
        + Sync
        + 'static,
        cx: &mut Context<Self>,
    ) {
        self.context_handler = Some(std::sync::Arc::new(handler));
        for (&key, mounted) in &self.cache {
            if let View::Activity(activity) = &mounted.view {
                self.install_activity_context(key, activity, cx.entity().downgrade(), cx);
            }
        }
        cx.notify();
    }
    fn install_activity_context(
        &self,
        key: u64,
        activity: &Entity<ActivityRow>,
        owner: WeakEntity<Self>,
        cx: &mut App,
    ) {
        let entity = activity.downgrade();
        activity.update(cx, |activity, cx| {
            activity.set_context_handler(move |target, event, window, cx| {
                let Some(entity) = entity.upgrade() else { return; };
                let handler = owner.update(cx, |this, cx| {
                    if !this.by_key.contains_key(&key)
                        || !matches!(this.cache.get(&key), Some(Mounted { view: View::Activity(current), .. }) if current == &entity) {
                        return None;
                    }
                    if this.context_handler.is_none() {
                        cx.emit(TranscriptEvent::LinkContextRequested { key, target: target.clone(), event: event.clone() });
                    }
                    this.context_handler.clone()
                }).ok().flatten();
                if let Some(handler) = handler { handler(key, target, event, window, cx); }
            }, cx);
        });
    }
    /// Session-owned remote semantics; path resolution metadata stays with the Shell's source key.
    pub fn set_copy_paths(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.copy_paths == enabled {
            return;
        }
        self.copy_paths = enabled;
        for mounted in self.cache.values() {
            if let View::Prose(_, presentation) = &mounted.view {
                presentation.update(cx, |p, cx| p.set_copy_paths(enabled, cx));
            }
        }
        for row in &self.rows {
            if let Some(source) = &row.activity_source {
                source.update(cx, |s, cx| s.set_copy_paths(enabled, cx));
            }
        }
        if let Some(prompt) = &self.extension_prompt {
            prompt.update(cx, |d, cx| d.set_copy_paths(enabled, cx));
        }
        cx.notify();
    }
    pub fn set_disabled_file_sources(&mut self, sources: HashSet<String>, cx: &mut Context<Self>) {
        if self.disabled_file_sources == sources {
            return;
        }
        self.disabled_file_sources = sources;
        for row in &self.rows {
            if let Some(Mounted {
                view: View::Prose(_, presentation),
                ..
            }) = self.cache.get(&row.key)
            {
                let enabled = !row.entry.content.inline_only()
                    && !self.disabled_file_sources.contains(&row.entry.id);
                presentation.update(cx, |p, cx| p.set_file_links(enabled, cx));
            }
        }
        cx.notify();
    }
    pub fn source_id(&self, key: u64) -> Option<&str> {
        self.by_key
            .get(&key)
            .map(|&ix| self.rows[ix].entry.id.as_str())
    }
    fn prompt_index(&self) -> usize {
        self.rows.len() + usize::from(self.compacting || self.working_indicator.is_some())
    }
    pub fn set_extension_prompt(
        &mut self,
        prompt: Option<Entity<NativeDialog>>,
        cx: &mut Context<Self>,
    ) {
        if self.extension_prompt == prompt {
            return;
        }
        let following = self.list.is_following_tail();
        let ix = self.prompt_index();
        let before = usize::from(self.extension_prompt.is_some());
        let after = usize::from(prompt.is_some());
        if let Some(previous) = &self.extension_prompt {
            previous.update(cx, |d, _| d.clear_option_reveal_handler());
        }
        let owner = cx.entity().downgrade();
        self.prompt_subscription = prompt.as_ref().map(|prompt| {
            let prompt_id = prompt.entity_id();
            prompt.update(cx, |d, cx| {
                d.set_copy_paths(self.copy_paths, cx);
                d.set_option_reveal_handler(move |bounds, _, cx| {
                    let owner = owner.clone();
                    // Prepaint owns the dialog borrow; mutate the reader after it is released.
                    cx.defer(move |cx| {
                        let _ = owner.update(cx, |this, cx| {
                            this.reveal_prompt_option(prompt_id, bounds, cx);
                        });
                    });
                });
            });
            cx.observe(prompt, |_, _, cx| cx.notify())
        });
        self.extension_prompt = prompt;
        self.prompt_height = px(0.);
        self.list.splice(ix..ix + before, after);
        if following {
            self.list.scroll_to_end();
        }
        cx.notify();
    }
    fn reveal_prompt_option(
        &mut self,
        prompt_id: gpui_kit::EntityId,
        bounds: gpui_kit::Bounds<gpui_kit::Pixels>,
        cx: &mut Context<Self>,
    ) {
        if self.extension_prompt.as_ref().map(Entity::entity_id) != Some(prompt_id) {
            return;
        }
        let viewport = self.list.viewport_bounds();
        let delta = if bounds.size.height > viewport.size.height || bounds.top() < viewport.top() {
            viewport.top() - bounds.top()
        } else if bounds.bottom() > viewport.bottom() {
            viewport.bottom() - bounds.bottom()
        } else {
            px(0.)
        };
        if delta != px(0.) {
            // ListState accepts logical reader distance, opposite the content displacement.
            self.list.scroll_by(-delta);
            cx.notify();
        }
    }
    fn measure_prompt(
        &mut self,
        prompt: &Entity<NativeDialog>,
        height: gpui_kit::Pixels,
        cx: &mut Context<Self>,
    ) {
        if self.extension_prompt.as_ref() != Some(prompt)
            || (self.prompt_height - height).abs() < px(0.5)
        {
            return;
        }
        let following = self.list.is_following_tail();
        self.prompt_height = height;
        let ix = self.prompt_index();
        self.list.remeasure_items(ix..ix + 1);
        if following {
            self.list.scroll_to_end();
        }
        cx.notify();
    }
    /// Presentation-only virtual tail. Does not allocate source/selection identities.
    pub fn set_working_indicator(&mut self, label: Option<&str>, cx: &mut Context<Self>) {
        if self.working_indicator.as_deref() == label {
            return;
        }
        let tail = self.list.is_following_tail();
        let ix = self.rows.len();
        let before = usize::from(self.compacting || self.working_indicator.is_some());
        let after = usize::from(self.compacting || label.is_some());
        self.working_indicator = label.map(str::to_owned);
        if before != after {
            self.list.splice(ix..ix + before, after);
        } else {
            self.list.remeasure_items(ix..ix + after);
        }
        if tail {
            self.list.scroll_to_end();
        }
        cx.notify();
    }
    /// Parent calls this before projection sync so delayed get_messages can identify the new boundary.
    pub fn set_compacting(&mut self, compacting: bool, cx: &mut Context<Self>) {
        if self.compacting == compacting {
            return;
        }
        let tail = self.list.is_following_tail();
        let before = usize::from(self.compacting || self.working_indicator.is_some());
        self.compacting = compacting;
        if compacting {
            self.previous_compactions = Some(
                self.rows
                    .iter()
                    .filter_map(|row| {
                        if matches!(row.entry.content, TranscriptContent::Compaction { .. }) {
                            Some(row.entry.id.clone())
                        } else {
                            None
                        }
                    })
                    .collect(),
            );
            self.awaiting_compaction = false;
            self.compaction_pulse = None;
        } else {
            self.awaiting_compaction = true;
            self.pulse_new_compaction(cx);
        }
        let after = usize::from(self.compacting || self.working_indicator.is_some());
        let ix = self.rows.len();
        if before != after {
            self.list.splice(ix..ix + before, after);
        } else {
            self.list.remeasure_items(ix..ix + after);
        }
        if tail {
            self.list.scroll_to_end();
        }
        cx.notify();
    }
    fn pulse_new_compaction(&mut self, cx: &mut Context<Self>) {
        if !self.awaiting_compaction {
            return;
        }
        let Some(previous) = &self.previous_compactions else {
            return;
        };
        let Some(key) = self
            .rows
            .iter()
            .rev()
            .find(|row| {
                matches!(row.entry.content, TranscriptContent::Compaction { .. })
                    && !previous.contains(&row.entry.id)
            })
            .map(|row| row.key)
        else {
            return;
        };
        self.awaiting_compaction = false;
        self.previous_compactions = None;
        let start = std::time::Instant::now();
        self.compaction_pulse = Some((key, start));
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(650))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.compaction_pulse == Some((key, start)) {
                    this.compaction_pulse = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }
    pub fn working_indicator(&self) -> Option<&str> {
        self.working_indicator.as_deref()
    }
    pub fn len(&self) -> usize {
        self.rows.len()
    }
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
    pub fn is_following_tail(&self) -> bool {
        self.list.is_following_tail()
    }
    /// Actual logical distance into the first visible row, not a predicted coordinate.
    pub fn get_before_first(&self) -> f32 {
        f32::from(self.list.logical_scroll_top().offset_in_item)
    }
    pub fn scroll_to_end(&mut self, cx: &mut Context<Self>) {
        self.list.scroll_to_end();
        cx.notify();
    }
    #[cfg(test)]
    pub(crate) fn prose_source_for_test(&self, key: u64) -> Option<Entity<TextViewState>> {
        match &self.cache.get(&key)?.view {
            View::Prose(source, _) => Some(source.clone()),
            _ => None,
        }
    }

    pub fn clear_selection(&mut self, cx: &mut Context<Self>) {
        self.selection.clear();
        for mounted in self.cache.values() {
            if let View::Prose(text, _) = &mounted.view {
                text.update(cx, |text, cx| text.clear_selection(cx));
            }
        }
        cx.notify();
    }
    /// Retires all identities, even if the next session reuses the same string IDs.
    pub fn reset(&mut self, cx: &mut Context<Self>) {
        self.clear_selection(cx);
        self.rows.clear();
        self.extension_prompt = None;
        self.prompt_subscription = None;
        self.prompt_height = px(0.);
        self.working_indicator = None;
        self.compacting = false;
        self.previous_compactions = None;
        self.awaiting_compaction = false;
        self.compaction_pulse = None;
        self.by_id.clear();
        self.by_key.clear();
        self.cache.clear();
        self.recent.clear();
        self.list.reset(0);
        self.list.scroll_to_end();
        cx.notify();
    }
    pub fn set_theme(&mut self, dark: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.dark == dark {
            return;
        }
        self.dark = dark;
        for mounted in self.cache.values() {
            match &mounted.view {
                View::Prose(_, presentation) => {
                    presentation.update(cx, |p, cx| p.set_theme(dark, cx))
                }
                View::Activity(activity) => {
                    activity.update(cx, |a, cx| a.set_theme(dark, window, cx))
                }
                View::Boundary { .. } => {}
            }
        }
        for row in &self.rows {
            if let Some(source) = &row.activity_source
                && let TranscriptContent::Activity(content) = &row.entry.content
            {
                source.update(cx, |s, cx| s.set_content(content, dark, cx));
                source.update(cx, |s, cx| s.set_copy_paths(self.copy_paths, cx));
            }
        }
        if let Some(prompt) = &self.extension_prompt {
            prompt.update(cx, |d, cx| d.set_theme(dark, window, cx));
        }
        cx.notify();
    }
    pub fn append_entry(
        &mut self,
        entry: TranscriptEntry,
        cx: &mut Context<Self>,
    ) -> Result<(), TranscriptError> {
        if self.by_id.contains_key(&entry.id) {
            return Err(TranscriptError::DuplicateId(entry.id));
        }
        let tail = self.list.is_following_tail();
        let ix = self.rows.len();
        let key = self.allocate_key();
        self.by_id.insert(entry.id.clone(), ix);
        self.by_key.insert(key, ix);
        self.rows.push(Row {
            entry,
            key,
            link_revision: 0,
            expanded: false,
            details_scroll: ScrollHandle::new(),
            activity_source: None,
            activity_subscription: None,
            loading_started: None,
            history_requested: false,
        });
        self.list.splice(ix..ix, 1);
        self.pulse_new_compaction(cx);
        if ix > 0 && self.is_compact(ix - 1) {
            // The former tail is now compact beside another activity.
            self.list.remeasure_items(ix - 1..ix);
        }
        if tail {
            self.list.scroll_to_end();
        }
        cx.notify();
        Ok(())
    }
    fn allocate_key(&mut self) -> u64 {
        let key = self.next_key;
        self.next_key = self
            .next_key
            .checked_add(1)
            .expect("transcript identity space exhausted");
        key
    }
    /// Updates one source row without retiring its identity, disclosure or focus.
    pub fn update_content(
        &mut self,
        id: &str,
        content: TranscriptContent,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(&ix) = self.by_id.get(id) else {
            return false;
        };
        let was_requested = std::mem::take(&mut self.rows[ix].history_requested);
        if same_content(&self.rows[ix].entry.content, &content) {
            if was_requested {
                cx.notify();
            }
            return true;
        }
        self.rows[ix].link_revision = self.rows[ix].link_revision.wrapping_add(1);
        if !matches!(content, TranscriptContent::Compaction { loading: true, .. }) {
            self.rows[ix].loading_started = None;
        }
        let key = self.rows[ix].key;
        if let TranscriptContent::Activity(content) = &content {
            if let Some(source) = &self.rows[ix].activity_source {
                source.update(cx, |s, cx| s.set_content(content, self.dark, cx));
                source.update(cx, |s, cx| s.set_copy_paths(self.copy_paths, cx));
            }
        } else {
            self.rows[ix].activity_source = None;
            self.rows[ix].activity_subscription = None;
        }
        let neighbor_changed = self.rows[ix].entry.content.is_activity() != content.is_activity();
        let append = match (self.rows[ix].entry.content.text(), content.text()) {
            (Some(before), Some(after))
                if same_prose_kind(&self.rows[ix].entry.content, &content) =>
            {
                after.strip_prefix(before).map(str::to_owned)
            }
            _ => None,
        };
        if let Some(chunk) = append {
            self.append_text(id, &chunk, cx);
            // Pending acknowledgments change only bubble style, not selection identity.
            self.rows[ix].entry.content = content;
        } else {
            if self.selection_contains(key)
                && (self.rows[ix].entry.content.text().is_some() || content.text().is_some())
            {
                self.selection.invalidate();
            }
            if let (
                Some(Mounted {
                    view: View::Activity(activity),
                    ..
                }),
                TranscriptContent::Activity(content),
            ) = (self.cache.get(&key), &content)
            {
                activity.update(cx, |a, cx| a.set_content(content.clone(), cx));
            } else {
                self.cache.remove(&key);
                self.recent.retain(|&cached| cached != key);
            }
            self.rows[ix].entry.content = content;
        }
        let start = if neighbor_changed {
            ix.saturating_sub(1)
        } else {
            ix
        };
        self.list.remeasure_items(start..ix + 1);
        cx.notify();
        true
    }
    fn selection_contains(&self, key: u64) -> bool {
        let Some(range) = self.selection.stored_range() else {
            return false;
        };
        let (Some(a), Some(b), Some(ix)) = (
            self.by_key.get(&range.anchor.item),
            self.by_key.get(&range.cursor.item),
            self.by_key.get(&key),
        ) else {
            return false;
        };
        ((*a).min(*b)..=(*a).max(*b)).contains(ix)
    }
    /// Bootstrap/reconcile only. Streaming callers should use the single-row APIs.
    pub fn set_entries(
        &mut self,
        entries: Vec<TranscriptEntry>,
        cx: &mut Context<Self>,
    ) -> Result<(), TranscriptError> {
        let mut ids = HashSet::with_capacity(entries.len());
        for entry in &entries {
            if !ids.insert(entry.id.as_str()) {
                return Err(TranscriptError::DuplicateId(entry.id.clone()));
            }
        }
        let tail = self.list.is_following_tail();
        let old_len = self.rows.len();
        let prefix = self
            .rows
            .iter()
            .zip(&entries)
            .take_while(|(a, b)| a.entry.id == b.id)
            .count();
        let suffix = self.rows[prefix..]
            .iter()
            .rev()
            .zip(entries[prefix..].iter().rev())
            .take_while(|(a, b)| a.entry.id == b.id)
            .count();
        // Activity insertions/removals and changes outside the excerpt do not
        // compromise selection. Compare only the selected prose identity span.
        if let Some(range) = self.selection.stored_range() {
            if let (Some(a), Some(b)) = (
                self.prose_index(range.anchor.item),
                self.prose_index(range.cursor.item),
            ) {
                let selected: Vec<_> = self.rows[a.min(b)..=a.max(b)]
                    .iter()
                    .filter(|r| r.entry.content.text().is_some())
                    .map(|r| r.entry.id.as_str())
                    .collect();
                let first = entries.iter().position(|e| e.id == selected[0]);
                let last = entries
                    .iter()
                    .position(|e| e.id == selected[selected.len() - 1]);
                let stable = match (first, last) {
                    (Some(first), Some(last)) if first <= last => entries[first..=last]
                        .iter()
                        .filter(|e| e.content.text().is_some())
                        .map(|e| e.id.as_str())
                        .eq(selected.iter().copied()),
                    _ => false,
                };
                if !stable {
                    self.selection.invalidate();
                }
            } else {
                self.selection.invalidate();
            }
        }
        let selected_keys: HashSet<_> = self
            .rows
            .iter()
            .filter(|row| self.selection_contains(row.key))
            .map(|row| row.key)
            .collect();
        let compact_before: HashMap<_, _> = self
            .rows
            .iter()
            .enumerate()
            .map(|(ix, row)| (row.key, self.is_compact(ix)))
            .collect();
        let mut changed = HashSet::new();
        let mut old: HashMap<_, _> = std::mem::take(&mut self.rows)
            .into_iter()
            .map(|r| (r.entry.id.clone(), r))
            .collect();
        for entry in entries {
            let row = if let Some(mut row) = old.remove(&entry.id) {
                if !same_content(&row.entry.content, &entry.content) {
                    row.link_revision = row.link_revision.wrapping_add(1);
                    changed.insert(row.key);
                    if let TranscriptContent::Activity(content) = &entry.content {
                        if let Some(source) = &row.activity_source {
                            source.update(cx, |s, cx| s.set_content(content, self.dark, cx));
                            source.update(cx, |s, cx| s.set_copy_paths(self.copy_paths, cx));
                        }
                    } else {
                        row.activity_source = None;
                        row.activity_subscription = None;
                    }
                    if let Some(Mounted {
                        view: View::Activity(activity),
                        ..
                    }) = self.cache.get(&row.key)
                    {
                        row.expanded = activity.read(cx).expanded();
                    }
                    // Compatible append preserves directed endpoint offsets and mounted entities.
                    let append = match (row.entry.content.text(), entry.content.text()) {
                        (Some(before), Some(after))
                            if same_prose_kind(&row.entry.content, &entry.content) =>
                        {
                            after.strip_prefix(before)
                        }
                        _ => None,
                    };
                    if let Some(chunk) = append {
                        self.grow_row(&mut row, chunk, cx);
                    } else {
                        if selected_keys.contains(&row.key)
                            && (row.entry.content.text().is_some()
                                || entry.content.text().is_some())
                        {
                            self.selection.invalidate();
                        }
                        if let (
                            Some(Mounted {
                                view: View::Activity(activity),
                                ..
                            }),
                            TranscriptContent::Activity(content),
                        ) = (self.cache.get(&row.key), &entry.content)
                        {
                            activity.update(cx, |a, cx| a.set_content(content.clone(), cx));
                        } else {
                            self.cache.remove(&row.key);
                            self.recent.retain(|&key| key != row.key);
                        }
                    }
                }
                if !matches!(
                    entry.content,
                    TranscriptContent::Compaction { loading: true, .. }
                ) {
                    row.loading_started = None;
                }
                row.history_requested = false;
                row.entry = entry;
                row
            } else {
                Row {
                    entry,
                    key: self.allocate_key(),
                    link_revision: 0,
                    expanded: false,
                    details_scroll: ScrollHandle::new(),
                    activity_source: None,
                    activity_subscription: None,
                    loading_started: None,
                    history_requested: false,
                }
            };
            self.rows.push(row);
        }
        self.by_id.clear();
        self.by_key.clear();
        for (ix, row) in self.rows.iter().enumerate() {
            self.by_id.insert(row.entry.id.clone(), ix);
            self.by_key.insert(row.key, ix);
        }
        self.pulse_new_compaction(cx);
        self.cache.retain(|key, _| self.by_key.contains_key(key));
        self.recent.retain(|key| self.by_key.contains_key(key));
        if prefix != old_len || old_len != self.rows.len() {
            self.list
                .splice(prefix..old_len - suffix, self.rows.len() - prefix - suffix);
        }
        // A surviving neighbor can change padding even when its source is unchanged.
        for (ix, row) in self.rows.iter().enumerate() {
            if compact_before
                .get(&row.key)
                .is_some_and(|before| *before != self.is_compact(ix))
            {
                changed.insert(row.key);
            }
        }
        // Preserve geometry of unchanged rows.
        for key in changed {
            if let Some(&ix) = self.by_key.get(&key) {
                self.list.remeasure_items(ix..ix + 1);
            }
        }
        if self.rows.is_empty() {
            self.list.reset(
                usize::from(self.compacting || self.working_indicator.is_some())
                    + usize::from(self.extension_prompt.is_some()),
            );
            self.list.scroll_to_end();
        } else if tail {
            self.list.scroll_to_end();
        }
        cx.notify();
        Ok(())
    }
    pub fn append_text(&mut self, id: &str, chunk: &str, cx: &mut Context<Self>) -> bool {
        let Some(&ix) = self.by_id.get(id) else {
            return false;
        };
        if self.rows[ix].entry.content.text().is_none() {
            return false;
        }
        if chunk.is_empty() {
            return true;
        }
        self.rows[ix].link_revision = self.rows[ix].link_revision.wrapping_add(1);
        // Temporarily borrow the row separately without cloning historical source.
        let key = self.rows[ix].key;
        let bound = self.endpoint_bound(key);
        let inline = self.rows[ix].entry.content.inline_only();
        let before = bound
            .map(|end| markdown_prefix(self.rows[ix].entry.content.text().unwrap(), end, inline));
        let text = match &mut self.rows[ix].entry.content {
            TranscriptContent::User(t)
            | TranscriptContent::PendingUser(t)
            | TranscriptContent::Assistant(t) => t,
            TranscriptContent::Notice(notice) => &mut notice.text,
            _ => unreachable!(),
        };
        text.push_str(chunk);
        if let (Some(end), Some(before)) = (bound, before)
            && (before.is_none() || markdown_prefix(text, end, inline) != before)
        {
            self.selection.invalidate();
        }
        self.push_view(key, chunk, cx);
        self.list.remeasure_items(ix..ix + 1);
        cx.notify();
        true
    }
    fn endpoint_bound(&self, key: u64) -> Option<usize> {
        self.selection.stored_range().and_then(|r| {
            [r.anchor, r.cursor]
                .into_iter()
                .filter(|e| e.item == key && e.offset > 0)
                .map(|e| e.offset)
                .max()
        })
    }
    fn grow_row(&mut self, row: &mut Row, chunk: &str, cx: &mut App) {
        let text = row.entry.content.text().unwrap();
        if let Some(end) = self.endpoint_bound(row.key) {
            let inline = row.entry.content.inline_only();
            let before = markdown_prefix(text, end, inline);
            let mut after = text.to_owned();
            after.push_str(chunk);
            if before.is_none() || markdown_prefix(&after, end, inline) != before {
                self.selection.invalidate();
            }
        }
        self.push_view(row.key, chunk, cx);
    }
    fn push_view(&self, key: u64, chunk: &str, cx: &mut App) {
        if let Some(Mounted {
            view: View::Prose(text, _),
            ..
        }) = self.cache.get(&key)
        {
            text.update(cx, |text, cx| text.push_str(chunk, cx));
        }
    }
    fn prose_index(&self, key: u64) -> Option<usize> {
        self.by_key
            .get(&key)
            .copied()
            .filter(|&ix| self.rows[ix].entry.content.text().is_some())
    }
    fn copy_selection(&self, range: TextSelectionDomainRange) -> String {
        let (Some(a), Some(b)) = (
            self.prose_index(range.anchor.item),
            self.prose_index(range.cursor.item),
        ) else {
            return String::new();
        };
        let (start, end, first, last) = if (a, range.anchor.offset) <= (b, range.cursor.offset) {
            (range.anchor, range.cursor, a, b)
        } else {
            (range.cursor, range.anchor, b, a)
        };
        let mut result = String::new();
        for ix in first..=last {
            let Some(source) = self.rows[ix].entry.content.text() else {
                continue;
            };
            let Ok(text) = TextViewState::copy_markdown_slice_with_extensions(
                source,
                (ix == first).then_some(start.offset),
                (ix == last).then_some(end.offset),
                SelectionFormat::Plain,
                &markdown::extensions().inline_only(self.rows[ix].entry.content.inline_only()),
            ) else {
                return String::new();
            };
            if !result.is_empty() && !text.is_empty() {
                result.push('\n');
            }
            result.push_str(&text);
        }
        result
    }
    fn mount(&mut self, ix: usize, owner: WeakEntity<Self>, cx: &mut App) -> Option<View> {
        let row = self.rows.get_mut(ix)?;
        let key = row.key;
        if let TranscriptContent::Compaction { available, loading } = row.entry.content {
            if !loading {
                row.loading_started = None;
            } else if row.loading_started.is_none() {
                row.loading_started = Some(std::time::Instant::now());
                let notify = owner.clone();
                cx.spawn(async move |cx| {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(180))
                        .await;
                    let _ = notify.update(cx, |this, cx| this.row_changed(key, cx));
                })
                .detach();
            }
            // Dividers have no prose/markdown state and do not occupy the 128-view cache.
            return Some(View::Boundary {
                id: row.entry.id.clone(),
                available,
                loading: loading || row.history_requested,
                show_loading: row
                    .loading_started
                    .is_some_and(|start| start.elapsed() >= std::time::Duration::from_millis(180)),
            });
        }
        let row = self.rows.get(ix)?;
        if !self.cache.contains_key(&key) {
            let mut subscriptions = vec![];
            let view = if let Some(source) = row.entry.content.text() {
                let domain = self.selection.clone();
                let text = cx.new(|cx| {
                    let mut text = TextViewState::markdown_with_extensions(
                        source,
                        markdown::extensions().inline_only(row.entry.content.inline_only()),
                        cx,
                    );
                    text.set_selection_domain(Some((domain, key)), cx);
                    text
                });
                let notify = owner.clone();
                let presentation = cx.new(|cx| {
                    MarkdownPresentation::new(
                        text.clone(),
                        self.dark,
                        move |cx| {
                            let _ = notify.update(cx, |this, cx| this.row_changed(key, cx));
                        },
                        cx,
                    )
                });
                presentation.update(cx, |p, cx| {
                    p.set_copy_paths(self.copy_paths, cx);
                    p.set_file_links(
                        !row.entry.content.inline_only()
                            && !self.disabled_file_sources.contains(&row.entry.id),
                        cx,
                    );
                    p.set_inline_only(row.entry.content.inline_only(), cx);
                });
                let notify = owner.clone();
                subscriptions.push(cx.observe(&presentation, move |_, cx| {
                    let _ = notify.update(cx, |_, cx| cx.notify());
                }));
                View::Prose(text, presentation)
            } else {
                let TranscriptContent::Activity(content) = &row.entry.content else {
                    unreachable!()
                };
                let activity = cx.new(|cx| {
                    let mut activity = ActivityRow::new(
                        format!("{}-activity-{key}", self.id),
                        content.clone(),
                        self.dark,
                        cx,
                    );
                    if let Some(source) = &row.activity_source {
                        activity.set_source_state(source.clone(), cx);
                    }
                    activity
                        .source_state()
                        .update(cx, |s, cx| s.set_copy_paths(self.copy_paths, cx));
                    activity.set_details_scroll(row.details_scroll.clone());
                    if row.expanded {
                        activity.toggle(cx);
                    }
                    activity
                });
                self.install_activity_context(key, &activity, owner.clone(), cx);
                let notify = owner.clone();
                subscriptions.push(cx.observe(&activity, move |activity, cx| {
                    let expanded = activity.read(cx).expanded();
                    let _ = notify.update(cx, |this, cx| {
                        let current = matches!(this.cache.get(&key), Some(Mounted { view: View::Activity(current), .. }) if current == &activity);
                        if !current { return; }
                        if let Some(&ix) = this.by_key.get(&key) {
                            if this.rows[ix].expanded != expanded {
                                this.rows[ix].expanded = expanded;
                                this.list.remeasure_items(ix..ix + 1);
                            }
                            this.row_changed(key, cx);
                        }
                    });
                }));
                let source = activity.read(cx).source_state();
                let source_notify = owner.clone();
                self.rows[ix].activity_subscription =
                    Some(cx.observe(&source, move |source, cx| {
                        let _ = source_notify.update(cx, |this, cx| {
                            if this.by_key.get(&key).is_some_and(|&ix| {
                                this.rows[ix].activity_source.as_ref() == Some(&source)
                            }) {
                                this.row_changed(key, cx);
                            }
                        });
                    }));
                self.rows[ix].activity_source = Some(source);
                subscriptions.push(
                    cx.subscribe(&activity, move |source, event: &ActivityEvent, cx| {
                        let _ = owner.update(cx, |this, cx| {
                            let current = matches!(this.cache.get(&key), Some(Mounted { view: View::Activity(current), .. }) if current == &source);
                            if !current || !this.by_key.contains_key(&key) { return; }
                            match event {
                                ActivityEvent::OpenLink(url) => {
                                    cx.emit(TranscriptEvent::OpenLink(url.clone()));
                                }
                                ActivityEvent::LinkRequested { target, origin, anchor } => cx.emit(TranscriptEvent::LinkRequested { key, target: target.clone(), origin: origin.clone(), anchor: *anchor }),
                                ActivityEvent::LinkContextRequested { target, event } => cx.emit(TranscriptEvent::LinkContextRequested { key, target: target.clone(), event: event.clone() }),
                            }
                        });
                    }),
                );
                View::Activity(activity)
            };
            self.cache.insert(
                key,
                Mounted {
                    view,
                    _subscriptions: subscriptions,
                },
            );
        }
        self.recent.retain(|&cached| cached != key);
        self.recent.push_back(key);
        while self.recent.len() > CACHE_LIMIT {
            if let Some(evicted) = self.recent.pop_front() {
                // Read disclosure state synchronously: eviction may precede an observer delivery.
                if let Some(Mounted {
                    view: View::Activity(activity),
                    ..
                }) = self.cache.get(&evicted)
                    && let Some(&ix) = self.by_key.get(&evicted)
                {
                    self.rows[ix].expanded = activity.read(cx).expanded();
                }
                self.cache.remove(&evicted);
            }
        }
        self.cache.get(&key).map(|mounted| mounted.view.clone())
    }
    fn is_compact(&self, ix: usize) -> bool {
        self.rows
            .get(ix)
            .is_some_and(|row| row.entry.content.is_activity())
            && self
                .rows
                .get(ix + 1)
                .is_some_and(|row| row.entry.content.is_activity())
    }
    fn row_changed(&mut self, key: u64, cx: &mut Context<Self>) {
        if let Some(&ix) = self.by_key.get(&key) {
            self.list.remeasure_items(ix..ix + 1);
            cx.notify();
        }
    }
}
impl Render for Transcript {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let owner = cx.entity().downgrade();
        let dark = self.dark;
        let t = support::tokens(dark);
        let gutter = (f32::from(window.viewport_size().width) * 0.05).clamp(20., 60.);
        let prompt_overlay = self.extension_prompt.clone().map(|prompt| {
            let list_state = self.list.clone();
            let ix = self.prompt_index();
            let notify = owner.clone();
            gpui_kit::canvas(
                move |bounds, window, cx| {
                    let wheel_reader = list_state.clone();
                    let wheel_owner = notify.clone();
                    let mut child = div()
                        .id("transcript-extension-tail")
                        .test_support()
                        .w_full()
                        .flex()
                        .justify_center()
                        // The retained tail is a sibling of List, not one of its painted children.
                        .on_scroll_wheel(move |event, window, cx| {
                            wheel_reader
                                .scroll_by(-event.delta.pixel_delta(window.line_height()).y);
                            let _ = wheel_owner.update(cx, |_, cx| cx.notify());
                            cx.stop_propagation();
                        })
                        .child(
                            div()
                                .w_full()
                                .max_w(px(880.))
                                .pl(px(gutter + 8.))
                                .pr(px(gutter + 20.))
                                .pb(px(22.))
                                .child(prompt.clone()),
                        )
                        .into_any_element();
                    let measured = child.layout_as_root(
                        size(
                            gpui_kit::AvailableSpace::Definite(bounds.size.width),
                            gpui_kit::AvailableSpace::MinContent,
                        ),
                        window,
                        cx,
                    );
                    let measured_prompt = prompt.clone();
                    let measured_owner = notify.clone();
                    cx.defer(move |cx| {
                        let _ = measured_owner.update(cx, |this, cx| {
                            this.measure_prompt(&measured_prompt, measured.height, cx)
                        });
                    });
                    // The spacer is painted by List first; use this frame's real reader position.
                    let origin = list_state
                        .bounds_for_item(ix)
                        .map(|b| b.origin + gpui_kit::point(px(0.), px(28.)))
                        .unwrap_or(gpui_kit::point(bounds.left(), bounds.bottom() + px(1.)));
                    let mask = gpui_kit::ContentMask { bounds };
                    window.with_content_mask(Some(mask), |window| {
                        child.prepaint_at(origin, window, cx)
                    });
                    // A prompt editor must not turn native selection reveal into a history jump.
                    window.take_autoscroll();
                    child
                },
                move |bounds, mut child, window, cx| {
                    window.with_content_mask(Some(gpui_kit::ContentMask { bounds }), |window| {
                        child.paint(window, cx)
                    });
                },
            )
            .absolute()
            .inset_0()
        });
        div()
            .id(self.id.clone())
            .test_support()
            .track_focus(&self.focus)
            .relative()
            .overflow_hidden()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(
                list(self.list.clone(), move |ix, window, cx| {
                    let Some(entity) = owner.upgrade() else {
                        return div().into_any_element();
                    };
                    let prompt_spacer = {
                        let this = entity.read(cx);
                        (this.extension_prompt.is_some() && ix == this.prompt_index()).then_some(this.prompt_height)
                    };
                    if let Some(height) = prompt_spacer {
                        return div().id("transcript-extension-spacer").test_support().w_full().h(height).into_any_element();
                    }
                    let compacting = {
                        let this = entity.read(cx);
                        ix == this.rows.len() && this.compacting
                    };
                    if compacting {
                        return div()
                            .id("tau-compacting")
                            .test_support()
                            .role(gpui_kit::accesskit::Role::Status)
                            .w_full()
                            .flex()
                            .justify_center()
                            .child(
                                div()
                                    .w_full()
                                    .max_w(px(880.))
                                    .pl(px(gutter + 8.))
                                    .pr(px(gutter + 20.))
                                    .child(
                                        div()
                                            .w_full()
                                            .h(px(22.))
                                            .my(px(16.))
                                            .flex()
                                            .items_center()
                                            .gap(px(10.))
                                            .text_color(rgb(t.faint))
                                            .child(compaction_rule(
                                                u64::MAX,
                                                "left",
                                                t,
                                                CompactionPaint::default(),
                                            ))
                                            .child(compaction_label(u64::MAX, "compacting"))
                                            .child(compaction_rule(
                                                u64::MAX,
                                                "right",
                                                t,
                                                CompactionPaint::default(),
                                            )),
                                    ),
                            )
                            .into_any_element();
                    }
                    let indicator = {
                        let this = entity.read(cx);
                        (ix == this.rows.len())
                            .then(|| this.working_indicator.clone())
                            .flatten()
                    };
                    if let Some(label) = indicator {
                        return div()
                            .id("tau-working-indicator")
                            .test_support()
                            .w_full()
                            .flex()
                            .justify_center()
                            .child(
                                div()
                                    .w_full()
                                    .max_w(px(880.))
                                    .pl(px(gutter + 8.))
                                    .pr(px(gutter + 20.))
                                    .pb(px(22.))
                                    .flex()
                                    .items_center()
                                    .child(working_spinner(label, t.muted, cx.reduce_motion())),
                            )
                            .into_any_element();
                    }
                    let metadata = entity.update(cx, |this, cx| {
                        let view = this.mount(ix, owner.clone(), cx)?;
                        let row = this.rows.get(ix)?;
                        Some((
                            row.key,
                            row.link_revision,
                            matches!(
                                row.entry.content,
                                TranscriptContent::User(_) | TranscriptContent::PendingUser(_)
                            ),
                            matches!(row.entry.content, TranscriptContent::PendingUser(_)),
                            this.is_compact(ix),
                            match &row.entry.content {
                                TranscriptContent::Notice(notice) => Some(notice.clone()),
                                _ => None,
                            },
                            view,
                        ))
                    });
                    let Some((key, link_revision, user, pending, compact, notice, view)) = metadata else {
                        return div().into_any_element();
                    };
                    let mut row = div()
                        .id(format!("transcript-row-{key}"))
                        .test_support()
                        .w_full()
                        .max_w(px(880.))
                        .pl(px(gutter + if user { -5. } else { 8. }))
                        .pr(px(gutter + 20.))
                        .pb(px(if compact || matches!(&view, View::Boundary { .. }) {
                            0.
                        } else {
                            22.
                        }));
                    match view {
                        View::Boundary {
                            id,
                            available,
                            loading,
                            show_loading,
                        } => {
                            let focus = window
                                .use_keyed_state(format!("compaction-focus-{key}"), cx, |_, cx| {
                                    cx.focus_handle()
                                })
                                .read(cx)
                                .clone();
                            let focus_visible = available
                                && !loading
                                && focus.is_focused(window)
                                && window.last_input_was_keyboard();
                            let paint = CompactionPaint {
                                loading: show_loading,
                                reduced_motion: cx.reduce_motion(),
                                available,
                                focus_visible,
                                pulse: entity
                                    .read(cx)
                                    .compaction_pulse
                                    .filter(|(pulse_key, _)| *pulse_key == key)
                                    .map(|(_, start)| start),
                            };
                            let children = || {
                                vec![
                                    compaction_rule(key, "left", t, paint),
                                    compaction_label(key, "compacted"),
                                    compaction_rule(key, "right", t, paint),
                                ]
                            };
                            let divider = if available {
                                let notify = owner.clone();
                                BaseButton::new(format!("compaction-{key}"))
                                    .track_focus(&focus)
                                    .group("compaction-divider")
                                    .text_color(rgb(t.faint))
                                    .hover(move |button| button.text_color(rgb(t.muted)))
                                    .focus_visible(move |button| button.text_color(rgb(t.muted)))
                                    .opacity(if loading { 0.45 } else { 1. })
                                    .accessibility_label("Load earlier messages")
                                    .disabled(loading)
                                    .a11y_synthetic_children(move |tree| {
                                        if loading {
                                            tree.parent_node().set_busy();
                                        }
                                    })
                                    .w_full()
                                    .h(px(22.))
                                    .my(px(16.))
                                    .gap(px(10.))
                                    .children(children())
                                    .on_click(move |_, _, cx| {
                                        let _ = notify.update(cx, |this, cx| {
                                            let Some(&ix) = this.by_key.get(&key) else {
                                                return;
                                            };
                                            let row = &mut this.rows[ix];
                                            // Recheck live state: another input may beat the next paint.
                                            if row.entry.id != id
                                                || row.history_requested
                                                || !matches!(
                                                    row.entry.content,
                                                    TranscriptContent::Compaction {
                                                        available: true,
                                                        loading: false
                                                    }
                                                )
                                            {
                                                return;
                                            }
                                            row.history_requested = true;
                                            cx.notify();
                                            cx.emit(TranscriptEvent::LoadHistory(id.clone()));
                                        });
                                    })
                                    .into_any_element()
                            } else {
                                // Passive divs retain their implicit role.
                                div()
                                    .id(format!("compaction-{key}"))
                                    .test_support()
                                    .w_full()
                                    .h(px(22.))
                                    .my(px(16.))
                                    .flex()
                                    .items_center()
                                    .gap(px(10.))
                                    .text_color(rgb(t.faint))
                                    .children(children())
                                    .into_any_element()
                            };
                            row = row.child(divider);
                        }
                        View::Activity(activity) => row = row.child(activity),
                        View::Prose(_, presentation) => {
                            let notify = owner.clone();
                            let source_presentation = presentation.clone();
                            let context_owner = notify.clone();
                            let context_presentation = presentation.clone();
                            let prose = MarkdownPresentation::decorate(
                                &presentation,
                                if notice.is_some() { 12. } else { 13. },
                                dark,
                                window,
                                cx,
                            )
                            .when(user, |prose| {
                                prose.style(markdown::text_style_at_size(dark, 13., window)
                                    .with_code_background(rgb(t.panel_raised).into())
                                    .with_table_head(gpui_kit::StyleRefinement::default()
                                        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                        .bg(rgb(t.panel_raised))))
                            })
                            .when(notice.is_some(), |prose| {
                                prose.line_height(gpui_kit::relative(1.5))
                            })
                            .on_link_activate(move |link, event, window, cx| {
                                let Some(target) = markdown::activation_target(link, event) else { return; };
                                let origin = window.focused(cx);
                                let _ = notify.update(cx, |this, cx| {
                                    if this.prose_index(key).is_some_and(|ix| this.rows[ix].link_revision == link_revision) && matches!(this.cache.get(&key), Some(Mounted { view: View::Prose(_, current), .. }) if current == &source_presentation) {
                                        cx.emit(TranscriptEvent::LinkRequested { key, target, origin, anchor: event.position() });
                                    }
                                });
                            })
                            .on_link_context_menu(move |raw, event, window, cx| {
                                let Some(target) = markdown::link_target(raw) else { return; };
                                let handler = context_owner.update(cx, |this, cx| {
                                    if !this.prose_index(key).is_some_and(|ix| this.rows[ix].link_revision == link_revision) || !matches!(this.cache.get(&key), Some(Mounted { view: View::Prose(_, current), .. }) if current == &context_presentation) {
                                        return None;
                                    }
                                    if this.context_handler.is_none() {
                                        cx.emit(TranscriptEvent::LinkContextRequested { key, target: target.clone(), event: event.clone() });
                                    }
                                    this.context_handler.clone()
                                }).ok().flatten();
                                if let Some(handler) = handler { handler(key, target, event, window, cx); }
                            });
                            row = if user {
                                row.flex().child(
                                    div()
                                        .max_w(gpui_kit::relative(0.9))
                                        .px(px(13.))
                                        .py(px(9.))
                                        .rounded(px(7.))
                                        .bg(rgb(if dark { 0x1b1f29 } else { 0xebeef0 }))
                                        .opacity(if pending { 0.88 } else { 1.0 })
                                        .child(prose),
                                )
                            } else if let Some(notice) = notice {
                                row.child(notice::surface(key, &notice, dark, prose))
                            } else {
                                row.child(prose)
                            };
                        }
                    }
                    div()
                        .w_full()
                        .flex()
                        .justify_center()
                        .child(row)
                        .into_any_element()
                })
                .flex_1()
                .min_h_0()
                .pt(px(28.))
                .pb(px(if self.compacting { 0. } else { 30. })),
            )
            .children(prompt_overlay)
            .child(
                Scrollbar::vertical(&self.list)
                    .mode(ScrollbarMode::Scrolling)
                    .styles(|_| support::scrollbar_styles(dark)),
            )
    }
}
fn compaction_label(key: u64, label: &str) -> gpui_kit::AnyElement {
    div()
        .id(format!("compaction-label-{key}"))
        .test_support()
        .min_h(px(22.))
        .px(px(2.))
        .py(px(4.))
        .text_size(px(10.))
        .line_height(gpui_kit::relative(1.25))
        .font_family("Menlo")
        .child(gpui_kit::Text::new(
            format!("compaction-text-{key}").into(),
            label.to_owned().into(),
        ))
        .into_any_element()
}

#[derive(Clone, Copy, Default)]
struct CompactionPaint {
    loading: bool,
    reduced_motion: bool,
    available: bool,
    focus_visible: bool,
    pulse: Option<std::time::Instant>,
}
fn compaction_rule(
    key: u64,
    side: &str,
    t: support::Tokens,
    paint: CompactionPaint,
) -> gpui_kit::AnyElement {
    use gpui_kit::{Animation, AnimationExt, relative};
    let mut border = rgb(if t.canvas == 0xfcfcfc {
        0x6b7d8f
    } else {
        t.border
    });
    border.a = if t.canvas == 0xfcfcfc {
        0.24 * 0.72
    } else {
        0.72
    };
    let mut highlighted = rgb(t.muted);
    highlighted.a = 0.55;
    let rule = div()
        .id(format!("compaction-rule-{side}-{key}"))
        .test_support()
        .relative()
        .flex_1()
        .min_w(px(12.))
        .h(px(1.))
        .overflow_hidden()
        .bg(if paint.focus_visible {
            highlighted
        } else {
            border
        })
        .when(paint.available, |rule| {
            rule.group_hover("compaction-divider", move |rule| rule.bg(highlighted))
        });
    let trail = move || {
        div()
            .absolute()
            .top_0()
            .h_full()
            .w(relative(0.3))
            .bg(rgb(t.muted))
            .opacity(0.7)
    };
    if let Some(start) = paint.pulse {
        if paint.reduced_motion {
            return rule
                .child(if side == "left" {
                    trail().right_0().opacity(1.)
                } else {
                    trail().left_0().opacity(1.)
                })
                .into_any_element();
        }
        let left = side == "left";
        return rule
            .with_animation(
                "compaction-pulse",
                Animation::new(std::time::Duration::from_millis(650)),
                move |rule, _| {
                    let progress = (start.elapsed().as_secs_f32() / 0.65).clamp(0., 1.);
                    let eased = css_ease(progress, 0., 0., 0.58, 1.);
                    let offset = if left {
                        1.03 - eased * 1.65
                    } else {
                        -0.33 + eased * 1.65
                    };
                    let opacity = if progress < 0.25 {
                        css_ease(progress / 0.25, 0., 0., 0.58, 1.) * 0.8
                    } else {
                        (1. - css_ease((progress - 0.25) / 0.75, 0., 0., 0.58, 1.)) * 0.8
                    };
                    rule.child(trail().left(relative(offset)).opacity(opacity))
                },
            )
            .into_any_element();
    }
    if !paint.loading {
        rule.into_any_element()
    } else if paint.reduced_motion {
        // No Animation wrapper: reduced motion must not schedule animation frames.
        rule.child(trail().left_0()).into_any_element()
    } else {
        rule.with_animation(
            "compaction-loading",
            Animation::new(std::time::Duration::from_millis(850)).repeat(),
            move |rule, progress| {
                let eased = css_ease(progress, 0.42, 0., 0.58, 1.);
                rule.child(trail().left(relative(-0.33 + eased * 1.65)))
            },
        )
        .into_any_element()
    }
}

/// CSS cubic-bezier: solve the time axis before evaluating the value axis.
fn css_ease(progress: f32, x1: f32, y1: f32, x2: f32, y2: f32) -> f32 {
    let bezier = |t: f32, a: f32, b: f32| {
        3. * (1. - t).powi(2) * t * a + 3. * (1. - t) * t.powi(2) * b + t.powi(3)
    };
    let (mut low, mut high) = (0., 1.);
    for _ in 0..16 {
        let t = (low + high) / 2.;
        if bezier(t, x1, x2) < progress {
            low = t;
        } else {
            high = t;
        }
    }
    bezier((low + high) / 2., y1, y2)
}

pub(crate) fn working_spinner(label: String, color: u32, reduced_motion: bool) -> impl IntoElement {
    use gpui_kit::{Animation, AnimationExt};
    div()
        .id("working-spinner")
        .test_support()
        .role(gpui_kit::accesskit::Role::Status)
        .aria_label(label)
        .relative()
        .size(px(8.))
        .with_animation(
            "working-trail",
            Animation::new(std::time::Duration::from_millis(640))
                .repeat()
                .with_max_fps(12.5),
            move |spinner, progress| {
                let step = (progress * 8.) as usize % 8;
                let opacity = [1., 0.72, 0.5, 0.34, 0.24, 0.17, 0.12, 0.08];
                let positions = [
                    (0., 0.),
                    (3., 0.),
                    (6., 0.),
                    (6., 3.),
                    (6., 6.),
                    (3., 6.),
                    (0., 6.),
                    (0., 3.),
                ];
                spinner.children(positions.into_iter().enumerate().map(|(ix, (x, y))| {
                    div()
                        .absolute()
                        .left(px(x))
                        .top(px(y))
                        .size(px(2.))
                        .bg(rgb(color))
                        .opacity(if reduced_motion {
                            0.45
                        } else {
                            opacity[(step + 7 - ix) % 8]
                        })
                }))
            },
        )
}

fn markdown_prefix(source: &str, end: usize, inline: bool) -> Option<String> {
    TextViewState::copy_markdown_slice_with_extensions(
        source,
        None,
        Some(end),
        SelectionFormat::Plain,
        &markdown::extensions().inline_only(inline),
    )
    .ok()
}
fn same_prose_kind(a: &TranscriptContent, b: &TranscriptContent) -> bool {
    a.inline_only() == b.inline_only()
        && matches!(
            (a, b),
            (
                TranscriptContent::User(_) | TranscriptContent::PendingUser(_),
                TranscriptContent::User(_) | TranscriptContent::PendingUser(_)
            ) | (
                TranscriptContent::Assistant(_),
                TranscriptContent::Assistant(_)
            ) | (TranscriptContent::Notice(_), TranscriptContent::Notice(_))
        )
}
fn same_content(a: &TranscriptContent, b: &TranscriptContent) -> bool {
    match (a, b) {
        (TranscriptContent::User(a), TranscriptContent::User(b))
        | (TranscriptContent::PendingUser(a), TranscriptContent::PendingUser(b))
        | (TranscriptContent::Assistant(a), TranscriptContent::Assistant(b)) => a == b,
        (
            TranscriptContent::Compaction {
                available: a,
                loading: al,
            },
            TranscriptContent::Compaction {
                available: b,
                loading: bl,
            },
        ) => (a, al) == (b, bl),
        (TranscriptContent::Notice(a), TranscriptContent::Notice(b)) => a == b,
        (TranscriptContent::Activity(a), TranscriptContent::Activity(b)) => match (a, b) {
            (ActivityContent::Thinking { text: a }, ActivityContent::Thinking { text: b }) => {
                a == b
            }
            (
                ActivityContent::Skill {
                    name: a,
                    prompt: ap,
                    text: at,
                },
                ActivityContent::Skill {
                    name: b,
                    prompt: bp,
                    text: bt,
                },
            ) => (a, ap, at) == (b, bp, bt),
            (
                ActivityContent::Tool {
                    name: a,
                    argument: aa,
                    arguments: aas,
                    result: ar,
                    running: au,
                    failed: af,
                },
                ActivityContent::Tool {
                    name: b,
                    argument: ba,
                    arguments: bas,
                    result: br,
                    running: bu,
                    failed: bf,
                },
            ) => (a, aa, aas, ar, au, af) == (b, ba, bas, br, bu, bf),
            _ => false,
        },
        _ => false,
    }
}
#[cfg(test)]
#[path = "activity_transcript_selection_tests.rs"]
mod activity_transcript_selection_tests;
#[cfg(test)]
#[path = "activity_transcript_wheel_tests.rs"]
mod activity_transcript_wheel_tests;
#[cfg(test)]
#[path = "transcript/integration_tests.rs"]
mod integration_tests;
#[cfg(test)]
#[path = "transcript/notice_tests.rs"]
pub(crate) mod notice_tests;
#[cfg(test)]
#[path = "renderer_transcript_tests.rs"]
pub(crate) mod renderer_transcript_tests;
#[cfg(test)]
#[path = "transcript/tests.rs"]
pub(crate) mod tests;
