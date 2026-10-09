# Native dependency patches

## Prepared dependency set

`gpui/scripts/prepare.ts` registers **22 Base patches** and **five Core patches**,
including the native interaction and accessibility fixes below; **two merman-core patches** (parser manifest and ER
semantic default direction); **one merman-analysis manifest patch**; **four
merman-render family patches** (class/ER, state/sequence, flowchart and polish);
and **one merman-layout-elk adapter patch**. Cargo has four Merman path
overrides: core, analysis, render and layout-elk. These are source preparation
registrations, not package or binary counts. The patches are wired through the
official preparation path; the integration is not awaiting patch registration.

The native flowchart profile wires
`flowchart.nativePostCompaction = "LEFT_RIGHT_CONSTRAINT_LOCKING"` and
`flowchart.nativeAlignLayers = true`. Generic post-compaction applies to flat
layouts; compound layouts retain provider compaction/frames. The marker-overflow
SVG exception is narrowly limited to `overflow="visible"` on marker elements;
other resource restrictions remain. The opt-in high-degree treatment above
Source's threshold of 8 is not ported, and the self-loop fixture retains about
8.6% height drift. Retained SVG extent includes provider extent after alignment.

Profiles and generic algorithm ports include beautiful-mermaid MIT attribution
and licence in the patches. The ELK-derived compactor includes its EPL-2.0
licence and corresponding modified source. Public patched render licensing is
MIT, with separate EPL obligations for `merman-elk-layered` and the compactor.
See the diagram notices; dependency inventory is not binary inventory or legal-
compliance certification. Distribution requires the corresponding upstream
release sources and checked-in patches for EPL-covered modifications, not an
MIT-only notice. Profile changes require visual review; registration alone does
not establish parity or approval.

These are focused compatibility patches, not a commitment to a long-lived fork. The Base patches apply to the exact published `gpui-base` 0.7.0 crate, whose SHA-256 is:

```text
64bb52b29a8dcdc8d3f595b8b0839c076f237572ab601ba4577d3da8ae085728
```

`../scripts/prepare.ts` verifies that archive, extracts it outside the enclosing Git checkout, applies all patches with `git apply --check`, and copies the result into ignored `.cache/crates/gpui-base`. Cargo's `[patch.crates-io]` uses that copy. A patch fingerprint and source-tree hash detect stale or locally altered cached source; the Cargo registry is never edited. The archive is reused from Cargo's cache if available, otherwise downloaded from `static.crates.io` over HTTPS. Build with `bun run --cwd gpui build:native`, not an unprepared bare Cargo invocation.

Extraction outside the checkout is intentional: `git apply` run in a nested dependency directory can silently skip all unprefixed paths and still return zero. Preparation also places the published README at the workspace-relative path required by an upstream test include.

## Native OS integration

- `native-menu-core.patch` adds live responder and global action validators.
  A denied responder consumes dispatch before capture or parent/global fallback;
  a nearer local handler retains priority over a global validator. Validation
  cannot manufacture a missing handler. Action lists follow the same policy.
- `native-menu-base.patch` validates editor selection, editability and undo/redo
  history, and uses metadata-only Root/TextView Copy validation. Queries never
  read the clipboard or invoke source projection. Empty Copy projections fail
  closed instead of falling through to another responder.
- `ime-composition.patch` adds an off-by-default `set_protect_ime_keys` option.
  Application-delivered Enter/Escape stay local while text is marked; normal
  delivery resumes after commitment. Tau's editable controls opt in. This does
  not replace AppKit's input context or certify candidate-window behavior.
- `window-close-request.patch` shares the native close veto with a public
  `Window::request_close` API, including synchronous reentrancy and callback
  replacement safety. The unconditional removal primitive stays distinct.
- `accessibility-test-platform.patch` adds permanent TestPlatform activation,
  action delivery and latest-tree capture through the actual AccessKit callbacks.
  It holds only the latest update and needs no OS permissions.
- `focus-trap-accessibility.patch` preserves wrapped Element roles, properties
  and synthetic children. Rendered-tree regressions also exercise accessible
  Focus and Click. It does not establish modal background isolation or full
  editable-text accessibility.

