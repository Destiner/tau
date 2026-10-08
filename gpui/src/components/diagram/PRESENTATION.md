# Native Mermaid presentation profiles

## Registered presentation profiles

The profiles below describe source-derived geometry and appearance contracts;
they do not imply pixel parity or visual approval. The official preparation path wires
`flowchart.nativePostCompaction = "LEFT_RIGHT_CONSTRAINT_LOCKING"` and
`flowchart.nativeAlignLayers = true`, along with the fourth
`merman-render-polish.patch`.

The generic post-compactor is flat-layout only; compound layouts keep provider
compaction/frames. Alignment preserves the union of provider and natural SVG
extents. The SVG gate permits only `overflow="visible"` on marker elements;
resources otherwise remain restricted. High-degree treatment above 8 is not
ported. The parallel/self-loop fixture retains approximately 8.6% height drift.
The bounds below remain the measured natural boxes and are not a parity claim.

Profile changes require visual review in both appearances, alongside semantic,
resource-safety and natural-geometry regressions. Historical measurements are
illustrative, not a certification of current rendering.

## Integration boundary

`diagram.rs` uses Merman's `Engine::parse_metadata_sync` to detect the actual
family after source preprocessing. This is metadata detection, not a second full
semantic parse. Input is bounded to 8000 UTF-16 units first; operation checkpoints
surround metadata detection. The controlled semantic preparation must return the
same exact diagram type before rendering. There is no source regex, source
rewriting, fixture-coordinate adjustment or natural-viewBox replacement.

`profile.rs` enables the prepared compact class/ER and state/sequence profiles.
`flow_profile.rs` is declared and applied only for flowcharts, before engine
construction. Flowcharts use native ELK; state/class/ER keep Dagre, sequence keeps
its timeline renderer and XY keeps its typed local renderer. Root `fontSize:false`
leaves family text roles in control. The shared theme font size is 11px only for
class/ER edge and auxiliary text, otherwise 13px.

`gpui/scripts/prepare.ts` registers the core ER-direction patch, four renderer
patches (class/ER, state/sequence, flowchart and polish), and the ELK-option patch.
Cargo uses four Merman path overrides: core, analysis, render and layout-elk.
Prepared sources are derived outputs; maintain checked-in patches instead of
editing cached sources. Dependency validation uses the committed lockfile with
`--locked`.

## Fonts, appearance and semantics

HostTheme retains explicit light/dark appearance, canvas, surface and text roles.
The site's family delta deep-merges into the materialized host theme, rather than
replacing it. Source-compatible muted/stroke mixes are resolved from canvas/text
in `Appearance::resolve`, not from invalid self-referencing CSS variables.

Actual TauInter/macOS shaping is used for measurement and rasterization. Headers
and node/actor labels request TauInter 13/500; edge/message roles use 11/400;
class members and ER attributes request Menlo 11/400; PK/FK/UK badges use
TauInter 9/600. No blanket CSS forces all text to sans or monospace. State and
flowchart line height is 1.3. Family geometry comes from semantic rows, text and
graph constraints, not source fixture dimensions.

Inheritance and aggregation remain hollow, composition/dependency filled, and
optional ER circles use the canvas on both schemes. ER defaults to LR at semantic
construction; explicit TB/BT/LR/RL still win. Regression tests must cover default and explicit ER directions.

## Natural bounds with actual native fonts

These illustrative historical measurements use beautiful-mermaid 1.1.3 as the
source reference and actual native fonts. Light and dark have the same natural
dimensions; the measurements do not establish pixel parity.

| Family / fixture         | Source width × height | Native width × height |
| ------------------------ | --------------------- | --------------------- |
| Basic flowchart          | 111.892 × 145.8       | 111.265 × 145.8       |
| Reported flowchart, 7/8  | 257.886 × 953.540     | 264.556 × 952.325     |
| Compact flowchart, 11/13 | 580.219 × 554.464     | 576.547 × 552.370     |
| State                    | 151.363 × 374.1       | 151.589 × 374.1       |
| Sequence                 | 280 × 160             | 280 × 160             |
| Class                    | 200 × 256             | 200 × 256             |
| ER                       | 590.042 × 136         | 592.177 × 136         |
| XY                       | 750 × 492             | 750 × 492             |

These are natural boxes, not pixel parity. Glyph widths/shapes, baselines and
complex routing can differ. ELK bundling is opt-in post-layout routing, not ELK's
`mergeEdges`; the latter is deliberately false. Fixed bounds allowances in the
flow profile are generic Source policies and only expand actual geometry.

## Maintenance verification

Use `components::diagram::` library tests to check both appearances across all
six families: effective profiles, emitted font roles, node/header heights, PK
semantics, inheritance markers and hollow policy, topology and quoted shapes,
source bounds, Unicode, raster transparency, cancellation and limits. Keep the
one-unit layout-budget regression exercising native ELK without partial output.
Natural-geometry regressions should detect unintended profile drift without
substituting fixed fixture coordinates for source-derived layout.

Host presentation additionally requires real Metal event/paint tests; library
raster tests alone do not establish native upload, camera or interaction behavior.
Run locked library tests, lint and native build validation when changing profiles
or upgrading dependencies. Review routing and typography in both appearances;
passing semantic or bounds tests is not visual acceptance.

## Remaining boundaries

No new screenshot comparison or visual acceptance is claimed. State composite
routing remains Dagre; sequence notes/activations/control blocks retain Merman's
superset pipeline. Complex flowchart ordering, self-loops and groups are not exact
ELK.js replicas. Complex-script/BiDi and italic fallback need broader review.

An authored `%%{init: {"theme":"dark"}}%%` class source currently returns
`UnsafeResource` at the final SVG gate; the gate was not relaxed in this scoped
integration. The original fenced source must remain visible on every error.
Normal host dark/light rendering and safe font directives pass. Retain regression coverage for this rejection; any support expansion must preserve
the resource gate rather than bypassing it.
