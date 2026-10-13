use super::*;
use crate::app::tests::{app, project, shell};
use gpui_kit::{Role, TestApp, TestAppWindow, size, test::TestWindowExt};

struct Fixture {
    root: TestAppWindow<Root>,
    shell: Entity<Shell>,
    requests: async_channel::Receiver<Command>,
    app: TestApp,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            self.root.update(|_, w, cx| w.blur(cx));
            self.app.run_until_parked();
        }
    }
}
impl Fixture {
    fn new() -> Self {
        let mut app = app();
        let (commands, requests) = async_channel::bounded(1024);
        let shell = app.new_entity(|_| {
            let mut s = shell(commands);
            s.loaded = false;
            s
        });
        let mut root = app.open_window(|w, cx| Root::new(shell.clone(), w, cx));
        root.simulate_resize(size(px(1000.), px(700.)));
        root.update(|_, w, cx| {
            w.activate_window();
            w.bounds_changed(cx);
            shell.update(cx, |s, cx| s.mount(w, cx));
        });
        let mut f = Self {
            root,
            shell,
            requests,
            app,
        };
        f.draw();
        f
    }
    fn only_admin_load(&self) -> bool {
        let mut loads = 0;
        while let Ok(command) = self.requests.try_recv() {
            match command {
                Command::Admin(crate::app::admin::Command::Load { .. }) => loads += 1,
                _ => panic!("unexpected command: first-run must not start Pi, import, or connect"),
            }
        }
        loads == 1
    }
    fn draw(&mut self) {
        self.root.draw();
        self.app.run_until_parked();
        self.root.draw();
    }
    fn advance(&mut self, millis: u64) {
        self.app.advance_clock(Duration::from_millis(millis));
        self.draw();
    }
    fn loaded(&mut self, projects: Vec<ProjectSummary>) {
        self.event(Event::Loaded(Ok((
            WorkspaceSnapshot {
                active_project_path: projects.first().map(|p| p.path.clone()).unwrap_or_default(),
                projects,
                pi_path: None,
            },
            Preferences::default(),
        ))));
    }
    fn event(&mut self, event: Event) {
        self.root.update(|_, w, cx| {
            self.shell.update(cx, |s, cx| {
                s.ingest(event, cx);
                s.sync(w, cx);
            });
        });
        self.draw();
    }
    fn click(&mut self, id: &'static str) {
        let position = self.root.update(|_, w, _| w.find(id).bounds().center());
        self.root
            .simulate_click(position, gpui_kit::MouseButton::Left);
        self.draw();
    }
    fn press(&mut self, key: &str) {
        let keystroke = gpui_kit::Keystroke::parse(key).unwrap();
        self.root.simulate_event(gpui_kit::KeyDownEvent {
            keystroke: keystroke.clone(),
            is_held: false,
            prefer_character_input: false,
        });
        self.root.simulate_event(gpui_kit::KeyUpEvent { keystroke });
        self.draw();
    }
    fn loading(&mut self, visible: bool) {
        self.root.update(|_, w, cx| {
            assert!(w.find("first-run").visible());
            assert_eq!(w.try_find("first-run-loading").is_some(), visible);
            assert_eq!(w.try_find("working-spinner").is_some(), visible);
            assert!(w.try_find("first-run-actions").is_none());
            assert!(w.try_find("first-run-version").is_none());
            assert!(w.try_find("sidebar").is_none());
            assert!(w.try_find("header").is_none());
            assert_eq!(w.find("first-run-project").disabled(), Some(true));
            assert!(self.shell.read(cx).sessions.is_empty());
        });
    }
    fn focus_remote_button(&mut self) {
        for _ in 0..12 {
            if self
                .root
                .update(|_, w, _| w.find("first-run-remote").focused() == Some(true))
            {
                break;
            }
            self.press("tab");
        }
        self.root.update(|_, w, _| {
            assert_eq!(w.find("first-run-remote").role(), Some(Role::Button));
            assert_eq!(w.find("first-run-remote").focused(), Some(true));
        });
    }
}