These patches are registered per crate with the ordinary preparation path. See
`src/native/` for production host contracts and lasting native integration tests.
There is no dependency on browser comparison scenes or migration fixtures.

## Patches and evidence

### `grapheme-navigation.patch`

**Signal:** with `e + combining acute + emoji`, Right moved into the combining sequence instead of past it. Similar scalar movement splits joined emoji and flags.

Left/Right and Shift-Left/Right now use grapheme boundaries, while keeping token atomicity, CRLF handling, and hidden-line clamping. Boundary search materializes the current logical line, not the whole document. Very long single-line performance still needs measurement.

Backspace/Delete retain the original scalar-boundary behavior. This matches the current Chromium probe but differs from WebKit for a combining accent; that discrepancy is an explicit acceptance blocker, not silently fixed by changing the test oracle.

Two added upstream tests cover combining marks, joined emoji, flags, selection/collapse, CRLF, and adjacent tokens. Shared rendered scenarios `input-grapheme-navigation` and `input-joined-emoji-navigation` exercise the public event path and UTF-16 observations.

### `textarea-rows.patch`

**Signals:** an ordinary 100px textarea with 1px borders and 8px horizontal padding wrapped at 10 monospaced characters instead of 12; Cmd+Left jumped to the newline-delimited line's start rather than the current visual row.

Plain multiline inputs no longer reserve the editor's extra right clearance in wrapping and related horizontal-scroll calculations. Their start/end actions use visual-row boundaries, including repeated presses. Code editors keep their clearance and documented second-press logical-line behavior.

Six added upstream tests cover available wrapping width, horizontal overflow, plain-textarea row boundaries, preservation of editor behavior, and preserving the visual column when moving vertically after End. The last case also exposed an incorrect line-end column affinity shared by textarea/editor movement. `textarea-wrapped-rows` checks actual Metal-shaped text against fixed system-monospace/width constraints and browser observations; it does not infer expected offsets from whichever renderer runs.

### `disabled-focus.patch`

**Signal:** disabled Base input and textarea states could still take mouse focus and change selection; they also remained in the Tab order.

Disabled editors no longer register focus tracking, place pointer selections, or honor the public `focus()` helper. Readonly editors remain focusable. Two added upstream tests exercise pointer focus, Tab focus, reenabling, and readonly for both input types.

`set_disabled` has no window argument. The owning Tau prototype therefore moves focus away when disabling a currently focused control. No broad upstream API change is made for that policy.

### `input-presentation.patch`

**Signals:** transparent caret colour was resolved to foreground rather than hidden; the 1.5px caret extended into the next glyph; selection disappeared when disabling a selected input; disabled text was dimmed twice. Calling public `focus()` and then processing its focus notification started blinking twice, toggling the initial caret off.

Adds opt-in presentation fields for caret visibility/width/centering, unfocused selection, and disabled text opacity. Defaults preserve existing presentation. Tau enables unfocused selection only while disabled, retains the engine's selection/history, and suppresses carets through the visibility flag rather than a transparent colour. Cursor start now resets to a visible phase instead of toggling. Centered carets are clamped inward at visible viewport boundaries so a requested 2px stroke is not clipped to 1px in an empty field. Two upstream regressions cover presentation resolution and repeated starts; real Metal/browser tests verify visible and hidden caret pixels, disabled selection, and pale disabled controls.

### `single-line-navigation.patch`

**Signal:** Up/Down left an editable single-line input's caret unchanged. Base both omitted those action listeners for single-line fields and returned early from their movement handlers. Direct movement tests alone missed the missing event wiring; real key scenarios exposed both layers.

Editable single-line fields now handle Up/Down as start/end movement, including collapsing a selection. Shift variants select toward the boundary, returning to the anchor before extending in the opposite direction. Existing read-only/disabled behavior is preserved; context-menu handling still takes precedence and textarea/editor vertical movement is unchanged.

