use super::*;
use crate::app::tests::{app, shell};
use gpui_kit::{TestApp, TestAppWindow, base::Root, size, test::TestWindowExt};

fn fixture(dark: bool, scale: f32) -> (TestApp, TestAppWindow<Root>, Entity<Shell>) {
    let mut app = app();
    let (commands, _) = async_channel::bounded(64);
    let shell = app.new_entity(|_| {
        let mut s = shell(commands);
        s.loaded = true;
        s.preferences.theme = if dark { "dark" } else { "light" }.into();
        s.workspace = Some(WorkspaceSnapshot {
            projects: Vec::new(),
            active_project_path: String::new(),
            pi_path: None,
        });
        s
    });
    let mut window = app.open_window(|w, cx| {
        shell.update(cx, |s, cx| {
            s.window = Some(w.window_handle());
            s.mount(w, cx);
            s.sync(w, cx);
        });
        Root::new(shell.clone(), w, cx)
    });
    window.simulate_scale_factor_change(scale);
    window.simulate_resize(size(px(1000.), px(700.)));
    window.draw();
    app.run_until_parked();
    window.draw();
    (app, window, shell)
}

fn advance(app: &TestApp, window: &mut TestAppWindow<Root>, millis: u64) {
    app.run_until_parked();
    app.advance_clock(Duration::from_millis(millis));
    app.run_until_parked();
    window.draw();
}

fn assert_placement(window: &mut TestAppWindow<Root>, trigger: &'static str) {
    window.update(|_, w, _| {
        let button = w.find(trigger).bounds();
        let tooltip = w.find("first-run-tooltip").bounds();
        assert_eq!(tooltip.top() - button.bottom(), px(6.));
        assert!(tooltip.top() >= button.bottom());
        assert!(tooltip.left() >= px(6.));
        assert!(tooltip.right() <= w.viewport_size().width - px(6.));
        assert!(tooltip.bottom() <= w.viewport_size().height - px(6.));
    });
}

#[test]
fn first_run_root_hints_use_measured_button_bottom_at_450ms_and_clamp_on_resize() {
    for dark in [false, true] {
        for scale in [1., 2.] {
            for trigger in ["first-run-project", "first-run-palette"] {
                let (app, mut window, _) = fixture(dark, scale);
                let focus = window.update(|_, w, cx| w.focused(cx));
                // Deliberately off-center: pointer location must not position the tooltip.
                let hover_point = window.update(|_, w, _| {
                    let bounds = w.find(trigger).bounds();
                    point(bounds.right() - px(2.), bounds.top() + px(2.))
                });
                window.simulate_mouse_move(hover_point);
                advance(&app, &mut window, 449);
                window.update(|_, w, _| assert!(w.try_find("first-run-tooltip").is_none()));
                advance(&app, &mut window, 1);
                assert_placement(&mut window, trigger);
                window.update(|_, w, cx| assert_eq!(w.focused(cx), focus));
                window.simulate_resize(size(px(430.), px(500.)));
                window.draw();
                app.run_until_parked();
                window.draw();
                assert_placement(&mut window, trigger);
                window.simulate_mouse_move(point(px(1.), px(80.)));
                advance(&app, &mut window, 500);
                window.update(|_, w, _| assert!(w.try_find("first-run-tooltip").is_none()));
            }
        }
    }
}

#[test]
fn first_run_root_hint_exit_menu_palette_and_disable_invalidate_pending_requests() {
    for trigger in ["first-run-project", "first-run-palette"] {
        for interruption in ["exit", "click", "disable", "palette"] {
            let (app, mut window, shell) = fixture(true, 2.);
            let anchor = window.update(|_, w, _| w.find(trigger).bounds().center());
            window.simulate_mouse_move(anchor);
            advance(&app, &mut window, 449);
            match interruption {
                "exit" => window.simulate_mouse_move(point(px(1.), px(80.))),
                "click" => window.simulate_click(anchor, MouseButton::Left),
                "disable" => window.update(|_, _, cx| {
                    shell.update(cx, |s, cx| {
                        s.picker_busy = true;
                        cx.notify();
                    });
                }),
                "palette" => window.update(|_, w, cx| {
                    shell.update(cx, |s, cx| s.action(Action::Palette, w, cx));
                }),
                _ => unreachable!(),
            }
            advance(&app, &mut window, 1);
            window.update(|_, w, _| assert!(w.try_find("first-run-tooltip").is_none()));
            advance(&app, &mut window, 500);
            window.update(|_, w, _| assert!(w.try_find("first-run-tooltip").is_none()));
        }
    }
}

#[test]
fn first_run_root_reentry_starts_a_fresh_delay_and_modal_closes_visible_hint() {
    for trigger in ["first-run-project", "first-run-palette"] {
        let (app, mut window, shell) = fixture(true, 2.);
        let anchor = window.update(|_, w, _| w.find(trigger).bounds().center());
        window.simulate_mouse_move(anchor);
        advance(&app, &mut window, 449);
        window.simulate_mouse_move(point(px(1.), px(80.)));
        window.simulate_mouse_move(anchor);
        advance(&app, &mut window, 1);
        window.update(|_, w, _| assert!(w.try_find("first-run-tooltip").is_none()));
        advance(&app, &mut window, 448);
        window.update(|_, w, _| assert!(w.try_find("first-run-tooltip").is_none()));
        advance(&app, &mut window, 1);
        assert_placement(&mut window, trigger);
        window.update(|_, w, cx| {
            shell.update(cx, |s, cx| s.action(Action::Palette, w, cx));
        });
        advance(&app, &mut window, 500);
        window.update(|_, w, _| assert!(w.try_find("first-run-tooltip").is_none()));
    }
}

#[test]
fn first_run_root_visible_hint_closes_on_project_menu_or_palette_without_delayed_leak() {
    for trigger in ["first-run-project", "first-run-palette"] {
        let (app, mut window, _) = fixture(false, 1.);
        let anchor = window.update(|_, w, _| w.find(trigger).bounds().center());
        window.simulate_mouse_move(anchor);
        advance(&app, &mut window, 450);
        assert_placement(&mut window, trigger);
        window.simulate_click(anchor, MouseButton::Left);
        advance(&app, &mut window, 500);
        window.update(|_, w, _| assert!(w.try_find("first-run-tooltip").is_none()));
        window.simulate_keystroke("escape");
        advance(&app, &mut window, 500);
        window.update(|_, w, _| assert!(w.try_find("first-run-tooltip").is_none()));
    }
}
