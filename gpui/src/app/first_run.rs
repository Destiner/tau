//! No-project surface. The service owns workspace loading; this state only delays its indicator.
use super::*;
use crate::components::menus::{Menu, MenuEvent, MenuItem};
use gpui_kit::base::{Button, ElementExt};
use gpui_kit::{AnyElement, FocusHandle, MouseButton, TestSupportExt, point, relative, svg};

const LOADING_DELAY: Duration = Duration::from_millis(200);
const STATE_KEY: &str = "tau-first-run-state";

struct FirstRunTooltip(&'static str);

impl Render for FirstRunTooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui_kit::base::Tooltip::new("first-run-tooltip").child(self.0)
    }
}

struct State {
    waiting: bool,
    visible: bool,
    task: Option<gpui_kit::Task<()>>,
    project_focus: FocusHandle,
    palette_focus: FocusHandle,
    local_focus: FocusHandle,
    remote_focus: FocusHandle,
    menu: Entity<Menu>,
    menu_admitted: bool,
    dark: bool,
    menu_anchor: gpui_kit::Point<gpui_kit::Pixels>,
    _menu_subscription: Subscription,
    _menu_observer: Subscription,
}

impl Shell {
    fn first_run_loading(&self) -> bool {
        !self.loaded && self.workspace.is_none() && self.error.is_none()
    }

    /// Compose instead of sidebar/main, inside the existing shell key/focus root.
    /// Keep palette, remote, confirmation and fallback overlays outside this branch.
    pub(super) fn first_run_view(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if self
            .workspace
            .as_ref()
            .is_some_and(|w| !w.projects.is_empty())
        {
            return None;
        }
        let dark = self.dark;
        let project_admitted = self.project_actions_admitted(cx) && !self.palette.is_open(cx);
        let palette_admitted = self.project_actions_admitted(cx);
        let owner = cx.entity().downgrade();
        let state = window.use_keyed_state(STATE_KEY, cx, |window, cx| {
            let menu = cx.new(|cx| {
                let mut menu = Menu::new_context(dark, project_items(project_admitted), cx);
                menu.set_min_width(px(152.), cx);
                menu
            });
            let observer = cx.observe(&menu, |_, _, cx| cx.notify());
            let subscription = cx.subscribe_in(&menu, window, move |_, _, event, w, cx| {
                let action = match event {
                    MenuEvent::Selected(id) if id == "local" => Action::OpenLocalProject,
                    MenuEvent::Selected(id) if id == "remote" => Action::OpenRemoteProject,
                    _ => return,
                };
                let _ = owner.update(cx, |s, cx| s.action(action, w, cx));
            });
            State {
                waiting: false,
                visible: false,
                task: None,
                project_focus: cx.focus_handle(),
                palette_focus: cx.focus_handle(),
                local_focus: cx.focus_handle(),
                remote_focus: cx.focus_handle(),
                menu,
                menu_admitted: project_admitted,
                dark,
                menu_anchor: point(px(0.), px(30.)),
                _menu_subscription: subscription,
                _menu_observer: observer,
            }
        });
        let waiting = self.first_run_loading();
        let owner = cx.entity().downgrade();
        state.update(cx, |state, cx| {
            if state.menu_admitted != project_admitted {
                state.menu_admitted = project_admitted;
                state.menu.update(cx, |menu, cx| {
                    menu.set_items(project_items(project_admitted), cx);
                    if !project_admitted {
                        menu.dismiss(cx);
                    }
                });
            }
            if state.dark != dark {
                state.dark = dark;
                state
                    .menu
                    .update(cx, |menu, cx| menu.set_theme(dark, window, cx));
            }
            if state.waiting == waiting {
                return;
            }
            state.task = None;
            state.waiting = waiting;
            state.visible = false;
            if waiting {
                state.task = Some(cx.spawn(async move |state, cx| {
                    cx.background_executor().timer(LOADING_DELAY).await;
                    let current = owner
                        .update(cx, |s, _| s.first_run_loading())
                        .unwrap_or(false);
                    if current {
                        let _ = state.update(cx, |state, cx| {
                            if state.waiting {
                                state.visible = true;
                                cx.notify();
                            }
                        });
                    }
                }));
            }
        });
        let retained = state.read(cx);
        let visible = waiting && retained.visible;
        let project_focus = retained.project_focus.clone();
        let palette_focus = retained.palette_focus.clone();
        let local_focus = retained.local_focus.clone();
        let remote_focus = retained.remote_focus.clone();
        let menu = retained.menu.clone();
        let menu_open = menu.read(cx).is_open();
        let anchor_state = state.clone();
        let trigger_state = state.clone();
        let t = support::tokens(self.dark);
        let titlebar = div()
            .id("first-run-titlebar")
            .test_support()
            .flex()
            .items_center()
            .w_full()
            .h(px(30.))
            .flex_shrink_0()
            .pr(px(8.))
            .child(
                div()
                    .id("first-run-drag-region")
                    .test_support()
                    .flex_1()
                    .h_full()
                    .min_w_0()
                    .on_mouse_down(MouseButton::Left, |event, window, _| {
                        if event.click_count == 2 {
                            window.titlebar_double_click();
                        } else if event.click_count == 1 {
                            window.start_window_move();
                        }
                    }),
            )
            .child(
                div()
                    .on_prepaint(move |bounds, _, cx| {
                        anchor_state.update(cx, |state, _| {
                            state.menu_anchor = point(bounds.left(), bounds.bottom() + px(5.));
                        });
                    })
                    .child(
                        first_run_icon("first-run-project", project_admitted, self.dark)
                            .track_focus(&project_focus)
                            .selected(menu_open)
                            .when(menu_open, |b| b.bg(rgb(t.hover)).text_color(rgb(t.text)))
                            .accessibility_label("Open Project")
                            .tooltip(|_, cx| {
                                cx.new(|_| FirstRunTooltip("Open Project · ⌘O")).into()
                            })
                            .on_click(move |_, window, cx| {
                                trigger_state.update(cx, |state, cx| {
                                    let anchor = state.menu_anchor;
                                    let origin = state.project_focus.clone();
                                    state.menu.update(cx, |menu, cx| {
                                        if menu.is_open() {
                                            menu.dismiss(cx);
                                            origin.focus(window, cx);
                                        } else {
                                            menu.open_at(anchor, Some(origin), window, cx);
                                        }
                                    });
                                });
                            })
                            .child(svg().path("product-icons/folder-plus.svg").size(px(16.))),
                    ),
            )
            .child(
                first_run_icon("first-run-palette", palette_admitted, self.dark)
                    .track_focus(&palette_focus)
                    .accessibility_label("Open Command Palette")
                    .tooltip(|_, cx| cx.new(|_| FirstRunTooltip("Command Palette · ⌘K")).into())
                    .on_click(cx.listener(|s, _, w, cx| s.action(Action::Palette, w, cx)))
                    .child(svg().path("product-icons/search.svg").size(px(16.))),
            );
        let empty = self
            .workspace
            .as_ref()
            .is_some_and(|w| w.projects.is_empty());
        let content = div()
            .id("first-run-content")
            .test_support()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .items_center()
            .justify_center()
            .p(px(24.))
            .text_color(rgb(t.muted))
            .when(visible, |content| {
                content.child(
                    div()
                        .id("first-run-loading")
                        .test_support()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(10.))
                        .text_size(px(11.))
                        .child(components::transcript::working_spinner(
                            "Loading workspace".into(),
                            t.muted,
                            cx.reduce_motion(),
                        ))
                        .child("Loading workspace"),
                )
            })
            .when(empty, |content| {
                content
                    .child(
                        div()
                            .id("first-run-version")
                            .test_support()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .px(px(6.))
                            .py(px(4.))
                            .mb(px(14.))
                            .font_family("Menlo")
                            .text_size(px(10.))
                            .line_height(relative(1.25))
                            .child(div().text_color(rgb(t.text)).child("tau"))
                            .child(
                                div()
                                    .id("first-run-version-number")
                                    .test_support()
                                    .text_color(rgb(t.faint))
                                    .child(app_version()),
                            ),
                    )
                    .child(
                        div()
                            .id("first-run-actions")
                            .test_support()
                            .flex()
                            .gap(px(2.))
                            .child(
                                first_run_button("first-run-local", project_admitted, self.dark)
                                    .track_focus(&local_focus)
                                    .on_click(cx.listener(|s, _, w, cx| {
                                        s.action(Action::OpenLocalProject, w, cx)
                                    }))
                                    .child("Open Local Project"),
                            )
                            .child(
                                first_run_button("first-run-remote", project_admitted, self.dark)
                                    .track_focus(&remote_focus)
                                    .on_click(cx.listener(|s, _, w, cx| {
                                        s.action(Action::OpenRemoteProject, w, cx)
                                    }))
                                    .child("Open Remote Project"),
                            ),
                    )
            });
        Some(
            div()
                .id("first-run")
                .test_support()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .min_h_0()
                .h_full()
                .child(titlebar)
                .child(content)
                .child(menu)
                .into_any_element(),
        )
    }
}

