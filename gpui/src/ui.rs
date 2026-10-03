use anyhow::{Result, ensure};
use gpui_kit::{
    App, Context, Entity, FocusHandle, Focusable, IntoElement, Render, Subscription, Window,
    base::{
        Button, ScrollbarStyles, Theme,
        input::{
            Input, InputBase, InputEditorStyle, InputEvent, InputState, RopeExt, Textarea,
            TextareaState,
        },
    },
    div,
    prelude::*,
    px, rgb,
};

use crate::model::{Observation, Props};

fn monospace_family() -> &'static str {
    // Match CSS ui-monospace, not its Menlo fallback, on macOS.
    if cfg!(target_os = "macos") {
        ".AppleSystemUIFontMonospaced"
    } else {
        "monospace"
    }
}

#[derive(Clone, Copy)]
struct Palette {
    canvas: u32,
    text: u32,
    muted: u32,
    accent: u32,
    danger: u32,
    border: u32,
    hover_border: u32,
    hover: u32,
    on_accent: u32,
    faint: u32,
    selection: u32,
}
impl Palette {
    fn new(dark: bool) -> Self {
        if dark {
            Self {
                canvas: 0x10141c,
                text: 0xbfbdb6,
                muted: 0x6c7380,
                accent: 0xe6b450,
                danger: 0xd95757,
                border: 0x1b1f29,
                hover_border: 0x3f444f,
                hover: 0x1a202b,
                on_accent: 0x10141c,
                faint: 0x5a6378,
                selection: 0x3388ff40,
            }
        } else {
            Self {
                canvas: 0xfcfcfc,
                text: 0x5c6166,
                muted: 0x6e757f,
                accent: 0xf29718,
                danger: 0xe65050,
                border: 0xd9dee2,
                hover_border: 0xa9afb6,
                hover: 0xeeeff1,
                on_accent: 0x10141c,
                faint: 0xa1a8b2,
                selection: 0x035bd626,
            }
        }
    }
}

fn mix(a: u32, b: u32, weight: f32) -> gpui_kit::Rgba {
    let a = rgb(a);
    let b = rgb(b);
    gpui_kit::Rgba {
        r: a.r * weight + b.r * (1. - weight),
        g: a.g * weight + b.g * (1. - weight),
        b: a.b * weight + b.b * (1. - weight),
        a: 1.,
    }
}

impl Palette {
    fn editor_style(self, readonly: bool, hide_caret: bool, disabled: bool) -> InputEditorStyle {
        InputEditorStyle {
            foreground: rgb(if readonly { self.muted } else { self.text }).into(),
            muted_foreground: rgb(self.faint).into(),
            background: rgb(self.canvas).into(),
            border: rgb(self.border).into(),
            selection: gpui_kit::rgba(self.selection).into(),
            caret: rgb(self.accent).into(),
            caret_width: Some(px(2.)),
            center_caret: true,
            caret_visible: !hide_caret,
            unfocused_selection: disabled,
            disabled_opacity: 1.,
            ..Default::default()
        }
    }
}

