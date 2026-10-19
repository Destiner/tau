//! Shared native palette, controls and bundled assets.

use std::borrow::Cow;

use gpui_kit::{
    ElementId, SharedString, Svg,
    base::{Button as BaseButton, ScrollbarStyles, Theme, input::InputEditorStyle},
    div,
    prelude::*,
    px, rgb, rgba, svg,
};

/// Observe the actual element box without adding a layout/scroll-content child.
pub fn observe_bounds<E: gpui_kit::IntoElement>(
    element: E,
    callback: impl FnOnce(gpui_kit::Bounds<gpui_kit::Pixels>, &mut gpui_kit::Window, &mut gpui_kit::App)
    + 'static,
) -> BoundsObserver<E::Element> {
    BoundsObserver {
        inner: element.into_element(),
        callback: Some(Box::new(callback)),
    }
}

type BoundsCallback =
    Box<dyn FnOnce(gpui_kit::Bounds<gpui_kit::Pixels>, &mut gpui_kit::Window, &mut gpui_kit::App)>;
pub struct BoundsObserver<E> {
    inner: E,
    callback: Option<BoundsCallback>,
}
impl<E: gpui_kit::Element> gpui_kit::IntoElement for BoundsObserver<E> {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl<E: gpui_kit::Element> gpui_kit::Element for BoundsObserver<E> {
    type RequestLayoutState = E::RequestLayoutState;
    type PrepaintState = E::PrepaintState;
    fn id(&self) -> Option<gpui_kit::ElementId> {
        self.inner.id()
    }
    fn a11y_role(&self) -> Option<gpui_kit::accesskit::Role> {
        self.inner.a11y_role()
    }
    fn write_a11y_info(&self, node: &mut gpui_kit::accesskit::Node) {
        self.inner.write_a11y_info(node);
    }
    fn a11y_synthetic_children(
        &mut self,
        prepaint: &mut Self::PrepaintState,
        builder: &mut gpui_kit::A11ySubtreeBuilder,
    ) {
        self.inner.a11y_synthetic_children(prepaint, builder);
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        self.inner.source_location()
    }
    fn request_layout(
        &mut self,
        id: Option<&gpui_kit::GlobalElementId>,
        inspector: Option<&gpui_kit::InspectorElementId>,
        window: &mut gpui_kit::Window,
        cx: &mut gpui_kit::App,
    ) -> (gpui_kit::LayoutId, Self::RequestLayoutState) {
        self.inner.request_layout(id, inspector, window, cx)
    }
    fn prepaint(
        &mut self,
        id: Option<&gpui_kit::GlobalElementId>,
        inspector: Option<&gpui_kit::InspectorElementId>,
        bounds: gpui_kit::Bounds<gpui_kit::Pixels>,
        layout: &mut Self::RequestLayoutState,
        window: &mut gpui_kit::Window,
        cx: &mut gpui_kit::App,
    ) -> Self::PrepaintState {
        if let Some(callback) = self.callback.take() {
            callback(bounds, window, cx);
        }
        self.inner
            .prepaint(id, inspector, bounds, layout, window, cx)
    }
    fn paint(
        &mut self,
        id: Option<&gpui_kit::GlobalElementId>,
        inspector: Option<&gpui_kit::InspectorElementId>,
        bounds: gpui_kit::Bounds<gpui_kit::Pixels>,
        layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut gpui_kit::Window,
        cx: &mut gpui_kit::App,
    ) {
        self.inner
            .paint(id, inspector, bounds, layout, prepaint, window, cx);
    }
}

/// Colours resolved against the canvas where CSS uses translucent washes.
/// `scrim` and `selected` are RGBA; all other fields are RGB.
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
    /// CSS `--selected` row wash, not editor/source text selection.
    pub selected: u32,
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
            selected: 0x4752664d,
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
            selected: 0x6b7d8f33,
        }
    }
}

/// Vue changes only the row-selection wash on native window deactivation.
/// Keyboard focus, text selection, accent and disabled policy are independent.
pub fn tokens_for_window(dark: bool, window: &gpui_kit::Window) -> Tokens {
    let mut t = tokens(dark);
    if !window.is_window_active() {
        t.selected = if dark { 0x47526626 } else { 0x6b7d8f1c };
    }
    t
}

/// Preserve CSS --border alpha on raised tooltip surfaces, not the canvas approximation.
pub fn tooltip_border(dark: bool) -> gpui_kit::Rgba {
    if dark {
        rgb(tokens(true).border)
    } else {
        gpui_kit::Rgba {
            a: 0.24,
            ..rgb(0x6b7d8f)
        }
    }
}

/// CSS `0 8px 20px var(--shadow-soft)`; GPUI's blur radius is Gaussian sigma.
pub fn tooltip_shadow(dark: bool) -> Vec<gpui_kit::BoxShadow> {
    vec![
        gpui_kit::BoxShadow::new(
            px(0.),
            px(8.),
            rgba(if dark { 0x00000073 } else { 0x0000001f }).into(),
        )
        .blur_radius(px(10.)),
    ]
}

