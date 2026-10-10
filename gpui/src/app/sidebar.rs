//! Workspace presentation only; Pi remains the owner of identity and history.
use super::*;
use chrono::{DateTime, Datelike, Local, TimeZone};
use components::chrome::{ArchivedDetails, SessionDetails, SessionStatus};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct SessionIdentity {
    project: String,
    pi_id: String,
    // Empty Pi identities are distinct until materialization, never one shared draft.
    ephemeral: String,
}
impl SessionIdentity {
    fn live(runtime: &str, live: &LiveSession) -> Self {
        let id = &live.controller.state().identity.id;
        Self {
            project: live.project.path.clone(),
            pi_id: if id.is_empty() {
                live.source_id.clone()
            } else {
                id.clone()
            },
            ephemeral: if id.is_empty() && live.source_id.is_empty() {
                runtime.into()
            } else {
                String::new()
            },
        }
    }
    fn saved(project: &str, id: &str) -> Self {
        Self {
            project: project.into(),
            pi_id: id.into(),
            ephemeral: String::new(),
        }
    }
}
#[derive(Default)]
pub(super) struct State {
    unread: HashSet<SessionIdentity>,
    last_user: HashMap<SessionIdentity, u64>,
    runtime_identity: HashMap<String, SessionIdentity>,
    archive_source: Vec<ArchiveMetadata>,
    archive_rows: Vec<SessionItem>,
    archive_details: BTreeMap<String, ArchivedDetails>,
    archive_day: Option<chrono::NaiveDate>,
    theme: Option<(gpui_kit::EntityId, bool)>,
    projection_dirty: bool,
    metadata_dirty: bool,
    rendered: bool,
    rendered_archive: bool,
    minute: Option<u64>,
    creation_sequence: u64,
    created: HashMap<String, u64>,
    #[cfg(test)]
    projection_builds: usize,
    #[cfg(test)]
    metadata_builds: usize,
    #[cfg(test)]
    archive_rebuilds: usize,
}
impl State {
    /// Draft/status/read/lifecycle changes do not invalidate archive metadata.
    pub(super) fn invalidate_projection(&mut self) {
        self.projection_dirty = true;
    }
    /// Workspace, registration, identity, title, archive or activity-time changes.
    pub(super) fn invalidate_metadata(&mut self) {
        self.metadata_dirty = true;
        self.invalidate_projection();
    }
    /// Call once after insertion, never on selection or runtime restart.
    pub(super) fn runtime_created(&mut self, runtime: &str) {
        self.creation_sequence += 1;
        self.created.insert(runtime.into(), self.creation_sequence);
        self.invalidate_metadata();
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct ArchiveMetadata {
    id: String,
    title: String,
    project: String,
    model: String,
    sort_at: u64,
    last_user: u64,
    activity_at: u64,
    fallback_time: String,
}

/// One project/Pi index per projection, not a live-runtime scan per saved row.
fn runtime_index(sessions: &BTreeMap<String, LiveSession>) -> HashMap<(&str, &str), &str> {
    let mut index = HashMap::with_capacity(sessions.len() * 2);
    for (key, live) in sessions {
        let project = live.project.path.as_str();
        for id in [
            live.source_id.as_str(),
            live.controller.state().identity.id.as_str(),
        ] {
            if !id.is_empty() {
                index.entry((project, id)).or_insert(key.as_str());
            }
        }
    }
    index
}
fn indicator(dialog: bool, working: bool, draft: &str, unread: bool) -> Option<SessionStatus> {
    if dialog {
        Some(SessionStatus::Unread)
    } else if working {
        Some(SessionStatus::Working)
    } else if !draft.trim().is_empty() {
        Some(SessionStatus::Draft)
    } else if unread {
        Some(SessionStatus::Unread)
    } else {
        None
    }
}
fn status_label(status: Option<SessionStatus>) -> &'static str {
    match status {
        Some(SessionStatus::Unread) => "Unread",
        Some(SessionStatus::Working) => "Working",
        Some(SessionStatus::Draft) => "Draft",
        None => "Idle",
    }
}
fn relative_time(timestamp: u64, fallback: &str, now: u64) -> String {
    if timestamp == 0 {
        return fallback.into();
    }
    let seconds = now.saturating_sub(timestamp) / 1000;
    let (unit, divisor) = match seconds {
        0..=59 => return "now".into(),
        60..=3599 => ('m', 60),
        3600..=86399 => ('h', 3600),
        86400..=604799 => ('d', 86400),
        604800..=31535999 => ('w', 604800),
        _ => ('y', 31536000),
    };
    format!("{}{unit}", seconds / divisor)
}
fn tooltip_markdown(source: &str) -> String {
    if source.trim().is_empty() {
        "New Session".into()
    } else {
        source.chars().take(240).collect()
    }
}
fn group_at<T: TimeZone>(timestamp: u64, now: DateTime<T>) -> &'static str {
    if timestamp == 0 {
        return "Older";
    }
    let Some(date) = i64::try_from(timestamp)
        .ok()
        .and_then(|ts| now.timezone().timestamp_millis_opt(ts).single())
        .map(|date| date.date_naive())
    else {
        return "Older";
    };
    let today = now.date_naive();
    let days = today.signed_duration_since(date).num_days();
    if days <= 0 {
        return "Today";
    }
    if days == 1 {
        return "Yesterday";
    }
    let monday = |day: chrono::NaiveDate| {
        day - chrono::Duration::days(day.weekday().num_days_from_monday() as i64)
    };
    let weeks = monday(today).signed_duration_since(monday(date)).num_days() / 7;
    if weeks <= 0 {
        "This Week"
    } else if weeks == 1 {
        "Last Week"
    } else {
        "Older"
    }
}
fn archive_rows(source: &[ArchiveMetadata], now: u64) -> Vec<SessionItem> {
    let mut ordered: Vec<_> = source.iter().collect();
    // Vue's stable global archive order, with last-user timestamp as tie breaker.
    ordered.sort_by(|a, b| {
        b.sort_at
            .cmp(&a.sort_at)
            .then_with(|| b.last_user.cmp(&a.last_user))
    });
    ordered
        .into_iter()
        .map(|m| SessionItem {
            id: m.id.clone(),
            title: m.title.clone(),
            time: relative_time(m.activity_at, &m.fallback_time, now),
            status: None,
            unread: false,
            can_archive: false,
        })
        .collect()
}
impl Shell {
    fn archive_metadata(&self, index: &HashMap<(&str, &str), &str>) -> Vec<ArchiveMetadata> {
        let mut source = Vec::new();
        if let Some(workspace) = &self.workspace {
            for project in &workspace.projects {
                for session in project.sessions.iter().filter(|s| s.archived) {
                    let key = index
                        .get(&(project.path.as_str(), session.id.as_str()))
                        .copied();
                    let live_name = key
                        .and_then(|key| self.sessions.get(key))
                        .map(|live| live.controller.state().identity.name.as_str())
                        .filter(|s| !s.is_empty());
                    source.push(ArchiveMetadata {
                        id: key.unwrap_or(&session.id).into(),
                        title: live_name.unwrap_or(&session.title).into(),
                        project: project.name.clone(),
                        model: session.model.clone(),
                        sort_at: session.sort_at,
                        last_user: session.last_user_message_at,
                        activity_at: session.sort_at.max(session.last_user_message_at).max(
                            self.sidebar_state
                                .last_user
                                .get(
                                    &key.and_then(|key| {
                                        self.sessions
                                            .get(key)
                                            .map(|live| SessionIdentity::live(key, live))
                                    })
                                    .unwrap_or_else(|| {
                                        SessionIdentity::saved(&project.path, &session.id)
                                    }),
                                )
                                .copied()
                                .unwrap_or_default(),
                        ),
                        fallback_time: session.last_active.clone(),
                    });
                }
            }
        }
        source
    }
    pub(super) fn project_archived_items(&self) -> Vec<SessionItem> {
        archive_rows(
            &self.archive_metadata(&runtime_index(&self.sessions)),
            Local::now().timestamp_millis().max(0) as u64,
        )
    }
    /// Call only for generation-validated RPC ingress, BEFORE Controller::ingest.
    /// Do not infer completion from working=false (bootstrap, failure and abort also do that).
    pub(super) fn observe_sidebar_event(
        &mut self,
        runtime: &str,
        generation: u64,
        event: &serde_json::Value,
    ) {
        let Some(live) = self.sessions.get(runtime) else {
            return;
        };
        if live.starting || live.controller.generation() != generation {
            return;
        }
        let binding = SessionIdentity::live(runtime, live);
        self.sidebar_state
            .runtime_identity
            .entry(runtime.into())
            .or_insert_with(|| binding.clone());
        let kind = event["type"].as_str().unwrap_or_default();
        if runtime != self.active
            && (kind == "agent_settled"
                || (kind == "message_update"
                    && event["assistantMessageEvent"]["type"] == "text_delta")
                || (kind == "compaction_end"
                    && event["errorMessage"]
                        .as_str()
                        .is_some_and(|s| !s.is_empty())))
            && self.sidebar_state.unread.insert(binding.clone())
        {
            self.sidebar_state.invalidate_projection();
        }
        if kind == "message_start" && event["message"]["role"] == "user" {
            let value = &event["message"]["timestamp"];
            let timestamp = value.as_u64().or_else(|| {
                value
                    .as_str()
                    .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                    .map(|date| date.timestamp_millis().max(0) as u64)
            });
            if let Some(timestamp) = timestamp {
                let previous = self.sidebar_state.last_user.entry(binding).or_default();
                if timestamp > *previous {
                    *previous = timestamp;
                    self.sidebar_state.invalidate_metadata();
                }
            }
        }
    }
    /// Invoke after a successful selection, including create_live's selection path.
    pub(super) fn mark_sidebar_read(&mut self, runtime: &str) {
        if let Some(live) = self.sessions.get(runtime) {
            let binding = SessionIdentity::live(runtime, live);
            if self.sidebar_state.unread.remove(&binding) {
                self.sidebar_state.invalidate_projection();
            }
            if let Some(previous) = self.sidebar_state.runtime_identity.get(runtime)
                && !previous.ephemeral.is_empty()
                && previous.project == binding.project
                && self.sidebar_state.unread.remove(previous)
            {
                self.sidebar_state.invalidate_projection();
            }
        }
    }
    pub(super) fn handle_read_state(&mut self, action: &Action) -> bool {
        let Action::SetUnread {
            project_id,
            session_id,
            unread,
        } = action
        else {
            return false;
        };
        let binding = self
            .sessions
            .get(session_id)
            .filter(|live| &live.project.path == project_id)
            .map(|live| SessionIdentity::live(session_id, live))
            .unwrap_or_else(|| SessionIdentity::saved(project_id, session_id));
        if *unread {
            self.sidebar_state.unread.insert(binding);
        } else {
            self.sidebar_state.unread.remove(&binding);
        }
        self.sidebar_state.invalidate_projection();
        true
    }
    fn sidebar_projection(
        &self,
        now_ms: u64,
        index: &HashMap<(&str, &str), &str>,
    ) -> (Vec<ProjectItem>, BTreeMap<String, SessionDetails>) {
        let mut by_project: HashMap<&str, Vec<(&String, &LiveSession)>> = HashMap::new();
        for (key, live) in &self.sessions {
            by_project
                .entry(&live.project.path)
                .or_default()
                .push((key, live));
        }
        let dormant: HashMap<_, _> = self
            .preferences
            .drafts
            .iter()
            .map(|draft| {
                (
                    (draft.project.as_str(), draft.source_id.as_str()),
                    draft.text.as_str(),
                )
            })
            .collect();
        let mut projects = Vec::new();
        let mut details = BTreeMap::new();
        if let Some(workspace) = &self.workspace {
            for project in &workspace.projects {
                let mut known = HashSet::with_capacity(project.sessions.len());
                let mut rows = Vec::new();
                for session in &project.sessions {
                    known.insert(session.id.as_str());
                    let key = index
                        .get(&(project.path.as_str(), session.id.as_str()))
                        .copied();
                    let live = key.and_then(|key| self.sessions.get(key));
                    let binding = live
                        .map(|live| SessionIdentity::live(key.unwrap(), live))
                        .unwrap_or_else(|| SessionIdentity::saved(&project.path, &session.id));
                    let unread = self.sidebar_state.unread.contains(&binding);
                    let state = live.map(|live| live.controller.state());
                    let working = state.is_some_and(|s| s.working);
                    let draft = state
                        .map(|s| s.draft.as_str())
                        .or_else(|| {
                            dormant
                                .get(&(project.path.as_str(), session.id.as_str()))
                                .copied()
                        })
                        .unwrap_or_default();
                    let status = indicator(
                        state.is_some_and(|s| !s.extension_dialogs.is_empty()),
                        working,
                        draft,
                        unread,
                    );
                    let timestamp = session.sort_at.max(session.last_user_message_at).max(
                        self.sidebar_state
                            .last_user
                            .get(&binding)
                            .copied()
                            .unwrap_or_default(),
                    );
                    let time = if working && !session.archived {
                        String::new()
                    } else {
                        relative_time(timestamp, &session.last_active, now_ms)
                    };
                    let name = state
                        .map(|s| s.identity.name.as_str())
                        .filter(|s| !s.is_empty())
                        .unwrap_or(&session.title);
                    let id = key.unwrap_or(&session.id).to_owned();
                    // Metadata is title Markdown, never a tool-output-derived fake last message.
                    details.insert(
                        id.clone(),
                        SessionDetails {
                            status: if session.archived {
                                "Archived"
                            } else {
                                status_label(status)
                            }
                            .into(),
                            message: tooltip_markdown(
                                session.title_markdown.as_deref().unwrap_or(name),
                            ),
                            relative_time: (!time.is_empty()).then_some(time.clone()),
                        },
                    );
                    if !session.archived {
                        rows.push((
                            false,
                            timestamp,
                            SessionItem {
                                id,
                                title: name.into(),
                                time,
                                status,
                                unread,
                                can_archive: self.lifecycle.is_none()
                                    && live.is_none_or(|live| {
                                        !live.starting
                                            && !live.controller.state().working
                                            && !live.controller.state().stopping
                                    }),
                            },
                        ));
                    }
                }
                for &(key, live) in by_project.get(project.path.as_str()).into_iter().flatten() {
                    if known.contains(live.source_id.as_str())
                        || known.contains(live.controller.state().identity.id.as_str())
                    {
                        continue;
                    }
                    let state = live.controller.state();
                    let binding = SessionIdentity::live(key, live);
                    let unread = self.sidebar_state.unread.contains(&binding);
                    let status = indicator(
                        !state.extension_dialogs.is_empty(),
                        state.working,
                        &state.draft,
                        unread,
                    );
                    let name = if state.identity.name.is_empty() {
                        tau_session::session::normalize_session_name(&state.draft)
                    } else {
                        state.identity.name.clone()
                    };
                    let name = if name.is_empty() {
                        "New Session".into()
                    } else {
                        name
                    };
                    details.insert(
                        key.clone(),
                        SessionDetails {
                            status: status_label(status).into(),
                            message: tooltip_markdown(&name),
                            relative_time: None,
                        },
                    );
                    rows.push((
                        true,
                        self.sidebar_state
                            .created
                            .get(key)
                            .copied()
                            .unwrap_or_default(),
                        SessionItem {
                            id: key.clone(),
                            title: name,
                            time: String::new(),
                            status,
                            unread,
                            can_archive: false,
                        },
                    ));
                }
                rows.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
                projects.push(ProjectItem {
                    id: project.path.clone(),
                    title: project.name.clone(),
                    location: project
                        .connection_string
                        .as_ref()
                        .filter(|s| !s.is_empty())
                        .map(|connection| format!("{connection} · {}", project.working_directory))
                        .unwrap_or_else(|| project.working_directory.clone()),
                    collapsed: project.collapsed,
                    sessions: rows.into_iter().map(|(_, _, row)| row).collect(),
                });
            }
        }
        (projects, details)
    }
    /// Parent's timer must refresh the window when this becomes true, even while idle.
    pub(super) fn sidebar_clock_due(&self) -> bool {
        let now = Local::now();
        self.sidebar_state.rendered
            && (self.sidebar_state.minute != Some(now.timestamp_millis().max(0) as u64 / 60000)
                || self.sidebar_state.archive_day != Some(now.date_naive()))
    }
    pub(super) fn sync_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_sidebar_at(Local::now(), window, cx);
    }
    fn sync_sidebar_at(
        &mut self,
        now: DateTime<Local>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let sidebar = self.sidebar.clone();
        let now_ms = now.timestamp_millis().max(0) as u64;
        let entity_changed =
            self.sidebar_state.theme.map(|t| t.0) != sidebar.as_ref().map(|s| s.entity_id());
        let metadata_changed = !self.sidebar_state.rendered || self.sidebar_state.metadata_dirty;
        let minute_changed = self.sidebar_state.minute != Some(now_ms / 60000);
        let day_changed = self.sidebar_state.archive_day != Some(now.date_naive());
        let mode_changed = !self.sidebar_state.rendered
            || self.sidebar_state.rendered_archive != self.session_actions.archived;
        let projection_changed = metadata_changed
            || minute_changed
            || entity_changed
            || (!self.session_actions.archived
                && (self.sidebar_state.projection_dirty || mode_changed));
        let archive_changed = metadata_changed || minute_changed || day_changed || entity_changed;
        // Identity reconciliation is a metadata boundary, never a stream fingerprint.
        if metadata_changed {
            for (runtime, live) in &self.sessions {
                let binding = SessionIdentity::live(runtime, live);
                if let Some(previous) = self
                    .sidebar_state
                    .runtime_identity
                    .insert(runtime.clone(), binding.clone())
                    && previous != binding
                    && previous.project == binding.project
                    && !previous.ephemeral.is_empty()
                    && !binding.pi_id.is_empty()
                {
                    if self.sidebar_state.unread.remove(&previous) {
                        self.sidebar_state.unread.insert(binding.clone());
                    }
                    if let Some(timestamp) = self.sidebar_state.last_user.remove(&previous) {
                        self.sidebar_state.last_user.insert(binding, timestamp);
                    }
                }
            }
            self.sidebar_state
                .runtime_identity
                .retain(|runtime, _| self.sessions.contains_key(runtime));
            self.sidebar_state
                .created
                .retain(|runtime, _| self.sessions.contains_key(runtime));
        }
        let index = (projection_changed || metadata_changed).then(|| runtime_index(&self.sessions));
        let projection =
            projection_changed.then(|| self.sidebar_projection(now_ms, index.as_ref().unwrap()));
        let source = metadata_changed.then(|| self.archive_metadata(index.as_ref().unwrap()));
        let state = &mut self.sidebar_state;
        #[cfg(test)]
        {
            state.projection_builds += usize::from(projection_changed);
            state.metadata_builds += usize::from(metadata_changed);
        }
        if let Some(source) = source {
            state.archive_source = source;
        }
        if metadata_changed || day_changed {
            let source = &state.archive_source;
            if metadata_changed {
                state.archive_rows = archive_rows(source, now_ms);
            }
            state.archive_details = source
                .iter()
                .map(|m| {
                    (
                        m.id.clone(),
                        ArchivedDetails {
                            group: group_at(m.activity_at, now).into(),
                            project: m.project.clone(),
                            model: (!m.model.is_empty()).then_some(m.model.clone()),
                        },
                    )
                })
                .collect();
            state.archive_day = Some(now.date_naive());
            #[cfg(test)]
            {
                state.archive_rebuilds += 1;
            }
        }
        if minute_changed && !metadata_changed {
            // Time copy ages without sorting/rebuilding archive rows or metadata.
            let times: HashMap<_, _> = state
                .archive_source
                .iter()
                .map(|m| {
                    (
                        m.id.as_str(),
                        relative_time(m.activity_at, &m.fallback_time, now_ms),
                    )
                })
                .collect();
            for row in &mut state.archive_rows {
                if let Some(time) = times.get(row.id.as_str()) {
                    row.time.clone_from(time);
                }
            }
        }
        let theme = sidebar.as_ref().map(|s| (s.entity_id(), self.dark));
        let theme_changed = state.theme != theme;
        state.theme = theme;
        state.rendered = true;
        state.rendered_archive = self.session_actions.archived;
        state.minute = Some(now_ms / 60000);
        if projection_changed {
            state.projection_dirty = false;
        }
        state.metadata_dirty = false;
        if let Some(sidebar) = sidebar {
            sidebar.update(cx, |sidebar, cx| {
                if mode_changed || (self.session_actions.archived && archive_changed) {
                    let mut props = sidebar_props();
                    props.active = self.active.clone();
                    props.archived = self.session_actions.archived;
                    if props.archived {
                        props.sessions = state.archive_rows.clone();
                    }
                    sidebar.set_props(props, cx);
                } else {
                    sidebar.set_active_session(self.active.clone(), cx);
                }
                if let Some((projects, details)) = projection {
                    sidebar.set_projects(projects, cx);
                    sidebar.set_session_details(details, cx);
                }
                if metadata_changed || day_changed || entity_changed {
                    sidebar.set_archived_details(state.archive_details.clone(), cx);
                }
                if theme_changed {
                    sidebar.set_theme(self.dark, window, cx);
                }
            });
        }
    }
}

#[cfg(test)]
mod tests;
