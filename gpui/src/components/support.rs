//! Shared native palette, controls and bundled assets.

use std::borrow::Cow;

use gpui_kit::{
    ElementId, SharedString, Svg,
    base::{Button as BaseButton, ScrollbarStyles, Theme, input::InputEditorStyle},
    div,
    prelude::*,
    px, rgb, rgba, svg,
};

/// Colours resolved against the canvas where CSS uses translucent washes.
/// `scrim` alone is RGBA; all other fields are RGB.
#[derive(Clone, Copy)]
pub struct Tokens {
    pub canvas: u32,
    pub panel: u32,
    pub panel_raised: u32,
    pub text: u32,
    pub muted: u32,
    pub faint: u32,
    pub border: u32,
    pub accent: u32,
    pub on_accent: u32,
    pub hover: u32,
    pub hover_border: u32,
    pub danger: u32,
    pub scrim: u32,
}

pub fn tokens(dark: bool) -> Tokens {
    if dark {
        Tokens {
            canvas: 0x10141c,
            panel: 0x0d1017,
            panel_raised: 0x141821,
            text: 0xbfbdb6,
            muted: 0x6c7380,
            faint: 0x5a6378,
            border: 0x1b1f29,
            accent: 0xe6b450,
            on_accent: 0x10141c,
            hover: 0x1b202b,        // rgb(71 82 102 / 20%) over canvas
            hover_border: 0x3f4550, // muted 45%, border 55%
            danger: 0xd95757,
            scrim: 0x00000066,
        }
    } else {
        Tokens {
            canvas: 0xfcfcfc,
            panel: 0xf8f9fa,
            panel_raised: 0xffffff,
            text: 0x5c6166,
            muted: 0x6e757f,
            faint: 0xa1a8b2,
            border: 0xd9dee2, // rgb(107 125 143 / 24%) over canvas
            accent: 0xf29718,
            on_accent: 0x10141c,
            hover: 0xeeeff1,        // rgb(107 125 143 / 10%) over canvas
            hover_border: 0xa9afb5, // muted 45%, border 55%
            danger: 0xe65050,
            scrim: 0x0000001a,
        }
    }
}

pub fn root(dark: bool) -> gpui_kit::Div {
    let t = tokens(dark);
    div()
        .size_full()
        .bg(rgb(t.canvas))
        .text_color(rgb(t.text))
        .font_family(super::font::family())
        .text_size(px(13.))
        .line_height(gpui_kit::relative(1.45))
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ButtonVariant {
    Primary,
    Secondary,
    Ghost,
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

/// Styled button; BaseButton still owns disabled pointer/keyboard policy.
#[derive(gpui_kit::IntoElement)]
pub struct NativeButton {
    base: BaseButton,
    disabled: bool,
    variant: ButtonVariant,
    dark: bool,
    backdrop: u32,
}

impl NativeButton {
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self.base = self.base.disabled(disabled);
        self
    }

    /// Opaque surface behind this button, used for CSS group-opacity compositing.
    pub fn backdrop(mut self, backdrop: u32) -> Self {
        self.backdrop = backdrop;
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&gpui_kit::ClickEvent, &mut gpui_kit::Window, &mut gpui_kit::App) + 'static,
    ) -> Self {
        self.base = self.base.on_click(handler);
        self
    }

    pub fn track_focus(mut self, handle: &gpui_kit::FocusHandle) -> Self {
        self.base = self.base.track_focus(handle);
        self
    }

    fn styled_base(self) -> BaseButton {
        let t = tokens(self.dark);
        let primary = self.variant == ButtonVariant::Primary;
        let variant = self.variant;
        if self.disabled {
            // GPUI opacity fades each primitive separately, not the CSS group.
            // Preblend once so the primary border and fill remain identical.
            let fill = mix(t.accent, self.backdrop, 0.45);
            self.base
                .bg(if primary { fill } else { rgba(0) })
                .border_color(match variant {
                    ButtonVariant::Primary => fill,
                    ButtonVariant::Secondary => mix(t.border, self.backdrop, 0.45),
                    ButtonVariant::Ghost => rgba(0),
                })
                .text_color(mix(
                    if primary { t.on_accent } else { t.muted },
                    self.backdrop,
                    0.45,
                ))
        } else {
            self.base
                .hover(move |this| {
                    if primary {
                        let accent = mix(t.accent, t.on_accent, 0.88);
                        this.bg(accent).border_color(accent)
                    } else {
                        let this = this.bg(rgb(t.hover)).text_color(rgb(t.text));
                        if variant == ButtonVariant::Secondary {
                            this.border_color(rgb(t.hover_border))
                        } else {
                            this
                        }
                    }
                })
                .focus_visible(move |this| {
                    this.border_color(rgb(if primary { t.on_accent } else { t.accent }))
                })
        }
    }
}

