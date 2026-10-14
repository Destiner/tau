//! Production native shell. Pi owns execution and history; this entity owns UI lifetime.
#[cfg(feature = "native-os-tests")]
mod native_smoke;
#[cfg(feature = "native-os-tests")]
pub use native_smoke::run_native_smoke;
mod admin;
mod admin_bridge;
#[cfg(test)]
mod command_tests;
mod empty_sessions;
mod events;
mod file_preview;
mod first_run;
#[cfg(test)]
mod history_admission_tests;
mod lifecycle;
mod loading;
mod model_scope;
mod palette;
mod projection;
mod queue_recovery;
mod remote;
#[cfg(test)]
mod renderer_tests;
mod saved_history;
mod service;
mod session_actions;
mod sidebar;
#[cfg(test)]
mod tests;
mod view;
mod workspace_mutations;

use crate::{
    components::{
        self,
        chrome::{Action, Header, ProjectItem, SessionItem, Sidebar, SidebarProps},
        composer::{Choice, Composer, ComposerOptions, Event as ComposerEvent},
        dialogs::{DialogEvent, DialogKind, DialogOption, DialogProps, NativeDialog},
        queue::{MessageQueue, QueueEvent, QueueProps},
        support,
        transcript::{Transcript, TranscriptEvent},
    },
    native::{
        HostCallbacks, Intent, LifecycleCallbacks, NativeHost, Outcome, PathChooser, PathOperation,
        PathOutcome, Request,
    },
};
use gpui_kit::{
    AnyWindowHandle, App, AppContext, AssetSource, Context, Entity, IntoElement, Render,
    Subscription, Window,
    base::{Root, Theme, ThemeAppearance, input::InputEvent},
    div,
    prelude::*,
    px, rgb,
};
use service::{Command, Draft, Event, Preferences};
use std::{
    borrow::Cow,
    collections::{BTreeMap, VecDeque},
    path::PathBuf,
    rc::Rc,
    time::{Duration, Instant},
};
use tau_core::{
    models::{ProjectSummary, WorkspaceSnapshot},
    profile::{self, StorageProfile},
};
use tau_session::session::{Controller, Effect, ExtensionResponse, QueueKind, Status};

struct Assets;
impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui_kit::Result<Option<Cow<'static, [u8]>>> {
        Ok(support::asset(path))
    }
    fn list(&self, _: &str) -> gpui_kit::Result<Vec<gpui_kit::SharedString>> {
        Ok(vec![])
    }
}

struct LiveSession {
    project: ProjectSummary,
    source_id: String,
    source_path: String,
    ephemeral: bool,
    registration_pending: std::collections::BTreeSet<(u64, String, String)>,
    controller: Controller,
    starting: bool,
    start_attempt: u64,
    start_deadline: u64,
    composer: Entity<Composer>,
    startup_events: VecDeque<tau_core::pi::PiEvent>,
    transcript: Entity<Transcript>,
    queue: Entity<MessageQueue>,
    projection: projection::Projection,
    shown_draft: String,
    persisted_draft: String,
    persisted_recovered: usize,
    restored_recovered: Vec<String>,
    dirty: bool,
    projection_dirty: bool,
    ui_status: Option<(bool, bool)>,
    choices: Option<u64>,
    command_choices: Option<u64>,
    model_scope_read: Option<tau_session::session::ModelScopeToken>,
    presented_history: Option<(String, String)>,
    extension: Option<(String, Entity<NativeDialog>)>,
    extension_subscription: Option<Subscription>,
    extension_generation: Option<u64>,
    extension_error: Option<(u64, String, String)>,
    _subscriptions: Vec<Subscription>,
}

pub struct Shell {
    commands: async_channel::Sender<Command>,
    workspace: Option<WorkspaceSnapshot>,
    workspace_mutations: workspace_mutations::State,
    preferences: Preferences,
    sessions: BTreeMap<String, LiveSession>,
    active: String,
    sidebar: Option<Entity<Sidebar>>,
    header: Option<Entity<Header>>,
    window: Option<AnyWindowHandle>,
    shell_focus: Option<gpui_kit::FocusHandle>,
    picker: Option<Rc<PathChooser<Root>>>,
    picker_busy: bool,
    subscriptions: Vec<Subscription>,
    dark: bool,
    error: Option<String>,
    lifecycle: Option<Request>,
    confirmation: Option<Entity<NativeDialog>>,
    quit_error: Option<(Request, String)>,
    fallback: Option<(String, Entity<NativeDialog>)>,
    preview: file_preview::State,
    save_revision: u64,
    exit_revision: Option<(u64, Request)>,
    requested_session: Option<String>,
    epoch: Instant,
    loaded: bool,
    save_failed: bool,
    register_retries: Vec<Command>,
    empty_candidates: std::collections::BTreeSet<String>,
    empty_disposals: std::collections::BTreeSet<(String, u64, u64)>,
    pending_saves: BTreeMap<u64, Preferences>,
    restored: bool,
    session_actions: session_actions::SessionActions,
    palette: palette::State,
    remote: remote::State,
    admin: admin::State,
    sidebar_state: sidebar::State,
    session_loading: loading::State,
    runtime_lifecycle: lifecycle::State,
    saved_history: saved_history::State,
}