enum Control {
    Button(FocusHandle),
    Input(Entity<InputState>),
    Textarea(Entity<TextareaState>),
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum TestFault {
    #[default]
    None,
    HideControl,
    DropClicks,
    DropChanges,
}

pub struct Scene {
    fault: TestFault,
    capture_mode: bool,
    capture_caret: bool,
    props: Props,
    palette: Palette,
    control: Control,
    clicks: usize,
    changes: usize,
    fallback_focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

gpui_kit::actions!(tau_prototype, [NextField, PreviousField]);

pub fn init(cx: &mut App) {
    let theme = Theme::global_mut(cx);
    theme.scrollbar = theme
        .scrollbar
        .clone()
        .with_styles(ScrollbarStyles::default().thumb(|thumb| thumb.radius(px(4.))));
    cx.bind_keys([
        gpui_kit::KeyBinding::new("tab", NextField, Some("TauScene")),
        gpui_kit::KeyBinding::new("shift-tab", PreviousField, Some("TauScene")),
    ]);
}

fn checked_selection(value: &str, start: usize, end: usize) -> Result<()> {
    let mut boundaries = vec![0];
    let mut offset = 0;
    for ch in value.chars() {
        offset += ch.len_utf16();
        boundaries.push(offset);
    }
    ensure!(
        start <= end && boundaries.contains(&start) && boundaries.contains(&end),
        "selection must use in-range UTF-16 scalar boundaries"
    );
    Ok(())
}

impl Scene {
    pub fn new(
        component: String,
        props: Props,
        dark: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut subscriptions = Vec::new();
        let control = match component.as_str() {
            "input" | "UiInput" => {
                let state = cx.new(|cx| {
                    InputState::new(window, cx)
                        .default_value(props.value.clone())
                        .placeholder(props.placeholder.clone())
                });
                state.update(cx, |state, cx| {
                    state.set_disabled(props.disabled, cx);
                    state.set_readonly(props.readonly, cx);
                    let end = state.text().len();
                    state.set_selected_range(end..end, cx);
                });
                subscriptions.push(cx.subscribe_in(
                    &state,
                    window,
                    |this, _, event: &InputEvent, _, cx| {
                        if matches!(event, InputEvent::Change)
                            && this.fault != TestFault::DropChanges
                        {
                            this.changes += 1;
                            cx.notify();
                        }
                    },
                ));
                Control::Input(state)
            }
            "textarea" | "UiTextarea" => {
                let state = cx.new(|cx| {
                    TextareaState::new(window, cx)
                        .default_value(props.value.clone())
                        .placeholder(props.placeholder.clone())
                        .rows(props.rows.max(1))
                });
                state.update(cx, |state, cx| {
                    state.set_disabled(props.disabled, cx);
                    state.set_readonly(props.readonly, cx);
                    let end = state.text().len();
                    state.set_selected_range(end..end, cx);
                    state.set_scroll_sensitivity(0.5);
                    // Multi-line padding belongs inside the engine's scrolling viewport.
                    state.set_editor_paddings(gpui_kit::Edges {
                        top: px(6.),
                        right: px(8.),
                        bottom: px(6.),
                        left: px(8.),
                    });
                });
                subscriptions.push(cx.subscribe_in(
                    &state,
                    window,
                    |this, _, event: &InputEvent, _, cx| {
                        if matches!(event, InputEvent::Change)
                            && this.fault != TestFault::DropChanges
                        {
                            this.changes += 1;
                            cx.notify();
                        }
                    },
                ));
                Control::Textarea(state)
            }
            _ => Control::Button(cx.focus_handle()),
        };
        let fallback_focus = cx.focus_handle().tab_stop(false);
        fallback_focus.focus(window, cx);
        Self {
            fault: TestFault::None,
            capture_mode: false,
            capture_caret: false,
            props,
            palette: Palette::new(dark),
            control,
            clicks: 0,
            changes: 0,
            fallback_focus,
            _subscriptions: subscriptions,
        }
    }

    pub fn set_theme(&mut self, dark: bool, cx: &mut Context<Self>) {
        self.palette = Palette::new(dark);
        cx.notify();
    }

    pub fn prepare_capture(&mut self, fault: TestFault) {
        self.fault = fault;
        self.capture_mode = true;
    }

    pub fn set_capture_caret(&mut self, visible: bool, cx: &mut Context<Self>) {
        self.capture_caret = visible;
        cx.notify();
    }

    pub fn observe(&self, window: &Window, cx: &App) -> Observation {
        let (value, selection, focused, disabled, readonly) = match &self.control {
            Control::Button(focus) => (
                self.props.label.clone(),
                None,
                focus.is_focused(window),
                self.props.disabled,
                self.props.readonly,
            ),
            Control::Input(state) => {
                let state = state.read(cx);
                let range = state.selected_range();
                (
                    state.value().to_string(),
                    Some([
                        state.text().offset_to_offset_utf16(range.start),
                        state.text().offset_to_offset_utf16(range.end),
                    ]),
                    state.focus_handle(cx).is_focused(window),
                    state.presentation().is_disabled(),
                    state.presentation().is_readonly(),
                )
            }
            Control::Textarea(state) => {
                let state = state.read(cx);
                let range = state.selected_range();
                (
                    state.value().to_string(),
                    Some([
                        state.text().offset_to_offset_utf16(range.start),
                        state.text().offset_to_offset_utf16(range.end),
                    ]),
                    state.focus_handle(cx).is_focused(window),
                    state.presentation().is_disabled(),
                    state.presentation().is_readonly(),
                )
            }
        };
        Observation {
            value,
            selection,
            focused,
            disabled,
            readonly,
            clicks: self.clicks,
            changes: self.changes,
        }
    }

    pub fn blur(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.fallback_focus.focus(window, cx);
    }

    fn next_field(&mut self, _: &NextField, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(false, window, cx);
    }

    fn previous_field(&mut self, _: &PreviousField, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(true, window, cx);
    }

    fn navigate(&mut self, backwards: bool, window: &mut Window, cx: &mut Context<Self>) {
        if backwards {
            window.focus_prev(cx);
        } else {
            window.focus_next(cx);
        }
        if let Control::Input(state) = &self.control
            && state.read(cx).focus_handle(cx).is_focused(window)
        {
            state.update(cx, |state, cx| {
                state.set_selected_range(0..state.text().len(), cx)
            });
        }
    }

