//! Offline Mermaid preparation and camera-resolution rasterization.
//!
//! Construct one `DiagramRenderer` per font profile, then call `render` on a bounded
//! background executor. Cancel superseded `Operation`s and discard revision-stale
//! results in the caller. Failures mean retain the original fenced source.
//! No source, SVG, font paths, or upstream error strings are logged or included in errors.

mod flow_profile;
mod fonts;
mod presentation;
mod profile;
mod security;
#[cfg(test)]
mod tests;
mod xy;

use merman::svg::{
    DeterministicTextMeasurer, HostTheme, HostThemeAppearance, MeasurementProfileId, Presentation,
    RenderResourcePolicy, RootBackgroundPostprocessor, ScopedCssPostprocessor, SvgPipeline,
    TextMeasurementPolicy, TextMeasurementProfile, TextMeasurementProfileIdentity, ThemeRole,
};
use merman::{
    Engine, MermaidConfig, ParseOptions, RenderOutput, RenderTarget, Renderer, SvgEnvironment,
    SvgRequest,
};
use serde_json::Value;
#[cfg(test)]
use serde_json::json;
use std::{sync::Arc, time::Duration};

pub const MAX_SOURCE_UTF16: usize = 8_000;
pub const MAX_SVG_BYTES: usize = 2_000_000;
pub const MAX_SVG_ELEMENTS: usize = 20_000;
pub const MAX_DIMENSION: f64 = 16_384.0;
pub const MAX_RASTER_PIXELS: u64 = 16_777_216;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagramError {
    Invalid,
    Unsupported,
    SourceLimit,
    ComplexityLimit,
    OutputLimit,
    DimensionLimit,
    RasterLimit,
    Cancelled,
    Deadline,
    UnsafeResource,
    FontUnavailable,
    Internal,
}
impl std::fmt::Display for DiagramError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "diagram {:?}", self)
    }
}
impl std::error::Error for DiagramError {}

/// Clone the handle to cancel a worker. The deadline includes queue wait time.
#[derive(Clone)]
pub struct Operation(merman::OperationControl);
impl Operation {
    pub fn new(timeout: Duration) -> Self {
        Self(merman::OperationControl::new().with_deadline(timeout))
    }
    pub fn cancel(&self) {
        self.0.cancel();
    }
    pub fn checkpoint(&self) -> Result<(), DiagramError> {
        self.0.checkpoint().map_err(|e| {
            if e.reason == merman::CancelReason::DeadlineExceeded {
                DiagramError::Deadline
            } else {
                DiagramError::Cancelled
            }
        })
    }
}

