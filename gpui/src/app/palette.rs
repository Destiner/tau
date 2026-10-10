//! Vue command identities, projected from the production shell, not fixture props.
use super::*;
use crate::components::palette::{Palette, PaletteEvent, PaletteItem, PalettePage};
use gpui_kit::{FocusHandle, KeyBinding};

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
        MoveProjectDown
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
    Project(String),
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
                    PaletteEvent::Dismissed => s.palette_restore_focus(w, cx),
                    PaletteEvent::Selected(id) => s.palette_selected(id, w, cx),
                    PaletteEvent::Back => {}
                }
            }));
        self.palette.view = Some(view);
        self.sync_palette(window, cx);
    }
    pub(super) fn palette_keymap(element: gpui_kit::Div, cx: &mut Context<Self>) -> gpui_kit::Div {
        element
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
        if self.palette_blocked() {
            return;
        }
        if !self.palette.is_open(cx) {
            self.palette.binding = self.palette_binding();
            self.palette.origin = w.focused(cx);
        }
        self.sync_palette(w, cx);
        let (pages, _) = self.palette_pages();
        let Some(row) = pages[0].items.iter().find(|row| row.id == id) else {
            return;
        };
        if let Some(page) = &row.destination {
            self.show_palette_page(page, w, cx);
        } else {
            self.palette_selected(id, w, cx);
        }
    }
    fn palette_modal_blocked(&self) -> bool {
        self.lifecycle.is_some()
            || self.confirmation.is_some()
            || self.error.is_some()
            || self.picker_busy
            || self.remote.is_open()
            || self
                .sessions
                .get(&self.active)
                .is_some_and(|l| l.extension.is_some())
    }
    fn palette_blocked(&self) -> bool {
        !self.loaded || self.palette_modal_blocked()
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
        if self.palette_blocked() {
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
        if let Some(v) = self.palette.view.clone() {
            v.update(cx, |p, cx| p.open_page(page, w, cx));
        }
        self.sync_admin(w, cx);
        cx.notify();
    }
    pub(super) fn sync_palette(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        let (pages, targets) = self.palette_pages();
        self.palette.targets = targets;
        if let Some(v) = self.palette.view.clone() {
            let blocked = self.palette_modal_blocked();
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
    fn palette_restore_focus(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        let origin = self.palette.origin.take();
        self.sync_admin(w, cx);
        if self.admin_reporter_open(cx) {
            self.open_admin_reporter(w, cx);
            return;
        }
        if self.palette_modal_blocked() || self.palette.is_open(cx) {
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
        let (_, current) = self.palette_pages();
        if self.palette_blocked() || current.get(id) != Some(&target) {
            self.palette_restore_focus(w, cx);
            return;
        }
        // Close without emitting Dismissed: focus handoff belongs to this dispatch.
        if let Some(view) = self.palette.view.clone() {
            view.update(cx, |palette, cx| palette.close(cx));
        }
        let changes_context = matches!(
            target,
            Target::Live(..)
                | Target::Select(..)
                | Target::Project(..)
                | Target::New(..)
                | Target::Archive(..)
        );
        let owns_focus = matches!(
            target,
            Target::Local
                | Target::Remote
                | Target::Rename
                | Target::Composer
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
            Target::Project(path) => {
                let session = self
                    .workspace
                    .as_ref()
                    .and_then(|ws| ws.projects.iter().find(|p| p.path == path))
                    .and_then(|p| {
                        p.sessions
                            .iter()
                            .find(|s| s.selected && !s.archived)
                            .or_else(|| p.sessions.iter().find(|s| !s.archived))
                    })
                    .map(|s| s.id.clone());
                if let Some(session) = session {
                    self.select(&session, w, cx);
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
                    Target::Composer => self.action(Action::FocusComposer, w, cx),
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
    fn palette_pages(&self) -> (Vec<PalettePage>, BTreeMap<String, Target>) {
        let mut pages: Vec<_> = [
            ("root", "Search commands…"),
            ("sessions", "Search sessions…"),
            ("projects", "Search projects…"),
            ("models", "Search models…"),
            ("efforts", "Choose thinking effort…"),
        ]
        .into_iter()
        .map(|(id, placeholder)| PalettePage {
            id: id.into(),
            placeholder: placeholder.into(),
            items: vec![],
        })
        .collect();
        let mut targets = BTreeMap::new();
        if self.palette_blocked() {
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
                    Some(Target::Project(p.path.clone())),
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
        if let Some(p) = self.selected_project() {
            root(
                "session.new",
                "New Session",
                "⌘N",
                Target::New(p.path.clone()),
            );
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
            root("focus.composer", "Focus Composer", "⌘L", Target::Composer);
            let state = live.controller.state();
            if self.palette.binding == self.palette_binding()
                && state.ready
                && !live.starting
                && !state.stopping
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
                if !live.controller.queue_busy()
                    && (!state.queue.steering.is_empty() || !state.queue.follow_up.is_empty())
                {
                    root(
                        "queue.clear",
                        "Clear Queued Messages",
                        "",
                        Target::ClearQueue,
                    );
                }
            }
            if state.ready
                && !live.starting
                && !state.working
                && !state.stopping
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
#[path = "palette/tests.rs"]
mod tests;