impl Styled for NativeButton {
    fn style(&mut self) -> &mut gpui_kit::StyleRefinement {
        self.base.style()
    }
}

impl InteractiveElement for NativeButton {
    fn interactivity(&mut self) -> &mut gpui_kit::Interactivity {
        self.base.interactivity()
    }
}

impl StatefulInteractiveElement for NativeButton {}

impl ParentElement for NativeButton {
    fn extend(&mut self, elements: impl IntoIterator<Item = gpui_kit::AnyElement>) {
        self.base.extend(elements);
    }
}

impl RenderOnce for NativeButton {
    fn render(self, _: &mut gpui_kit::Window, _: &mut gpui_kit::App) -> impl IntoElement {
        self.styled_base()
    }
}

pub fn button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    variant: ButtonVariant,
    dark: bool,
) -> NativeButton {
    let label = label.into();
    let t = tokens(dark);
    let primary = variant == ButtonVariant::Primary;
    let base = BaseButton::new(id)
        .accessibility_label(label.clone())
        .h(px(24.))
        .px(px(9.))
        .text_size(px(11.))
        .font_weight(gpui_kit::FontWeight::MEDIUM)
        .border_1()
        .border_color(if primary {
            rgb(t.accent)
        } else if variant == ButtonVariant::Ghost {
            rgba(0x00000000)
        } else {
            rgb(t.border)
        })
        .rounded(px(4.))
        .bg(if primary {
            rgb(t.accent)
        } else {
            rgba(0x00000000)
        })
        .text_color(rgb(if primary { t.on_accent } else { t.muted }))
        .child(label);
    NativeButton {
        base,
        disabled: false,
        variant,
        dark,
        backdrop: t.canvas,
    }
}

/// UiButton's compact `sm` size; the promised `button` entry point is `md`.
pub fn button_sm(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    variant: ButtonVariant,
    dark: bool,
) -> NativeButton {
    button(id, label, variant, dark)
        .h(px(22.))
        .px(px(7.))
        .text_size(px(10.))
}

pub fn input_style(dark: bool) -> InputEditorStyle {
    let t = tokens(dark);
    InputEditorStyle {
        foreground: rgb(t.text).into(),
        muted_foreground: rgb(t.faint).into(),
        background: rgb(t.canvas).into(),
        border: rgb(t.border).into(),
        selection: rgba(if dark { 0x3388ff40 } else { 0x035bd626 }).into(),
        caret: rgb(t.accent).into(),
        caret_width: Some(px(2.)),
        center_caret: true,
        caret_visible: true,
        unfocused_selection: false,
        disabled_opacity: 1.,
        ..Default::default()
    }
}

/// Vue leaves scrollbars to the platform (only queue requests `thin`).
/// Project the same quiet overlay ink onto Base's retained-editor and custom bars.
pub fn scrollbar_styles(dark: bool) -> ScrollbarStyles {
    let ink = tokens(dark).text;
    ScrollbarStyles::default()
        .track(|track| track.bg(rgba(0).into()).border_color(rgba(0).into()))
        .thumb(|thumb| thumb.bg(rgba((ink << 8) | 0x59)).radius(px(4.)))
        .thumb_hover(|thumb| thumb.bg(rgba((ink << 8) | 0x8c)).radius(px(4.)))
        .thumb_active(|thumb| thumb.bg(rgba((ink << 8) | 0x8c)).radius(px(4.)))
}

/// Call at application initialization and when its appearance changes.
/// Preserves Base's mode, motion, geometry, and the owning scroll handles.
pub fn install_scrollbar_theme(dark: bool, cx: &mut gpui_kit::App) {
    let scrollbar = Theme::global(cx).scrollbar;
    Theme::global_mut(cx).scrollbar = scrollbar.with_styles(scrollbar_styles(dark));
}

pub fn mono() -> &'static str {
    if cfg!(target_os = "macos") {
        ".AppleSystemUIFontMonospaced"
    } else {
        "monospace"
    }
}