Three upstream regressions cover action dispatch, boundaries/empty/read-only values, and selection-anchor reversal. Shared `input-vertical-arrows` and `input-vertical-selection` scenarios exercise 16 intermediate checkpoints with Unicode text, repeated boundary keys, selection collapse/reversal, read-only, disabled, and empty states. The original native implementation failed ten of those checkpoints; Chromium and WebKit agree on the corrected contract.

### `textarea-selection.patch`

**Signal:** `ab\n\ncd`, selected backward from UTF-16 offset 5 to 3, had correct state but painted a narrow newline stripe, unlike macOS WebKit's full-width row. The highlight was also vertically displaced by the font's upper half-leading.

The opt-in `InputEditorStyle::macos_textarea_selection` extends selected hard-newline rows to the content edge and aligns selection paint using the font's ascent and absolute descent, not a hard-coded screenshot offset. Only Tau's macOS textarea enables it; soft-wrap boundaries, partial final rows, single-line input and code-editor defaults stay distinct. One new upstream geometry test plus an extended soft-wrap test cover the range shapes. Real WebKit/Metal pixel regressions check forward/backward painting, width, top/bottom edges and padding in both fonts/themes; logical observations still have to agree.

### `input-scroll-sensitivity.patch`

**Feedback:** the multiline input felt too fast. The native event path already preserves pixel units without amplification; Tau deliberately opts into **0.5× wheel sensitivity** rather than rewriting macOS events or claiming a unit-conversion fix.

The setting defaults to 1.0 for upstream inputs/editors and scales after pixel/line conversion. Fractional deltas, direction changes and endpoint clamping remain intact. Only the Tau textarea opts in. Keyboard/caret reveal, programmatic movement and scrollbar dragging do not use the multiplier. Invalid settings reset to 1.0. One upstream regression checks unit conversion, defaults, invalid settings and unchanged editing state; a real Metal event-and-pixel regression verifies displacement, fractional accumulation, reversal, clamping and direct thumb dragging. Real-device feel still needs hands-on review.

Rounded thumbs need no dependency patch: Tau projects a 4px radius via Base's `ScrollbarTheme`, clamped to half the thumb width and inherited by hover/active states. Metal pixel checks inspect all four corners and visible end caps in both themes.

### `composer-caret-reveal.patch`

**Signal:** Base's auto-grow textarea always retains a full line-height between the caret's row origin and the viewport bottom. At the ten-row cap, appending to an eleven-line draft scrolled 19px while WebKit's textarea scrolled 13px, hiding the first line entirely and moving the final line 6px too high. The browser's scrollTop and Base's own scroll offset exposed the discrepancy without hardcoding a post-input scroll position.

`InputEditorStyle::auto_grow_cursor_margin` opts a single auto-growing textarea into a 13px reveal inset; the default remains one shaped line. The inset is bounded between zero and half the viewport, and does not affect ordinary inputs, code editors, wheel scroll, dragging, or content/history. An upstream unit regression checks defaults and bounds; the native/browser multiline editing fixture checks the real-key path and rendered output in both themes. The composer retains Base's scrolling/idle scrollbar mode and projects the same 4px thumb radius as the accepted Tau textarea. Immediately after editing, Base still paints its thumb during the idle hold; browser overlay scrollbars are hidden in the capture.

### `textarea-vertical-edges.patch`

**Signal:** Up from the middle of the first textarea row and Down from the middle of the last row retained the caret's column instead of reaching offset 0/document length. The original boundary scenario only started at the endpoints, so it did not detect this.

Editable plain-textarea arrow movement now reuses Base's existing document-edge target when no further visual row exists. It retains the preferred column for subsequent vertical movement and respects wrap-end affinity. Code-editor, readonly, page-key and Shift-selection policies are unchanged. Three upstream tests cover ordinary boundaries/reversal, one-row/empty values, and preservation of code-editor policy. Shared real-key scenarios add Unicode, blank rows, selection collapse and actual shaped wrap boundaries; twelve checkpoints failed against independently authored expectations and both Vue engines before this fix.

### `paragraph-navigation.patch`

**Signal:** Option+Up/Down did nothing because Base had neither key bindings nor logical-line movement handlers for these shortcuts. Timur explicitly chose native macOS semantics over fixed logical-line or visual-row stops.

