//! Production TranscriptNotice.vue surface; the body remains a transcript Copy source.
use super::support;
use gpui_kit::{FontWeight, IntoElement, TestSupportExt, div, prelude::*, px, rgb};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeSeverity {
    Info,
    Warning,
    Error,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub severity: NoticeSeverity,
    pub label: String,
    pub text: String,
    /// ErrorNotice.vue uses inline Markdown, unlike extension notifications.
    pub inline: bool,
}
impl Notice {
    pub fn label(&self) -> &str {
        if !self.label.is_empty() {
            &self.label
        } else {
            match self.severity {
                NoticeSeverity::Info => "Info",
                NoticeSeverity::Warning => "Warning",
                NoticeSeverity::Error => "Error",
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
pub(super) fn surface(
    key: u64,
    notice: &Notice,
    dark: bool,
    body: impl IntoElement,
) -> gpui_kit::AnyElement {
    let t = support::tokens(dark);
    let tone = match notice.severity {
        NoticeSeverity::Info => t.muted,
        NoticeSeverity::Warning => t.accent,
        NoticeSeverity::Error => t.danger,
    };
    // Light --border is translucent: retain its alpha until compositing over the fill.
    let fill = mix(tone, t.panel_raised, 0.07);
    let border = if dark {
        mix(tone, t.border, 0.32)
    } else {
        let tone = rgb(tone);
        let base = rgb(0x6b7d8f);
        let alpha = 0.32 + 0.68 * 0.24;
        gpui_kit::Rgba {
            r: tone.r * 0.32 + base.r * 0.68 * 0.24 + fill.r * (1. - alpha),
            g: tone.g * 0.32 + base.g * 0.68 * 0.24 + fill.g * (1. - alpha),
            b: tone.b * 0.32 + base.b * 0.68 * 0.24 + fill.b * (1. - alpha),
            a: 1.,
        }
    };
    let mut card = div()
        .id(format!("transcript-notice-{key}"))
        .test_support()
        .w_full()
        .min_w_0()
        .flex()
        .flex_col()
        .px(px(10.))
        .py(px(7.))
        .border_1()
        .border_color(border)
        .rounded(px(6.))
        .bg(fill)
        .gap(px(2.))
        .font_family(super::super::font::family())
        .child(
            div()
                .id(format!("transcript-notice-label-{key}"))
                .test_support()
                .min_w_0()
                .text_ellipsis()
                .text_size(px(9.))
                .font_weight(FontWeight::MEDIUM)
                .line_height(gpui_kit::relative(1.2))
                .text_color(rgb(tone))
                .child(gpui_kit::Text::new(
                    format!("transcript-notice-label-text-{key}").into(),
                    notice.label().to_owned().into(),
                )),
        )
        .child(
            div()
                .id(format!("transcript-notice-body-{key}"))
                .test_support()
                .min_w_0()
                .text_size(px(12.))
                .line_height(gpui_kit::relative(1.5))
                .text_color(rgb(t.text))
                .child(body),
        );
    card.text_style().fractional_line_height = Some(true);
    card.into_any_element()
}
