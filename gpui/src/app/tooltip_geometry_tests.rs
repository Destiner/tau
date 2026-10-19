//! Actual Root + shaped link activation + shell feedback geometry.
use super::*;
use crate::components::markdown;
use gpui_kit::{
    Bounds, IntoElement, Render, TestApp, TestAppWindow,
    base::{Root, TextView, TextViewState},
    point,
    prelude::*,
    size,
};
use std::{
    ops::Range,
    sync::{Arc, Mutex},
};

type Activations = Arc<Mutex<Vec<(gpui_kit::ClickEvent, Bounds<Pixels>)>>>;
type GeometryFixture = (
    TestApp,
    TestAppWindow<Root>,
    Entity<Shell>,
    Entity<TextViewState>,
    Activations,
);

struct GeometryHost {
    shell: Entity<Shell>,
    text: Entity<TextViewState>,
    activations: Activations,
}
impl Render for GeometryHost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let shell = self.shell.clone();
        let activations = self.activations.clone();
        gpui_kit::div().size_full().child(self.shell.clone()).child(
            gpui_kit::div()
                .absolute()
                .left(px(400.))
                .top(px(60.))
                .w(px(180.))
                .child(
                    TextView::new(&self.text)
                        .scrollable(false)
                        .markdown_extensions(markdown::extensions())
                        .text_size(px(13.))
                        .line_height(gpui_kit::relative(1.62))
                        .on_link_activate_anchored(move |target, event, bounds, w, cx| {
                            let Some(target) = markdown::activation_target(target, event) else {
                                return;
                            };
                            activations.lock().unwrap().push((event.clone(), bounds));
                            shell.update(cx, |s, cx| {
                                s.open_source_link(
                                    "preview-fixture",
                                    LinkSource::Project,
                                    target,
                                    w.focused(cx),
                                    point(bounds.left(), bounds.bottom()),
                                    w,
                                    cx,
                                )
                            });
                        }),
                ),
        )
    }
}

fn geometry_fixture(source: &str) -> GeometryFixture {
    let mut app = crate::app::tests::app();
    let (commands, _) = async_channel::bounded(64);
    let shell = app.new_entity(|_| {
        let mut s = crate::app::tests::shell(commands);
        s.workspace = Some(crate::app::WorkspaceSnapshot {
            projects: vec![crate::app::tests::project()],
            active_project_path: crate::app::tests::project().path,
            pi_path: None,
        });
        s
    });
    let text = app.new_entity(|cx| {
        TextViewState::markdown_with_extensions(source, markdown::extensions(), cx)
    });
    let activations = Arc::new(Mutex::new(Vec::new()));
    let window = app.open_window(|w, cx| {
        shell.update(cx, |s, cx| {
            s.window = Some(w.window_handle());
            s.mount(w, cx);
            s.create_retained(
                "preview-fixture".into(),
                crate::app::tests::project(),
                String::new(),
                String::new(),
                String::new(),
                w,
                cx,
            );
            s.active = "preview-fixture".into();
        });
        let host = cx.new(|_| GeometryHost {
            shell: shell.clone(),
            text: text.clone(),
            activations: activations.clone(),
        });
        Root::new(host, w, cx)
    });
    (app, window, shell, text, activations)
}

fn hit_points(
    window: &mut TestAppWindow<Root>,
    text: &Entity<TextViewState>,
    range: Range<usize>,
) -> Vec<Point<Pixels>> {
    window.update(|_, _, cx| {
        let text = text.read(cx);
        let bounds = text.bounds();
        let mut points = Vec::new();
        for y in 0..f32::from(bounds.size.height).ceil() as usize {
            for x in 0..f32::from(bounds.size.width).ceil() as usize {
                let at = bounds.origin + point(px(x as f32), px(y as f32));
                if text
                    .rendered_offset_at(at)
                    .is_some_and(|offset| range.contains(&offset))
                    && text.rendered_offset_at(at - point(px(1.), px(0.)))
                        != text.rendered_offset_at(at + point(px(1.), px(0.)))
                {
                    points.push(at);
                }
            }
        }
        assert!(
            !points.is_empty(),
            "must hit actual shaped text, not a synthetic box"
        );
        points
    })
}

