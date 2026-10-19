use super::{
    DiagramTheme,
    profile::{Appearance, color},
};

pub(super) fn css(theme: DiagramTheme) -> String {
    let c = |v: u32| format!("#{:06x}", v & 0xffffff);
    let mut css = format!(
        ".arrowheadPath, .arrowMarkerPath, marker[id$='-arrowhead'] path, marker[id$='-filled-head'] path, marker[id$='-barbEnd'] path {{ fill:{} !important;stroke:{} !important; }} .edgeLabel rect {{ fill:{} !important;opacity:1; }} .main > rect.background {{ fill:transparent !important; }}",
        c(theme.accent_text),
        c(theme.accent_text),
        c(theme.canvas)
    );
    let a = Appearance::resolve(theme);
    css.push_str(&format!(
        " .nodeLabel {{ font-weight:500; }} .edgeLabel text {{ font-size:11px; font-weight:400; fill:{} !important; }} .cluster-label text {{ font-size:12px; font-weight:600; }} .actor-line {{ stroke:{} !important; stroke-width:1; stroke-dasharray:6 4; }} .messageText {{ fill:{} !important; }} marker.extension, marker.aggregation, marker.extension path, marker.extension polygon, marker.aggregation path {{ fill:{} !important; stroke:{} !important; }}",
        color(a.secondary), color(a.muted), color(a.muted), color(theme.canvas), color(a.text)
    ));
    css.push_str(&format!(" marker.er circle {{ fill:{} !important; }} marker.composition, marker.dependency, marker.composition path, marker.dependency path {{ fill:{} !important;stroke:{} !important; }}",color(theme.canvas),color(a.text),color(a.text)));
    for i in 0..32 {
        let color = series_color(i, theme);
        let wash = mix(theme.canvas, color, 0.25);
        css.push_str(&format!(" .bar-plot-{i} rect {{ fill:{} !important;stroke:{} !important;stroke-width:1.5; }} .line-plot-{i} path {{ stroke:{} !important;fill:none; }}",c(wash),c(color),c(color)));
    }
    css
}
/// Captured Vue compatibility: JS receives non-hex CSS variables for both
/// accent/background. Secondary hues use fallback blue and the light branch.
pub(super) fn series_color(index: usize, theme: DiagramTheme) -> u32 {
    if index == 0 {
        return theme.accent_text;
    }
    let (h, s, _) = rgb_to_hsl(0x3b82f6);
    let tier = index.div_ceil(2) as f64;
    let odd = index % 2 == 1;
    let dark = odd;
    let l = if dark {
        (48. - tier * 13.).max(25.)
    } else {
        (55. + tier * 11.).min(78.)
    } / 100.;
    let h = (h + if dark { -8. } else { 12. } * tier).rem_euclid(360.);
    hsl_to_rgb(h, s.clamp(0.55, 0.85), l)
}
fn rgb_to_hsl(color: u32) -> (f64, f64, f64) {
    let r = ((color >> 16) & 255) as f64 / 255.;
    let g = ((color >> 8) & 255) as f64 / 255.;
    let b = (color & 255) as f64 / 255.;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.;
    let d = max - min;
    if d == 0. {
        return (0., 0., l);
    }
    let s = d / (1. - (2. * l - 1.).abs());
    let h = if max == r {
        ((g - b) / d).rem_euclid(6.)
    } else if max == g {
        (b - r) / d + 2.
    } else {
        (r - g) / d + 4.
    };
    (h * 60., s, l)
}
fn hsl_to_rgb(h: f64, s: f64, l: f64) -> u32 {
    let c = (1. - (2. * l - 1.).abs()) * s;
    let x = c * (1. - ((h / 60.).rem_euclid(2.) - 1.).abs());
    let m = l - c / 2.;
    let (r, g, b) = match (h / 60.) as u32 {
        0 => (c, x, 0.),
        1 => (x, c, 0.),
        2 => (0., c, x),
        3 => (0., x, c),
        4 => (x, 0., c),
        _ => (c, 0., x),
    };
    let channel = |v: f64| ((v + m) * 255.).round() as u32;
    channel(r) << 16 | channel(g) << 8 | channel(b)
}
pub(super) fn mix(bg: u32, fg: u32, amount: f64) -> u32 {
    let mut out = 0;
    for shift in [0, 8, 16] {
        let a = ((bg >> shift) & 255) as f64;
        let b = ((fg >> shift) & 255) as f64;
        out |= ((a + (b - a) * amount).round() as u32) << shift;
    }
    out
}