fn icon_name(name: &str) -> &str {
    match name {
        "x" => "cross",
        "square" => "stop",
        "send" => "triangle",
        "arrow-up" => "queue",
        "alert-circle" => "warning",
        other => other,
    }
}

pub fn icon(name: &str, size: f32, color: u32) -> Svg {
    svg()
        .path(format!("product-icons/{}.svg", icon_name(name)))
        .size(px(size))
        .text_color(rgb(color))
}

/// A smaller, pixel-aligned glyph box; the trigger and previous glyph center stay put.
pub fn selector_arrow(color: u32) -> gpui_kit::Div {
    let extent = 6.;
    let overhang = (extent - 4.) / 2.;
    div()
        .absolute()
        .size(px(extent))
        .flex_shrink_0()
        .right(px(8.5 - overhang))
        .top(gpui_kit::relative(0.5))
        .mt(px(-3.5 - overhang))
        .child(icon("selector-chevron", extent, color).flex_shrink_0())
}

/// Bundled production icons and the CSS-derived selector chevron.
pub fn asset(path: &str) -> Option<Cow<'static, [u8]>> {
    let bytes: &'static [u8] = match path {
        "product-icons/search.svg" => include_bytes!("../../assets/product-icons/search.svg"),
        "product-icons/selector-chevron.svg" => {
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="5.656854" height="5.656854" viewBox="-0.828427 -0.828427 5.656854 5.656854"><path transform="rotate(45 2 2)" d="M3 0H4V4H0V3H3Z"/></svg>"#
        }
        "product-icons/chevron-left.svg" => {
            include_bytes!("../../assets/product-icons/chevron-left.svg")
        }
        "product-icons/archive.svg" => include_bytes!("../../assets/product-icons/archive.svg"),
        "product-icons/bug.svg" => include_bytes!("../../assets/product-icons/bug.svg"),
        "product-icons/check.svg" => include_bytes!("../../assets/product-icons/check.svg"),
        "product-icons/chevron-down.svg" => {
            include_bytes!("../../assets/product-icons/chevron-down.svg")
        }
        "product-icons/chevron-right.svg" => {
            include_bytes!("../../assets/product-icons/chevron-right.svg")
        }
        "product-icons/chevron-up.svg" => {
            include_bytes!("../../assets/product-icons/chevron-up.svg")
        }
        "product-icons/copy.svg" => include_bytes!("../../assets/product-icons/copy.svg"),
        "product-icons/cross.svg" => include_bytes!("../../assets/product-icons/cross.svg"),
        "product-icons/folder.svg" => include_bytes!("../../assets/product-icons/folder.svg"),
        "product-icons/folder-plus.svg" => {
            include_bytes!("../../assets/product-icons/folder-plus.svg")
        }
        "product-icons/grip.svg" => include_bytes!("../../assets/product-icons/grip.svg"),
        "product-icons/info.svg" => include_bytes!("../../assets/product-icons/info.svg"),
        "product-icons/plus.svg" => include_bytes!("../../assets/product-icons/plus.svg"),
        "product-icons/queue.svg" => include_bytes!("../../assets/product-icons/queue.svg"),
        "product-icons/stop.svg" => include_bytes!("../../assets/product-icons/stop.svg"),
        "product-icons/triangle.svg" => include_bytes!("../../assets/product-icons/triangle.svg"),
        "product-icons/trash.svg" => include_bytes!("../../assets/product-icons/trash.svg"),
        "product-icons/warning.svg" => include_bytes!("../../assets/product-icons/warning.svg"),
        _ => return None,
    };
    Some(Cow::Borrowed(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{
        Context, MouseButton, Refineable, Render, Style, TestApp, Window, test::TestWindowExt,
    };
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };

    struct ButtonHost {
        empty: bool,
        busy: bool,
        dark: bool,
        variant: ButtonVariant,
        clicks: Rc<Cell<usize>>,
        visual_style: Rc<RefCell<Option<Style>>>,
    }

    impl ButtonHost {
        fn control(&self) -> NativeButton {
            button_sm("styled-button", "Submit", self.variant, self.dark)
                .backdrop(tokens(self.dark).panel_raised)
                // Also cover toggling the builder state back to enabled.
                .disabled(true)
                .disabled(self.empty || self.busy)
        }
    }

    impl Render for ButtonHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let clicks = self.clicks.clone();
            let visual_style = self.visual_style.clone();
            let mut probe = self.control().styled_base();
            let style = probe.style().clone();
            probe.interactivity().base_style.refine(&style);
            div()
                .size_full()
                .bg(rgb(tokens(self.dark).panel_raised))
                .child(
                    self.control()
                        .on_click(move |_, _, _| clicks.set(clicks.get() + 1)),
                )
                .child(gpui_kit::canvas(
                    |_, window, _| {
                        let bounds = window.find("styled-button").bounds();
                        window.insert_hitbox(bounds, gpui_kit::HitboxBehavior::Normal)
                    },
                    move |_, hitbox, window, cx| {
                        // Use actual pointer input over the painted button bounds.
                        let style =
                            probe
                                .interactivity()
                                .compute_style(None, Some(&hitbox), window, cx);
                        *visual_style.borrow_mut() = Some(style);
                    },
                ))
        }
    }

    #[test]
    fn disabled_variants_preblend_once_against_the_owned_surface() {
        for dark in [false, true] {
            let t = tokens(dark);
            for backdrop in [t.canvas, t.panel_raised] {
                for variant in [
                    ButtonVariant::Primary,
                    ButtonVariant::Secondary,
                    ButtonVariant::Ghost,
                ] {
                    let mut base = button("button", "Label", variant, dark)
                        .disabled(true)
                        .backdrop(backdrop)
                        .styled_base();
                    let style = base.style();
                    assert_eq!(style.opacity, None, "never fade GPUI primitives twice");
                    let fill = mix(t.accent, backdrop, 0.45);
                    let border = match variant {
                        ButtonVariant::Primary => fill,
                        ButtonVariant::Secondary => mix(t.border, backdrop, 0.45),
                        ButtonVariant::Ghost => rgba(0),
                    };
                    assert_eq!(style.border_color, Some(border.into()));
                    assert_eq!(
                        style.background.as_ref().and_then(gpui_kit::Fill::color),
                        Some(
                            if variant == ButtonVariant::Primary {
                                fill
                            } else {
                                rgba(0)
                            }
                            .into()
                        )
                    );
                    assert_eq!(
                        style.text.color,
                        Some(
                            mix(
                                if variant == ButtonVariant::Primary {
                                    t.on_accent
                                } else {
                                    t.muted
                                },
                                backdrop,
                                0.45,
                            )
                            .into()
                        )
                    );
                }
            }
        }
    }

    #[test]
    fn actual_hover_and_activation_follow_empty_busy_enabled_disabled_transitions() {
        for dark in [false, true] {
            for variant in [
                ButtonVariant::Primary,
                ButtonVariant::Secondary,
                ButtonVariant::Ghost,
            ] {
                let clicks = Rc::new(Cell::new(0));
                let mut app = TestApp::new();
                app.update(gpui_kit::init);
                let mut window = app.open_window({
                    let clicks = clicks.clone();
                    move |_, _| ButtonHost {
                        empty: true,
                        busy: false,
                        dark,
                        variant,
                        clicks,
                        visual_style: Rc::new(RefCell::new(None)),
                    }
                });
                window.draw();
                for (empty, busy) in [
                    (true, false),
                    (false, true),
                    (false, false),
                    (true, false),
                    (false, false),
                    (false, true),
                ] {
                    window.update(|host, _, cx| {
                        host.empty = empty;
                        host.busy = busy;
                        cx.notify();
                    });
                    window.draw();
                    let bounds = window.update(|_, w, _| w.find("styled-button").bounds());
                    window.simulate_mouse_move(gpui_kit::point(
                        bounds.right() + px(20.),
                        bounds.bottom() + px(20.),
                    ));
                    window.draw();
                    let rest =
                        window.update(|host, _, _| host.visual_style.borrow().clone().unwrap());
                    window.simulate_mouse_move(bounds.center());
                    window.draw();
                    let hover =
                        window.update(|host, _, _| host.visual_style.borrow().clone().unwrap());
                    if empty || busy {
                        assert_eq!(rest.background, hover.background);
                        assert_eq!(rest.border_color, hover.border_color);
                        assert_eq!(rest.text.color, hover.text.color);
                    } else {
                        assert_ne!(
                            rest.background, hover.background,
                            "enabled hover must still work"
                        );
                    }
                    let before = clicks.get();
                    window.simulate_click(bounds.center(), MouseButton::Left);
                    window.draw();
                    for key in ["enter", "space"] {
                        let keystroke = gpui_kit::Keystroke::parse(key).unwrap();
                        window.simulate_event(gpui_kit::KeyDownEvent {
                            keystroke: keystroke.clone(),
                            is_held: false,
                            prefer_character_input: false,
                        });
                        window.simulate_event(gpui_kit::KeyUpEvent { keystroke });
                        window.draw();
                    }
                    assert_eq!(clicks.get() - before, if empty || busy { 0 } else { 3 });
                }
            }
        }
    }

    struct ScrollbarHost {
        dark: bool,
        scroll: gpui_kit::ScrollHandle,
    }
    impl Render for ScrollbarHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            use gpui_kit::{
                TestSupportExt,
                base::{Scrollbar, ScrollbarMode},
            };
            root(self.dark).child(
                div()
                    .relative()
                    .w(px(200.))
                    .h(px(100.))
                    .child(
                        div()
                            .id("scroll-viewport")
                            .test_support()
                            .h(px(100.))
                            .overflow_y_scroll()
                            .track_scroll(&self.scroll)
                            .child(div().h(px(400.)).w_full()),
                    )
                    .child(
                        Scrollbar::vertical(&self.scroll)
                            .mode(ScrollbarMode::Always)
                            .styles(|_| scrollbar_styles(self.dark)),
                    ),
            )
        }
    }

    #[test]
    fn custom_scrollbars_paint_shared_dark_and_light_rest_and_hover_ink() {
        for dark in [false, true] {
            let mut app = TestApp::new();
            app.update(gpui_kit::init);
            let mut window = app.open_window(|_, _| ScrollbarHost {
                dark,
                scroll: gpui_kit::ScrollHandle::new(),
            });
            window.draw();
            window.update(|_, window, _| {
                let expected = rgba((tokens(dark).text << 8) | 0x59);
                assert!(
                    window
                        .painted_quads()
                        .iter()
                        .any(|quad| quad.background == expected.into())
                );
            });
            let position = window.update(|_, window, _| {
                let bounds = window.find("scroll-viewport").bounds();
                gpui_kit::point(bounds.right() - px(8.), bounds.top() + px(12.))
            });
            window.simulate_mouse_move(position);
            window.draw();
            window.update(|_, window, _| {
                let expected = rgba((tokens(dark).text << 8) | 0x8c);
                assert!(
                    window
                        .painted_quads()
                        .iter()
                        .any(|quad| quad.background == expected.into())
                );
            });
        }
    }

    #[test]
    fn palettes_keep_distinct_surfaces_and_alpha_scrims() {
        assert_eq!(tokens(false).canvas, 0xfcfcfc);
        assert_eq!(tokens(true).border, 0x1b1f29);
        assert_eq!(tokens(false).scrim, 0x0000001a);
        assert_eq!(tokens(true).scrim, 0x00000066);
    }

    #[test]
    fn bundled_icons_are_complete_and_paths_are_contained() {
        for name in [
            "archive",
            "bug",
            "check",
            "chevron-down",
            "chevron-right",
            "chevron-up",
            "copy",
            "cross",
            "folder",
            "folder-plus",
            "grip",
            "info",
            "plus",
            "queue",
            "stop",
            "triangle",
            "trash",
            "warning",
        ] {
            let path = format!("product-icons/{name}.svg");
            let markup = std::str::from_utf8(&asset(&path).unwrap())
                .unwrap()
                .to_owned();
            assert!(markup.contains("<path"), "{name}");
            assert!(markup.contains("viewBox=\"0 0 256 256\""));
            assert!(markup.contains("fill=\"currentColor\""));
            if name == "queue" {
                for attr in [
                    "fill=\"none\"",
                    "stroke=\"currentColor\"",
                    "stroke-width=\"16\"",
                    "stroke-linecap=\"round\"",
                    "stroke-linejoin=\"round\"",
                ] {
                    assert!(markup.contains(attr), "queue missing {attr}");
                }
            }
        }
        for (alias, canonical) in [
            ("x", "cross"),
            ("square", "stop"),
            ("send", "triangle"),
            ("arrow-up", "queue"),
        ] {
            assert_eq!(icon_name(alias), canonical);
        }
        assert!(asset("product-icons/../copy.svg").is_none());
        assert!(asset("product-icons/unknown.svg").is_none());
    }
}
