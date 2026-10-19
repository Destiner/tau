# Native diagram API

`components::diagram` is a library only: no Markdown hooks, windows, viewer, cache,
telemetry, or background queue are installed here.

```rust,ignore
let renderer = DiagramRenderer::new()?; // once, outside paint; local font IO
let operation = Operation::new(Duration::from_secs(3));
// Move cloned renderer/operation + owned source to a bounded background executor.
let diagram = renderer.render(&source, DiagramTheme::ayu(dark), &operation)?;
let bounds = diagram.view_box();
let image = renderer.rasterize(&diagram, RasterCamera {
    viewport: bounds, // or visible camera rect in SVG viewBox coordinates
    backing_scale: device_scale,
    zoom: camera_zoom,
}, &operation)?;
```

`Diagram` has a typed family, semantic model, trusted generated SVG, and finite
natural viewBox (including origin). Its constructor is private. No arbitrary SVG
input is accepted by the rasterizer. SVG ids are local to one image; do not inject
multiple copies into a shared DOM without separately namespacing them.
`RasterImage.rgba` is **premultiplied RGBA8**, not a PNG or BGRA. The caller must
adapt to its native image upload API. A viewport is a rect in diagram coordinates;
its output dimensions are ceil(viewport size × backing scale × zoom). Crop the
viewport as zoom increases rather than allocating a whole enormous diagram.
Both dimension and pixel ceilings are checked before allocation. Viewport origin
is bounded and effective device scale is capped at 64. Re-rasterize on camera,
backing-scale, or scheme changes. Do not upscale a fixed low-resolution image.

A renderer, operation, and diagram are Send + Sync. Concurrent operations have
independent engines, budgets and missing-glyph flags. Deadline starts when
`Operation` is created, including queue wait. Clones share cancellation. Keep the
original source on **every** error; errors contain no user content or upstream
error strings. The caller owns concurrency limits, revision checks, stale-result
discard, cache eviction and reservations for layout stability. Suggested cache
key: source + theme + font profile/version; image key additionally includes the
viewport, zoom and backing scale. Never log sources or upstream errors.

Limits: 8000 UTF-16 source units (24000 UTF-8 bytes), 512 total model items,
256 nodes/actors/entities, 384 edges/messages/relations, 32 XY series, 32000 model
text bytes, nesting 32, layout work 2000000, SVG 2000000 bytes / 20000 elements,
finite dimensions/origins at most 16384, raster at most 16777216 pixels.
Cooperative checkpoints cover preparation and pre/post rasterization. usvg/resvg
and individual font callbacks are not preemptible: a hard wall-clock termination
guarantee requires a separate bounded helper process, not an in-process thread.

## Fonts and resources

Uses unchanged bundled TauInter 400/500/600/650/700 instances. On macOS, loads
only the named Arial Unicode and Arial regular/bold/italic/bold-italic files in
`/System/Library/Fonts/Supplemental`, plus `/System/Library/Fonts/Menlo.ttc`. No system-directory scanning, cache writes,
font imports or network access. Initialization returns FontUnavailable if the
local fallback policy cannot cover actual fixtures (α ✓ ≤). No Apple fonts are
redistributed. Missing glyphs observed during measurement return FontUnavailable
rather than knowingly publishing tofu. Italic uses explicit local Arial, since
there is no bundled TauInter italic; this is a deliberate non-Inter fallback.
Measurement and rasterization share the font database, primary weight/style
selection and glyph-fallback ordering. Explicit `Menlo`/`monospace` requests use
Menlo in both paths; unrecognized families use the deliberate TauInter policy.
There is no global CSS rule forcing every text element to TauInter.
Complex-script segmentation/BiDi still
needs deeper parity coverage beyond the current Unicode fixtures.

Merman strict parsing and resvg-safe sanitation run before host presentation and
a final structural resource gate. The gate rejects scripts, foreignObject,
images (including data), anchors/navigation, use/imports, events, nonlocal paint
URLs, DTD/entities and unapproved elements/attributes. Only local structural
paint/marker/filter fragments survive. Both usvg image callbacks are disabled;
font callbacks select only already loaded faces. No external-resource fallback.

## Fixtures and presentation

`fixtures.json` is a durable copy of the verified audit fixtures, including all
six families, reported 7/8 and compact 11/13 graphs, quoted/multiline labels,
forward definitions, four slants, fourteen quoted shapes, malformed statements,
and malicious inputs. Raw source passes: no source adapter is necessary for
these fixtures in merman; physical blank rows stay intact in native text mode.
Tests compare flowchart ids/labels/shapes/ordered endpoints/edge labels to the
beautiful-mermaid models, inspect other-family semantics, paint every valid
fixture in both schemes, and enforce transparency/resource/limit/camera rules.

