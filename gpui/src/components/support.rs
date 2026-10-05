//! Shared native palette, controls and bundled assets.

use std::borrow::Cow;

use gpui_kit::{
    ElementId, SharedString, Svg,
    base::{Button as BaseButton, input::InputEditorStyle},
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

pub fn button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    variant: ButtonVariant,
    dark: bool,
) -> BaseButton {
    let t = tokens(dark);
    let primary = variant == ButtonVariant::Primary;
    BaseButton::new(id)
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
        .styles(|styles| styles.disabled(|this| this.opacity(0.45)))
        .child(label.into())
}

/// UiButton's compact `sm` size; the promised `button` entry point is `md`.
pub fn button_sm(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    variant: ButtonVariant,
    dark: bool,
) -> BaseButton {
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

/// The selector's CSS 5px square, 1px right/bottom borders, rotated 45°.
pub fn selector_arrow(color: u32) -> gpui_kit::Div {
    let extent = 5. * std::f32::consts::SQRT_2;
    let overhang = (extent - 5.) / 2.;
    div()
        .absolute()
        .right(px(8. - overhang))
        .top(gpui_kit::relative(0.5))
        .mt(px(-4. - overhang))
        .child(icon("selector-chevron", extent, color))
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