pub fn run() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let mut isolated = None;
    let mut project = None;
    let mut session = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--isolated" => {
                isolated =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        anyhow::anyhow!("--isolated requires a directory")
                    })?))
            }
            "--project" => {
                project = Some(
                    args.next()
                        .ok_or_else(|| anyhow::anyhow!("--project requires a directory"))?,
                )
            }
            "--session" => {
                session = Some(
                    args.next()
                        .ok_or_else(|| anyhow::anyhow!("--session requires an ID or path"))?,
                )
            }
            _ => anyhow::bail!(
                "Unknown option; use --isolated <dir>, --project <dir>, --session <id or path>"
            ),
        }
    }
    let profile = match isolated {
        Some(dir) => StorageProfile::isolated(dir.join("tau"), dir.join("pi-sessions")),
        None => {
            StorageProfile::production().map_err(|_| anyhow::anyhow!("Cannot open Tau data"))?
        }
    };
    run_with(profile, project, session, |_, _| {})
}

fn run_with(
    profile: StorageProfile,
    project: Option<String>,
    session: Option<String>,
    on_ready: impl FnOnce(Entity<Shell>, &mut App) + 'static,
) -> anyhow::Result<()> {
    let data = profile.data_dir().to_path_buf();
    profile::initialize(profile).map_err(|_| anyhow::anyhow!("Cannot initialize Tau storage"))?;
    let (commands, events) = service::launch(data);
    gpui_kit::application().with_assets(Assets).run(move |cx| {
        let dark = false;
        components::init(cx, dark);
        let _ = cx
            .text_system()
            .add_fonts(vec![Cow::Borrowed(include_bytes!(
                "../assets/InterVariable.ttf"
            ))]);
        let quit_events = events.clone();
        let shell = cx.new(|cx: &mut Context<Shell>| {
            let shell = Shell {
                commands: commands.clone(),
                workspace: None,
                workspace_mutations: workspace_mutations::State::default(),
                preferences: Preferences::default(),
                sessions: BTreeMap::new(),
                active: String::new(),
                sidebar: None,
                header: None,
                window: None,
                shell_focus: None,
                picker: None,
                picker_busy: false,
                subscriptions: vec![],
                dark,
                error: None,
                lifecycle: None,
                confirmation: None,
                quit_error: None,
                fallback: None,
                preview: file_preview::State::default(),
                save_revision: 0,
                exit_revision: None,
                requested_session: session,
                epoch: Instant::now(),
                loaded: false,
                save_failed: false,
                register_retries: vec![],
                empty_candidates: Default::default(),
                empty_disposals: Default::default(),
                pending_saves: BTreeMap::new(),
                restored: false,
                session_actions: session_actions::SessionActions::default(),
                palette: palette::State::default(),
                remote: remote::State::default(),
                admin: admin::State::default(),
                sidebar_state: sidebar::State::default(),
                session_loading: loading::State::default(),
                runtime_lifecycle: lifecycle::State::default(),
                saved_history: saved_history::State::default(),
            };
            cx.spawn(async move |weak, cx| {
                while let Ok(first) = events.recv().await {
                    let mut batch = vec![first];
                    for _ in 0..63 {
                        match events.try_recv() {
                            Ok(event) => batch.push(event),
                            Err(_) => break,
                        }
                    }
                    let target = weak
                        .update(cx, |shell, cx| {
                            for event in batch {
                                shell.ingest(event, cx);
                            }
                            shell.window
                        })
                        .ok()
                        .flatten();
                    if let Some(target) = target {
                        let _ = target.update(cx, |_, window, cx| {
                            let _ = weak.update(cx, |shell, cx| shell.sync(window, cx));
                        });
                    }
                }
            })
            .detach();
            // Deadlines only: no synthetic messages or progress.
            cx.spawn(async move |weak, cx| {
                loop {
                    cx.background_executor()
                        .timer(Duration::from_millis(250))
                        .await;
                    if weak
                        .update(cx, |shell, cx| {
                            let now = shell.epoch.elapsed().as_millis() as u64;
                            let keys: Vec<_> = shell.sessions.keys().cloned().collect();
                            let mut changed = false;
                            for key in keys {
                                let live = shell.sessions.get_mut(&key).unwrap();
                                let before = (
                                    live.controller.state().status,
                                    live.controller.pending_count(),
                                    live.controller.state().extension_dialogs.len(),
                                );
                                let timed_out = live.starting && now >= live.start_deadline;
                                let cancel_attempt = if timed_out {
                                    let attempt = live.start_attempt;
                                    live.start_attempt += 1;
                                    live.starting = false;
                                    live.startup_events.clear();
                                    live.controller
                                        .transport_failed(live.controller.generation());
                                    Some(attempt)
                                } else {
                                    None
                                };
                                let effects = live.controller.tick(now);
                                let after = (
                                    live.controller.state().status,
                                    live.controller.pending_count(),
                                    live.controller.state().extension_dialogs.len(),
                                );
                                if timed_out || before != after || !effects.is_empty() {
                                    live.dirty = true;
                                    live.projection_dirty = true;
                                    changed = true;
                                }
                                shell.effects(&key, effects, cx);
                                if let Some(attempt) = cancel_attempt {
                                    shell.dispatch(
                                        Command::CancelStart {
                                            runtime: key,
                                            attempt,
                                        },
                                        cx,
                                    );
                                }
                            }
                            let live_count = shell.sessions.len();
                            shell.reconcile_empty_sessions(cx);
                            shell.source_recoveries(cx);
                            shell.release_idle_runtimes(cx);
                            let retired = live_count != shell.sessions.len();
                            if changed {
                                shell.sidebar_state.invalidate_projection();
                                shell.persist(false, cx);
                            }
                            if changed || retired || shell.sidebar_clock_due() {
                                shell.refresh_window(cx);
                            }
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .detach();
            shell
        });
        let factory_shell = shell.clone();
        let request_shell = shell.clone();
        let settled_shell = shell.clone();
        let new_shell = shell.clone();
        let failed_shell = shell.clone();
        let host = NativeHost::install(
            cx,
            "Tau",
            Duration::from_secs(120),
            HostCallbacks {
                create_window: Rc::new(move |cx| open(factory_shell.clone(), cx)),
                reveal_window: Rc::new(|target, cx| {
                    let _ = crate::native::reveal_window(target, cx);
                }),
                window_failed: Rc::new(move |cx| {
                    failed_shell.update(cx, |s, cx| {
                        s.error = Some("The window could not open. Try reopening Tau.".into());
                        cx.notify();
                    })
                }),
                new_session: Rc::new(move |cx| {
                    cx.global::<NativeHost>().clone().reopen(cx);
                    let target = new_shell.read(cx).window;
                    if let Some(target) = target {
                        let _ = target.update(cx, |_, w, cx| {
                            new_shell.update(cx, |s, cx| s.new_session(None, w, cx))
                        });
                    }
                }),
                lifecycle: LifecycleCallbacks {
                    requested: Rc::new(move |request, cx| {
                        #[cfg(feature = "native-os-tests")]
                        eprintln!("OBSERVED native lifecycle callback=requested");
                        let no_windows = cx.global::<NativeHost>().lifecycle().windows().is_empty();
                        request_shell.update(cx, |s, cx| {
                            s.lifecycle = Some(request);
                            s.sidebar_state.invalidate_projection();
                            if no_windows && !s.is_busy() {
                                s.begin_exit(request, true, cx);
                            } else {
                                s.refresh_window(cx);
                            }
                        });
                        if no_windows
                            && request_shell.read(cx).exit_revision.is_none()
                            && let Ok(target) = open(request_shell.clone(), cx)
                        {
                            let lifecycle = cx.global::<NativeHost>().lifecycle();
                            let _ = target.update(cx, |_, window, cx| lifecycle.attach(window, cx));
                            let _ = crate::native::reveal_window(target, cx);
                        }
                    }),
                    settled: Rc::new(move |request, outcome, cx| {
                        #[cfg(feature = "native-os-tests")]
                        eprintln!("OBSERVED native lifecycle callback=settled outcome={outcome:?}");
                        settled_shell.update(cx, |s, cx| {
                            if s.lifecycle == Some(request) {
                                s.lifecycle = None;
                                s.sidebar_state.invalidate_projection();
                                s.confirmation = None;
                                s.quit_error = None;
                                s.exit_revision = None;
                                s.runtime_lifecycle.exit_wait = None;
                                s.runtime_lifecycle.exit_saved = None;
                                s.runtime_lifecycle.exit_fence_pending = None;
                                cx.notify();
                                if outcome != Outcome::Approved {
                                    s.restore_input_focus(cx);
                                }
                            }
                        });
                    }),
                },
            },
        );
        let shutdown = commands.clone();
        cx.on_app_quit(move |_| {
            let shutdown = shutdown.clone();
            // Unblock reader sinks before waiting for worker-owned process cleanup.
            quit_events.close();
            async move {
                let (done, completed) = async_channel::bounded(1);
                if shutdown.send(Command::Shutdown(done)).await.is_ok() {
                    let _ = completed.recv().await;
                }
            }
        })
        .detach();
        host.lifecycle().set_quit_on_last_window_close(true);
        host.open_window(cx);
        let _ = commands.try_send(Command::Load { project });
        cx.activate(true);
        on_ready(shell, cx);
    });
    Ok(())
}

struct WindowSurface {
    shell: Entity<Shell>,
    mounted: bool,
}

impl Render for WindowSurface {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .when(self.mounted, |d| d.child(self.shell.clone()))
    }
}

fn open(shell: Entity<Shell>, cx: &mut App) -> gpui_kit::Result<AnyWindowHandle> {
    let options = gpui_kit::WindowOptions {
        window_bounds: Some(gpui_kit::WindowBounds::Windowed(
            gpui_kit::Bounds::centered(None, gpui_kit::size(px(1100.), px(760.)), cx),
        )),
        window_min_size: Some(gpui_kit::size(px(720.), px(480.))),
        titlebar: Some(gpui_kit::TitlebarOptions {
            title: Some("Tau".into()),
            appears_transparent: true,
            traffic_light_position: Some(gpui_kit::point(px(9.), px(7.))),
        }),
        ..Default::default()
    };
    let mut surface = None;
    let handle: AnyWindowHandle = cx
        .open_window(options, |window, cx| {
            let content = cx.new(|_| WindowSurface {
                shell: shell.clone(),
                mounted: false,
            });
            surface = Some(content.clone());
            cx.new(|cx| {
                let root = Root::new(content, window, cx);
                let chooser = Rc::new(PathChooser::<Root>::new(window, cx));
                shell.update(cx, |s, _| s.picker = Some(chooser));
                root
            })
        })?
        .into();
    handle.update(cx, |_, window, cx| {
        shell.update(cx, |s, cx| {
            s.window = Some(window.window_handle());
            s.mount(window, cx);
        });
        // open_window paints before returning; never paint retained controls
        // until their window-owned entities and subscriptions have been rebuilt.
        surface.as_ref().unwrap().update(cx, |s, cx| {
            s.mounted = true;
            cx.notify();
        });
    })?;
    Ok(handle)
}

impl Shell {
    fn dispatch(&mut self, command: Command, cx: &mut Context<Self>) {
        if !self.empty_session_retry_is_current(&command) {
            return;
        }
        if !self.import_admitted()
            && matches!(
                &command,
                Command::Import(_)
                    | Command::Remote(service::RemoteCommand {
                        operation: service::RemoteOperation::Import,
                        ..
                    })
            )
        {
            if let Command::Remote(command) = command {
                self.remote_failed(command, cx);
            } else {
                self.error = Some("Finish project cleanup before importing again.".into());
            }
            return;
        }
        let command = match command {
            Command::Import(path) => {
                let Some(command) = self.prepare_import(path) else {
                    return;
                };
                command
            }
            command => command,
        };
        self.protect_empty_registration(&command);
        if let Command::CancelStart { runtime, attempt } = &command {
            self.runtime_lifecycle_track_cancel(runtime, *attempt);
        }
        let failed = if (matches!(&command, Command::Start { .. }) && self.lifecycle.is_some())
            || matches!(&command, Command::Send { runtime, .. } if self.runtime_cleanup_pending(runtime))
        {
            Some(command)
        } else {
            self.commands
                .try_send(command)
                .err()
                .map(|failed| failed.into_inner())
        };
        if let Some(failed) = failed {
            self.sidebar_state.invalidate_projection();
            if self.lifecycle_dispatch_failed(&failed, cx) {
                self.refresh_window(cx);
                return;
            }
            match failed {
                command @ Command::DisposeEmpty { .. } => {
                    let previous = self.error.clone();
                    self.empty_disposal_dispatch_failed(command, cx);
                    if let Some(request) = self.lifecycle {
                        if let Some(error) = self.error.take() {
                            self.quit_error = Some((request, error));
                        }
                        self.error = previous;
                    }
                    self.refresh_window(cx);
                    return;
                }
                Command::Admin(command) => {
                    self.admin_failed(command, cx);
                    return;
                }
                Command::Remote(command) => {
                    self.remote_failed(command, cx);
                    return;
                }
                Command::ReadModelScope { runtime, token, .. } => {
                    self.model_scope_result(runtime, token, None, cx);
                    return;
                }
                Command::Send {
                    runtime,
                    generation,
                    request,
                } => {
                    if let Some(live) = self.sessions.get_mut(&runtime)
                        && live.controller.generation() == generation
                    {
                        let id = request["id"].as_str().unwrap_or_default();
                        let effects = if request["type"] == "extension_ui_response" {
                            if live
                                .controller
                                .state()
                                .extension_dialogs
                                .iter()
                                .any(|r| r.id == id && r.submitting)
                            {
                                live.extension_error = Some((
                                    generation,
                                    id.into(),
                                    "The extension response could not be sent. Try again.".into(),
                                ));
                            }
                            live.controller.extension_sent(id, false)
                        } else {
                            live.controller.request_failed(id)
                        };
                        live.dirty = true;
                        live.projection_dirty = true;
                        self.effects(&runtime, effects, cx);
                        self.refresh_window(cx);
                    }
                    return;
                }
                Command::Start { runtime, .. } => {
                    if let Some(live) = self.sessions.get_mut(&runtime) {
                        live.starting = false;
                        live.startup_events.clear();
                        live.controller
                            .transport_failed(live.controller.generation());
                        live.dirty = true;
                        live.projection_dirty = true;
                    }
                }
                Command::ExitFence { .. } => {
                    if let Some(request) = self.lifecycle {
                        self.quit_error = Some((request, "Session storage receipts could not be drained. Retry before closing Tau.".into()));
                    }
                    self.refresh_window(cx);
                    return;
                }
                Command::Save { revision, .. } => {
                    self.pending_saves.remove(&revision);
                    self.save_failed = true;
                    if let Some(request) = self.lifecycle.filter(|_| revision == self.save_revision)
                    {
                        self.quit_error = Some((
                            request,
                            "Drafts could not be saved. Retry before closing Tau.".into(),
                        ));
                    } else {
                        self.error = Some("Drafts could not be saved. Try again.".into());
                    }
                    if self
                        .exit_revision
                        .is_some_and(|(expected, _)| expected == revision)
                    {
                        self.exit_revision = None;
                    }
                    self.refresh_window(cx);
                    return;
                }
                command @ (Command::Register { .. } | Command::CancelStart { .. }) => {
                    self.register_retries.push(command);
                    if let Some(request) = self.lifecycle {
                        self.quit_error = Some((
                            request,
                            "Session changes still need saving. Retry before closing Tau.".into(),
                        ));
                        self.refresh_window(cx);
                        return;
                    }
                }
                Command::Archive {
                    project,
                    id,
                    revision,
                    archived,
                } => {
                    self.finish_archive(project, id, revision, archived, false);
                    self.refresh_window(cx);
                    return;
                }
                Command::StoreName {
                    runtime,
                    generation,
                    revision,
                    id,
                    path,
                    name,
                    ..
                } => {
                    self.finish_name(runtime, generation, revision, id, path, name, false);
                    self.refresh_window(cx);
                    return;
                }
                _ => {}
            }
            self.error = Some("Tau is busy saving changes. Try again.".into());
            cx.notify();
        }
    }
    fn refresh_window(&self, cx: &mut Context<Self>) {
        let weak = cx.entity().downgrade();
        if let Some(target) = self.window {
            cx.defer(move |cx| {
                let _ = target.update(cx, |_, window, cx| {
                    let _ = weak.update(cx, |s, cx| s.sync(window, cx));
                });
            });
        }
    }
    fn mount(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.window = Some(window.window_handle());
        let focus = self.shell_focus.get_or_insert_with(|| cx.focus_handle());
        // A fresh window needs a shortcut route; remounts retain the current owner.
        if window.focused(cx).is_none() {
            focus.focus(window, cx);
        }
        self.subscriptions.clear();
        for live in self.sessions.values_mut() {
            live.dirty = true;
            live.composer
                .update(cx, |composer, cx| composer.reattach_window(window, cx));
            // Retain request-owned editors across window/session remounts.
        }
        let dark = self.theme_dark(window);
        self.sidebar =
            Some(cx.new(|cx| Sidebar::new(sidebar_props(), vec![], app_version(), dark, cx)));
        self.header = Some(cx.new(|cx| Header::new(String::new(), "Tau".into(), dark, window, cx)));
        let sidebar = self.sidebar.as_ref().unwrap().clone();
        self.subscriptions.push(cx.subscribe_in(
            &sidebar,
            window,
            |s, _, action: &Action, w, cx| {
                s.action(action.clone(), w, cx);
            },
        ));
        sidebar.update(cx, |sidebar, cx| {
            sidebar.set_width(self.preferences.sidebar_width.unwrap_or(260.), cx);
            self.preferences.sidebar_width = Some(sidebar.width());
        });
        self.subscriptions.push(cx.subscribe(
            &sidebar,
            |s, sidebar, event: &components::chrome::SidebarWidthChanged, cx| {
                s.preferences.sidebar_width = Some(sidebar.read(cx).width());
                debug_assert_eq!(event.0, sidebar.read(cx).width());
                s.schedule_sidebar_width_save(cx);
            },
        ));
        let header = self.header.as_ref().unwrap().clone();
        header.update(cx, |h, cx| h.set_renamable(false, cx));
        self.subscriptions.push(cx.subscribe_in(
            &header,
            window,
            |s, _, action: &Action, w, cx| {
                s.action(action.clone(), w, cx);
            },
        ));
        self.subscriptions
            .push(cx.observe_window_appearance(window, |s, w, cx| s.sync(w, cx)));
        self.mount_palette(window, cx);
        if let Some(palette) = self.palette.view.clone() {
            self.subscriptions
                .push(cx.observe(&palette, |s, _, cx| s.sync_chrome_admission(cx)));
        }
        self.mount_remote(window, cx);
        self.mount_admin(window, cx);
        // Mount establishes the route, not a delayed claim on an already-ready editor.
        // Later readiness transitions still restore the composer after its first paint.
        self.sync_with_focus_policy(window, cx, false);
    }
    fn theme_dark(&self, window: &Window) -> bool {
        match self.preferences.theme.as_str() {
            "dark" => true,
            "light" => false,
            _ => matches!(
                window.appearance(),
                gpui_kit::WindowAppearance::Dark | gpui_kit::WindowAppearance::VibrantDark
            ),
        }
    }
    fn is_busy(&self) -> bool {
        self.picker_busy
            || self.remote.is_busy()
            || !self.register_retries.is_empty()
            || !self.empty_disposals.is_empty()
            || self.workflow_operations_pending()
            || self.sessions.values().any(|live| {
                let state = live.controller.state();
                live.starting
                    || state.working
                    || state.stopping
                    || live.controller.pending_count() > 0
                    || !state.extension_dialogs.is_empty()
                    || !state.queue.steering.is_empty()
                    || !state.queue.follow_up.is_empty()
            })
    }
    fn selected_project(&self) -> Option<ProjectSummary> {
        if let Some(live) = self.sessions.get(&self.active) {
            return Some(live.project.clone());
        }
        self.workspace
            .as_ref()
            .and_then(|w| {
                w.projects
                    .iter()
                    .find(|p| p.path == w.active_project_path)
                    .or(w.projects.first())
            })
            .cloned()
    }
    fn new_session(
        &mut self,
        project: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.project_actions_admitted(cx) || self.palette.is_open(cx) {
            return;
        }
        let explicit_project = project.is_some();
        let project = match project {
            Some(path) => self
                .workspace
                .as_ref()
                .and_then(|workspace| workspace.projects.iter().find(|p| p.path == path).cloned()),
            None => self.selected_project(),
        };
        if let Some(project) = project {
            self.create_live(
                uuid::Uuid::new_v4().to_string(),
                project,
                String::new(),
                String::new(),
                String::new(),
                window,
                cx,
            );
            if let Some(focus) = self.shell_focus.clone() {
                focus.focus(window, cx);
            }
            self.restore_input_focus_in(None, window, cx);
        } else if !explicit_project {
            self.error = Some("Import a project before creating a session.".into());
            cx.notify();
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn create_live(
        &mut self,
        key: String,
        project: ProjectSummary,
        source_id: String,
        source_path: String,
        draft: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.discard_empty_before_switch(&key, cx);
        self.initialize_workspace_session(&project, &source_path, cx);
        self.create_retained(
            key.clone(),
            project,
            source_id,
            source_path,
            draft,
            window,
            cx,
        );
        self.active = key.clone();
        self.mark_sidebar_read(&key);
        self.touch_runtime(&key);
        self.start_live(&key, cx);
        self.navigate_saved_history(cx);
        self.persist(false, cx);
        self.sync(window, cx);
    }
    #[allow(clippy::too_many_arguments)]
    fn create_retained(
        &mut self,
        key: String,
        project: ProjectSummary,
        source_id: String,
        source_path: String,
        draft: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.sessions.contains_key(&key) {
            return;
        }
        let ephemeral = source_id.is_empty() && source_path.is_empty();
        let mut controller = Controller::new(&key);
        controller.tick(self.epoch.elapsed().as_millis() as u64);
        if !source_id.is_empty() {
            controller.bind_source(&source_id, &source_path);
        }
        controller.set_draft(&draft);
        let composer = cx.new(|cx| {
            Composer::new(
                ComposerOptions {
                    models: vec![],
                    model_heading: "Models".into(),
                    efforts: vec![],
                    model: String::new(),
                    effort: String::new(),
                    draft: draft.clone(),
                    dark: self.dark,
                    disabled: false,
                    streaming: false,
                },
                window,
                cx,
            )
        });
        let transcript = cx.new(|cx| Transcript::new(&key, self.dark, cx));
        let queue = cx.new(|cx| {
            MessageQueue::new(format!("queue-{key}"), QueueProps::default(), self.dark, cx)
        });
        let event_key = key.clone();
        let events = cx.subscribe(&composer, move |s, _, event: &ComposerEvent, cx| {
            s.composer_event(&event_key, event, cx)
        });
        let editor = composer.read(cx).editor();
        let draft_key = key.clone();
        let changes = cx.subscribe(&editor, move |s, editor, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                if let Some(live) = s.sessions.get_mut(&draft_key) {
                    let text = editor.read(cx).value().to_string();
                    live.shown_draft = text.clone();
                    live.controller.set_draft(text);
                    live.dirty = true;
                }
                s.sidebar_state.invalidate_projection();
                s.persist(false, cx);
                s.refresh_window(cx);
            }
        });
        let context_key = key.clone();
        let context_transcript = transcript.downgrade();
        let context_owner = cx.entity().downgrade();
        transcript.update(cx, |transcript, cx| {
            transcript.set_context_handler(
                move |key, target, event, window, cx| {
                    let Some(entity) = context_transcript.upgrade() else {
                        return;
                    };
                    let _ = context_owner.update(cx, |shell, cx| {
                        if !shell
                            .sessions
                            .get(&context_key)
                            .is_some_and(|live| live.transcript == entity)
                        {
                            return;
                        }
                        if let Some(source) = shell.transcript_link_source(&context_key, key, cx) {
                            shell.open_link_menu(
                                &context_key,
                                source,
                                target,
                                event.position,
                                window,
                                cx,
                            );
                        }
                    });
                },
                cx,
            );
        });
        let menu_key = key.clone();
        let menu_queue = queue.downgrade();
        let menu_owner = cx.entity().downgrade();
        queue.update(cx, |queue, _| {
            queue.set_context_handler(move |target, event, window, cx| {
                let Some(entity) = menu_queue.upgrade() else {
                    return;
                };
                let Some(preview) = entity.read(cx).preview().cloned() else {
                    return;
                };
                let _ = menu_owner.update(cx, |shell, cx| {
                    if shell
                        .sessions
                        .get(&menu_key)
                        .is_some_and(|live| live.queue == entity)
                    {
                        shell.open_link_menu(
                            &menu_key,
                            file_preview::LinkSource::Queue { preview },
                            target,
                            event.position,
                            window,
                            cx,
                        );
                    }
                });
            })
        });
        let queue_key = key.clone();
        let queue_events = cx.subscribe_in(
            &queue,
            window,
            move |s, source, event: &QueueEvent, window, cx| {
                if !s
                    .sessions
                    .get(&queue_key)
                    .is_some_and(|live| &live.queue == source)
                {
                    return;
                }
                match event {
                    QueueEvent::Clear => {
                        if !s.project_actions_admitted(cx) {
                            return;
                        }
                        if let Some(live) = s.sessions.get_mut(&queue_key) {
                            live.dirty = true;
                            let effects = live.controller.clear_queue();
                            s.effects(&queue_key, effects, cx);
                            s.refresh_window(cx);
                        }
                    }
                    QueueEvent::Recover(index) => {
                        if let Some(ticket) = s.queue_recovery(&queue_key, source, *index, cx) {
                            s.restore_queue_draft(ticket, window, cx);
                        }
                    }
                    QueueEvent::OpenLink(target) => {
                        let queue = source.read(cx);
                        if let Some(preview) = queue.preview().cloned() {
                            let Some(anchor) = queue.link_anchor() else {
                                return;
                            };
                            s.open_source_link(
                                &queue_key,
                                file_preview::LinkSource::Queue { preview },
                                target.clone(),
                                window.focused(cx),
                                anchor,
                                window,
                                cx,
                            );
                        }
                    }
                    QueueEvent::Dismiss => {
                        if let Some(live) = s.sessions.get_mut(&queue_key) {
                            live.controller.dismiss_queue_feedback();
                            live.dirty = true;
                            s.refresh_window(cx);
                        }
                    }
                }
            },
        );
        let history_key = key.clone();
        let links = cx.subscribe_in(
            &transcript,
            window,
            move |s, source, event: &TranscriptEvent, window, cx| {
                if !s
                    .sessions
                    .get(&history_key)
                    .is_some_and(|live| &live.transcript == source)
                {
                    return;
                }
                match event {
                    TranscriptEvent::LinkRequested {
                        key,
                        target,
                        origin,
                        anchor,
                    } => {
                        if let Some(source) = s.transcript_link_source(&history_key, *key, cx) {
                            s.open_source_link(
                                &history_key,
                                source,
                                target.clone(),
                                origin.clone(),
                                *anchor,
                                window,
                                cx,
                            );
                        }
                    }
                    TranscriptEvent::LinkContextRequested { key, target, event } => {
                        if let Some(source) = s.transcript_link_source(&history_key, *key, cx) {
                            s.open_link_menu(
                                &history_key,
                                source,
                                target.clone(),
                                event.position,
                                window,
                                cx,
                            );
                        }
                    }
                    TranscriptEvent::OpenLink(link) => {
                        if let Some(crate::components::markdown::LinkTarget::Web(url)) =
                            crate::components::markdown::LinkTarget::explicit(link)
                        {
                            s.open_source_link(
                                &history_key,
                                file_preview::LinkSource::Project,
                                crate::components::markdown::LinkTarget::Web(url),
                                window.focused(cx),
                                window.mouse_position(),
                                window,
                                cx,
                            );
                        }
                    }
                    TranscriptEvent::LoadHistory(marker) => {
                        if let Some(live) = s.sessions.get_mut(&history_key) {
                            live.dirty = true;
                            live.projection_dirty = true;
                            let effects = live.controller.load_history(marker);
                            // Admission may be a no-op without a revision change. Restore
                            // this locally busy row from its authoritative controller state.
                            if let Some(entry) = live
                                .controller
                                .state()
                                .transcript
                                .iter()
                                .find(|entry| entry.id == *marker)
                            {
                                let row = projection::row(entry);
                                live.transcript.update(cx, |transcript, cx| {
                                    transcript.update_content(&row.id, row.content, cx);
                                });
                            }
                            s.effects(&history_key, effects, cx);
                            s.refresh_window(cx);
                        }
                    }
                }
            },
        );
        self.sessions.insert(
            key.clone(),
            LiveSession {
                project: project.clone(),
                source_id,
                source_path: source_path.clone(),
                ephemeral,
                registration_pending: Default::default(),
                controller,
                starting: false,
                start_attempt: 0,
                start_deadline: 0,
                composer,
                startup_events: VecDeque::new(),
                transcript,
                queue,
                projection: projection::Projection::default(),
                shown_draft: draft.clone(),
                persisted_draft: draft,
                persisted_recovered: 0,
                restored_recovered: vec![],
                dirty: true,
                projection_dirty: true,
                ui_status: None,
                choices: None,
                command_choices: None,
                model_scope_read: None,
                presented_history: None,
                extension: None,
                extension_subscription: None,
                extension_generation: None,
                extension_error: None,
                _subscriptions: vec![events, changes, queue_events, links],
            },
        );
        self.sidebar_state.runtime_created(&key);
    }
    fn start_live(&mut self, key: &str, cx: &mut Context<Self>) {
        if self.runtime_cleanup_pending(key) {
            return;
        }
        let released = self.runtime_was_released(key);
        let source = self.resumable_source(key);
        if released
            && (source.is_none()
                || self.runtime_lifecycle.reconnect_required.contains(key)
                || self
                    .sessions
                    .get(key)
                    .is_some_and(|live| live.project.connection_string.is_some()))
        {
            return;
        }
        let Some(live) = self.sessions.get_mut(key) else {
            return;
        };
        if live.starting || (live.controller.generation() != 0 && !released) {
            return;
        }
        live.controller
            .tick(self.epoch.elapsed().as_millis() as u64);
        live.starting = true;
        self.sidebar_state.invalidate_projection();
        live.start_attempt += 1;
        live.start_deadline = self.epoch.elapsed().as_millis() as u64 + 10_000;
        live.startup_events.clear();
        let command = Command::Start {
            runtime: key.into(),
            attempt: live.start_attempt,
            project: live.project.clone(),
            source,
        };
        self.dispatch(command, cx);
    }
    fn composer_event(&mut self, key: &str, event: &ComposerEvent, cx: &mut Context<Self>) {
        let admitted = self.active == key
            && !self.runtime_cleanup_pending(key)
            && self.project_actions_admitted(cx)
            && !self.palette.is_open(cx)
            && self
                .sessions
                .get(key)
                .is_some_and(|live| live.controller.state().extension_dialogs.is_empty());
        let Some(live) = self.sessions.get_mut(key) else {
            return;
        };
        live.dirty = true;
        live.projection_dirty = true;
        let effects = if !admitted {
            vec![]
        } else {
            match event {
                ComposerEvent::Send(text) | ComposerEvent::FollowUp(text)
                    if live.controller.state().working =>
                {
                    let kind = if matches!(event, ComposerEvent::FollowUp(_)) {
                        QueueKind::FollowUp
                    } else {
                        QueueKind::Steer
                    };
                    live.controller.submit_queued(text, kind)
                }
                ComposerEvent::Send(text) | ComposerEvent::FollowUp(text) => {
                    live.controller.submit(text)
                }
                ComposerEvent::Stop => live.controller.abort(),
                ComposerEvent::ModelChanged(id) => {
                    let (provider, model) = id.split_once('\u{1f}').unwrap_or(("", id));
                    live.controller.set_model(provider, model)
                }
                ComposerEvent::EffortChanged(effort) => live.controller.set_thinking(effort),
            }
        };
        if let ComposerEvent::Send(text) | ComposerEvent::FollowUp(text) = event
            && effects.is_empty()
            && !text.trim().is_empty()
        {
            live.controller.set_draft(text.clone());
            // Composer cleared its editor before emitting Send; rejected
            // admission must put that text back through the normal sync.
            live.shown_draft.clear();
        }
        self.sidebar_state.invalidate_projection();
        self.effects(key, effects, cx);
        self.persist(false, cx);
        self.refresh_window(cx);
    }
    fn effects(&mut self, key: &str, effects: Vec<Effect>, cx: &mut Context<Self>) {
        let Some(live) = self.sessions.get(key) else {
            return;
        };
        let generation = live.controller.generation();
        let project = live.project.path.clone();
        for effect in effects {
            match effect {
                Effect::Send(request) => {
                    self.dispatch(
                        Command::Send {
                            runtime: key.into(),
                            generation,
                            request,
                        },
                        cx,
                    );
                }
                Effect::RegisterSession {
                    runtime_id,
                    generation,
                    id,
                    path,
                    name,
                } => {
                    let command = Command::Register {
                        runtime: runtime_id,
                        generation,
                        project: project.clone(),
                        id,
                        path,
                        name,
                    };
                    // Controller proof can outlive its transport; retain its storage
                    // owner before the ordinary dispatch/retry admission fence.
                    self.protect_empty_registration(&command);
                    self.dispatch(command, cx);
                }
                Effect::SessionNameChanged { identity } => {
                    self.sync_session_name(key, generation, identity, cx);
                }
                Effect::IdentityChanged { current, .. } => {
                    self.sidebar_state.invalidate_metadata();
                    if self.active == key
                        && let Some(header) = &self.header
                    {
                        header.update(cx, |h, cx| h.set_renamable(false, cx));
                    }
                    self.clear_session_action_identity(key);
                    if let Some(live) = self.sessions.get_mut(key) {
                        live.source_id = current.id;
                        live.source_path = current.path;
                        live.dirty = true;
                    }
                }
            }
        }
    }
    fn begin_exit(&mut self, request: Request, shutdown: bool, cx: &mut Context<Self>) {
        if !cx.global::<NativeHost>().lifecycle().is_pending(request) {
            return;
        }
        if self.lifecycle != Some(request)
            || self.exit_phase_owned(request)
            || self
                .quit_error
                .as_ref()
                .is_some_and(|(owner, _)| *owner == request)
        {
            return;
        }
        if self.picker_busy || self.remote.is_busy() {
            self.quit_error = Some((
                request,
                if self.remote.is_busy() {
                    "Wait for the remote project operation before closing."
                } else {
                    "Finish or cancel the folder picker before closing."
                }
                .into(),
            ));
            self.refresh_window(cx);
            return;
        }
        self.quit_error = None;
        self.runtime_lifecycle.exit_saved = None;
        self.runtime_lifecycle.exit_fence_pending = None;
        self.runtime_lifecycle.exit_wait = Some((request, shutdown));
        self.drain_exit_durability(cx);
        self.source_recoveries(cx);
        self.retry_workflow_operations(cx);
        self.advance_exit(cx);
        for live in self.sessions.values_mut() {
            live.dirty = true;
        }
    }
    fn preferences_snapshot(&self) -> Preferences {
        let mut preferences = self.preferences.clone();
        preferences.selected_draft = self.active.clone();
        preferences.drafts = self
            .sessions
            .iter()
            .filter_map(|(key, live)| {
                let text = &live.controller.state().draft;
                (!text.is_empty()
                    || !live.controller.state().recovered_drafts.is_empty()
                    || !live.restored_recovered.is_empty())
                .then(|| Draft {
                    key: key.clone(),
                    project: live.project.path.clone(),
                    source_id: live.controller.state().identity.id.clone(),
                    source_path: live.controller.state().identity.path.clone(),
                    text: text.clone(),
                    recovered: live
                        .restored_recovered
                        .iter()
                        .chain(&live.controller.state().recovered_drafts)
                        .cloned()
                        .collect(),
                })
            })
            .collect();
        // Unopened drafts are not discarded when another session changes.
        for draft in &self.preferences.drafts {
            if !self.sessions.contains_key(&draft.key) {
                preferences.drafts.push(draft.clone());
            }
        }
        preferences
    }
    fn persist(&mut self, shutdown: bool, cx: &mut Context<Self>) {
        if !self.loaded {
            return;
        }
        let snapshot = self.preferences_snapshot();
        if !shutdown
            && self
                .pending_saves
                .last_key_value()
                .is_some_and(|(_, pending)| pending == &snapshot)
        {
            return;
        }
        self.save_revision += 1;
        let revision = self.save_revision;
        self.pending_saves.insert(revision, snapshot.clone());
        self.save_failed = false;
        self.dispatch(
            Command::Save {
                revision,
                preferences: Some(snapshot),
                shutdown,
            },
            cx,
        );
    }
}
fn app_version() -> String {
    env!("CARGO_PKG_VERSION").into()
}
fn sidebar_props() -> SidebarProps {
    SidebarProps {
        sessions: vec![],
        active: String::new(),
        collapsed: false,
        present: true,
        archived: false,
    }
}

#[cfg(test)]
mod queue_shortcut_tests;
