# Pinned GPUI prototype patches

These are prototype-only fixes, not an approved long-lived fork. The seven Base patches apply to the exact published `gpui-base` 0.7.0 crate, whose SHA-256 is:

```text
64bb52b29a8dcdc8d3f595b8b0839c076f237572ab601ba4577d3da8ae085728
```

`../scripts/prepare.ts` verifies that archive, extracts it outside the enclosing Git checkout, applies all patches with `git apply --check`, and copies the result into ignored `.cache/crates/gpui-base`. Cargo's `[patch.crates-io]` uses that copy. A patch fingerprint and source-tree hash detect stale or locally altered cached source; the Cargo registry is never edited. The archive is reused from Cargo's cache if available, otherwise downloaded from `static.crates.io` over HTTPS. Build with `bun run --cwd gpui build:native`, not an unprepared bare Cargo invocation.

Extraction outside the checkout is intentional: `git apply` run in a nested dependency directory can silently skip all unprefixed paths and still return zero. Preparation also places the published README at the workspace-relative path required by an upstream test include.

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

## Validation and maintenance

The combined patched Base library passed **1,242 upstream unit tests** locally, including the seventeen added tests. These use Base's test platform; they do not replace the native Metal and browser harness tests.

Optional upstream check, after preparation, from the repository root:

```sh
CARGO_TARGET_DIR="$PWD/gpui/target" cargo test \
  --manifest-path gpui/.cache/crates/gpui-base/Cargo.toml --lib
```

This optional upstream test run resolves the published crate's separate development dependencies; the prototype itself is built against the committed `gpui/Cargo.lock` with `--locked`.

For an upgrade, inspect the upstream fixes, remove superseded hunks, replace the archive checksum deliberately, regenerate the prototype lockfile, and rerun both upstream and rendered regressions. Do not let preparation accept a patch that no longer applies, edit the global registry, or keep a stale patched binary with `--skip-build`.

Upstream source/license: [GPUI Kit v0.7.0 Base](https://github.com/longbridge/gpui-kit/tree/v0.7.0/crates/base), Apache-2.0. The source archive retains its upstream license; only the focused patches are checked in here.
