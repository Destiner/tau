//! Retained sidebar issue reporter. Persistence is acknowledged by the shell worker.
use std::{cell::Cell, rc::Rc, time::Duration};

use super::{font, support};
use gpui_kit::{
    App, Bounds, BoxShadow, Context, Entity, EntityInputHandler, EventEmitter, FocusHandle,
    Focusable, IntoElement, KeyDownEvent, MouseDownEvent, Pixels, Render, Subscription, Window,
    base::{
        Align, Button, Checkbox, ElementExt, InputBase, Placement, Positioner, TestSupportExt,
        Textarea,
        input::{Enter, Escape, InputEvent, TextareaState},
    },
    div,
    prelude::*,
    px, rgb, rgba,
};

const ERROR: &str = "The report could not be saved. Try again.";
const MAX_LENGTH: usize = 10_000;
type AnchorBounds = Rc<Cell<Option<Bounds<Pixels>>>>;

// UiCheckbox's own SVG, scaled into the 11px interior of its bordered 13px box.
const CHECK_POINTS: [(f32, f32); 3] = [(2.6, 6.3), (4.8, 8.6), (9.4, 3.6)];
const CHECK_STROKE: f32 = 1.9;
fn report_checkmark(color: gpui_kit::Rgba) -> impl IntoElement {
    gpui_kit::canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let scale = bounds.size.width / 12.;
            let points =
                CHECK_POINTS.map(|(x, y)| bounds.origin + gpui_kit::point(scale * x, scale * y));
            let width = scale * CHECK_STROKE;
            for segment in points.windows(2) {
                let mut path = gpui_kit::PathBuilder::stroke(width);
                path.move_to(segment[0]);
                path.line_to(segment[1]);
                if let Ok(path) = path.build() {
                    window.paint_path(path, color);
                }
            }
            // Round caps and join, as specified by the inline SVG (not the shared icon).
            for center in points {
                window.paint_quad(
                    gpui_kit::fill(
                        Bounds::new(
                            center - gpui_kit::point(width / 2., width / 2.),
                            gpui_kit::size(width, width),
                        ),
                        color,
                    )
                    .corner_radii(width / 2.),
                );
            }
        },
    )
    .size(px(11.))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// The shell captures its current Pi identity here, not at popup creation.
    Submit {
        description: String,
        include_session: bool,
    },
    Dismissed,
}

pub struct AdminControls {
    dark: bool,
    enabled: bool,
    open: bool,
    has_session: bool,
    include_session: bool,
    busy: bool,
    submitted_draft: Option<String>,
    error: bool,
    sent: bool,
    suspended: bool,
    editor: Entity<TextareaState>,
    editor_style: gpui_kit::base::input::InputEditorStyle,
    trigger_focus: FocusHandle,
    checkbox_focus: FocusHandle,
    panel_focus: FocusHandle,
    trigger_bounds: AnchorBounds,
    hint_epoch: usize,
    hint_visible: bool,
    _input_events: Subscription,
}
impl EventEmitter<Event> for AdminControls {}
impl AdminControls {
    pub fn set_theme(&mut self, dark: bool, _: &mut Window, cx: &mut Context<Self>) {
        if self.dark == dark {
            return;
        }
        self.dark = dark;
        let mut style = self.editor_style.clone();
        let theme = support::input_style(dark);
        style.foreground = theme.foreground;
        style.muted_foreground = theme.muted_foreground;
        style.background = theme.background;
        style.border = theme.border;
        style.selection = theme.selection;
        style.caret = theme.caret;
        self.set_editor_style(style, cx);
    }

    #[cfg(test)]
    pub fn editor_style(&self) -> &gpui_kit::base::input::InputEditorStyle {
        &self.editor_style
    }

    pub fn set_editor_style(
        &mut self,
        style: gpui_kit::base::input::InputEditorStyle,
        cx: &mut Context<Self>,
    ) {
        self.editor_style = style.clone();
        self.editor.update(cx, |editor, cx| {
            editor.set_editor_style(style);
            cx.notify();
        });
        cx.notify();
    }

