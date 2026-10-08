//! Inert XY presentation over Merman's typed semantic model. Layout constants and
//! chart primitives follow beautiful-mermaid 1.1.3 (MIT; see NOTICES.md).
use super::{
    DiagramError, DiagramTheme, Operation,
    fonts::Fonts,
    presentation::{mix, series_color},
    profile::{Appearance, SANS, color},
};
use merman::diagrams::xychart::{
    XyChartAxisRenderModel as Axis, XyChartDiagramRenderModel as Chart, XyChartPlotType as Kind,
};
use std::fmt::Write;

pub(super) fn render(
    chart: &Chart,
    theme: DiagramTheme,
    fonts: &Fonts,
    op: &Operation,
) -> Result<String, DiagramError> {
    op.checkpoint()?;
    for label in [
        chart.title.as_deref().unwrap_or(""),
        title(&chart.x_axis),
        title(&chart.y_axis),
    ] {
        if !fonts.covers(label) {
            return Err(DiagramError::FontUnavailable);
        }
    }
    let horizontal = chart.orientation.eq_ignore_ascii_case("horizontal");
    let count = match &chart.x_axis {
        Axis::Band { categories, .. } => categories.len(),
        _ => chart
            .plots
            .iter()
            .find(|p| !p.values.is_empty())
            .map_or(1, |p| p.values.len()),
    }
    .max(1);
    if chart.plots.iter().map(|p| p.values.len()).sum::<usize>() > 4096 {
        return Err(DiagramError::ComplexityLimit);
    }
    let labels: Vec<String> = match &chart.x_axis {
        Axis::Band { categories, .. } => categories.clone(),
        Axis::Linear { min, max, .. } => (0..count)
            .map(|i| {
                tick(
                    min.unwrap_or(0.)
                        + i as f64 * (max.unwrap_or(count as f64) - min.unwrap_or(0.))
                            / (count - 1).max(1) as f64,
                )
            })
            .collect(),
    };
    let values: Vec<f64> = chart
        .plots
        .iter()
        .flat_map(|p| p.values.iter().copied())
        .collect();
    if values.iter().any(|v| !v.is_finite()) {
        return Err(DiagramError::Invalid);
    }
    let (min, max) = match chart.y_axis {
        Axis::Linear { min, max, .. } => (
            min.unwrap_or_else(|| values.iter().copied().fold(0., f64::min)),
            max.unwrap_or_else(|| values.iter().copied().fold(0., f64::max)),
        ),
        _ => return Err(DiagramError::Unsupported),
    };
    if !min.is_finite() || !max.is_finite() || max < min || !(max - min).is_finite() {
        return Err(DiagramError::Invalid);
    }
    let ticks = nice_ticks(min, max);
    let x_title = title(&chart.x_axis);
    let y_title = title(&chart.y_axis);
    let title_text = chart.title.as_deref().unwrap_or("");
    let top = 22.
        + if title_text.is_empty() { 0. } else { 42. }
        + if chart.plots.len() > 1 { 28. } else { 0. };
    let bottom_title = if horizontal { y_title } else { x_title };
    let left_title = if horizontal { x_title } else { y_title };
    let bottom = 60. + if bottom_title.is_empty() { 0. } else { 30. };
    let axis_labels: Vec<String> = if horizontal {
        labels.clone()
    } else {
        ticks.iter().map(|v| tick(*v)).collect()
    };
    let mut label_width: f64 = if horizontal { 40. } else { 58. };
    for label in &axis_labels {
        label_width = label_width.max(measure(fonts, label, 14.)?);
    }
    let left = 22. + label_width + 18. + if left_title.is_empty() { 0. } else { 30. };
    let width = left + 600. + 22.;
    let height = top + 340. + bottom;
    let a = Appearance::resolve(theme);
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {width} {height}\" width=\"{width}\" height=\"{height}\" font-family=\"{SANS}\"><title>{}</title>",
        escape(title_text)
    );
    let cat = |i: usize| {
        if horizontal {
            top + (i as f64 + 0.5) * 340. / count as f64
        } else {
            left + (i as f64 + 0.5) * 600. / count as f64
        }
    };
    let val = |v: f64| {
        let t = (v - min) / if max == min { 1. } else { max - min };
        if horizontal {
            left + t * 600.
        } else {
            top + 340. - t * 340.
        }
    };
    // Anchor the dot grid to the ticks, keeping spacing near 20 logical pixels.
    let x_base = if horizontal {
        ticks
            .get(1)
            .map_or(100., |v| (val(*v) - val(ticks[0])).abs())
    } else {
        600. / count as f64
    };
    let y_base = if horizontal {
        340. / count as f64
    } else {
        ticks
            .get(1)
            .map_or(340. / 6., |v| (val(*v) - val(ticks[0])).abs())
    };
    let gap = |base: f64| base / (base / 20.).round().max(1.);
    let gx = gap(x_base.max(1.));
    let gy = gap(y_base.max(1.));
    let ax = if horizontal { val(ticks[0]) } else { cat(0) };
    let ay = if horizontal { cat(0) } else { val(ticks[0]) };
    let start_x = ax - ((ax - left) / gx).ceil() * gx;
    let start_y = ay - ((ay - top) / gy).ceil() * gy;
    let mut y = start_y;
    while y <= top + 340. + 0.5 {
        op.checkpoint()?;
        let mut x = start_x;
        while x <= left + 600. + 0.5 {
            write!(svg, "<circle class=\"xy-grid\" cx=\"{x}\" cy=\"{y}\" r=\"1.5\" fill=\"{}\" opacity=\"0.65\"/>", color(a.divider)).unwrap();
            x += gx;
        }
        y += gy;
    }
    let bars = chart
        .plots
        .iter()
        .filter(|p| p.plot_type == Kind::Bar)
        .count();
    let band = if horizontal { 340. } else { 600. } / count as f64;
    let bar_size = (band * 0.8 / bars.max(1) as f64).min(40.);
    let mut bar_index = 0;
    for (index, plot) in chart.plots.iter().enumerate() {
        op.checkpoint()?;
        let stroke = color(series_color(index, theme));
        let fill = color(mix(theme.canvas, series_color(index, theme), 0.25));
        if plot.plot_type == Kind::Bar {
            for (i, v) in plot.values.iter().enumerate() {
                let offset = cat(i) - bar_size * bars as f64 / 2. + bar_index as f64 * bar_size;
                let base = val(0_f64.max(min));
                let end = val(if horizontal { v.max(min) } else { *v });
                let (x, y, w, h) = if horizontal {
                    (base.min(end), offset, (base - end).abs(), bar_size)
                } else {
                    (offset, base.min(end), bar_size, (base - end).abs())
                };
                let path = rounded_bar(x, y, w, h, horizontal);
                write!(svg,"<path class=\"xy-bar\" d=\"{path}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"1.5\"/>").unwrap();
            }
            bar_index += 1;
        } else {
            let points: Vec<(f64, f64)> = plot
                .values
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    if horizontal {
                        (val(*v), cat(i))
                    } else {
                        (cat(i), val(*v))
                    }
                })
                .collect();
            if points.is_empty() {
                continue;
            }
            let path = curve(&points);
            write!(svg,"<path class=\"xy-line-shadow\" d=\"{path}\" transform=\"translate(0,2)\" fill=\"none\" stroke=\"{stroke}\" stroke-width=\"5\" stroke-linecap=\"round\" opacity=\"0.12\"/><path class=\"xy-line\" d=\"{path}\" fill=\"none\" stroke=\"{stroke}\" stroke-width=\"2.5\" stroke-linecap=\"round\" stroke-linejoin=\"round\"/>").unwrap();
            if chart
                .plots
                .iter()
                .filter(|p| p.plot_type == Kind::Line)
                .map(|p| p.values.len())
                .max()
                .unwrap_or(0)
                <= 12
            {
                for (x, y) in points {
                    write!(svg,"<circle class=\"xy-point\" cx=\"{x}\" cy=\"{y}\" r=\"5\" fill=\"{stroke}\" stroke=\"{}\" stroke-width=\"2\"/>",color(theme.canvas)).unwrap();
                }
            }
        }
    }
    for (i, label) in labels.iter().enumerate() {
        let (x, y, anchor) = if horizontal {
            (left - 18., cat(i), "end")
        } else {
            (cat(i), top + 340. + 18., "middle")
        };
        text(&mut svg, label, x, y, 14, 400, anchor, a.muted, None);
    }
    for v in ticks {
        let (x, y, anchor) = if horizontal {
            (val(v), top + 340. + 18., "middle")
        } else {
            (left - 18., val(v), "end")
        };
        text(&mut svg, &tick(v), x, y, 14, 400, anchor, a.muted, None);
    }
    if !bottom_title.is_empty() {
        text(
            &mut svg,
            bottom_title,
            left + 300.,
            height - 22.,
            15,
            500,
            "middle",
            a.secondary,
            None,
        );
    }
    if !left_title.is_empty() {
        text(
            &mut svg,
            left_title,
            26.,
            top + 170.,
            15,
            500,
            "middle",
            a.secondary,
            Some(-90),
        );
    }
    if !title_text.is_empty() {
        text(
            &mut svg,
            title_text,
            width / 2.,
            40.,
            18,
            600,
            "middle",
            a.text,
            None,
        );
    }
    if chart.plots.len() > 1 {
        let mut bar = 0;
        let mut line = 0;
        let mut legends = Vec::new();
        for plot in &chart.plots {
            let label = if plot.plot_type == Kind::Bar {
                bar += 1;
                format!("Bar {bar}")
            } else {
                line += 1;
                format!("Line {line}")
            };
            let w = 20. + measure(fonts, &label, 14.)?;
            legends.push((label, w));
        }
        let total = legends.iter().map(|(_, w)| w).sum::<f64>() + 16. * (legends.len() - 1) as f64;
        let mut x = width / 2. - total / 2.;
        let y = 22. + if title_text.is_empty() { 0. } else { 42. } + 14.;
        for (i, (label, w)) in legends.iter().enumerate() {
            let stroke = color(series_color(i, theme));
            if chart.plots[i].plot_type == Kind::Bar {
                write!(svg,"<rect class=\"xy-legend-bar\" x=\"{x}\" y=\"{}\" width=\"14\" height=\"14\" rx=\"3\" stroke=\"{stroke}\" stroke-width=\"1.5\" fill=\"{}\"/>",y-7.,color(mix(theme.canvas,series_color(i,theme),0.25))).unwrap();
            } else {
                write!(svg,"<line class=\"xy-legend-line\" x1=\"{x}\" x2=\"{}\" y1=\"{y}\" y2=\"{y}\" stroke=\"{stroke}\" stroke-width=\"2.5\" stroke-linecap=\"round\"/>",x+14.).unwrap();
            }
            text(&mut svg, label, x + 20., y, 14, 400, "start", a.muted, None);
            x += w + 16.;
        }
    }
    svg.push_str("</svg>");
    op.checkpoint()?;
    Ok(svg)
}
fn title(axis: &Axis) -> &str {
    match axis {
        Axis::Band { title, .. } | Axis::Linear { title, .. } => title,
    }
}
fn measure(fonts: &Fonts, s: &str, size: f64) -> Result<f64, DiagramError> {
    if !fonts.covers(s) {
        return Err(DiagramError::FontUnavailable);
    }
    Ok(fonts.measure(
        s,
        &merman::svg::TextStyle {
            font_family: Some(SANS.into()),
            font_size: size,
            ..Default::default()
        },
    ))
}
fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
#[allow(clippy::too_many_arguments)]
fn text(
    svg: &mut String,
    s: &str,
    x: f64,
    y: f64,
    size: u16,
    weight: u16,
    anchor: &str,
    fill: u32,
    rotate: Option<i32>,
) {
    let transform = rotate.map_or(String::new(), |r| {
        format!(" transform=\"rotate({r},{x},{y})\"")
    });
    write!(svg,"<text x=\"{x}\" y=\"{y}\" dy=\"0.35em\" font-size=\"{size}\" font-weight=\"{weight}\" text-anchor=\"{anchor}\" fill=\"{}\"{transform}>{}</text>",color(fill),escape(s)).unwrap();
}
fn tick(v: f64) -> String {
    if v.fract() == 0. {
        format!("{v:.0}")
    } else if v.abs() < 10. {
        format!("{v:.1}")
    } else {
        format!("{v:.0}")
    }
}
fn nice_ticks(min: f64, max: f64) -> Vec<f64> {
    if max <= min {
        return vec![min];
    }
    let raw = (max - min) / 6.;
    let magnitude = 10_f64.powf(raw.log10().floor());
    if magnitude == 0. || !magnitude.is_finite() {
        return vec![min];
    }
    let residual = raw / magnitude;
    let interval = magnitude
        * if residual <= 1.5 {
            1.
        } else if residual <= 3. {
            2.
        } else if residual <= 7. {
            5.
        } else {
            10.
        };
    let ticks: Vec<_> = (0..16)
        .map(|i| (min / interval).ceil() * interval + i as f64 * interval)
        .take_while(|v| v.is_finite() && *v <= max + interval * 0.001)
        .collect();
    if ticks.is_empty() { vec![min] } else { ticks }
}
fn rounded_bar(x: f64, y: f64, w: f64, h: f64, _horizontal: bool) -> String {
    let r = 8_f64.min(w / 2.).min(h / 2.);
    format!(
        "M{x},{} Q{x},{y} {},{y} H{} Q{},{y} {},{} V{} Q{},{} {},{} H{} Q{x},{} {x},{} Z",
        y + r,
        x + r,
        x + w - r,
        x + w,
        x + w,
        y + r,
        y + h - r,
        x + w,
        y + h,
        x + w - r,
        y + h,
        x + r,
        y + h,
        y + h - r
    )
}
fn curve(points: &[(f64, f64)]) -> String {
    let n = points.len();
    let mut path = format!("M{},{}", points[0].0, points[0].1);
    if n == 1 {
        return path;
    }
    let h: Vec<f64> = points.windows(2).map(|p| p[1].0 - p[0].0).collect();
    // Degenerate horizontal-series values still get a finite, semantic polyline.
    if n == 2
        || h.iter().any(|v| v.abs() < f64::EPSILON)
        || h.windows(2).any(|v| v[0].signum() != v[1].signum())
    {
        for (x, y) in &points[1..] {
            write!(path, " L{x},{y}").unwrap();
        }
        return path;
    }
    let delta: Vec<f64> = points
        .windows(2)
        .zip(&h)
        .map(|(p, h)| (p[1].1 - p[0].1) / h)
        .collect();
    let mut c = vec![0.; n];
    let mut cp = vec![0.; n];
    let mut dp = vec![0.; n];
    for i in 1..n - 1 {
        let diag = 2. * (h[i - 1] + h[i]);
        let rhs = 3. * (delta[i] - delta[i - 1]);
        let w = diag - h[i - 1] * cp[i - 1];
        cp[i] = h[i] / w;
        dp[i] = (rhs - h[i - 1] * dp[i - 1]) / w;
    }
    for i in (1..n - 1).rev() {
        c[i] = dp[i] - cp[i] * c[i + 1];
    }
    let mut slopes = vec![0.; n];
    for i in 0..n - 1 {
        slopes[i] = delta[i] - h[i] * (2. * c[i] + c[i + 1]) / 3.;
    }
    slopes[n - 1] = delta[n - 2] + h[n - 2] * c[n - 2] / 3.;
    for i in 0..n - 1 {
        let seg = h[i] / 3.;
        write!(
            path,
            " C{},{} {},{} {},{}",
            points[i].0 + seg,
            points[i].1 + slopes[i] * seg,
            points[i + 1].0 - seg,
            points[i + 1].1 - slopes[i + 1] * seg,
            points[i + 1].0,
            points[i + 1].1
        )
        .unwrap();
    }
    path
}