/// UiTooltip's compact surface only. Owners retain semantics, timing and placement.
pub fn tooltip_surface<E: Styled>(surface: E, dark: bool) -> E {
    let t = tokens(dark);
    surface
        .max_w(px(260.))
        .px(px(7.))
        .py(px(3.))
        .rounded(px(4.))
        .border_1()
        .border_color(tooltip_border(dark))
        .bg(rgb(t.panel_raised))
        .shadow(tooltip_shadow(dark))
        .text_color(rgb(t.text))
        .text_size(px(10.))
        .line_height(gpui_kit::relative(1.25))
}

pub fn tooltip_shortcut<E: Styled>(surface: E, dark: bool) -> E {
    let t = tokens(dark);
    surface
        .px(px(4.))
        .py(px(1.))
        .border_1()
        .border_color(gpui_kit::Rgba {
            a: 0.35,
            ..rgb(t.muted)
        })
        .rounded(px(3.))
        .text_color(rgb(t.muted))
        .text_size(px(10.))
        .line_height(gpui_kit::relative(1.2))
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

/// UiSelect's 5px content box plus right/bottom borders, rotated around its center.
pub fn selector_arrow(color: u32) -> gpui_kit::Div {
    let extent = 6. * std::f32::consts::SQRT_2;
    let overhang = (extent - 6.) / 2.;
    div()
        .absolute()
        .size(px(extent))
        .flex_shrink_0()
        // Both containing blocks end inside the trigger's 1px border.
        .right(px(8. - overhang))
        .top(gpui_kit::relative(0.5))
        .mt(px(-4. - overhang))
        .child(icon("selector-chevron", extent, color).flex_shrink_0())
}

/// Bundled production icons and the CSS-derived selector chevron.
pub fn asset(path: &str) -> Option<Cow<'static, [u8]>> {
    let bytes: &'static [u8] = match path {
        "product-icons/search.svg" => include_bytes!("../../assets/product-icons/search.svg"),
        "product-icons/selector-chevron.svg" => {
            include_bytes!("../../assets/product-icons/selector-chevron.svg")
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
        Context, MouseButton, Refineable, Render, Style, TestApp, TestSupportExt, Window,
        test::TestWindowExt,
    };
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };

    #[test]
    fn bounds_observer_preserves_border_box_and_fitting_scroll_extent() {
        struct Host {
            measured: Rc<Cell<Option<gpui_kit::Bounds<gpui_kit::Pixels>>>>,
            scroll: gpui_kit::ScrollHandle,
        }
        impl Render for Host {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                let measured = self.measured.clone();
                div().size_full().child(observe_bounds(
                    div()
                        .id("surface")
                        .test_support()
                        .absolute()
                        .left(px(220.))
                        .top(px(100.))
                        .w(px(200.))
                        .h(px(42.))
                        .p(px(8.))
                        .border_1()
                        .overflow_y_scroll()
                        .track_scroll(&self.scroll)
                        .flex()
                        .flex_col()
                        .child(div().id("row").test_support().h(px(24.)).flex_shrink_0()),
                    move |bounds, _, _| measured.set(Some(bounds)),
                ))
            }
        }
        let mut app = TestApp::new();
        let mut window = app.open_window_with_options(Default::default(), |_, _| Host {
            measured: Rc::new(Cell::new(None)),
            scroll: gpui_kit::ScrollHandle::new(),
        });
        for _ in 0..3 {
            window.draw();
            window.update(|host, w, _| {
                let surface = w.find("surface").bounds();
                let row = w.find("row").bounds();
                assert_eq!(host.measured.get(), Some(surface));
                assert_eq!(surface.left(), px(220.));
                assert_eq!(row.top() - surface.top(), px(9.));
                assert_eq!(surface.bottom() - row.bottom(), px(9.));
                assert_eq!(host.scroll.max_offset().y, px(0.));
            });
        }
    }

    #[test]
    fn selector_asset_raster_bounds_match_the_production_rotated_border_not_an_icon_center() {
        let mut app = TestApp::new();
        app.update(|cx| {
            let bytes = asset("product-icons/selector-chevron.svg").unwrap();
            assert_eq!(
                bytes.as_ref(),
                include_bytes!("../../assets/product-icons/selector-chevron.svg")
            );
            let renderer = cx.svg_renderer();
            let parsed = renderer.parse_svg(&bytes).unwrap();
            let image = renderer
                .render_parsed(
                    &parsed,
                    gpui_kit::SvgSize::ExactSize(gpui_kit::size(512.into(), 512.into())),
                )
                .unwrap();
            let pixels = image.as_bytes(0).unwrap();
            let mut left = 512;
            let mut right = 0;
            let mut top = 512;
            let mut bottom = 0;
            for y in 0..512 {
                for x in 0..512 {
                    if pixels[(y * 512 + x) * 4 + 3] >= 128 {
                        left = left.min(x);
                        right = right.max(x + 1);
                        top = top.min(y);
                        bottom = bottom.max(y + 1);
                    }
                }
            }
            // CSS's 6x6 border box has visible vertices at y=-1/sqrt(2) and 6/sqrt(2).
            // Inside its rotated viewport that is [5/12, 1], not a vertically centered V.
            assert!(left <= 2 && right >= 510);
            assert!((top as f32 / 512. - 5. / 12.).abs() < 0.005);
            assert!(bottom >= 510);
            assert!(((top + bottom) as f32 / 1024. - 17. / 24.).abs() < 0.005);
        });
    }

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