#[test]
fn root_workspace_loading_199_200_is_owned_delayed_and_does_not_shift() {
    let mut f = Fixture::new();
    f.loading(false);
    let bounds = f
        .root
        .update(|_, w, _| w.find("first-run-content").bounds());
    f.advance(199);
    f.loading(false);
    f.advance(1);
    f.loading(true);
    f.root.update(|_, w, cx| {
        assert_eq!(w.find("first-run-content").bounds(), bounds);
        assert_eq!(w.find("first-run-titlebar").bounds().size.height, px(30.));
        assert_eq!(
            w.find("first-run-project").bounds().size,
            size(px(24.), px(24.))
        );
        assert_eq!(
            w.find("working-spinner").bounds().size,
            size(px(8.), px(8.))
        );
        assert!(
            f.shell.read(cx).sidebar.is_some(),
            "real shell mounts chrome, but first-run must not compose it"
        );
        assert!(w.try_find("sidebar").is_none());
    });
    assert!(
        f.only_admin_load(),
        "first-run must not start Pi or import a project"
    );
}

#[test]
fn root_real_empty_workspace_version_and_centered_actions_have_no_sidebar() {
    let mut f = Fixture::new();
    f.loaded(vec![]);
    f.advance(400);
    f.root.update(|_, w, cx| {
        assert!(f.shell.read(cx).loaded);
        assert!(
            f.shell
                .read(cx)
                .workspace
                .as_ref()
                .unwrap()
                .projects
                .is_empty()
        );
        assert!(w.find("first-run-version-number").visible());
        let content = w.find("first-run-content").bounds();
        let version = w.find("first-run-version").bounds();
        let actions = w.find("first-run-actions").bounds();
        assert!((f32::from(actions.center().x - content.center().x)).abs() <= 1.);
        assert!(
            (f32::from((version.top() + actions.bottom()) / 2. - content.center().y)).abs() <= 1.
        );
        assert_eq!(actions.top() - version.bottom(), px(14.));
        let local = w.find("first-run-local");
        let remote = w.find("first-run-remote");
        assert_eq!(local.bounds().size.height, px(28.));
        assert_eq!(remote.bounds().left() - local.bounds().right(), px(2.));
        assert_eq!(remote.role(), Some(Role::Button));
        assert_ne!(remote.disabled(), Some(true));
        assert!(w.try_find("first-run-loading").is_none());
        assert!(w.try_find("sidebar").is_none());
    });
}

#[test]
fn fast_workspace_load_cancels_delay_and_nonempty_workspace_uses_main_branch() {
    let mut f = Fixture::new();
    f.advance(199);
    f.loaded(vec![]);
    f.advance(1);
    f.root.update(|_, w, _| {
        assert!(w.find("first-run-actions").visible());
        assert!(w.try_find("first-run-loading").is_none());
    });
    f.loaded(vec![project()]);
    f.advance(400);
    f.root.update(|_, w, _| {
        assert!(w.find("sidebar").visible());
        assert!(w.find("header").visible());
        assert!(w.try_find("first-run").is_none());
    });
}

#[test]
fn workspace_failure_ends_loading_and_keeps_real_fallback_dialog() {
    let mut f = Fixture::new();
    f.advance(200);
    f.loading(true);
    f.event(Event::Loaded(Err(())));
    f.advance(400);
    f.root.update(|_, w, cx| {
        let s = f.shell.read(cx);
        assert!(
            s.error
                .as_ref()
                .unwrap()
                .contains("could not load its workspace")
        );
        assert!(s.fallback.as_ref().unwrap().1.read(cx).is_open());
        assert!(w.try_find("first-run-loading").is_none());
        assert!(w.try_find("working-spinner").is_none());
        assert!(w.try_find("first-run-actions").is_none());
        assert_eq!(w.find("first-run-project").disabled(), Some(true));
    });
}