New macOS bindings target the current newline-separated line's start/end, then the neighboring line boundary on a repeated press. Soft wraps are ignored. A nonempty selection uses its directional endpoint but does not skip a line merely because that endpoint is already at a boundary. Base still owns movement, selection normalization, history boundaries and scrolling; readonly/disabled inputs and code editors do not register the new handlers. No Shift+Option or page-key behavior is added.

Three upstream tests cover movement and real dispatch, Unicode, blank/empty lines, selection collapse, single-line fields, and inactive/code-editor guards. The shared browser/Metal suite checks 36 intermediate states across both control types and wrapping. Thirty native checkpoints failed before the fix; both Vue engines agreed with the authored expectations. Subsequent plain-arrow preferred-column behavior remains a separate browser-policy difference; paragraph navigation does not alter that policy.

### `code-block-scroll.patch`

**Signal:** Base rendered `Inline` directly under the code block container, so `overflow_x: scroll` on the public refinement still wrapped long code and could not provide a scrollable content width.

`TextViewStyle::with_code_block_horizontal_scroll(true)` opts fenced code into a horizontal viewport; default code still wraps. The width is the maximum _shaped_ hard-line width using the rendered mono family, size and highlighted runs, not a character-count estimate. The same selectable/highlighted `Inline` paints inside the track, while the copy action stays in a stationary sibling flex column. The viewport gives actions their actual laid-out width, so the last code glyph remains visible/selectable rather than sliding under an overlay. Base's existing axis-locking horizontal mask passes vertical gestures to the outer list. The scroll handle is keyed by code-block source span, so append-only updates can retain it; replacing the document can reset the position. Two upstream regressions check exact shaped mono width and hard-line handling, plus default wrap height, opt-in hard-line height and pinned action. The native Markdown probe exercises horizontal wheel, endpoint selection, reversing, syntax colors and clipboard in both appearances.

### `product-markdown-style.patch`

`TextViewStyle::with_table_fit_content(true)` bounds scrollable tables to their measured natural width; `with_table_css_cells(true)` separately opts scroll tables into 12px horizontal cell padding and horizontal-only row rules. The measured header/body font refinements are used for fitted column widths, and the viewport remains capped at its parent. Default wrap and scroll tables retain their original sizing and cell rules. The Markdown scene supplies the dark/light table surface explicitly instead of inheriting Base's default light surface.

Scrollable fenced code positions hover actions in a zero-height flex sibling: its intrinsic width is reserved without increasing the block's height, and the last glyph remains scrollable instead of hiding beneath an overlay. `with_code_block_paragraph_gap` overrides only the gap before an immediately following paragraph; defaults and code-to-code spacing stay unchanged. A per-block `markdown-code` hover group keeps other copy buttons idle, and `code-{ix}` bounds are test-support-only. Four upstream regressions exercise default vs fitted table widths, header/body cell alignment and right edge, two independent real mouse hovers, and tall versus short actions without changing the next code block's position.

### `markdown-strong-style.patch`

`TextViewStyle::with_strong_weight` opts a view's bold Markdown marks into an explicit weight. Default `None` still renders Base's original bold weight; Tau thinking prose uses 400 (matching the ActivityRow override) and other activity prose uses 600. The upstream unit checks default, opt-in, cloning and style equality. This patch follows the already-stable `product-markdown-style.patch` without changing it.

### `markdown-list-style.patch`

`TextViewStyle::with_list_style(indent, gap, marker)` opts one view into a fixed marker column, inter-item spacing, and a faint marker color. The default `None` preserves Base's inline list prefix and compact spacing, including other product scenes. The styled renderer still uses the original parsed list nodes, source selection, nested lists, and task checkboxes; it does not rewrite Markdown or place text manually. An upstream rendered regression compares the item glyph positions of styled and default lists and verifies the 5px added row spacing. Tau's queue preview opts in (16px column, 5px item gap, faint token), as does the Markdown product scene with its production CSS metrics.

### `markdown-inline-spacing.patch`