    pub fn focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.props.disabled {
            return;
        }
        match &self.control {
            Control::Button(handle) => handle.focus(window, cx),
            Control::Input(state) => state.update(cx, |state, cx| state.focus(window, cx)),
            Control::Textarea(state) => state.update(cx, |state, cx| state.focus(window, cx)),
        }
        cx.notify();
    }

    pub fn select(&mut self, start: usize, end: usize, cx: &mut Context<Self>) -> Result<()> {
        ensure!(start <= end, "selection start exceeds end");
        match &self.control {
            Control::Input(state) => state.update(cx, |state, cx| -> Result<()> {
                let text = state.text();
                checked_selection(&text.to_string(), start, end)?;
                state.set_selected_range(
                    text.offset_utf16_to_offset(start)..text.offset_utf16_to_offset(end),
                    cx,
                );
                Ok(())
            })?,
            Control::Textarea(state) => state.update(cx, |state, cx| -> Result<()> {
                let text = state.text();
                checked_selection(&text.to_string(), start, end)?;
                state.set_selected_range(
                    text.offset_utf16_to_offset(start)..text.offset_utf16_to_offset(end),
                    cx,
                );
                Ok(())
            })?,
            Control::Button(_) => anyhow::bail!("button has no text selection"),
        }
        Ok(())
    }