Family defaults and resolved appearance live in `profile.rs`; see
[PRESENTATION.md](PRESENTATION.md) for evidence, measured natural boxes, the
registered family patches, and routing/visual-review boundaries.
`with_site_config` deep-merges in pinned Merman 0.8.0; it does not replace the
materialized font/theme config. Regression coverage checks the effective config.

Library metadata selects the family before engine construction; controlled
semantic preparation must agree with that exact type. Flowchart alone uses ELK
and `flow_profile.rs`, with wrapping 400, 28/48 spacing, 20/10 per-side padding,
60×36 minima and 13/500 node, 11/400 edge and 12/600 group roles. State, class and
ER use prepared compact Dagre profiles; class members and ER attributes use
Menlo at 11/400. Root `fontSize:false` preserves family roles, with the shared
class/ER edge/auxiliary theme size set to 11px rather than flattening headers. Sequence actors are 80px
minimum × 40px, with 30px diagram margins, 13/500 actor text, 11/400 messages,
dashed muted lifelines and no mirrored footer actors. Marker fills are semantic:
inheritance/aggregation hollow, composition/dependency filled; optional ER
circles use canvas, never hard-coded white.

XY uses the generic `xy.rs` renderer over Merman's typed semantic model after
strict parsing and the upstream resource/pipeline checks. It emits inert escaped
SVG, passes the same final gate, retains transparency and the original semantic
model, and observes cancellation plus a 4096-point ceiling. Its family layout,
legend, dot grid, rounded bars, natural cubic splines, sparse-series points,
shadows and font roles derive from beautiful-mermaid 1.1.3. Secondary colours
intentionally match the captured JS compatibility behaviour: non-hex CSS
variables select fallback blue and the light HSL branch in **both** schemes.
Actual fonts measure label/legend widths; no screenshot coordinates or source
label substitutions are used. Degenerate horizontal splines use finite
polylines rather than producing NaN.

**Not a pixel-parity claim.** The prepared profiles now own semantic geometry
and measurement/drawing roles together; reviewed natural bounds have regression
coverage. Complex state/flowchart routing, sequence superset blocks, glyph run
boundaries, complex BiDi and italic fallback still need broader visual coverage. The grammar is a
strict Mermaid superset, not a byte-for-byte copy of Vue's adapter. All current
audit fixtures retain semantics; arbitrary partial/streamed source is not
guaranteed.

## Host integration and profile status

The pure-Rust pipeline supports six families: flowchart uses ELK, state/class/ER
use Dagre, sequence uses its timeline renderer and XY its typed local renderer.
Compact family profiles are opt-in; upstream defaults otherwise remain unchanged.
The official preparation path registers the family patches, ELK options, native
post-compaction and layer alignment. Registration does not imply visual
approval; see `PRESENTATION.md` for geometry contracts and residual differences.

The host diagram-view layer, rather than this library, owns two workers, a
16-entry queue and a cache capped at 50 entries / 16 MiB of SVG plus source.
That cache ceiling is **not total process memory**: raster images, fonts, parsed
models and worker allocations are separate. Strict SVG/resource gates and
restricted font callbacks remain in force. Budgets and cancellation are
cooperative, not hard preemption or a hard total-memory bound.

## Dependency compatibility patch

Merman 0.8.0 pins granit-parser 1.0.0 and its optional analysis crate pins
serde-saphyr 1.0.0. Cargo resolves even that optional closure; the pins force
existing rust-i18n 4.2.4 down to 4.2.0. To preserve all existing locked package
versions, the official `gpui/scripts/prepare.ts` verifies the immutable release
archives and applies two focused manifest-only patches before placing them in
`.cache/crates/{merman-core,merman-analysis}`. Only those dependency pins change
to 1.3.0. Registry archives and prepared output are never edited directly. The same preparation path registers the core ER-direction patch,
four opt-in renderer patches (class/ER, state/sequence, flowchart and polish),
and the ELK-option patch. Cargo has four Merman path overrides: core, analysis,
render and layout-elk. This module consumes those prepared APIs without
editing prepared outputs. Analysis is not enabled, but its manifest closure must resolve.
See NOTICES.md for provenance/licensing. Remove the compatibility patches when
an audited upstream release relaxes the pins.
