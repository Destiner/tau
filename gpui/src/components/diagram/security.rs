use super::{DiagramError, MAX_DIMENSION, MAX_SVG_BYTES, MAX_SVG_ELEMENTS, ViewBox};

pub(super) fn validate(svg: &str) -> Result<ViewBox, DiagramError> {
    if svg.len() > MAX_SVG_BYTES {
        return Err(DiagramError::OutputLimit);
    }
    // No DTD/entities, even if a future upstream pipeline changes its XML serializer.
    if svg.contains("<!DOCTYPE") || svg.contains("<!ENTITY") {
        return Err(DiagramError::UnsafeResource);
    }
    let doc = roxmltree::Document::parse(svg).map_err(|_| DiagramError::UnsafeResource)?;
    let root = doc.root_element();
    if !root.has_tag_name("svg") {
        return Err(DiagramError::UnsafeResource);
    }
    let mut elements = 0;
    for node in doc.descendants().filter(|n| n.is_element()) {
        elements += 1;
        if elements > MAX_SVG_ELEMENTS {
            return Err(DiagramError::OutputLimit);
        }
        if node
            .tag_name()
            .namespace()
            .is_some_and(|ns| ns != "http://www.w3.org/2000/svg")
        {
            return Err(DiagramError::UnsafeResource);
        }
        if !matches!(
            node.tag_name().name(),
            "svg"
                | "g"
                | "defs"
                | "path"
                | "rect"
                | "circle"
                | "ellipse"
                | "line"
                | "polyline"
                | "polygon"
                | "text"
                | "tspan"
                | "style"
                | "marker"
                | "symbol"
                | "clipPath"
                | "title"
                | "desc"
                | "filter"
                | "feDropShadow"
        ) {
            return Err(DiagramError::UnsafeResource);
        }
        for a in node.attributes() {
            let name = a.name();
            let visible_marker =
                name == "overflow" && node.has_tag_name("marker") && a.value() == "visible";
            if a.namespace().is_some()
                || (!name.starts_with("data-")
                    && !name.starts_with("aria-")
                    && !visible_marker
                    && !matches!(
                        name,
                        "id" | "class"
                            | "role"
                            | "name"
                            | "viewBox"
                            | "width"
                            | "height"
                            | "x"
                            | "y"
                            | "x1"
                            | "x2"
                            | "y1"
                            | "y2"
                            | "cx"
                            | "cy"
                            | "r"
                            | "rx"
                            | "ry"
                            | "dx"
                            | "dy"
                            | "d"
                            | "points"
                            | "transform"
                            | "fill"
                            | "fill-rule"
                            | "fill-opacity"
                            | "stroke"
                            | "stroke-width"
                            | "stroke-dasharray"
                            | "stroke-dashoffset"
                            | "stroke-linecap"
                            | "stroke-linejoin"
                            | "stroke-opacity"
                            | "opacity"
                            | "style"
                            | "font-family"
                            | "font-size"
                            | "font-weight"
                            | "font-style"
                            | "text-anchor"
                            | "dominant-baseline"
                            | "alignment-baseline"
                            | "textLength"
                            | "lengthAdjust"
                            | "letter-spacing"
                            | "word-spacing"
                            | "marker-start"
                            | "marker-mid"
                            | "marker-end"
                            | "markerWidth"
                            | "markerHeight"
                            | "markerUnits"
                            | "refX"
                            | "refY"
                            | "orient"
                            | "clip-path"
                            | "clip-rule"
                            | "filter"
                            | "filterUnits"
                            | "stdDeviation"
                            | "flood-color"
                            | "flood-opacity"
                            | "preserveAspectRatio"
                    ))
            {
                return Err(DiagramError::UnsafeResource);
            }
            if matches!(
                name,
                "style"
                    | "fill"
                    | "stroke"
                    | "filter"
                    | "clip-path"
                    | "marker-start"
                    | "marker-mid"
                    | "marker-end"
            ) {
                resource_value(a.value())?;
            }
        }
        if node.has_tag_name("style") {
            resource_value(node.text().unwrap_or(""))?;
        }
    }
    let values = root
        .attribute("viewBox")
        .ok_or(DiagramError::DimensionLimit)?
        .split(|c: char| c.is_ascii_whitespace() || c == ',')
        .filter(|s| !s.is_empty())
        .map(str::parse::<f64>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| DiagramError::DimensionLimit)?;
    if values.len() != 4
        || !values
            .iter()
            .all(|v| v.is_finite() && v.abs() <= MAX_DIMENSION)
        || values[2] <= 0.
        || values[3] <= 0.
    {
        return Err(DiagramError::DimensionLimit);
    }
    Ok(ViewBox {
        x: values[0],
        y: values[1],
        width: values[2],
        height: values[3],
    })
}
fn resource_value(value: &str) -> Result<(), DiagramError> {
    let lower = value.to_ascii_lowercase();
    // Generated CSS is plain ASCII syntax. Escape/comment tricks aren't needed here.
    if lower.contains('@')
        || lower.contains('\\')
        || lower.contains("/*")
        || lower.contains("expression(")
    {
        return Err(DiagramError::UnsafeResource);
    }
    let mut rest = lower.as_str();
    while let Some(i) = rest.find("url") {
        rest = &rest[i + 3..];
        let tail = rest.trim_start();
        if !tail.starts_with('(') {
            continue;
        }
        let end = tail.find(')').ok_or(DiagramError::UnsafeResource)?;
        let target = tail[1..end].trim().trim_matches(['\'', '"']);
        if !target.starts_with('#')
            || target.len() < 2
            || !target[1..]
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':'))
        {
            return Err(DiagramError::UnsafeResource);
        }
        rest = &tail[end + 1..];
    }
    Ok(())
}