#[test]
fn root_same_wrapped_link_pointer_positions_and_keyboard_share_occurrence_bounds() {
    let label = "A genuinely long link label with several wrapped words for activation";
    let (mut app, mut window, shell, text, activations) = geometry_fixture(&format!(
        "[{label}](docs/repeat.md) then [{label}](docs/repeat.md)"
    ));
    app.run_until_parked();
    window.draw();
    app.run_until_parked();
    window.draw();
    let ranges = window.update(|_, _, cx| {
        text.read(cx)
            .rendered_text()
            .as_str()
            .match_indices(label)
            .map(|(start, _)| start..start + label.len())
            .collect::<Vec<_>>()
    });
    assert_eq!(ranges.len(), 2);
    let first = hit_points(&mut window, &text, ranges[0].clone());
    let second = hit_points(&mut window, &text, ranges[1].clone());
    let (sender, receiver) = async_channel::unbounded();
    window.update(|_, _, cx| shell.update(cx, |s, _| s.preview.prepare_receiver = Some(receiver)));
    let mut first_bounds = None;
    for (index, at) in [
        first[first.len() / 4],
        first[first.len() * 3 / 4],
        second[second.len() / 2],
    ]
    .into_iter()
    .enumerate()
    {
        window.simulate_click(at, MouseButton::Left);
        app.run_until_parked();
        let (event, bounds) = activations.lock().unwrap().last().cloned().unwrap();
        assert!(matches!(event, gpui_kit::ClickEvent::Mouse(_)));
        assert!(bounds.size.height > px(30.), "the fixture must really wrap");
        if index < 2 {
            if let Some(previous) = first_bounds {
                assert_eq!(bounds, previous);
            } else {
                first_bounds = Some(bounds);
            }
        } else {
            assert!(
                bounds.bottom() > first_bounds.unwrap().bottom(),
                "same destination must not reuse first occurrence"
            );
        }
        let focus = window.update(|_, w, cx| w.focused(cx));
        app.advance_clock(Duration::from_millis(199));
        app.run_until_parked();
        window.draw();
        window.update(|_, w, _| assert!(w.try_find("file-link-feedback").is_none()));
        app.advance_clock(Duration::from_millis(1));
        app.run_until_parked();
        window.draw();
        window.update(|_, w, cx| {
            let surface = w.find("file-link-feedback").bounds();
            assert_eq!(surface.left(), bounds.left());
            assert!((surface.top() - bounds.bottom() - px(6.)).abs() <= px(0.5));
            assert_eq!(w.focused(cx), focus);
        });
        sender
            .try_send(Err("synthetic unavailable".into()))
            .unwrap();
        app.run_until_parked();
        window.draw();
        app.advance_clock(Duration::from_millis(1200));
        app.run_until_parked();
        window.draw();
    }
    // Focus the text owner through its non-link text, then tab to a genuine link.
    let separator = hit_points(&mut window, &text, ranges[0].end + 1..ranges[1].start - 1);
    window.simulate_click(separator[separator.len() / 2], MouseButton::Left);
    let handle = app.windows()[0];
    for key in ["tab", "enter"] {
        app.update(|cx| {
            cx.update_window(handle, |_, w, cx| w.press(key, cx))
                .unwrap();
        });
        app.run_until_parked();
        window.draw();
    }
    app.run_until_parked();
    let (event, bounds) = activations.lock().unwrap().last().cloned().unwrap();
    assert!(matches!(event, gpui_kit::ClickEvent::Keyboard(_)));
    assert_eq!(bounds, first_bounds.unwrap());
}

#[test]
fn root_generated_path_pointer_locations_produce_same_real_bottom_left_anchor() {
    let path = "src/a_long_module_name_with_multiple_parts.rs";
    let (app, mut window, shell, text, activations) = geometry_fixture(path);
    app.run_until_parked();
    window.draw();
    app.run_until_parked();
    window.draw();
    let points = hit_points(&mut window, &text, 1..path.len() - 1);
    let (sender, receiver) = async_channel::unbounded();
    window.update(|_, _, cx| shell.update(cx, |s, _| s.preview.prepare_receiver = Some(receiver)));
    let mut original = None;
    for at in [points[points.len() / 4], points[points.len() * 3 / 4]] {
        window.simulate_click(at, MouseButton::Left);
        app.run_until_parked();
        let (event, bounds) = activations.lock().unwrap().last().cloned().unwrap();
        assert_eq!(event.position(), at);
        if let Some(original) = original {
            assert_eq!(bounds, original);
        } else {
            original = Some(bounds);
        }
        window.update(|_, _, cx| {
            let s = shell.read(cx);
            assert_eq!(s.preview.owner.pending.as_ref().unwrap().written_path, path);
            assert_eq!(
                s.preview_anchor(),
                Some(point(bounds.left(), bounds.bottom()))
            );
        });
        app.advance_clock(Duration::from_millis(200));
        app.run_until_parked();
        window.draw();
        window.update(|_, w, _| {
            let feedback = w.find("file-link-feedback").bounds();
            assert_eq!(feedback.left(), bounds.left());
            assert!((feedback.top() - bounds.bottom() - px(6.)).abs() <= px(0.5));
        });
        sender
            .try_send(Err("synthetic unavailable".into()))
            .unwrap();
        app.run_until_parked();
        app.advance_clock(Duration::from_millis(1200));
        app.run_until_parked();
        window.draw();
    }
}

#[test]
fn root_feedback_measures_long_failure_and_clamps_both_axes_with_eight_pixel_margin() {
    for width in [1000., 180.] {
        let (app, mut window, shell) = super::super::tests::production_link_fixture();
        window.simulate_resize(size(px(width), px(700.)));
        window.update(|_, w, cx| w.bounds_changed(cx));
        window.draw();
        window.update(|_, w, _| assert_eq!(w.viewport_size(), size(px(width), px(700.))));
        let focus = window.update(|_, w, cx| w.focused(cx));
        for at in [point(px(width - 2.), px(698.)), point(px(-10.), px(-10.))] {
            window.update(|_, _, cx| {
                shell.update(cx, |s, cx| {
                    s.preview.anchor = Some(at);
                    s.preview.feedback = Some("Couldn’t open path. Try again.");
                    cx.notify();
                })
            });
            app.run_until_parked();
            window.draw();
            window.update(|_, w, cx| {
                let surface = w.find("file-link-feedback").bounds();
                let label = w.find("file-link-feedback-label").bounds();
                assert!(surface.left() >= px(8.) && surface.top() >= px(8.));
                assert!(
                    surface.right() <= px(width - 8.) && surface.bottom() <= px(692.),
                    "viewport width={width}, actual={:?}, anchor={at:?}, surface={surface:?}, label={label:?}", w.viewport_size()
                );
                assert!(surface.size.width <= px(240.).min(px(width - 16.)));
                assert_eq!(surface.size.height, label.size.height + px(12.));
                assert_eq!(w.focused(cx), focus);
            });
        }
    }
}
