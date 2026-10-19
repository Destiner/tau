use super::super::support;
use gpui_kit::{
    Bounds, BoxShadow, FocusHandle, IntoElement, Pixels, Role, TestSupportExt,
    base::{ElementExt, Placement, Positioner, Tooltip},
    div,
    prelude::*,
    px, rgb,
};
use std::{cell::Cell, rc::Rc};

/// Only reviewed protocol state crosses this API; no raw provider-error constructor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetryPresentation {
    pub attempt: Option<u64>,
    pub max_attempts: Option<u64>,
    pub reason: tau_session::session::RetryReason,
}
impl From<&tau_session::session::RetryState> for RetryPresentation {
    fn from(value: &tau_session::session::RetryState) -> Self {
        Self {
            attempt: value.attempt,
            max_attempts: value.max_attempts,
            reason: value.reason,
        }
    }
}

pub(crate) fn glow(accent: u32) -> Vec<BoxShadow> {
    vec![
        BoxShadow::new(
            px(0.),
            px(0.),
            gpui_kit::Rgba {
                a: 0.22,
                ..rgb(accent)
            }
            .into(),
        )
        .spread_radius(px(2.)),
    ]
}

pub(super) fn indicator(
    retry: &RetryPresentation,
    focus: &FocusHandle,
    dark: bool,
    show_reason: bool,
    bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
) -> impl IntoElement {
    let t = support::tokens(dark);
    let reason = retry.reason.label();
    let tooltip = show_reason.then(|| bounds.get()).flatten().map(|bounds| {
        gpui_kit::deferred(
            Positioner::side(bounds)
                .placement(Placement::Top)
                .offset(px(6.))
                .margin(px(6.))
                .child(
                    support::tooltip_surface(Tooltip::new("retry-tooltip-surface"), dark)
                        .child(div().id("retry-tooltip").test_support().child(reason)),
                ),
        )
        .with_priority(200)
    });
    div()
        .id("retry-indicator")
        .test_support()
        .role(Role::Status)
        .aria_label(format!("Retrying. {reason}"))
        .track_focus(focus)
        .tab_stop(true)
        .flex_shrink_0()
        .focus_visible(|style| style.shadow(glow(t.accent)))
        .h(px(22.))
        .px(px(4.))
        .rounded(px(4.))
        .text_size(px(10.))
        .font_weight(gpui_kit::FontWeight(550.))
        .text_color(rgb(t.accent))
        .flex()
        .items_center()
        .child("Retrying…")
        .on_prepaint(move |rect, window, _| {
            if bounds.replace(Some(rect)) != Some(rect) {
                window.request_animation_frame();
            }
        })
        .when_some(tooltip, |d, tooltip| d.child(tooltip))
}