/// Same seven roles as Vue's Mermaid theme; arrow accent is text, not app accent.
#[derive(Clone, Copy, Debug)]
pub struct DiagramTheme {
    pub dark: bool,
    pub canvas: u32,
    pub text: u32,
    pub faint: u32,
    pub accent_text: u32,
    pub muted: u32,
    pub panel_raised: u32,
    pub border: u32,
}
impl DiagramTheme {
    pub fn ayu(dark: bool) -> Self {
        let t = super::support::tokens(dark);
        Self {
            dark,
            canvas: t.canvas,
            text: t.text,
            faint: t.faint,
            accent_text: t.text,
            muted: t.muted,
            panel_raised: t.panel_raised,
            border: t.border,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagramFamily {
    Flowchart,
    State,
    Sequence,
    Class,
    Er,
    Xy,
}
impl DiagramFamily {
    fn detect(kind: &str) -> Result<Self, DiagramError> {
        match kind {
            "flowchart" | "flowchart-v2" => Ok(Self::Flowchart),
            "state" | "stateDiagram" | "stateDiagram-v2" => Ok(Self::State),
            "sequence" | "sequenceDiagram" => Ok(Self::Sequence),
            "class" | "classDiagram" => Ok(Self::Class),
            "er" | "erDiagram" => Ok(Self::Er),
            "xychart" | "xychart-beta" => Ok(Self::Xy),
            _ => Err(DiagramError::Unsupported),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewBox {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
/// Only this module can create trusted SVG. Debug deliberately omits user content.
#[derive(Clone)]
pub struct Diagram {
    family: DiagramFamily,
    model: Value,
    svg: Arc<str>,
    view_box: ViewBox,
}
impl Diagram {
    pub fn family(&self) -> DiagramFamily {
        self.family
    }
    pub fn semantic_model(&self) -> &Value {
        &self.model
    }
    pub fn svg(&self) -> &str {
        &self.svg
    }
    pub fn view_box(&self) -> ViewBox {
        self.view_box
    }
}
impl std::fmt::Debug for Diagram {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Diagram")
            .field("family", &self.family)
            .field("view_box", &self.view_box)
            .finish_non_exhaustive()
    }
}

/// Immutable font database; no GPUI/window handles, shared caches or process globals.
#[derive(Clone)]
pub struct DiagramRenderer {
    fonts: Arc<fonts::Fonts>,
}
impl DiagramRenderer {
    /// Bundled TauInter weights plus a deliberate, local macOS Unicode fallback.
    /// Does not scan system fonts or fetch missing glyphs. Initialization may do local IO.
    pub fn new() -> Result<Self, DiagramError> {
        Ok(Self {
            fonts: Arc::new(fonts::Fonts::new()?),
        })
    }

    pub fn render(
        &self,
        source: &str,
        theme: DiagramTheme,
        operation: &Operation,
    ) -> Result<Diagram, DiagramError> {
        self.render_with_layout_budget(source, theme, operation, 2_000_000)
    }

    fn render_with_layout_budget(
        &self,
        source: &str,
        theme: DiagramTheme,
        operation: &Operation,
        layout_work_units: usize,
    ) -> Result<Diagram, DiagramError> {
        operation.checkpoint()?;
        if source.encode_utf16().take(MAX_SOURCE_UTF16 + 1).count() > MAX_SOURCE_UTF16 {
            return Err(DiagramError::SourceLimit);
        }
        // Metadata detection preprocesses source, but does not construct the semantic model.
        operation.checkpoint()?;
        let metadata = Engine::new().parse_metadata_sync(source);
        operation.checkpoint()?;
        let metadata = metadata.map_err(|_| DiagramError::Invalid)?;
        let family = DiagramFamily::detect(&metadata.diagram_type)?;
        let config = profile::config(theme, family);
        let color = |v: u32| format!("#{:06x}", v & 0xffffff);
        let mut host = HostTheme::new()
            .with_appearance(if theme.dark {
                HostThemeAppearance::Dark
            } else {
                HostThemeAppearance::Light
            })
            .try_with_font_family(profile::SANS)
            .map_err(|_| DiagramError::Internal)?
            .try_with_font_size("13px")
            .map_err(|_| DiagramError::Internal)?;
        let appearance = profile::Appearance::resolve(theme);
        for (role, v) in [
            (ThemeRole::Canvas, theme.canvas),
            (ThemeRole::Surface, theme.panel_raised),
            (ThemeRole::SurfaceAlt, theme.panel_raised),
            (ThemeRole::Text, theme.text),
            (ThemeRole::SubtleText, appearance.muted),
            (ThemeRole::Border, appearance.stroke),
            (ThemeRole::Line, theme.faint),
            (ThemeRole::EdgeLabelBackground, theme.canvas),
        ] {
            host = host
                .try_with_role(role, color(v))
                .map_err(|_| DiagramError::Internal)?;
        }
        host = host
            .try_with_series_palette([color(theme.text), color(theme.muted), color(theme.faint)])
            .map_err(|_| DiagramError::Internal)?;
        let presentation = Presentation::new().with_theme(host).resolve();
        // with_site_config merges in 0.8.0; with_exact_site_config would discard
        // the materialized theme. Keep this order covered by effective-config tests.
        let engine = presentation
            .materialize_engine(Engine::new())
            .with_site_config(MermaidConfig::from_value(config));
        let renderer = Renderer::new()
            .with_engine(engine)
            .with_parse_options(ParseOptions::strict());
        let fonts = self.fonts.clone();
        let missing_glyph = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let missing = missing_glyph.clone();
        let measurer =
            DeterministicTextMeasurer::default().with_width_callback(move |text, style| {
                if !fonts.covers(text) {
                    missing.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                fonts.measure(text, style)
            });
        let measurement = TextMeasurementPolicy::uniform(TextMeasurementProfile::new(
            TextMeasurementProfileIdentity::new(
                MeasurementProfileId::new("tau.diagram.inter")
                    .map_err(|_| DiagramError::Internal)?,
                "static-local-v2-menlo",
            )
            .map_err(|_| DiagramError::Internal)?,
            measurer,
        ));
        let mut resources = RenderResourcePolicy::interactive();
        for (key, max) in [
            ("max_source_bytes", 24_000),
            ("max_model_items", 512),
            ("max_model_text_bytes", 32_000),
            ("max_model_nesting_depth", 32),
            ("max_svg_bytes", MAX_SVG_BYTES),
            ("max_svg_elements", MAX_SVG_ELEMENTS),
            ("max_layout_work_units", layout_work_units),
        ] {
            resources = resources
                .with_override(key, max)
                .map_err(|_| DiagramError::Internal)?;
        }
        let request = SvgRequest {
            environment: SvgEnvironment::deterministic()
                .with_text_measurement_policy(measurement)
                .with_resource_policy(resources),
            pipeline: Some(
                SvgPipeline::resvg_safe()
                    .with_postprocessor(RootBackgroundPostprocessor::new("transparent"))
                    .with_postprocessor(ScopedCssPostprocessor::new(presentation::css(theme))),
            ),
            presentation: presentation.render_policy(),
            options: merman::svg::SvgRenderOptions {
                diagram_id: Some("tau-diagram".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        let semantic = renderer
            .prepare_semantic_with(
                source,
                operation.0.clone(),
                ParseOptions::strict(),
                *resources.input_policy(),
            )
            .map_err(classify)?
            .ok_or(DiagramError::Invalid)?;
        if semantic.diagram_type() != metadata.diagram_type {
            return Err(DiagramError::Invalid);
        }
        let model = semantic.compatibility_json().map_err(classify)?;
        check_model(&model)?;
        let RenderOutput::Svg(Some(output)) = semantic
            .render(RenderTarget::Svg(request))
            .map_err(classify)?
        else {
            return Err(DiagramError::Invalid);
        };
        operation.checkpoint()?;
        if missing_glyph.load(std::sync::atomic::Ordering::Relaxed) {
            return Err(DiagramError::FontUnavailable);
        }
        let custom_svg = if family == DiagramFamily::Xy {
            let chart = serde_json::from_value(model.clone()).map_err(|_| DiagramError::Invalid)?;
            Some(xy::render(&chart, theme, &self.fonts, operation)?)
        } else {
            None
        };
        let svg = custom_svg.as_deref().unwrap_or_else(|| output.svg());
        let view_box = security::validate(svg)?;
        Ok(Diagram {
            family,
            model,
            svg: Arc::from(svg),
            view_box,
        })
    }

    /// Rasterizes the visible camera rect, at current device scale and zoom.
    /// Output is premultiplied RGBA8 (convert channel order if the GPUI consumer needs BGRA).
    pub fn rasterize(
        &self,
        diagram: &Diagram,
        camera: RasterCamera,
        operation: &Operation,
    ) -> Result<RasterImage, DiagramError> {
        operation.checkpoint()?;
        let scale = camera.backing_scale * camera.zoom;
        let v = camera.viewport;
        if ![
            v.x,
            v.y,
            v.width,
            v.height,
            scale,
            camera.backing_scale,
            camera.zoom,
        ]
        .iter()
        .all(|v| v.is_finite())
            || v.width <= 0.
            || v.height <= 0.
            || scale <= 0.
            || scale > 64.
            || v.x.abs() > MAX_DIMENSION
            || v.y.abs() > MAX_DIMENSION
            || v.width > MAX_DIMENSION
            || v.height > MAX_DIMENSION
            || camera.backing_scale <= 0.
            || camera.zoom <= 0.
        {
            return Err(DiagramError::RasterLimit);
        }
        let width = (v.width * scale).ceil();
        let height = (v.height * scale).ceil();
        if width > MAX_DIMENSION
            || height > MAX_DIMENSION
            || width * height > MAX_RASTER_PIXELS as f64
        {
            return Err(DiagramError::RasterLimit);
        }
        let options = self.fonts.options();
        let tree = resvg::usvg::Tree::from_str(diagram.svg(), &options)
            .map_err(|_| DiagramError::UnsafeResource)?;
        operation.checkpoint()?;
        let mut pixmap = resvg::tiny_skia::Pixmap::new(width as u32, height as u32)
            .ok_or(DiagramError::RasterLimit)?;
        // usvg normalizes viewBox origin to zero before painting.
        let b = diagram.view_box;
        let transform = resvg::tiny_skia::Transform::from_row(
            scale as f32,
            0.,
            0.,
            scale as f32,
            ((b.x - v.x) * scale) as f32,
            ((b.y - v.y) * scale) as f32,
        );
        resvg::render(&tree, transform, &mut pixmap.as_mut());
        operation.checkpoint()?;
        Ok(RasterImage {
            width: pixmap.width(),
            height: pixmap.height(),
            rgba: pixmap.take(),
        })
    }
}
#[derive(Clone, Copy, Debug)]
pub struct RasterCamera {
    pub viewport: ViewBox,
    pub backing_scale: f64,
    pub zoom: f64,
}
pub struct RasterImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

fn classify(error: merman::RenderError) -> DiagramError {
    match error {
        merman::RenderError::Cancelled(e) => {
            if e.reason == merman::CancelReason::DeadlineExceeded {
                DiagramError::Deadline
            } else {
                DiagramError::Cancelled
            }
        }
        merman::RenderError::ResourceLimitExceeded(e) => match e.id {
            "max_source_bytes" => DiagramError::SourceLimit,
            "max_svg_bytes" | "max_svg_elements" => DiagramError::OutputLimit,
            _ => DiagramError::ComplexityLimit,
        },
        merman::RenderError::Parse(_) | merman::RenderError::NoDiagram => DiagramError::Invalid,
        _ => DiagramError::Invalid,
    }
}
fn check_model(model: &Value) -> Result<(), DiagramError> {
    // Individual node/edge limits complement merman's cross-family total/nesting budget.
    fn visit(v: &Value) -> Result<(), DiagramError> {
        match v {
            Value::Object(map) => {
                for (key, v) in map {
                    let limit = match key.as_str() {
                        "nodes" | "vertices" | "participants" | "actors" | "classes"
                        | "entities" | "states" => 256,
                        "plots" => 32,
                        "edges" | "relations" | "relationships" | "messages" => 384,
                        _ => usize::MAX,
                    };
                    let len = match v {
                        Value::Array(a) => a.len(),
                        Value::Object(o) => o.len(),
                        _ => 0,
                    };
                    if len > limit {
                        return Err(DiagramError::ComplexityLimit);
                    }
                    visit(v)?;
                }
            }
            Value::Array(a) => {
                for v in a {
                    visit(v)?;
                }
            }
            _ => (),
        }
        Ok(())
    }
    visit(model)
}