#[test]
fn fast_failure_before_delay_never_flashes_a_loader() {
    let mut f = Fixture::new();
    f.advance(199);
    f.event(Event::Loaded(Err(())));
    f.advance(1);
    f.advance(400);
    f.root.update(|_, w, cx| {
        assert!(w.try_find("first-run-loading").is_none());
        assert!(w.try_find("working-spinner").is_none());
        assert!(
            f.shell
                .read(cx)
                .fallback
                .as_ref()
                .unwrap()
                .1
                .read(cx)
                .is_open()
        );
    });
}

#[test]
fn fast_nonempty_workspace_load_discards_first_run_timer() {
    let mut f = Fixture::new();
    f.advance(199);
    f.loaded(vec![project()]);
    f.advance(1);
    f.advance(400);
    f.root.update(|_, w, _| {
        assert!(w.find("sidebar").visible());
        assert!(w.find("header").visible());
        assert!(w.try_find("first-run").is_none());
        assert!(w.try_find("first-run-loading").is_none());
    });
}

#[test]
fn real_remote_action_opens_retained_dialog_from_typed_button_keyboard_focus() {
    let mut f = Fixture::new();
    f.loaded(vec![]);
    f.focus_remote_button();
    f.press("enter");
    let view = f.root.update(|_, w, cx| {
        let s = f.shell.read(cx);
        assert!(s.remote.is_open());
        let view = s.remote.view.clone().unwrap();
        assert!(w.find("remote-field").visible());
        // RemoteDialog's input is intentionally private; the observed real input owns focus.
        assert_ne!(w.find("first-run-remote").focused(), Some(true));
        assert!(w.focused(cx).is_some());
        assert_eq!(w.find("first-run-remote").disabled(), Some(true));
        view
    });
    let editor_focus = f.root.update(|_, w, cx| w.focused(cx).unwrap());
    f.advance(300);
    f.root.update(|_, w, cx| {
        assert_eq!(f.shell.read(cx).remote.view.as_ref(), Some(&view));
        assert_eq!(w.focused(cx), Some(editor_focus));
        assert!(w.find("remote-field").visible());
    });
    assert!(
        f.only_admin_load(),
        "opening the form does not connect or start Pi"
    );
}

#[test]
fn project_trigger_uses_shared_menu_and_remote_selection_routes_to_real_shell() {
    let mut f = Fixture::new();
    f.loaded(vec![]);
    f.click("first-run-project");
    f.root.update(|_, w, _| {
        assert!(w.find("menu-surface").visible());
        for id in ["first-run-menu-local", "first-run-menu-remote"] {
            assert!(w.find(id).visible());
            assert_ne!(w.find(id).disabled(), Some(true));
        }
    });
    f.press("escape");
    f.root.update(|_, w, _| {
        assert!(w.try_find("menu-surface").is_none());
        assert_eq!(w.find("first-run-project").focused(), Some(true));
    });
    f.click("first-run-project");
    f.press("down");
    f.press("down");
    f.press("enter");
    f.root.update(|_, w, cx| {
        assert!(f.shell.read(cx).remote.is_open());
        assert!(w.find("remote-field").visible());
        assert!(w.try_find("menu-surface").is_none());
    });
}

#[test]
fn palette_uses_existing_route_and_busy_project_controls_are_inert() {
    let mut f = Fixture::new();
    f.loaded(vec![]);
    f.click("first-run-palette");
    f.root.update(|_, w, cx| {
        assert!(f.shell.read(cx).palette.is_open(cx));
        assert_eq!(w.find("first-run-project").disabled(), Some(true));
        assert_ne!(w.find("first-run-palette").disabled(), Some(true));
    });
    f.press("escape");
    f.root
        .update(|_, _, cx| assert!(!f.shell.read(cx).palette.is_open(cx)));
    f.root.update(|_, _, cx| {
        f.shell.update(cx, |s, cx| {
            s.picker_busy = true;
            cx.notify();
        });
    });
    f.draw();
    f.click("first-run-remote");
    f.click("first-run-project");
    f.root.update(|_, w, cx| {
        assert!(!f.shell.read(cx).remote.is_open());
        assert!(w.try_find("menu-surface").is_none());
        assert_eq!(w.find("first-run-local").disabled(), Some(true));
    });
}