fn project_items(admitted: bool) -> Vec<MenuItem> {
    [
        ("local", "Open Local Project"),
        ("remote", "Open Remote Project"),
    ]
    .into_iter()
    .map(|(id, label)| MenuItem {
        id: id.into(),
        element_id: format!("first-run-menu-{id}"),
        label: label.into(),
        shortcut: None,
        disabled: !admitted,
    })
    .collect()
}

fn first_run_button(id: &'static str, admitted: bool, dark: bool) -> Button {
    let t = support::tokens(dark);
    Button::new(id)
        .disabled(!admitted)
        .h(px(28.))
        .px(px(11.))
        .rounded(px(4.))
        .border_1()
        .border_color(gpui_kit::rgba(0))
        .bg(gpui_kit::rgba(0))
        .text_size(px(11.))
        .font_weight(gpui_kit::FontWeight::MEDIUM)
        .text_color(rgb(t.muted))
        .opacity(if admitted { 1. } else { 0.35 })
        .when(admitted, |b| {
            b.hover(move |b| b.bg(rgb(t.hover)).text_color(rgb(t.text)))
        })
        .focus_visible(move |b| {
            b.border_color(rgb(t.accent)).shadow(vec![
                gpui_kit::BoxShadow::new(px(0.), px(0.), rgb(t.accent).into())
                    .spread_radius(px(1.))
                    .inset(),
            ])
        })
}

fn first_run_icon(id: &'static str, admitted: bool, dark: bool) -> Button {
    let t = support::tokens(dark);
    first_run_button(id, admitted, dark)
        .size(px(24.))
        .p_0()
        .border_0()
        .focus_visible(move |b| {
            b.text_color(rgb(t.text)).shadow(vec![
                gpui_kit::BoxShadow::new(
                    px(0.),
                    px(0.),
                    gpui_kit::rgba((t.accent << 8) | 0x38).into(),
                )
                .spread_radius(px(2.)),
            ])
        })
}

#[cfg(test)]
#[path = "first_run_tests.rs"]
mod tests;
