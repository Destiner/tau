//! Vue command identities, projected from the production shell, not fixture props.
use super::*;
use crate::components::palette::{Palette, PaletteEvent, PaletteItem, PalettePage};
use gpui_kit::{FocusHandle, Focusable, KeyBinding};

gpui_kit::actions!(
    tau_palette,
    [
        Toggle,
        SwitchSession,
        SwitchProject,
        ChooseModel,
        ChooseEffort,
        OpenLocal,
        OpenRemote,
        Rename,
        Archive,
        ShowArchived,
        FocusComposer,
        StopPi,
        MoveProjectUp,
        MoveProjectDown,
        OpenProject,
        MarkUnread,
        FocusSidebar,
        FocusTranscript,
        ReviewUnsent,
        LoadHistory,
        ReconnectRemote,
        NextSession,
        PreviousSession
    ]
);

#[derive(Default)]
pub(super) struct State {
    pub(super) view: Option<Entity<Palette>>,
    subscription: Option<Subscription>,
    origin: Option<FocusHandle>,
    binding: Option<(String, u64, String, String)>,
    targets: BTreeMap<String, Target>,
}
impl State {
    pub(super) fn is_open(&self, cx: &App) -> bool {
        self.view.as_ref().is_some_and(|v| v.read(cx).is_open())
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
enum Target {
    New(String),
    Local,
    Remote,
    Select(String, String, String),
    Live(String, u64, String, String),
    Project(String, Option<Box<Target>>),
    Archive(String),
    Restore(String),
    Archived,
    Rename,
    Collapse(String),
    Reorder(Vec<String>),
    Composer,
    Send,
    FollowUp,
    Stop,
    ClearQueue,
    ReadState(String, String, bool),
    Sidebar,
    Transcript,
    Review,
    History(String),
    Reconnect,
    Adjacent(Box<Target>),
    Remove(String),
    Updates,
    RetryWorkflows,
    Quit,
    ReportIssue,
    DisableAdmin,
    Model(String, String),
    Effort(String),
}

impl Shell {
    pub(super) fn mount_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.bind_keys([
            KeyBinding::new("secondary-k", Toggle, Some("TauShell")),
            KeyBinding::new("secondary-o", OpenProject, Some("TauShell")),
            KeyBinding::new("secondary-shift-u", MarkUnread, Some("TauShell")),
            KeyBinding::new("secondary-shift-l", FocusSidebar, Some("TauShell")),
            KeyBinding::new("secondary-shift-t", FocusTranscript, Some("TauShell")),
            KeyBinding::new("secondary-shift-d", ReviewUnsent, Some("TauShell")),
            KeyBinding::new("secondary-shift-b", LoadHistory, Some("TauShell")),
            KeyBinding::new("secondary-shift-c", ReconnectRemote, Some("TauShell")),
            KeyBinding::new("ctrl-tab", NextSession, Some("TauShell")),
            KeyBinding::new("ctrl-shift-tab", PreviousSession, Some("TauShell")),
            KeyBinding::new("secondary-p", SwitchSession, Some("TauShell")),
            KeyBinding::new("secondary-shift-p", SwitchProject, Some("TauShell")),
            KeyBinding::new("secondary-shift-m", ChooseModel, Some("TauShell")),
            KeyBinding::new("secondary-shift-e", ChooseEffort, Some("TauShell")),
            KeyBinding::new("secondary-shift-o", OpenLocal, Some("TauShell")),
            KeyBinding::new("secondary-alt-o", OpenRemote, Some("TauShell")),
            KeyBinding::new("secondary-shift-r", Rename, Some("TauShell")),
            KeyBinding::new("secondary-shift-a", Archive, Some("TauShell")),
            KeyBinding::new("secondary-shift-h", ShowArchived, Some("TauShell")),
            KeyBinding::new("secondary-l", FocusComposer, Some("TauShell")),
            KeyBinding::new("secondary-.", StopPi, Some("TauShell")),
            KeyBinding::new("secondary-alt-up", MoveProjectUp, Some("TauShell")),
            KeyBinding::new("secondary-alt-down", MoveProjectDown, Some("TauShell")),
        ]);
        // InputState and subscriptions belong to this window. Never retain them on reopen.
        self.palette = State::default();
        let view = cx.new(|cx| Palette::new(self.dark, vec![], window, cx));
        self.palette.subscription =
            Some(cx.subscribe_in(&view, window, |s, view, event, w, cx| {
                if s.palette.view.as_ref() != Some(view) {
                    return;
                }
                match event {
                    PaletteEvent::Dismissed => {
                        if view.read(cx).restore_focus_on_dismiss() {
                            s.palette_restore_focus(w, cx);
                        } else {
                            s.palette.origin = None;
                            s.finish_header_palette_focus(cx);
                            s.sync(w, cx);
                        }
                    }
                    PaletteEvent::Selected(id) => s.palette_selected(id, w, cx),
                    PaletteEvent::Back => {}
                }
            }));
        self.palette.view = Some(view);
        self.sync_palette(window, cx);
    }
    pub(super) fn palette_keymap(element: gpui_kit::Div, cx: &mut Context<Self>) -> gpui_kit::Div {
        element
            .on_action(
                cx.listener(|s, _: &OpenProject, w, cx| s.palette_command("project.open", w, cx)),
            )
            .on_action(cx.listener(|s, _: &MarkUnread, w, cx| {
                let id = if s.palette_session_unread(&s.active) {
                    "session.markRead"
                } else {
                    "session.markUnread"
                };
                s.palette_command(id, w, cx);
            }))
            .on_action(
                cx.listener(|s, _: &FocusSidebar, w, cx| s.palette_command("focus.sidebar", w, cx)),
            )
            .on_action(cx.listener(|s, _: &FocusTranscript, w, cx| {
                s.palette_command("focus.transcript", w, cx)
            }))
            .on_action(
                cx.listener(|s, _: &ReviewUnsent, w, cx| s.palette_command("queue.review", w, cx)),
            )
            .on_action(
                cx.listener(|s, _: &LoadHistory, w, cx| s.palette_command("history.load", w, cx)),
            )
            .on_action(cx.listener(|s, _: &ReconnectRemote, w, cx| {
                s.palette_command("remote.reconnect", w, cx)
            }))
            .on_action(
                cx.listener(|s, _: &NextSession, w, cx| s.palette_command("session.next", w, cx)),
            )
            .on_action(cx.listener(|s, _: &PreviousSession, w, cx| {
                s.palette_command("session.previous", w, cx)
            }))
            .on_action(cx.listener(|s, _: &Toggle, w, cx| s.toggle_palette(w, cx)))
            .on_action(
                cx.listener(|s, _: &SwitchSession, w, cx| {
                    s.palette_command("session.switch", w, cx)
                }),
            )
            .on_action(
                cx.listener(|s, _: &SwitchProject, w, cx| {
                    s.palette_command("project.switch", w, cx)
                }),
            )
            .on_action(
                cx.listener(|s, _: &ChooseModel, w, cx| s.palette_command("model.choose", w, cx)),
            )
            .on_action(
                cx.listener(|s, _: &ChooseEffort, w, cx| s.palette_command("effort.choose", w, cx)),
            )
            .on_action(
                cx.listener(|s, _: &OpenLocal, w, cx| {
                    s.palette_command("project.openLocal", w, cx)
                }),
            )
            .on_action(cx.listener(|s, _: &OpenRemote, w, cx| {
                s.palette_command("project.openRemote", w, cx)
            }))
            .on_action(
                cx.listener(|s, _: &Rename, w, cx| s.palette_command("session.rename", w, cx)),
            )
            .on_action(cx.listener(|s, _: &Archive, w, cx| {
                let id = if s
                    .saved_session(&s.active)
                    .is_some_and(|(_, session, _)| session.archived)
                {
                    "session.unarchive"
                } else {
                    "session.archive"
                };
                s.palette_command(id, w, cx);
            }))
            .on_action(cx.listener(|s, _: &ShowArchived, w, cx| {
                s.palette_command("sidebar.toggleArchived", w, cx)
            }))
            .on_action(
                cx.listener(|s, _: &FocusComposer, w, cx| {
                    s.palette_command("focus.composer", w, cx)
                }),
            )
            .on_action(
                cx.listener(|s, _: &StopPi, w, cx| s.palette_command("composer.stop", w, cx)),
            )
            .on_action(
                cx.listener(|s, _: &MoveProjectUp, w, cx| {
                    s.palette_command("project.moveUp", w, cx)
                }),
            )
            .on_action(cx.listener(|s, _: &MoveProjectDown, w, cx| {
                s.palette_command("project.moveDown", w, cx)
            }))
    }
    fn palette_command(&mut self, id: &str, w: &mut Window, cx: &mut Context<Self>) {
        if self.palette_blocked(cx) {
            return;
        }
        if !self.palette.is_open(cx) {
            self.palette.binding = self.palette_binding();
            self.palette.origin = w.focused(cx);
        }
        self.sync_palette(w, cx);
        let (pages, _) = self.palette_pages(cx);
        let Some(row) = pages[0].items.iter().find(|row| row.id == id) else {
            return;
        };
        if let Some(page) = &row.destination {
            self.show_palette_page(page, w, cx);
        } else {
            self.palette_selected(id, w, cx);
        }
    }
    fn palette_modal_blocked(&self, cx: &App) -> bool {
        self.lifecycle.is_some()
            || self.confirmation.is_some()
            || self.error.is_some()
            || self.fallback_is_open(cx)
            || self.preview_open()
            || self.picker_busy
            || self.remote.is_open()
    }
    fn palette_blocked(&self, cx: &App) -> bool {
        !self.loaded || self.palette_modal_blocked(cx)
    }
    fn palette_editing_blocked(&self) -> bool {
        self.sessions.get(&self.active).is_some_and(|live| {
            live.extension.is_some() || !live.controller.state().extension_dialogs.is_empty()
        })
    }
    pub(super) fn palette_binding(&self) -> Option<(String, u64, String, String)> {
        let Some(l) = self.sessions.get(&self.active) else {
            return self
                .selected_project()
                .map(|p| (String::new(), 0, p.path, String::new()));
        };
        Some((
            self.active.clone(),
            l.controller.generation(),
            l.controller.state().identity.id.clone(),
            l.controller.state().identity.path.clone(),
        ))
    }
    pub(super) fn toggle_palette(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        if self.palette.is_open(cx) {
            if let Some(v) = self.palette.view.clone() {
                v.update(cx, |p, cx| p.dismiss(cx));
            }
            return;
        }
        self.show_palette_page("root", w, cx);
    }
    pub(super) fn show_palette_page(&mut self, page: &str, w: &mut Window, cx: &mut Context<Self>) {
        if self.palette_blocked(cx) {
            self.finish_header_palette_focus(cx);
            return;
        }
        if self.palette.view.is_none() {
            self.mount_palette(w, cx);
        }
        if !self.palette.is_open(cx) {
            self.palette.origin = w.focused(cx);
            self.palette.binding = self.palette_binding();
        }
        self.sync_palette(w, cx);
        if let Some(header) = self.header.clone() {
            header.update(cx, |h, _| h.prepare_palette_open());
        }
        if let Some(v) = self.palette.view.clone() {
            v.update(cx, |p, cx| p.open_page(page, w, cx));
        }
        self.sync_admin(w, cx);
        cx.notify();
    }
    pub(super) fn sync_palette(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        let (pages, targets) = self.palette_pages(cx);
        self.palette.targets = targets;
        if let Some(v) = self.palette.view.clone() {
            let blocked = self.palette_modal_blocked(cx);
            v.update(cx, |p, cx| {
                p.set_theme(self.dark, w, cx);
                p.refresh_pages(pages, w, cx);
                // A new modal owns focus; dismissal must not focus behind it.
                if blocked && p.is_open() {
                    p.dismiss(cx);
                }
            });
        }
    }
    fn finish_header_palette_focus(&mut self, cx: &mut Context<Self>) {
        if let Some(header) = self.header.clone() {
            header.update(cx, |h, _| h.finish_palette_focus());
        }
    }
    fn palette_restore_focus(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        if !self.palette.is_open(cx) {
            self.finish_header_palette_focus(cx);
        }
        let origin = self.palette.origin.take();
        // Closing changes composer admission even when no Pi event follows.
        self.sync(w, cx);
        if self.admin_reporter_open(cx) {
            self.open_admin_reporter(w, cx);
            return;
        }
        if self.palette_modal_blocked(cx) || self.palette.is_open(cx) {
            return;
        }
        // Validate origin membership after painting: even a currently mounted
        // composer can leave the dispatch tree when this command disables it.
        let origin = (self.palette.binding == self.palette_binding())
            .then_some(origin)
            .flatten();
        if let Some(focus) = self.shell_focus.clone() {
            focus.focus(w, cx);
        }
        self.restore_input_focus_from(origin, cx);
    }
    fn palette_selected(&mut self, id: &str, w: &mut Window, cx: &mut Context<Self>) {
        let Some(target) = self.palette.targets.get(id).cloned() else {
            self.palette_restore_focus(w, cx);
            return;
        };
        // Recheck admission and exact captured identity at execution, not just rendering.
        let (_, current) = self.palette_pages(cx);
        if self.palette_blocked(cx) || current.get(id) != Some(&target) {
            self.palette_restore_focus(w, cx);
            return;
        }
        // Close without emitting Dismissed: focus handoff belongs to this dispatch.
        if let Some(view) = self.palette.view.clone() {
            view.update(cx, |palette, cx| palette.close(cx));
        }
        self.finish_header_palette_focus(cx);
        let changes_context = matches!(
            target,
            Target::Live(..)
                | Target::Select(..)
                | Target::Project(..)
                | Target::New(..)
                | Target::Archive(..)
                | Target::Adjacent(..)
                | Target::Remove(..)
        );
        let owns_focus = matches!(
            target,
            Target::Local
                | Target::Remote
                | Target::Rename
                | Target::Composer
                | Target::Sidebar
                | Target::Transcript
                | Target::Review
                | Target::Updates
                | Target::ReportIssue
                | Target::Quit
        );
        let mut created_session = false;
        match target {
            Target::Live(key, generation, id, path) => {
                if self.sessions.get(&key).is_some_and(|l| {
                    l.controller.generation() == generation
                        && l.controller.state().identity.id == id
                        && l.controller.state().identity.path == path
                }) {
                    self.select(&key, w, cx);
                }
            }
            Target::Select(project, session, path) => {
                let valid = self.workspace.as_ref().is_some_and(|ws| {
                    ws.projects.iter().any(|p| {
                        p.path == project
                            && p.sessions
                                .iter()
                                .any(|s| s.id == session && s.path == path && !s.archived)
                    })
                });
                if valid {
                    self.action(Action::Select(session), w, cx);
                }
            }
            Target::Project(path, destination) => {
                if let Some(destination) = destination {
                    self.palette_navigate(*destination, w, cx);
                } else {
                    self.new_session(Some(path), w, cx);
                    created_session = true;
                }
            }
            Target::Local => self.action(Action::OpenLocalProject, w, cx),
            Target::Remote => self.action(Action::OpenRemoteProject, w, cx),
            Target::Archived => self.action(Action::ToggleArchived, w, cx),
            Target::Quit => cx.dispatch_action(&crate::native::Quit),
            Target::ReportIssue => self.open_admin_reporter(w, cx),
            Target::DisableAdmin => {
                let command = self.admin.set_mode(false);
                self.dispatch_admin(command, cx);
                self.sync_admin(w, cx);
            }
            target => {
                if self.palette.binding != self.palette_binding() {
                    self.palette_restore_focus(w, cx);
                    return;
                }
                match target {
                    Target::Adjacent(target) => self.palette_navigate(*target, w, cx),
                    Target::Remove(path) => self.remove_project(&path, cx),
                    Target::ReadState(project_id, session_id, unread) => {
                        self.handle_read_state(&Action::SetUnread {
                            project_id,
                            session_id,
                            unread,
                        });
                        self.sync(w, cx);
                    }
                    Target::Sidebar => {
                        self.sync(w, cx);
                        if let Some(sidebar) = self.sidebar.clone() {
                            sidebar.update(cx, |sidebar, cx| sidebar.focus_selected_row(w, cx));
                        }
                    }
                    Target::Transcript => {
                        self.sync(w, cx);
                        if let Some(live) = self.sessions.get(&self.active) {
                            let transcript = live.transcript.read(cx);
                            let mounted = !self.active_session_loading()
                                && (!transcript.is_empty()
                                    || transcript.working_indicator().is_some()
                                    || !live.controller.state().extension_dialogs.is_empty()
                                    || live.controller.state().compacting);
                            if mounted {
                                transcript.focus_handle(cx).focus(w, cx);
                            } else if let Some(focus) = &self.shell_focus {
                                // An empty/loading transcript has no node to route the next shortcut.
                                focus.focus(w, cx);
                            }
                        }
                    }
                    Target::Review => {
                        self.sync(w, cx);
                        if let Some(live) = self.sessions.get(&self.active) {
                            live.queue
                                .clone()
                                .update(cx, |queue, cx| queue.open_review(w, cx));
                        }
                    }
                    Target::History(marker) => {
                        let key = self.active.clone();
                        if let Some(live) = self.sessions.get_mut(&key) {
                            let effects = live.controller.load_history(&marker);
                            live.dirty = true;
                            live.projection_dirty = true;
                            self.effects(&key, effects, cx);
                            self.refresh_window(cx);
                        }
                    }
                    Target::Reconnect => self.retry_pi(&self.active.clone(), cx),
                    Target::Updates => self.action(Action::CheckUpdates, w, cx),
                    Target::RetryWorkflows => self.retry_workflows(cx),
                    Target::New(p) => {
                        self.new_session(Some(p), w, cx);
                        created_session = true;
                    }
                    Target::Archive(id) => self.action(Action::Archive(id), w, cx),
                    Target::Restore(id) => self.action(Action::Restore(id), w, cx),
                    Target::Rename => {
                        if let Some(h) = self.header.clone() {
                            h.update(cx, |h, cx| h.begin_rename(w, cx));
                        }
                    }
                    Target::Collapse(p) => self.action(Action::ProjectToggle(p), w, cx),
                    Target::Reorder(p) => self.action(Action::ReorderProjects(p), w, cx),
                    Target::Composer => {
                        self.sync(w, cx);
                        self.action(Action::FocusComposer, w, cx);
                    }
                    Target::Send | Target::FollowUp => {
                        let key = self.active.clone();
                        let draft = self.sessions[&key].controller.state().draft.clone();
                        let event = if target == Target::FollowUp {
                            ComposerEvent::FollowUp(draft)
                        } else {
                            ComposerEvent::Send(draft)
                        };
                        self.composer_event(&key, &event, cx);
                    }
                    Target::Stop => {
                        let key = self.active.clone();
                        self.composer_event(&key, &ComposerEvent::Stop, cx);
                    }
                    Target::ClearQueue => {
                        let key = self.active.clone();
                        let live = self.sessions.get_mut(&key).unwrap();
                        let effects = live.controller.clear_queue();
                        live.dirty = true;
                        live.projection_dirty = true;
                        self.effects(&key, effects, cx);
                        self.refresh_window(cx);
                    }
                    Target::Model(provider, model) => {
                        let key = self.active.clone();
                        self.composer_event(
                            &key,
                            &ComposerEvent::ModelChanged(format!("{provider}\u{1f}{model}")),
                            cx,
                        );
                    }
                    Target::Effort(level) => {
                        let key = self.active.clone();
                        self.composer_event(&key, &ComposerEvent::EffortChanged(level), cx);
                    }
                    _ => {}
                }
            }
        }
        if changes_context {
            // Select first: the old composer is about to leave the focus tree.
            // The shell restores once the destination has rendered, including
            // its extension dialog or disabled-composer fallback.
            self.palette.origin = None;
            if !created_session {
                // Keep shortcuts routed while the destination editor awaits paint.
                if let Some(focus) = self.shell_focus.clone() {
                    focus.focus(w, cx);
                }
                self.restore_input_focus(cx);
            }
        } else if owns_focus {
            self.palette.origin = None;
        } else {
            self.palette_restore_focus(w, cx);
        }
    }
    fn palette_project(&self) -> Option<ProjectSummary> {
        let selected = self.selected_project()?;
        self.workspace
            .as_ref()
            .and_then(|ws| ws.projects.iter().find(|p| p.path == selected.path))
            .cloned()
            .or(Some(selected))
    }
    fn palette_project_destination(&self, project: &ProjectSummary, cx: &App) -> Option<Target> {
        if let Some(live) = self.sessions.get(&self.active).filter(|live| {
            live.project.path == project.path
                && !self
                    .saved_session(&self.active)
                    .is_some_and(|(_, session, _)| session.archived)
        }) {
            let identity = &live.controller.state().identity;
            return Some(Target::Live(
                self.active.clone(),
                live.controller.generation(),
                identity.id.clone(),
                identity.path.clone(),
            ));
        }
        let projects = self.palette_sidebar_projects();
        let displayed = self
            .sidebar
            .as_ref()
            .and_then(|sidebar| sidebar.read(cx).displayed_sessions(&project.path));
        let rows = displayed
            .as_ref()
            .unwrap_or(&projects.iter().find(|p| p.id == project.path)?.sessions);
        let selected = project
            .sessions
            .iter()
            .find(|s| s.selected && !s.archived)
            .and_then(|s| rows.iter().find(|r| r.id == s.id));
        self.palette_row_target(project, &selected.or_else(|| rows.first())?.id)
    }
    fn palette_adjacent(&self, direction: i32, cx: &App) -> Option<Target> {
        let project = self.palette_project()?;
        let live = self.sessions.get(&self.active)?;
        let identity = &live.controller.state().identity.id;
        let current = if identity.is_empty() {
            if live.source_id.is_empty() {
                &self.active
            } else {
                &live.source_id
            }
        } else {
            identity
        };
        let projects = self.palette_sidebar_projects();
        let displayed = self
            .sidebar
            .as_ref()
            .and_then(|sidebar| sidebar.read(cx).displayed_sessions(&project.path));
        let rows = displayed
            .as_ref()
            .unwrap_or(&projects.iter().find(|p| p.id == project.path)?.sessions);
        let index = rows.iter().position(|s| &s.id == current)?;
        let next = if direction > 0 {
            index.checked_add(1)?
        } else {
            index.checked_sub(1)?
        };
        self.palette_row_target(&project, &rows.get(next)?.id)
    }
    fn palette_row_target(&self, project: &ProjectSummary, id: &str) -> Option<Target> {
        if let Some((key, live)) = self.sessions.iter().find(|(key, live)| {
            live.project.path == project.path
                && (live.controller.state().identity.id == id
                    || (live.controller.state().identity.id.is_empty()
                        && (live.source_id == id
                            || (live.source_id.is_empty() && key.as_str() == id))))
        }) {
            let identity = &live.controller.state().identity;
            Some(Target::Live(
                key.clone(),
                live.controller.generation(),
                identity.id.clone(),
                identity.path.clone(),
            ))
        } else {
            let saved = project
                .sessions
                .iter()
                .find(|s| s.id == id && !s.archived)?;
            Some(Target::Select(
                project.path.clone(),
                saved.id.clone(),
                saved.path.clone(),
            ))
        }
    }
    fn palette_navigate(&mut self, target: Target, w: &mut Window, cx: &mut Context<Self>) {
        match target {
            Target::Live(key, generation, id, path)
                if self.sessions.get(&key).is_some_and(|live| {
                    live.controller.generation() == generation
                        && live.controller.state().identity.id == id
                        && live.controller.state().identity.path == path
                }) =>
            {
                self.select(&key, w, cx)
            }
            Target::Select(project, id, path)
                if self.workspace.as_ref().is_some_and(|ws| {
                    ws.projects.iter().any(|p| {
                        p.path == project
                            && p.sessions
                                .iter()
                                .any(|s| s.id == id && s.path == path && !s.archived)
                    })
                }) =>
            {
                self.action(Action::Select(id), w, cx)
            }
            _ => {}
        }
    }
    fn palette_pages(&self, cx: &App) -> (Vec<PalettePage>, BTreeMap<String, Target>) {
        let mut pages: Vec<_> = [
            ("root", "Search commands…"),
            ("sessions", "Search sessions…"),
            ("projects", "Search projects…"),
            ("models", "Search models…"),
            ("efforts", "Choose thinking effort…"),
            ("open-project", "Open project…"),
        ]
        .into_iter()
        .map(|(id, placeholder)| PalettePage {
            id: id.into(),
            placeholder: placeholder.into(),
            items: vec![],
        })
        .collect();
        let mut targets = BTreeMap::new();
        if self.palette_blocked(cx) {
            return (pages, targets);
        }
        let mut add = |page: usize,
                       id: String,
                       label: String,
                       detail: String,
                       shortcut: &str,
                       group: Option<String>,
                       target: Option<Target>,
                       destination: Option<&str>| {
            if !self.project_actions_admitted(cx)
                && !matches!(id.as_str(), "app.quit" | "app.retryWorkflows")
            {
                return;
            }
            if self.palette_editing_blocked()
                && !matches!(page, 1 | 2)
                && !matches!(
                    id.as_str(),
                    "session.new"
                        | "session.switch"
                        | "project.switch"
                        | "session.next"
                        | "session.previous"
                )
            {
                return;
            }
            if let Some(target) = target {
                targets.insert(id.clone(), target);
            }
            pages[page].items.push(PaletteItem {
                id,
                label,
                detail,
                shortcut: shortcut.into(),
                group,
                destination: destination.map(str::to_owned),
            });
        };
        if let Some(ws) = &self.workspace {
            if !ws.projects.is_empty() {
                add(
                    0,
                    "session.switch".into(),
                    "Switch Session…".into(),
                    "".into(),
                    "⌘P",
                    None,
                    None,
                    Some("sessions"),
                );
                add(
                    0,
                    "project.switch".into(),
                    "Switch Project…".into(),
                    "".into(),
                    "⌘⇧P",
                    None,
                    None,
                    Some("projects"),
                );
            }
            for p in &ws.projects {
                add(
                    2,
                    p.path.clone(),
                    p.name.clone(),
                    p.connection_string
                        .clone()
                        .unwrap_or(p.working_directory.clone()),
                    "",
                    None,
                    Some(Target::Project(
                        p.path.clone(),
                        self.palette_project_destination(p, cx).map(Box::new),
                    )),
                    None,
                );
                for (key, live) in self
                    .sessions
                    .iter()
                    .filter(|(_, l)| l.project.path == p.path)
                {
                    let saved = self.saved_session(key);
                    if saved.as_ref().is_some_and(|(_, s, _)| s.archived) {
                        continue;
                    }
                    let identity = &live.controller.state().identity;
                    let target = Target::Live(
                        key.clone(),
                        live.controller.generation(),
                        identity.id.clone(),
                        identity.path.clone(),
                    );
                    add(
                        1,
                        format!("live:{target:?}"),
                        if identity.name.is_empty() {
                            saved
                                .map(|(_, s, _)| s.title)
                                .unwrap_or_else(|| "New Session".into())
                        } else {
                            identity.name.clone()
                        },
                        if key == &self.active {
                            "Current".into()
                        } else {
                            String::new()
                        },
                        "",
                        Some(p.name.clone()),
                        Some(target),
                        None,
                    );
                }
                for s in p.sessions.iter().filter(|s| !s.archived) {
                    if self.sessions.values().any(|l| {
                        l.project.path == p.path
                            && (l.source_id == s.id || l.controller.state().identity.id == s.id)
                    }) {
                        continue;
                    }
                    let id = format!("session:{:?}", (&p.path, &s.id, &s.path));
                    let current =
                        self.saved_session(&self.active)
                            .is_some_and(|(project, session, _)| {
                                project.path == p.path && session.id == s.id
                            });
                    add(
                        1,
                        id,
                        s.title.clone(),
                        if current {
                            "Current".into()
                        } else {
                            s.last_active.clone()
                        },
                        "",
                        Some(p.name.clone()),
                        Some(Target::Select(p.path.clone(), s.id.clone(), s.path.clone())),
                        None,
                    );
                }
            }
        }
        add(
            0,
            "project.open".into(),
            "Open Project".into(),
            "".into(),
            "⌘O",
            None,
            None,
            Some("open-project"),
        );
        add(
            5,
            "open-project:local".into(),
            "Open Local Project".into(),
            "".into(),
            "⌘⇧O",
            None,
            Some(Target::Local),
            None,
        );
        add(
            5,
            "open-project:remote".into(),
            "Open Remote Project".into(),
            "".into(),
            "⌘⌥O",
            None,
            Some(Target::Remote),
            None,
        );
        let mut root = |id: &str, label: &str, shortcut: &str, target: Target| {
            add(
                0,
                id.into(),
                label.into(),
                "".into(),
                shortcut,
                None,
                Some(target),
                None,
            )
        };
        root(
            "project.openLocal",
            "Open Local Project",
            "⌘⇧O",
            Target::Local,
        );
        root(
            "project.openRemote",
            "Open Remote Project",
            "⌘⌥O",
            Target::Remote,
        );
        root("app.quit", "Quit Tau", "⌘Q", Target::Quit);
        if self.retry_workflow_pending() {
            root(
                "app.retryWorkflows",
                "Retry Pending Operations",
                "",
                Target::RetryWorkflows,
            );
        }
        if self.sidebar.is_some() {
            root("focus.sidebar", "Focus Sidebar", "⌘⇧L", Target::Sidebar);
            root("app.updates", "Show Update Status", "", Target::Updates);
        }
        if self.admin.enabled() {
            root("app.reportIssue", "Report Issue", "", Target::ReportIssue);
            root(
                "app.disableAdminMode",
                "Disable Admin Mode",
                "",
                Target::DisableAdmin,
            );
        }
        root(
            "sidebar.toggleArchived",
            if self.session_actions.archived {
                "Show Active Sessions"
            } else {
                "Show Archived Sessions"
            },
            "⌘⇧H",
            Target::Archived,
        );
        if let Some(p) = self.palette_project() {
            root(
                "session.new",
                "New Session",
                "⌘N",
                Target::New(p.path.clone()),
            );
            if self.remove_project_available(&p.path) {
                root(
                    "project.remove",
                    "Remove Project",
                    "",
                    Target::Remove(p.path.clone()),
                );
            }
            for (id, label, shortcut, direction) in [
                ("session.next", "Next Session", "Ctrl Tab", 1),
                ("session.previous", "Previous Session", "Ctrl ⇧Tab", -1),
            ] {
                if let Some(target) = self.palette_adjacent(direction, cx) {
                    root(id, label, shortcut, Target::Adjacent(Box::new(target)));
                }
            }
            root(
                if p.collapsed {
                    "project.expand"
                } else {
                    "project.collapse"
                },
                if p.collapsed {
                    "Expand Project"
                } else {
                    "Collapse Project"
                },
                "",
                Target::Collapse(p.path.clone()),
            );
            if let Some(ws) = &self.workspace
                && let Some(index) = ws
                    .projects
                    .iter()
                    .position(|project| project.path == p.path)
            {
                for (id, label, neighbor, shortcut) in [
                    (
                        "project.moveUp",
                        "Move Project Up",
                        index.checked_sub(1),
                        "⌘⌥↑",
                    ),
                    (
                        "project.moveDown",
                        "Move Project Down",
                        (index + 1 < ws.projects.len()).then_some(index + 1),
                        "⌘⌥↓",
                    ),
                ] {
                    if let Some(neighbor) = neighbor {
                        let mut paths: Vec<_> =
                            ws.projects.iter().map(|p| p.path.clone()).collect();
                        paths.swap(index, neighbor);
                        root(id, label, shortcut, Target::Reorder(paths));
                    }
                }
            }
        }
        if self.can_rename() {
            root("session.rename", "Rename Session", "⌘⇧R", Target::Rename);
        }
        if let Some((_, s, _)) = self.saved_session(&self.active) {
            if s.archived {
                root(
                    "session.unarchive",
                    "Unarchive Session",
                    "⌘⇧A",
                    Target::Restore(s.id),
                );
            } else if self.can_archive(&self.active) {
                root(
                    "session.archive",
                    "Archive Session",
                    "⌘⇧A",
                    Target::Archive(s.id),
                );
            }
        }
        if let Some(live) = self.sessions.get(&self.active) {
            let state = live.controller.state();
            let unread = self.palette_session_unread(&self.active);
            root(
                if unread {
                    "session.markRead"
                } else {
                    "session.markUnread"
                },
                if unread {
                    "Mark as Read"
                } else {
                    "Mark as Unread"
                },
                "⌘⇧U",
                Target::ReadState(live.project.path.clone(), self.active.clone(), !unread),
            );
            root(
                "focus.transcript",
                "Focus Transcript",
                "⌘⇧T",
                Target::Transcript,
            );
            // Draft editing is independent of Pi readiness and submit admission.
            if !self.runtime_cleanup_pending(&self.active) {
                root("focus.composer", "Focus Composer", "⌘L", Target::Composer);
            }
            if !live.restored_recovered.is_empty() || !state.recovered_drafts.is_empty() {
                root(
                    "queue.review",
                    "Review Unsent Messages",
                    "⌘⇧D",
                    Target::Review,
                );
            }
            if live.project.connection_string.is_some()
                && !live.starting
                && !state.ready
                && state.status == Status::Error
            {
                root(
                    "remote.reconnect",
                    "Reconnect Remote Session",
                    "⌘⇧C",
                    Target::Reconnect,
                );
            }
            if state.ready
                && !live.starting
                && !state.working
                && !state.stopping
                && !state.compacting
                && let Some(marker) = state
                    .transcript
                    .iter()
                    .find(|r| r.kind == "compaction" && r.history_available && !r.history_loading)
            {
                root(
                    "history.load",
                    "Load Earlier Messages",
                    "⌘⇧B",
                    Target::History(marker.id.clone()),
                );
            }
            if self.palette.binding == self.palette_binding()
                && state.ready
                && !live.starting
                && !state.stopping
                && !state.compacting
                && state.extension_dialogs.is_empty()
            {
                if state.working {
                    root("composer.stop", "Stop Pi", "⌘.", Target::Stop);
                }
                let extension_command = state.draft.trim().strip_prefix('/').is_some_and(|draft| {
                    let name = draft.split(' ').next().unwrap_or_default();
                    state
                        .commands
                        .iter()
                        .any(|command| command["name"] == name && command["source"] == "extension")
                });
                if !live.controller.submission_pending()
                    && (!state.working || !live.controller.queue_busy())
                    && !state.draft.trim().is_empty()
                    && (!state.working || !extension_command)
                {
                    root(
                        "composer.send",
                        if state.working {
                            "Queue Steering Message"
                        } else {
                            "Send Message"
                        },
                        "Enter",
                        Target::Send,
                    );
                    if state.working {
                        root(
                            "composer.followUp",
                            "Queue Follow-up Message",
                            "⌘Enter",
                            Target::FollowUp,
                        );
                    }
                }
            }
            if self.palette.binding == self.palette_binding()
                && state.ready
                && !live.controller.queue_busy()
                && (!state.queue.steering.is_empty() || !state.queue.follow_up.is_empty())
            {
                root(
                    "queue.clear",
                    "Clear Queued Messages",
                    "",
                    Target::ClearQueue,
                );
            }
            if state.ready
                && !live.starting
                && !state.working
                && !state.stopping
                && !state.compacting
                && !live.controller.setting_pending()
                && self.palette.binding == self.palette_binding()
            {
                let models = palette_models(state);
                if !models.is_empty() {
                    add(
                        0,
                        "model.choose".into(),
                        "Choose Model…".into(),
                        "".into(),
                        "⌘⇧M",
                        None,
                        None,
                        Some("models"),
                    );
                }
                for m in models {
                    let (Some(provider), Some(id)) = (m["provider"].as_str(), m["id"].as_str())
                    else {
                        continue;
                    };
                    add(
                        3,
                        format!("model:{provider}/{id}"),
                        m["name"].as_str().unwrap_or(id).into(),
                        id.into(),
                        "",
                        None,
                        Some(Target::Model(provider.into(), id.into())),
                        None,
                    );
                }
                if !state.thinking_levels.is_empty() {
                    add(
                        0,
                        "effort.choose".into(),
                        "Choose Thinking Effort…".into(),
                        "".into(),
                        "⌘⇧E",
                        None,
                        None,
                        Some("efforts"),
                    );
                }
                for effort in &state.thinking_levels {
                    let label = match effort.as_str() {
                        "off" => "Off",
                        "minimal" => "Minimal",
                        "low" => "Low",
                        "medium" => "Medium",
                        "high" => "High",
                        "xhigh" => "Extra High",
                        other => other,
                    };
                    add(
                        4,
                        format!("effort:{effort}"),
                        label.into(),
                        "".into(),
                        "",
                        None,
                        Some(Target::Effort(effort.clone())),
                        None,
                    );
                }
            }
        }
        if let Some((_, session, _)) = self.saved_session(&self.active) {
            for row in &mut pages[0].items {
                if matches!(row.id.as_str(), "session.archive" | "session.unarchive") {
                    row.detail = session.title.clone();
                }
            }
        }
        (pages, targets)
    }
}
fn palette_models(state: &tau_session::session::State) -> Vec<&serde_json::Value> {
    state.selectable_models()
}

#[cfg(test)]
#[path = "palette/rename_tests.rs"]
mod rename_tests;
#[cfg(test)]
#[path = "palette/tests.rs"]
mod tests;
