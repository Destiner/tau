//! Session-local transcript. Sources outlive the bounded presentation cache.
use super::{
    activity::{ActivityContent, ActivityEvent, ActivityRow},
    markdown::{self, MarkdownPresentation},
    support,
};
use gpui_kit::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, FollowMode, IntoElement,
    ListAlignment, ListState, Render, Subscription, TestSupportExt, WeakEntity, Window,
    base::{
        Scrollbar, ScrollbarMode, SelectionFormat, TextSelectionDomain, TextSelectionDomainRange,
        TextViewState,
    },
    div, list,
    prelude::*,
    px, rgb, rgba, size,
};
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Clone, Debug)]
pub enum TranscriptContent {
    User(String),
    Assistant(String),
    Activity(ActivityContent),
}
impl TranscriptContent {
    fn is_activity(&self) -> bool {
        matches!(self, Self::Activity(_))
    }
    fn text(&self) -> Option<&str> {
        match self {
            Self::User(text) | Self::Assistant(text) => Some(text),
            Self::Activity(_) => None,
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
}
struct Row {
    entry: TranscriptEntry,
    key: u64,
    expanded: bool,
}
#[derive(Clone)]
enum View {
    Prose(Entity<TextViewState>, Entity<MarkdownPresentation>),
    Activity(Entity<ActivityRow>),
}
struct Mounted {
    view: View,
    _subscriptions: Vec<Subscription>,
}
const CACHE_LIMIT: usize = 128;

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
        }
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
            }
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
            expanded: false,
        });
        self.list.splice(ix..ix, 1);
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
        if same_content(&self.rows[ix].entry.content, &content) {
            return true;
        }
        let key = self.rows[ix].key;
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
                    changed.insert(row.key);
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
                row.entry = entry;
                row
            } else {
                Row {
                    entry,
                    key: self.allocate_key(),
                    expanded: false,
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
            self.list.reset(0);
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
        // Temporarily borrow the row separately without cloning historical source.
        let key = self.rows[ix].key;
        let bound = self.endpoint_bound(key);
        let before =
            bound.map(|end| markdown_prefix(self.rows[ix].entry.content.text().unwrap(), end));
        let text = match &mut self.rows[ix].entry.content {
            TranscriptContent::User(t) | TranscriptContent::Assistant(t) => t,
            _ => unreachable!(),
        };
        text.push_str(chunk);
        if let (Some(end), Some(before)) = (bound, before)
            && (before.is_none() || markdown_prefix(text, end) != before)
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
            let before = markdown_prefix(text, end);
            let mut after = text.to_owned();
            after.push_str(chunk);
            if before.is_none() || markdown_prefix(&after, end) != before {
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
                &markdown::extensions(),
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
        let row = self.rows.get(ix)?;
        let key = row.key;
        if !self.cache.contains_key(&key) {
            let mut subscriptions = vec![];
            let view = if let Some(source) = row.entry.content.text() {
                let domain = self.selection.clone();
                let text = cx.new(|cx| {
                    let mut text =
                        TextViewState::markdown_with_extensions(source, markdown::extensions(), cx);
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
                    if row.expanded {
                        activity.toggle(cx);
                    }
                    activity
                });
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
                            cx.notify();
                        }
                    });
                }));
                subscriptions.push(
                    cx.subscribe(&activity, move |_, event: &ActivityEvent, cx| {
                        let ActivityEvent::OpenLink(url) = event;
                        let _ = owner.update(cx, |this, cx| {
                            if this.by_key.contains_key(&key) {
                                cx.emit(TranscriptEvent::OpenLink(url.clone()));
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
        div()
            .id(self.id.clone())
            .track_focus(&self.focus)
            .relative()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(
                list(self.list.clone(), move |ix, window, cx| {
                    let Some(entity) = owner.upgrade() else {
                        return div().into_any_element();
                    };
                    let metadata = entity.update(cx, |this, cx| {
                        let view = this.mount(ix, owner.clone(), cx)?;
                        let row = this.rows.get(ix)?;
                        Some((
                            row.key,
                            matches!(row.entry.content, TranscriptContent::User(_)),
                            this.is_compact(ix),
                            view,
                        ))
                    });
                    let Some((key, user, compact, view)) = metadata else {
                        return div().into_any_element();
                    };
                    let mut row = div()
                        .id(format!("transcript-row-{key}"))
                        .test_support()
                        .w_full()
                        .max_w(px(880.))
                        .pl(px(gutter + if user { -5. } else { 8. }))
                        .pr(px(gutter + 20.))
                        .pb(px(if compact { 0. } else { 22. }));
                    match view {
                        View::Activity(activity) => row = row.child(activity),
                        View::Prose(_, presentation) => {
                            let notify = owner.clone();
                            let prose = MarkdownPresentation::decorate(
                                &presentation,
                                13.,
                                dark,
                                window,
                                cx,
                            )
                            .on_link_click(move |url, _, _, cx| {
                                let _ = notify.update(cx, |this, cx| {
                                    if this.by_key.contains_key(&key) {
                                        cx.emit(TranscriptEvent::OpenLink(url.to_string()));
                                    }
                                });
                            });
                            row = if user {
                                row.flex().child(
                                    div()
                                        .max_w(gpui_kit::relative(0.9))
                                        .px(px(13.))
                                        .py(px(9.))
                                        .rounded(px(7.))
                                        .bg(rgb(if dark { 0x1b1f29 } else { 0xebeef0 }))
                                        .child(prose),
                                )
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
                .pb(px(30.)),
            )
            .child(
                Scrollbar::vertical(&self.list)
                    .mode(ScrollbarMode::Scrolling)
                    .styles(|style| {
                        style
                            .thumb(|thumb| thumb.bg(rgba((t.text << 8) | 0x59)).radius(px(4.)))
                            .thumb_hover(|thumb| {
                                thumb.bg(rgba((t.text << 8) | 0x8c)).radius(px(4.))
                            })
                            .thumb_active(|thumb| {
                                thumb.bg(rgba((t.text << 8) | 0x8c)).radius(px(4.))
                            })
                    }),
            )
    }
}
fn markdown_prefix(source: &str, end: usize) -> Option<String> {
    TextViewState::copy_markdown_slice_with_extensions(
        source,
        None,
        Some(end),
        SelectionFormat::Plain,
        &markdown::extensions(),
    )
    .ok()
}
fn same_prose_kind(a: &TranscriptContent, b: &TranscriptContent) -> bool {
    matches!(
        (a, b),
        (TranscriptContent::User(_), TranscriptContent::User(_))
            | (
                TranscriptContent::Assistant(_),
                TranscriptContent::Assistant(_)
            )
    )
}
fn same_content(a: &TranscriptContent, b: &TranscriptContent) -> bool {
    match (a, b) {
        (TranscriptContent::User(a), TranscriptContent::User(b))
        | (TranscriptContent::Assistant(a), TranscriptContent::Assistant(b)) => a == b,
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
#[path = "transcript/tests.rs"]
pub(crate) mod tests;