    pub fn patch(
        &mut self,
        patch: serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<()> {
        let mut props = serde_json::to_value(&self.props)?;
        let patch = patch
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("props patch must be an object"))?;
        for (key, value) in patch {
            ensure!(props.get(key).is_some(), "unknown prop: {key}");
            props[key] = value.clone();
        }
        let next: Props = serde_json::from_value(props)?;
        next.validate()?;
        if next.disabled && self.observe(window, cx).focused {
            self.blur(window, cx);
        }
        match &self.control {
            Control::Input(state) => state.update(cx, |state, cx| {
                if patch.contains_key("value") && state.value() != next.value {
                    state.set_value(next.value.clone(), window, cx);
                }
                if next.placeholder != self.props.placeholder {
                    state.set_placeholder(next.placeholder.clone(), window, cx);
                }
                state.set_disabled(next.disabled, cx);
                state.set_readonly(next.readonly, cx);
            }),
            Control::Textarea(state) => state.update(cx, |state, cx| {
                if patch.contains_key("value") && state.value() != next.value {
                    state.set_value(next.value.clone(), window, cx);
                    let end = state.text().len();
                    state.set_selected_range(end..end, cx);
                }
                if next.placeholder != self.props.placeholder {
                    state.set_placeholder(next.placeholder.clone(), window, cx);
                }
                if next.rows != self.props.rows {
                    state.set_rows(next.rows.max(1), cx);
                }
                state.set_disabled(next.disabled, cx);
                state.set_readonly(next.readonly, cx);
            }),
            Control::Button(_) => {}
        }
        self.props = next;
        cx.notify();
        Ok(())
    }
}

impl Render for Scene {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.palette;
        let disabled = self.props.disabled;
        let subject = match &self.control {
            Control::Button(handle) => {
                let primary = self.props.variant == "primary";
                let ghost = self.props.variant == "ghost";
                let md = self.props.size == "md";
                let border = if primary {
                    p.accent
                } else if ghost {
                    p.canvas
                } else {
                    p.border
                };
                let bg = if primary { p.accent } else { p.canvas };
                let fg = if primary { p.on_accent } else { p.muted };
                let scene = cx.entity();
                Button::new("subject")
                    .track_focus(handle)
                    .disabled(disabled)
                    .h(px(if md { 28. } else { 22. }))
                    .px(px(if md { 11. } else { 7. }))
                    .text_size(px(if md { 11. } else { 10. }))
                    .font_weight(gpui_kit::FontWeight::MEDIUM)
                    .border_1()
                    .border_color(mix(border, p.canvas, if disabled { 0.45 } else { 1. }))
                    .rounded(px(4.))
                    .bg(mix(bg, p.canvas, if disabled { 0.45 } else { 1. }))
                    .text_color(mix(fg, p.canvas, if disabled { 0.45 } else { 1. }))
                    .when(!disabled, |this| {
                        this.hover(|this| {
                            if primary {
                                let hover_accent = mix(p.accent, p.on_accent, 0.88);
                                this.bg(hover_accent).border_color(hover_accent)
                            } else {
                                this.bg(rgb(p.hover))
                                    .text_color(rgb(p.text))
                                    .border_color(rgb(if ghost {
                                        p.canvas
                                    } else {
                                        p.hover_border
                                    }))
                            }
                        })
                    })
                    .when(handle.is_focused(window) && !disabled, |this| {
                        this.border_color(rgb(if primary { p.on_accent } else { p.accent }))
                    })
                    .when(self.fault != TestFault::DropClicks, |this| {
                        this.on_click(move |_, _, cx| {
                            scene.update(cx, |scene, cx| {
                                scene.clicks += 1;
                                cx.notify();
                            });
                        })
                    })
                    .child(self.props.label.clone())
                    .into_any_element()
            }
            Control::Input(state) => {
                let bare = self.props.variant == "bare";
                let mono = self.props.variant == "mono" || self.props.monospaced;
                let focused = state.read(cx).focus_handle(cx).is_focused(window);
                let error = self.props.error;
                state.update(cx, |state, _| {
                    state.set_editor_style(p.editor_style(
                        self.props.readonly || disabled,
                        self.capture_mode && !self.capture_caret,
                        disabled,
                    ))
                });
                let border = if error {
                    p.danger
                } else if focused {
                    p.accent
                } else {
                    p.border
                };
                InputBase::new("subject")
                    .focused(focused)
                    .disabled(disabled)
                    .w(px(self.props.width))
                    .h(px(if bare { 21.4 } else { 28. }))
                    .px(px(if bare { 5. } else { 8. }))
                    .flex()
                    .items_center()
                    .text_size(px(if bare { 12. } else { 11. }))
                    .when(mono, |this| this.font_family(monospace_family()))
                    .bg(rgb(p.canvas))
                    .text_color(rgb(if self.props.readonly || disabled {
                        p.muted
                    } else {
                        p.text
                    }))
                    .rounded(px(4.))
                    .when(!bare, |this| this.border_1().border_color(rgb(border)))
                    .when(
                        !bare && !disabled && !focused && !self.props.readonly && !error,
                        |this| this.hover(|this| this.border_color(rgb(p.hover_border))),
                    )
                    .when(error && !disabled, |this| {
                        this.hover(|this| this.bg(mix(p.danger, p.canvas, 0.05)))
                    })
                    .when(disabled, |this| this.opacity(0.45))
                    .child(Input::new(state))
                    .into_any_element()
            }
            Control::Textarea(state) => {
                let bare = self.props.variant == "bare";
                let focused = state.read(cx).focus_handle(cx).is_focused(window);
                state.update(cx, |state, _| {
                    state.set_editor_style(InputEditorStyle {
                        macos_textarea_selection: cfg!(target_os = "macos"),
                        ..p.editor_style(
                            self.props.readonly || disabled,
                            self.capture_mode && !self.capture_caret,
                            disabled,
                        )
                    })
                });
                InputBase::new("subject")
                    .focused(focused)
                    .disabled(disabled)
                    .w(px(self.props.width))
                    .h(px(if self.props.height > 0. {
                        self.props.height
                    } else if bare {
                        self.props.rows as f32 * 11. * 1.45 + 12.
                    } else {
                        120.
                    }))
                    .when(self.props.monospaced, |this| {
                        this.font_family(monospace_family())
                    })
                    .text_size(px(11.))
                    .bg(rgb(p.canvas))
                    .text_color(rgb(p.text))
                    .rounded(px(4.))
                    .when(!bare, |this| {
                        this.border_1()
                            .border_color(rgb(if focused { p.accent } else { p.border }))
                    })
                    .when(
                        !bare && !disabled && !focused && !self.props.readonly,
                        |this| this.hover(|this| this.border_color(rgb(p.hover_border))),
                    )
                    .when(disabled, |this| this.opacity(0.45))
                    .child(Textarea::new(state))
                    .into_any_element()
            }
        };
        div()
            .id("scene")
            .track_focus(&self.fallback_focus)
            .key_context("TauScene")
            .on_action(cx.listener(Self::next_field))
            .on_action(cx.listener(Self::previous_field))
            .size_full()
            .bg(rgb(p.canvas))
            .text_color(rgb(p.text))
            .font_family("Inter")
            .text_size(px(13.))
            .line_height(gpui_kit::relative(1.45))
            .p(px(24.))
            .flex()
            .flex_col()
            .items_start()
            .child(
                div()
                    .opacity(if self.fault == TestFault::HideControl {
                        0.
                    } else {
                        1.
                    })
                    .child(subject),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::checked_selection;

    #[test]
    fn utf16_selection_requires_scalar_boundaries() {
        for (start, end) in [(0, 0), (1, 3), (3, 4), (4, 4)] {
            assert!(checked_selection("A😀B", start, end).is_ok());
        }
        for (start, end) in [(2, 2), (1, 2), (0, 5), (3, 1)] {
            assert!(checked_selection("A😀B", start, end).is_err());
        }
    }
}
