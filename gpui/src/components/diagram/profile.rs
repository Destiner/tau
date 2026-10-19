//! Product family defaults, not source-dependent geometry corrections.
use super::{DiagramFamily, DiagramTheme, flow_profile, presentation::mix};
use serde_json::{Value, json};

pub(super) const SANS: &str = "TauInter";
pub(super) const MONO: &str = "Menlo";

#[derive(Clone, Copy)]
pub(super) struct Appearance {
    pub text: u32,
    pub secondary: u32,
    pub muted: u32,
    pub stroke: u32,
    pub divider: u32,
}
impl Appearance {
    pub fn resolve(theme: DiagramTheme) -> Self {
        // Vue's self-referencing --muted/--border declarations are invalid at computed
        // value time. Match the captured renderer's fallback mixes, not the app tokens.
        Self {
            text: theme.text,
            secondary: mix(theme.canvas, theme.text, 0.6),
            muted: mix(theme.canvas, theme.text, 0.4),
            stroke: mix(theme.canvas, theme.text, 0.2),
            divider: mix(theme.canvas, theme.text, 0.12),
        }
    }
}
pub(super) fn color(v: u32) -> String {
    format!("#{:06x}", v & 0xffffff)
}
pub(super) fn config(theme: DiagramTheme, family: DiagramFamily) -> Value {
    let a = Appearance::resolve(theme);
    let sequence_presentation = json!({
        "actorFontFamily":SANS,"messageFontFamily":SANS,"noteFontFamily":SANS,
        "actorFontSize":13,"actorFontWeight":500,"messageFontSize":11,"messageFontWeight":400,"noteFontSize":11,"noteFontWeight":400,
        "actorMinWidth":80,"actorHeight":40,"actorPaddingX":16,"actorCenterGap":140,"headerGap":20,
        "messageRowHeight":40,"selfMessageHeight":30,"outerPadding":30
    });
    let mut config = json!({
        "securityLevel":"strict", "look":"classic", "layout":"dagre", "htmlLabels":false,
        "fontSize":false, "fontWeight":false,
        "flowchart":{"htmlLabels":false,"diagramPadding":12,"padding":10,"wrappingWidth":400,"minNodeWidth":0,"nodeSpacing":28,"rankSpacing":48},
        "state":json!({"padding":20,"minNodeWidth":0,"nodeSpacing":28,"rankSpacing":48,
            "presentationProfile":"compact","presentation":{
                "fontFamily":SANS,"fontSize":13,"fontWeight":500,"edgeFontSize":11,"edgeFontWeight":400,
                "paddingX":20,"paddingY":10,"nodeSpacing":28,"layerSpacing":48,"outerPadding":12}}),
        "class":{"diagramPadding":12,"padding":8,"nodeSpacing":40,"rankSpacing":60,
            "presentationProfile":"compact","memberFontFamily":MONO},
        "er":{"diagramPadding":12,"defaultDirection":"LR","minEntityWidth":140,"nodeSpacing":70,"rankSpacing":90,
            "presentationProfile":"compact","attributeFontFamily":MONO},
        "sequence":json!({"diagramMarginX":30,"diagramMarginY":30,"width":80,"height":40,"wrapPadding":16,"actorMargin":60,"messageMargin":40,"mirrorActors":false,"actorFontSize":13,"actorFontWeight":500,"messageFontSize":11,"messageFontWeight":400,"noteFontSize":11,"noteFontWeight":400,
            "presentationProfile":"compact","presentation":sequence_presentation}),
        "themeVariables":{"fontFamily":SANS,"fontSize":"13px","arrowheadColor":color(theme.accent_text),"edgeLabelBorderColor":color(a.divider),"primaryBorderColor":color(a.stroke),"nodeBorder":color(a.stroke),"actorBorder":color(a.stroke),"actorLineColor":color(a.muted),"signalTextColor":color(a.muted)}
    });
    if matches!(family, DiagramFamily::Class | DiagramFamily::Er) {
        // Stock class edge/auxiliary text reads the theme size; compartments own their roles.
        config["themeVariables"]["fontSize"] = json!("11px");
    }
    if family == DiagramFamily::Flowchart {
        flow_profile::apply(&mut config);
    }
    config
}