`TextViewStyle::with_inline_code_padding` opts into measured horizontal code padding (Tau uses 3.432px = 0.3 × the production 11.44px code size). The default remains Base's 2px. The value travels through InlineFlow's typography/layout cache key and its wrapping, painting and intrinsic table measurements; text and selection offsets are untouched. The opt-in list renderer also gives Tau's nested ordered markers decimal numbers, matches the nested indent, and uses collapsed-list bottom/top spacing rather than Base's full paragraph gap. Unstyled views retain Base's ordered depth markers and block spacing. Rendered tests cover width/wrap/default geometry, select-all and pointer drag across code, and the existing list indent/spacing test.

### `markdown-prose-measure.patch`

`TextViewStyle::with_prose_max_width` limits only root paragraphs, lists, quotes, headings and horizontal rules, in both ordinary and virtual-list rendering. Nested children inherit their parent's available width; fenced code and tables retain the full message column. The default `None` leaves Base's root layout unchanged. Tau derives 80ch from eighty measured zero-glyph advances in its 13px bundled Inter face; WebKit's actual production CSS computes 80ch to 656.09375px against the 720px column. A rendered Base test checks narrower prose wrapping, unchanged wide structured content, unchanged selected text and the default setting.

### `fractional-text-leading.patch` (`gpui-pre` 0.3.7) and `fractional-leading-selection-test.patch` (Base)

The independently verified `gpui-pre` archive has SHA-256 `0e87a42bb37c7cb4e76dd1ac0ce88851e46e976e0373a47ab3e0757abffee54d`. `TextStyleRefinement::fractional_line_height` defaults to false; only the plain-file `SelectableText` opts in via its existing `text_style` API. GPUI's `StyledText` layout and `TextStyle::line_height_in_pixels` retain their original snapping/rounding when unset, but use the exact 12px × 1.65 = 19.8px spacing for this run. The single resulting `TextLayout` drives paint, size, selection quads, point/index mapping, and scroll height. Highlighted `TextView` and all other product scenes keep their defaults.

The Base patch adds only a rendered regression and a standalone-Cargo patch override for this third prepared crate. It verifies 300 hard lines: both default 20px and opted-in 19.8px row geometry at rows 1, 25, 100, 200, 299, bidirectional point/index mapping, and a real pointer selection through `SelectableText`. Base's published standalone test manifest must resolve the prepared GPUI core too; root-workspace builds use the prototype's root `[patch.crates-io]` instead. No registry source is modified.

### `virtualized-text-selection.patch` (Base)

**Signal:** offscreen endpoints disappeared from mounted selection registrations, so Root Copy left the clipboard sentinel unchanged until those rows remounted.

An opt-in source-owned `TextSelectionDomain` separates logical UTF-8 endpoints and item order from live geometry. TextView supplies exact pointer mapping and pure rendered/source projection; Copy includes never-painted intermediates without mounting them. Ordinary participants keep their existing behavior. Scope clear, deletion and incompatible source updates invalidate the domain. Root retains Copy routing when the endpoint focus node unmounts. `copy_markdown_slice` supports explicit cold copy without creating a view or scheduling layout. `stored_range` lets a source owner inspect endpoints during mutation without reentering its own entity through an order provider; validated copy/paint still use provider-backed queries.

Base tests cover pure Markdown/Unicode/source mapping, forward/reverse domains, endpoint removal, lifecycle, append and reflow. Pending asynchronous remounts cannot invalidate source endpoints using their temporary empty document; append reconciliation checks only relevant endpoint prefixes, not unrelated suffixes or rows. The native strict proof dispatches pointer/wheel/Root Copy and checks actual remount highlights. Conversation regressions additionally cover cache eviction, 10k histories and Copy before an asynchronous parse commits.

### `estimated-list-height.patch` (`gpui-pre` 0.3.7)

**Signal:** unknown rows contributed zero to list height; large wheels clamped or sought through collapsed history. `measure_all()` fixed the bounded demo but scanned whole histories during layout/invalidation.