    pub fn new(dark: bool, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let editor = cx.new(|cx| {
            TextareaState::new(window, cx)
                .accessibility_label("Describe the Issue")
                .rows(6)
                .submit_on_enter(false)
        });
        editor.update(cx, |editor, _| {
            editor.set_editor_style(support::input_style(dark));
            editor.set_editor_paddings(gpui_kit::Edges {
                top: px(6.),
                right: px(8.),
                bottom: px(6.),
                left: px(8.),
            });
        });
        let input_events = cx.subscribe_in(
            &editor,
            window,
            |this, _editor, event: &InputEvent, _window, cx| {
                if matches!(event, InputEvent::Change) {
                    this.error = false;
                    this.sent = false;
                    cx.notify();
                } else if matches!(event, InputEvent::Focus | InputEvent::Blur) {
                    cx.notify();
                }
            },
        );
        Self {
            dark,
            enabled: false,
            open: false,
            has_session: false,
            include_session: false,
            busy: false,
            submitted_draft: None,
            error: false,
            sent: false,
            suspended: false,
            editor,
            editor_style: support::input_style(dark),
            trigger_focus: cx.focus_handle(),
            checkbox_focus: cx.focus_handle(),
            panel_focus: cx.focus_handle(),
            trigger_bounds: Rc::new(Cell::new(None)),
            hint_epoch: 0,
            hint_visible: false,
            _input_events: input_events,
        }
    }
    pub fn enabled(&self) -> bool {
        self.enabled
    }
    #[cfg(test)]
    pub fn is_busy(&self) -> bool {
        self.busy
    }
    pub fn editor(&self) -> Entity<TextareaState> {
        self.editor.clone()
    }
    pub fn is_editing(&self, window: &Window, cx: &App) -> bool {
        self.open && self.editor.read(cx).focus_handle(cx).is_focused(window)
    }
    pub fn set_enabled(&mut self, enabled: bool, _window: &mut Window, cx: &mut Context<Self>) {
        if self.enabled == enabled {
            return;
        }
        if !enabled {
            // Hiding is not cancellation of an accepted write. Keep its draft
            // readonly until the worker settles, but dismiss even while busy.
            if self.open {
                self.open = false;
                cx.emit(Event::Dismissed);
            }
            self.dismiss_hint();
        }
        self.enabled = enabled;
        cx.notify();
    }
    /// Only availability is retained; no session identifier or data is stored.
    pub fn set_session_available(&mut self, available: bool, cx: &mut Context<Self>) {
        if self.has_session == available {
            return;
        }
        self.has_session = available;
        cx.notify();
    }
    /// Hide beneath higher-priority remote, palette, and confirmation surfaces.
    pub fn set_suspended(&mut self, suspended: bool, cx: &mut Context<Self>) {
        if self.suspended == suspended {
            return;
        }
        self.suspended = suspended;
        cx.notify();
    }
    pub fn is_open(&self) -> bool {
        self.open
    }
    pub fn complete(&mut self, saved: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !self.busy {
            return;
        }
        let same_draft = self
            .submitted_draft
            .take()
            .is_some_and(|draft| self.editor.read(cx).value().as_ref() == draft);
        self.set_busy(false, cx);
        if !same_draft {
            return;
        }
        if saved {
            self.editor
                .update(cx, |editor, cx| editor.set_value("", window, cx));
            self.include_session = false;
            self.error = false;
            self.sent = true;
        } else {
            self.error = true;
        }
        cx.notify();
    }
    pub fn open_reporter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.enabled || self.suspended {
            return;
        }
        self.open = true;
        self.dismiss_hint();
        if self.has_session && !self.busy {
            self.checkbox_focus.focus(window, cx);
        } else {
            self.editor.read(cx).focus_handle(cx).focus(window, cx);
        }
        cx.notify();
    }
    fn close(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.dismiss(true, cx);
    }
    fn dismiss(&mut self, restore_focus: bool, cx: &mut Context<Self>) {
        if !self.open || (restore_focus && self.busy) {
            return;
        }
        self.open = false;
        // Pointer dismissal must keep the control the user just clicked focused.
        if restore_focus {
            cx.emit(Event::Dismissed);
        }
        cx.notify();
    }
    fn set_busy(&mut self, busy: bool, cx: &mut Context<Self>) {
        self.busy = busy;
        self.editor
            .update(cx, |editor, cx| editor.set_readonly(busy, cx));
        cx.notify();
    }
    fn toggle_session(&mut self, cx: &mut Context<Self>) {
        if self.has_session && !self.busy && !self.suspended {
            self.include_session = !self.include_session;
            cx.notify();
        }
    }
    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.enabled
            || !self.open
            || self.busy
            || self.suspended
            || self.editor.read(cx).value().trim().is_empty()
        {
            return;
        }
        if self.editor.read(cx).value().encode_utf16().count() > MAX_LENGTH {
            self.error = true;
            cx.notify();
            return;
        }
        self.error = false;
        self.sent = false;
        // Disabling the focused checkbox or Submit must not strand keyboard routing.
        // Readonly editors retain focus; unrelated inputs/popups must not lose theirs.
        if self.panel_focus.contains_focused(window, cx)
            && !self.editor.read(cx).focus_handle(cx).is_focused(window)
        {
            self.panel_focus.focus(window, cx);
        }
        self.submitted_draft = Some(self.editor.read(cx).value().to_string());
        self.set_busy(true, cx);
        cx.emit(Event::Submit {
            description: self.editor.read(cx).value().trim().to_string(),
            include_session: self.include_session && self.has_session,
        });
        cx.notify();
    }
    fn key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !self.open || self.suspended {
            return;
        }
        let composing = self.editor.update(cx, |editor, cx| {
            editor.marked_text_range(window, cx).is_some()
        });
        if composing {
            return;
        }
        let key = event.keystroke.key.to_lowercase();
        // Input actions run before raw key callbacks. Escape can unmark IME text
        // during that action; do not interpret its later raw event as dismissal.
        if key == "escape" && self.editor.read(cx).focus_handle(cx).is_focused(window) {
            return;
        }
        let modifiers = event.keystroke.modifiers;
        match key.as_str() {
            "escape" => self.close(window, cx),
            "enter" if modifiers.platform || modifiers.control => self.submit(window, cx),
            "s" if (modifiers.platform || modifiers.control) && modifiers.shift => {
                self.toggle_session(cx)
            }
            _ => return,
        }
        cx.stop_propagation();
        window.prevent_default();
    }
    fn dismiss_hint(&mut self) {
        self.hint_epoch += 1;
        self.hint_visible = false;
    }
    fn hover(&mut self, hovered: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.dismiss_hint();
        cx.notify();
        if !hovered || self.open || !self.enabled {
            return;
        }
        let epoch = self.hint_epoch;
        cx.spawn_in(window, async move |entity, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(450))
                .await;
            let _ = entity.update_in(cx, |this, _, cx| {
                if this.hint_epoch == epoch && this.enabled && !this.open {
                    this.hint_visible = true;
                    cx.notify();
                }
            });
        })
        .detach();
    }
    fn panel(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = support::tokens(self.dark);
        let disabled = !self.has_session || self.busy;
        let checked = self.include_session;
        let checkbox_color = |color| {
            let mut color = rgb(color);
            if disabled {
                let backdrop = rgb(t.panel_raised);
                color.r = color.r * 0.45 + backdrop.r * 0.55;
                color.g = color.g * 0.45 + backdrop.g * 0.55;
                color.b = color.b * 0.45 + backdrop.b * 0.55;
            }
            color
        };
        let focused = self.editor.read(cx).focus_handle(cx).is_focused(window);
        let scene = cx.entity().downgrade();
        let trigger = self.trigger_bounds.clone();
        let font = window.text_system().resolve_font(&gpui_kit::Font {
            family: font::family().into(),
            ..Default::default()
        });
        let normal_leading = |size| {
            window.text_system().ascent(font, px(size))
                + window.text_system().descent(font, px(size)).abs()
        };
        let checkbox = Checkbox::new("report-include-session")
            .track_focus(&self.checkbox_focus)
            .accessibility_label("Include Current Session")
            .checked(checked)
            .disabled(disabled)
            .size(px(13.))
            .flex_shrink_0()
            .border_1()
            .rounded(px(3.))
            .border_color(checkbox_color(if checked { t.accent } else { t.border }))
            .bg(if checked {
                checkbox_color(t.accent)
            } else {
                rgba(0x00000000)
            })
            .when(!disabled, |checkbox| {
                checkbox.hover(move |d| {
                    if checked {
                        d
                    } else {
                        d.border_color(rgb(t.hover_border)).bg(rgb(t.hover))
                    }
                })
            })
            .focus_visible(move |d| {
                d.border_color(rgb(t.accent)).shadow(vec![
                    BoxShadow::new(px(0.), px(0.), rgba((t.accent << 8) | 0x38).into())
                        .spread_radius(px(2.)),
                ])
            })
            .when(checked, |c| {
                c.child(report_checkmark(checkbox_color(t.canvas)))
            })
            .on_change(move |_, _, _, cx| {
                let _ = scene.update(cx, |this, cx| this.toggle_session(cx));
            });
        div()
            .id("issue-report-popover")
            .track_focus(&self.panel_focus)
            .test_support()
            .font_family(font::family())
            .text_color(rgb(t.text))
            .text_size(px(11.))
            .line_height(normal_leading(11.))
            .w(px(280.).min((window.viewport_size().width - px(18.)).max(px(0.))))
            .p(px(10.))
            .rounded(px(8.))
            .border_1()
            .border_color(rgb(t.border))
            .bg(rgb(t.panel_raised))
            .shadow(vec![
                BoxShadow::new(
                    px(0.),
                    px(8.),
                    rgba(if self.dark { 0x00000073 } else { 0x0000001f }).into(),
                )
                .blur_radius(px(10.)),
            ])
            .occlude()
            .flex()
            .flex_col()
            .gap(px(9.))
            .key_context("TauIssueReporter")
            .capture_key_down(cx.listener(Self::key))
            // Base resolves registered Input actions before raw key callbacks.
            // Capture secondary Enter so submission never inserts a newline.
            .capture_action(cx.listener(|this, action: &Enter, window, cx| {
                if this.suspended {
                    cx.propagate();
                    return;
                }
                let composing = this.editor.update(cx, |editor, cx| {
                    editor.marked_text_range(window, cx).is_some()
                });
                if action.secondary && !composing {
                    this.submit(window, cx);
                    cx.stop_propagation();
                } else {
                    cx.propagate();
                }
            }))
            .capture_action(cx.listener(|this, _: &Escape, window, cx| {
                if this.suspended {
                    cx.propagate();
                    return;
                }
                let composing = this.editor.update(cx, |editor, cx| {
                    editor.marked_text_range(window, cx).is_some()
                });
                if composing {
                    cx.propagate();
                } else {
                    this.close(window, cx);
                    cx.stop_propagation();
                }
            }))
            .on_mouse_down_out(cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                if this.suspended {
                    return;
                }
                if !trigger
                    .get()
                    .is_some_and(|bounds| bounds.contains(&event.position))
                {
                    this.dismiss(false, cx);
                }
            }))
            .child(
                div()
                    .id("report-include-row")
                    .test_support()
                    .flex()
                    .items_center()
                    .gap(px(7.))
                    .text_color(rgb(if disabled { t.faint } else { t.muted }))
                    .child(checkbox)
                    .child(
                        div()
                            .id("report-include-label")
                            .test_support()
                            .child("Include Current Session")
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_session(cx))),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(5.))
                    .text_size(px(10.))
                    .line_height(normal_leading(10.))
                    .text_color(rgb(t.muted))
                    .child(
                        div()
                            .id("report-description-label")
                            .test_support()
                            .child("Describe the Issue"),
                    )
                    .child(
                        div().id("report-description").test_support().child(
                            InputBase::new("report-description-field")
                                .role(None)
                                .w_full()
                                .h(px(108.))
                                .min_h(px(108.))
                                .rounded(px(4.))
                                .border_1()
                                .border_color(rgb(if focused { t.accent } else { t.border }))
                                .bg(rgb(t.canvas))
                                .when(focused, |field| {
                                    field.shadow(vec![
                                        BoxShadow::new(
                                            px(0.),
                                            px(0.),
                                            rgba((t.accent << 8) | 0x38).into(),
                                        )
                                        .spread_radius(px(2.)),
                                    ])
                                })
                                .text_size(px(11.))
                                .line_height(gpui_kit::relative(1.45))
                                .text_color(rgb(t.text))
                                .child(Textarea::new(&self.editor)),
                        ),
                    ),
            )
            .when(self.sent, |d| {
                d.child(
                    div()
                        .id("report-sent")
                        .test_support()
                        .text_color(rgb(t.muted))
                        .child("Report saved locally."),
                )
            })
            .when(self.error, |d| {
                d.child(
                    div()
                        .id("report-error")
                        .test_support()
                        .text_size(px(10.))
                        .line_height(gpui_kit::relative(1.45))
                        .text_color(rgb(t.danger))
                        .child(
                            if self.editor.read(cx).value().encode_utf16().count() > MAX_LENGTH {
                                "Use at most 10,000 characters."
                            } else {
                                ERROR
                            },
                        ),
                )
            })
            .child(
                div().flex().justify_end().child(
                    support::button_sm(
                        "report-submit",
                        if self.busy { "Saving…" } else { "Submit" },
                        support::ButtonVariant::Primary,
                        self.dark,
                    )
                    .backdrop(t.panel_raised)
                    .disabled(self.busy || self.editor.read(cx).value().trim().is_empty())
                    .on_click(cx.listener(Self::submit_click)),
                ),
            )
    }
    fn submit_click(
        &mut self,
        _: &gpui_kit::ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.submit(window, cx);
    }
}
impl Render for AdminControls {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = support::tokens(self.dark);
        let bounds = self.trigger_bounds.clone();
        let entity_id = cx.entity_id();
        let floating = self.open || self.hint_visible;
        let mut root = div()
            .id("admin-controls")
            .test_support()
            .flex()
            .flex_shrink_0();
        if !self.enabled || self.suspended {
            return root;
        }
        root = root.child(
            Button::new("report-trigger")
                .accessibility_label("Report an Issue")
                .track_focus(&self.trigger_focus)
                .on_prepaint(move |b, _, cx| {
                    if bounds.replace(Some(b)) != Some(b) && floating {
                        cx.notify(entity_id);
                    }
                })
                .size(px(28.))
                .p_0()
                .rounded(px(4.))
                .bg(rgba(0x00000000))
                .opacity(0.65)
                .hover(|b| b.opacity(1.))
                .focus_visible(move |b| b.bg(rgb(t.hover)).opacity(1.))
                .child(support::icon("bug", 16., t.muted))
                .on_hover(cx.listener(|this, hovered, window, cx| this.hover(*hovered, window, cx)))
                .on_click(cx.listener(|this, _, window, cx| {
                    if this.open {
                        this.close(window, cx);
                    } else {
                        this.open_reporter(window, cx);
                    }
                })),
        );
        if let Some(anchor) = self.trigger_bounds.get() {
            if self.open {
                root = root.child(
                    gpui_kit::deferred(
                        Positioner::side(anchor)
                            .placement(Placement::Top)
                            .align(Align::End)
                            .offset(px(5.))
                            // Reka's default collisionPadding is zero; the width cap
                            // is independent of the surface's collision boundary.
                            .margin(px(0.))
                            .child(self.panel(window, cx)),
                    )
                    .priority(20),
                );
            } else if self.hint_visible {
                root = root.child(
                    gpui_kit::deferred(
                        Positioner::side(anchor)
                            .placement(Placement::Top)
                            .offset(px(6.))
                            .margin(px(6.))
                            .child(
                                div()
                                    .id("report-tooltip")
                                    .test_support()
                                    .font_family(font::family())
                                    .max_w(px(260.))
                                    .px(px(7.))
                                    .py(px(3.))
                                    .rounded(px(4.))
                                    .border_1()
                                    .border_color(rgb(t.border))
                                    .bg(rgb(t.panel_raised))
                                    .shadow(vec![
                                        BoxShadow::new(
                                            px(0.),
                                            px(8.),
                                            rgba(if self.dark { 0x00000073 } else { 0x0000001f })
                                                .into(),
                                        )
                                        .blur_radius(px(10.)),
                                    ])
                                    .text_color(rgb(t.text))
                                    .text_size(px(10.))
                                    .line_height(gpui_kit::relative(1.25))
                                    .child("Report an Issue"),
                            ),
                    )
                    .priority(100),
                );
            }
        }
        root
    }
}
