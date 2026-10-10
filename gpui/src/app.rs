//! Production native shell. Pi owns execution and history; this entity owns UI lifetime.
#[cfg(feature = "native-os-tests")]
mod native_smoke;
#[cfg(feature = "native-os-tests")]
pub use native_smoke::run_native_smoke;
mod projection;
mod service;
mod session_actions;
#[cfg(test)]
mod tests;
mod view;

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
    follow_up: bool,
    choices: String,
    extension: Option<(String, Entity<NativeDialog>)>,
    extension_subscription: Option<Subscription>,
    _subscriptions: Vec<Subscription>,
}

pub struct Shell {
    commands: async_channel::Sender<Command>,
    workspace: Option<WorkspaceSnapshot>,
    preferences: Preferences,
    sessions: BTreeMap<String, LiveSession>,
    active: String,
    sidebar: Option<Entity<Sidebar>>,
    header: Option<Entity<Header>>,
    window: Option<AnyWindowHandle>,
    picker: Option<Rc<PathChooser<Root>>>,
    picker_busy: bool,
    subscriptions: Vec<Subscription>,
    dark: bool,
    error: Option<String>,
    lifecycle: Option<Request>,
    confirmation: Option<Entity<NativeDialog>>,
    save_revision: u64,
    exit_revision: Option<(u64, Request)>,
    requested_session: Option<String>,
    epoch: Instant,
    loaded: bool,
    save_failed: bool,
    register_retries: Vec<Command>,
    pending_saves: BTreeMap<u64, Preferences>,
    restored: bool,
    session_actions: session_actions::SessionActions,
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
                preferences: Preferences::default(),
                sessions: BTreeMap::new(),
                active: String::new(),
                sidebar: None,
                header: None,
                window: None,
                picker: None,
                picker_busy: false,
                subscriptions: vec![],
                dark,
                error: None,
                lifecycle: None,
                confirmation: None,
                save_revision: 0,
                exit_revision: None,
                requested_session: session,
                epoch: Instant::now(),
                loaded: false,
                save_failed: false,
                register_retries: vec![],
                pending_saves: BTreeMap::new(),
                restored: false,
                session_actions: session_actions::SessionActions::default(),
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
                            if changed {
                                shell.persist(false, cx);
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
                        let no_windows = cx.global::<NativeHost>().lifecycle().windows().is_empty();
                        request_shell.update(cx, |s, cx| {
                            s.lifecycle = Some(request);
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
                        settled_shell.update(cx, |s, cx| {
                            if s.lifecycle == Some(request) {
                                s.lifecycle = None;
                                s.confirmation = None;
                                s.exit_revision = None;
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
        let failed = if matches!(&command, Command::Start { .. }) && self.lifecycle.is_some() {
            Some(command)
        } else {
            self.commands
                .try_send(command)
                .err()
                .map(|failed| failed.into_inner())
        };
        if let Some(failed) = failed {
            match failed {
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
                            live.controller.extension_sent(id, false)
                        } else {
                            live.controller.request_failed(id)
                        };
                        live.dirty = true;
                        live.projection_dirty = true;
                        self.effects(&runtime, effects, cx);
                    }
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
                Command::Save { revision, .. } => {
                    self.pending_saves.remove(&revision);
                    self.save_failed = true;
                    self.exit_revision = None;
                }
                command @ (Command::Register { .. } | Command::CancelStart { .. }) => {
                    self.register_retries.push(command)
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
        self.subscriptions.clear();
        for live in self.sessions.values_mut() {
            live.dirty = true;
            live.composer
                .update(cx, |composer, cx| composer.reattach_window(window, cx));
            live.extension = None;
            live.extension_subscription = None;
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
                if !s.session_action(action, w, cx) {
                    s.action(action.clone(), w, cx);
                }
            },
        ));
        let header = self.header.as_ref().unwrap().clone();
        header.update(cx, |h, cx| h.set_renamable(false, cx));
        self.subscriptions.push(cx.subscribe_in(
            &header,
            window,
            |s, _, action: &Action, w, cx| {
                if !s.session_action(action, w, cx) {
                    s.action(action.clone(), w, cx);
                }
            },
        ));
        self.subscriptions
            .push(cx.observe_window_appearance(window, |s, w, cx| s.sync(w, cx)));
        self.sync(window, cx);
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
            || !self.register_retries.is_empty()
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
        let project = project
            .and_then(|path| {
                self.workspace
                    .as_ref()?
                    .projects
                    .iter()
                    .find(|p| p.path == path)
                    .cloned()
            })
            .or_else(|| self.selected_project());
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
        } else {
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
        self.start_live(&key, cx);
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
                    disabled: true,
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
                s.persist(false, cx);
            }
        });
        let queue_key = key.clone();
        let queue_events = cx.subscribe(&queue, move |s, _, event: &QueueEvent, cx| {
            if matches!(event, QueueEvent::Clear) {
                if let Some(live) = s.sessions.get_mut(&queue_key) {
                    live.dirty = true;
                    let effects = live.controller.clear_queue();
                    s.effects(&queue_key, effects, cx);
                    s.refresh_window(cx);
                }
            } else {
                s.error = Some("This queue action is not available yet. Copy uncertain work before reconnecting.".into());
                cx.notify();
            }
        });
        let links = cx.subscribe(&transcript, |_, _, event: &TranscriptEvent, cx| {
            let TranscriptEvent::OpenLink(link) = event;
            if link.starts_with("https://") || link.starts_with("http://") {
                cx.open_url(link);
            }
        });
        self.sessions.insert(
            key.clone(),
            LiveSession {
                project: project.clone(),
                source_id,
                source_path: source_path.clone(),
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
                follow_up: false,
                choices: String::new(),
                extension: None,
                extension_subscription: None,
                _subscriptions: vec![events, changes, queue_events, links],
            },
        );
    }
    fn start_live(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(live) = self.sessions.get_mut(key) else {
            return;
        };
        if live.starting || live.controller.generation() != 0 {
            return;
        }
        live.controller
            .tick(self.epoch.elapsed().as_millis() as u64);
        live.starting = true;
        live.start_attempt += 1;
        live.start_deadline = self.epoch.elapsed().as_millis() as u64 + 10_000;
        live.startup_events.clear();
        let command = Command::Start {
            runtime: key.into(),
            attempt: live.start_attempt,
            project: live.project.clone(),
            source: (!live.source_path.is_empty()).then(|| live.source_path.clone()),
        };
        self.dispatch(command, cx);
    }
    fn composer_event(&mut self, key: &str, event: &ComposerEvent, cx: &mut Context<Self>) {
        let Some(live) = self.sessions.get_mut(key) else {
            return;
        };
        live.dirty = true;
        live.projection_dirty = true;
        let follow_up = if matches!(event, ComposerEvent::Send(_)) {
            std::mem::take(&mut live.follow_up)
        } else {
            false
        };
        let effects = match event {
            ComposerEvent::Send(text) if live.controller.state().working => {
                let kind = if follow_up {
                    QueueKind::FollowUp
                } else {
                    QueueKind::Steer
                };
                live.controller.submit_queued(text, kind)
            }
            ComposerEvent::Send(text) => live.controller.submit(text),
            ComposerEvent::Stop => live.controller.abort(),
            ComposerEvent::ModelChanged(id) => {
                let (provider, model) = id.split_once('\u{1f}').unwrap_or(("", id));
                live.controller.set_model(provider, model)
            }
            ComposerEvent::EffortChanged(effort) => live.controller.set_thinking(effort),
        };
        if let ComposerEvent::Send(text) = event
            && effects.is_empty()
            && !text.trim().is_empty()
        {
            live.controller.set_draft(text.clone());
            // Composer cleared its editor before emitting Send; rejected
            // admission must put that text back through the normal sync.
            live.shown_draft.clear();
        }
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
                    self.dispatch(
                        Command::Register {
                            runtime: runtime_id,
                            generation,
                            project: project.clone(),
                            id,
                            path,
                            name,
                        },
                        cx,
                    );
                }
                Effect::SessionNameChanged { identity } => {
                    self.sync_session_name(key, generation, identity, cx);
                }
                Effect::IdentityChanged { current, .. } => {
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
    fn ingest(&mut self, event: Event, cx: &mut Context<Self>) {
        match event {
            Event::Loaded(Ok((workspace, preferences))) => { self.workspace = Some(workspace); self.preferences = preferences; self.loaded = true; }
            Event::Loaded(Err(())) => self.error = Some("Tau could not load its workspace. Reopen Tau to retry; saved data was not changed.".into()),
            Event::Workspace(Ok(mut workspace)) => {
                self.reconcile_archive(&mut workspace);
                for live in self.sessions.values_mut() { if let Some(project) = workspace.projects.iter().find(|p| p.path == live.project.path) { live.project = project.clone(); } }
                self.workspace = Some(workspace);
            }
            Event::NameStored { runtime, generation, revision, id, path, name, success } => self.finish_name(runtime, generation, revision, id, path, name, success),
            Event::Archived { project, id, revision, archived, success } => self.finish_archive(project, id, revision, archived, success),
            Event::RegistrationFailed(command) => {
                self.register_retries.push(command);
                self.error = Some("A completed session could not be added to the workspace. Retry saving it.".into());
            }
            Event::Workspace(Err(())) | Event::StorageFailed => self.error = Some("Workspace changes could not be saved. Try again.".into()),
            Event::Started { runtime, attempt, result } => {
                if let Some(live) = self.sessions.get_mut(&runtime) {
                    if attempt != live.start_attempt { return; }
                    live.starting = false;
                    live.dirty = true;
                    live.projection_dirty = true;
                    live.controller.tick(self.epoch.elapsed().as_millis() as u64);
                    let succeeded = result.is_ok();
                    let effects = match result { Ok(generation) => live.controller.begin_generation(generation), Err(()) => live.controller.transport_failed(live.controller.generation()) };
                    let buffered = std::mem::take(&mut live.startup_events);
                    self.effects(&runtime, effects, cx);
                    if succeeded { for event in buffered { self.ingest(Event::Pi(event), cx); } }
                }
            }
            Event::Written { runtime, generation, request, success } => {
                if let Some(live) = self.sessions.get_mut(&runtime) {
                    if live.controller.generation() != generation { return; }
                    live.dirty |= !success || request["type"] == "extension_ui_response";
                    live.projection_dirty |= !success;
                    let id = request["id"].as_str().unwrap_or_default();
                    let effects = if request["type"] == "extension_ui_response" { live.controller.extension_sent(id, success) } else if !success { live.controller.request_failed(id) } else { vec![] };
                    self.effects(&runtime, effects, cx);
                }
            }
            Event::Pi(event) => {
                if let Some(live) = self.sessions.get_mut(&event.runtime_id) {
                    if live.starting {
                        if live.startup_events.len() < 512 {
                            live.startup_events.push_back(event);
                        } else {
                            let runtime = event.runtime_id;
                            let attempt = live.start_attempt;
                            live.starting = false;
                            live.start_attempt += 1;
                            live.startup_events.clear();
                            live.controller.transport_failed(live.controller.generation());
                            live.dirty = true;
                            live.projection_dirty = true;
                            self.dispatch(Command::CancelStart { runtime, attempt }, cx);
                        }
                        return;
                    }
                    if live.controller.generation() != event.generation { return; }
                    live.dirty = true;
                    live.projection_dirty = true;
                    let effects = if event.kind == "rpc" { event.line.and_then(|line| serde_json::from_str(&line).ok()).map(|value| live.controller.ingest(&event.runtime_id, event.generation, value)).unwrap_or_default() }
                    else if matches!(event.kind, "exited" | "error") { live.controller.transport_failed(event.generation) } else { vec![] };
                    self.effects(&event.runtime_id, effects, cx);
                }
            }
            Event::Saved { revision, success, stopped } => {
                if let Some(saved) = self.pending_saves.remove(&revision)
                    && success {
                        for draft in &saved.drafts {
                            if let Some(live) = self.sessions.get_mut(&draft.key) {
                                live.persisted_draft = draft.text.clone();
                                live.persisted_recovered = draft.recovered.len();
                            }
                        }
                        for (key, live) in &mut self.sessions {
                            if !saved.drafts.iter().any(|draft| &draft.key == key) {
                                live.persisted_draft.clear();
                                live.persisted_recovered = 0;
                            }
                        }
                        self.preferences = saved;
                    }
                let cleanup_ran = !stopped.is_empty();
                for (runtime, generation) in stopped {
                    if let Some(live) = self.sessions.get_mut(&runtime) {
                        let effects = live.controller.transport_failed(generation);
                        live.projection_dirty = true;
                        self.effects(&runtime, effects, cx);
                    }
                }
                for live in self.sessions.values_mut() { live.dirty = true; }
                if !success { self.save_failed = true; self.error = Some("Drafts could not be saved or Pi could not stop. Retry before closing Tau.".into()); self.exit_revision = None; }
                else { self.save_failed = false; }
                if success && let Some((expected, request)) = self.exit_revision
                    && expected == revision {
                        self.exit_revision = None;
                        if !self.register_retries.is_empty() { self.save_failed = true; self.error = Some("A completed session still needs saving. Retry before closing Tau.".into()); }
                        else if revision != self.save_revision || self.sessions.values().any(|live| live.controller.state().draft != live.persisted_draft || live.controller.state().recovered_drafts.len() + live.restored_recovered.len() != live.persisted_recovered) {
                            self.begin_exit(request, cleanup_ran || matches!(request.intent(), Intent::Quit), cx);
                        } else { let lifecycle = cx.global::<NativeHost>().lifecycle(); lifecycle.resolve(request, true, cx); }
                    }
            }
        }
        if !self.save_failed
            && self.sessions.values().any(|live| {
                live.controller.state().draft != live.persisted_draft
                    || live.controller.state().recovered_drafts.len()
                        + live.restored_recovered.len()
                        != live.persisted_recovered
            })
        {
            self.persist(false, cx);
        }
        cx.notify();
    }
    fn begin_exit(&mut self, request: Request, shutdown: bool, cx: &mut Context<Self>) {
        if !cx.global::<NativeHost>().lifecycle().is_pending(request) {
            return;
        }
        let retries = std::mem::take(&mut self.register_retries);
        for command in retries {
            self.dispatch(command, cx);
        }
        self.save_failed = false;
        if self.loaded {
            self.persist(shutdown, cx);
        } else {
            self.save_revision += 1;
            self.dispatch(
                Command::Save {
                    revision: self.save_revision,
                    preferences: None,
                    shutdown,
                },
                cx,
            );
        }
        if !self.save_failed {
            self.exit_revision = Some((self.save_revision, request));
        }
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
    serde_json::from_str::<serde_json::Value>(include_str!("../../package.json"))
        .ok()
        .and_then(|package| package["version"].as_str().map(str::to_owned))
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").into())
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