`ListState::with_estimated_item_size` opts into persistent insertion hints, exact crossed-row wheel traversal with residual continuation, and bounded viewport filling. Positive-height exact geometry is capped at 1,024 rows; eviction keeps estimates. Width changes invalidate only cached exact geometry, with lazy zero-height epochs. Known zero-height subtrees can be skipped; cold zero runs and wheel traversal have step budgets. Distant scrollbar seeks use estimated positioning, then measure the destination. Existing default lists are unchanged.

Core unit regressions cover 1k/10k varied and zero-height rows, physical fractional/550px/reverse wheels, tail intent, resize, mutation and bounded continuation. End-of-scroll exactness also checks suffix epochs, so stale zero heights cannot falsely report the end after resize. This is an opt-in core capability. Its current consumers are excluded synthetic conversation fixtures, not an adopted shipping application list. Those fixtures remove eager measurement and whole-history render snapshots, create text views only for requested rows and cap the view cache at 128. Raw scrollbar coordinates remain estimated; physical wheel regressions compare the same rendered row bounds instead of normalizing offsets.

### `antialiased-text.patch` (`gpui-pre-macos` 0.3.7)

The published archive is independently SHA-256 verified:

```text
5a43af845b260b09393e923c4f1e1a67a10fcfc847b4815192bbfd02ed9fe725
```

**Signal:** equal 400-weight text and colour tokens still looked heavier than Vue. The native backend added luminance-dependent CoreGraphics glyph dilation while the browser explicitly requested antialiased, unsmoothed text.

Adds a process-local opt-in to disable that dilation, called before rendering in both interactive and headless modes. Default upstream behavior and macOS preferences are untouched. The normal bootstrap prepares this separate crate in `.cache/crates/gpui-pre-macos`; no registry source is edited. Fixed-region before/after captures demonstrate reduced text error in both themes. No emoji scale workaround or variable-weight remapping is included.

### `grayscale-text.patch` (`gpui-pre-macos` 0.3.7)

**Signal:** even after the dilation opt-out, 400-weight Inter and matching colour tokens still produced roughly 17% excess glyph coverage. The no-dilation branch never called `CGContextSetShouldSmoothFonts(false)`, leaving context-default smoothing active. A separate mono discrepancy came from substituting Menlo for the browser's system monospace family.

For Tau's existing antialiasing opt-in, explicitly disable smoothing on non-emoji glyphs. Upstream defaults, emoji rendering, and global macOS preferences remain untouched. The application separately requests the same macOS system monospace family as `ui-monospace`. Real text/placeholder rendering probes now compare normalized glyph coverage against WebKit and check opaque pixels retain the authored colour, catching both excessive stroke weight and colour substitutions. The caret's requested 2px width is independently checked in Metal captures.

### `markdown-parity.patch` (Base)

The nineteenth Base patch maintains native Markdown typography, layout and
selection contracts alongside the opt-in style patches above. Keep parser,
source-selection and rendered geometry regressions together when changing it;
visual adjustments must not rewrite source offsets or weaken selection behavior.

## Validation and maintenance

Maintain upstream Base regressions, including fractional-leading and logical
selection coverage, with the prepared core/macOS overrides. Base's test platform
does not replace real native Metal event/paint tests. Historical browser probes
explain the original contracts; maintaining a Vue comparison workflow is not a
requirement of the native implementation.

Optional upstream check, after preparation, from the repository root:

```sh
CARGO_TARGET_DIR="$PWD/gpui/target" cargo test \
  --manifest-path gpui/.cache/crates/gpui-base/Cargo.toml --lib
```

This optional upstream test run resolves the published crate's separate development dependencies; the prototype itself is built against the committed `gpui/Cargo.lock` with `--locked`.

For an upgrade, inspect the upstream fixes, remove superseded hunks, replace the archive checksum deliberately, regenerate the prototype lockfile, and rerun both upstream and real Metal rendered regressions. Do not let preparation accept a patch that no longer applies, edit the global registry, or keep a stale patched binary with `--skip-build`.

Upstream source/license: [GPUI Kit v0.7.0 Base](https://github.com/longbridge/gpui-kit/tree/v0.7.0/crates/base), Apache-2.0. The source archive retains its upstream license; only the focused patches are checked in here.
