# GPUI component prototype

An isolated feasibility experiment, not a replacement for Tau. The Vue/Tauri app, its runtime, and its component APIs are unchanged. No Pi processes, real workspaces, session files, or telemetry are involved.

## Current result

The standalone GPUI application renders a Tau-styled button, single-line input, and a **multiline editing probe** using unstyled GPUI Base controls. Base owns editing, selection, history, and event dispatch; the harness does not implement a second text editor.

| Component      | Scenarios | Checkpoints per theme |
| -------------- | --------: | --------------------: |
| Button         |        21 |                    56 |
| Input          |        42 |                    81 |
| Textarea probe |        18 |                    33 |
| Total          |        81 |                   170 |

Counts describe meaningful coverage, not minimum quotas. The unrelated development catalogue is not imported.

- **Chromium:** all 170 behavioral checkpoints match GPUI in light and dark (340 comparisons). Each implementation is also checked against authored expectations.
- **WebKit:** an explicit, failing audit remains. See [review blockers](#review-blockers); Chromium agreement is not macOS acceptance.
- **Visuals:** the reviewed native accent focus border, thinner focus treatment, and omitted input outer ring are retained. Follow-up fixes and remaining choices still require review. Every raw pixel difference is recorded; exact-difference flags are not an approval ledger and no numerical tolerance silently approves changes.
- **Harness:** real-browser and real-Metal fault tests catch invisible controls, missing click handlers, lost Change events, incorrect selection, incomplete captures, and out-of-control-area rendering faults. A pixel regression also requires selections to be visibly painted in both editors.
- **Dependency fixes:** four focused Base patches address navigation, disabled focus, and editor presentation; a separate opt-in macOS text patch matches the browser's antialiased policy. See [patches](patches/README.md).
- **CI:** an isolated workflow is provided, but has not been executed on a remote runner. Local Metal capture does not prove hosted-runner Metal availability.

Do not proceed to the rest of the UI or application-logic rewrite on the strength of these results alone. Button/input and harness review remain human gates.

## Setup and commands

Tested locally on Apple Silicon macOS, Rust 1.98.1, Bun 1.4.2, and Xcode 27.0. A working Metal device and full Xcode/Metal tools are required for native capture; there is no software/fake-renderer fallback. If `xcrun metal --version` reports a missing Metal Toolchain, install Apple's component with `xcodebuild -downloadComponent MetalToolchain`.

From the repository root:

```sh
bun install --frozen-lockfile
bun install --cwd gpui --frozen-lockfile
bun x playwright install chromium webkit

# Bootstrap the pinned, patched dependency and build the standalone executable.
bun run --cwd gpui build:native

# Interactive native windows; no app data or Pi connection.
gpui/target/debug/tau-gpui-prototype --component button
gpui/target/debug/tau-gpui-prototype --component input --dark
gpui/target/debug/tau-gpui-prototype --component textarea

# Capture and compare all scenarios in both themes.
bun gpui/scripts/run.ts
bun gpui/scripts/run.ts --component input --skip-build
bun gpui/scripts/run.ts --browser webkit --skip-build

# Browser-only expectations/candidates; native dependencies are unnecessary.
bun gpui/scripts/run.ts --reference-only

# Review the latest local report, with filtering and 1×/2× crop views.
open gpui/results/review/light-chromium/report/report.html

# Review follow-up: full regression captures, then only the 14 relevant checkpoints.
bun gpui/scripts/run.ts --out gpui/results/follow-up
# This audit still exits nonzero on the documented behavioral differences, retaining captures.
bun gpui/scripts/run.ts --browser webkit --skip-build --out gpui/results/follow-up
bun gpui/scripts/review.ts
open gpui/results/follow-up/light-chromium/report/feedback.html
```

`--out <directory>` changes the output root. Each run writes `<theme>-<browser>/request.json`, `vue/`, `native/`, and `report/`. Report crops are derived from the original captures; full frames remain linked. JSON records observations and both control-area and full-frame pixel metrics. The default magnitude heatmap distinguishes tiny channel rounding from larger changes; an exact-mask toggle retains the original view. A bounded ±2-physical-pixel sampling-offset diagnostic never moves images or changes pass/fail. Both crops have the same origin, shown explicitly; pixel diagnostics are expandable. Completed runs record rendering-input and native-binary fingerprints. The focused report requires WebKit evidence linked to the current Chromium run, so an old WebKit image cannot be reused just because scenario JSON stayed the same.

`--strict-visual` fails on any raster or bounds difference. Default success means **state assertions passed, visuals still require review**, not that a port has been accepted. Failing browser expectations are retained in full native comparisons so the report can show both implementations; reference-only runs fail directly. Console, page-loading, missing-font, and network faults always fail.

`--skip-build` deliberately uses the existing native binary. Omit it after changing Rust or a dependency patch. Normal builds run checksum-verified preparation and `cargo build --locked` first.

## Checks

```sh
bun run --cwd gpui format
bun run --cwd gpui lint
bun run --cwd gpui typecheck
bun run --cwd gpui test
bun run --cwd gpui test:e2e
bun run --cwd gpui test:native

bun gpui/scripts/prepare.ts
cargo fmt --check --manifest-path gpui/Cargo.toml
cargo clippy --locked --manifest-path gpui/Cargo.toml --all-targets -- -D warnings
cargo test --locked --manifest-path gpui/Cargo.toml
```

The native self-tests build the current executable and exercise actual Metal rendering. They are separate from fast unit tests and never silently skip an unavailable renderer. Their mutation hooks (`--fault hide-control|drop-clicks|drop-changes`) are accepted only in suite mode, not the interactive application.

The prototype has its own Bun/Cargo lockfiles, lint configuration, build cache, and output directory. Root lint excludes this independent workspace; the commands above check it explicitly. The existing app's normal lint, tests, frontend build, and Tauri build remain separate gates.

## How comparison works

1. `contract.ts` defines scenarios, actions, observations, viewport, and scale. `scenarios.ts` contains independently authored expected behavior, not results learned from either renderer.
2. `reference/main.ts` imports the production `UiButton`, `UiInput`, `UiTextarea`, and styles. Playwright supplies keyboard and pointer events. Each scenario mounts a fresh control, resetting editing/history state.
3. `src/ui.rs` styles real Base controls with Tau's palette and dimensions. Inputs retain Base state across prop updates; a fresh headless application/window is created for each scenario.
4. `src/main.rs` dispatches GPUI keyboard, text-input, and pointer events, then captures real Metal output with the macOS text system. The headless **platform window is a test window**, not a live AppKit window: this proves rendering and control event handling, not the operating-system input path.
5. `scripts/compare.ts` rejects missing/duplicate captures, invalid observations or bounds, unsafe filenames, and wrong image dimensions. It checks each implementation against expected state and compares their full observations.
6. Images are compared at **1280×640 physical pixels**, representing a **640×320 logical viewport at scale 2**. No resampling or alignment compensation occurs before comparison. Control metrics use the union of observed bounds plus a 4-logical-pixel margin; full-frame metrics separately detect stray paint outside that region.

Selection offsets are **UTF-16**, converted explicitly to/from Base's byte offsets. Setup selections must be in-range Unicode scalar boundaries; offsets inside surrogate pairs are rejected rather than rounded. Backward selections are exercised through Shift-arrow sequences and subsequent edits. Observations contain the sorted range; direction is currently tested behaviorally, not exposed as a separate field.

The input probe covers pointer selection/replacement, insertion/deletion, Enter, line/word-independent horizontal navigation, combining marks and joined emoji, selection-anchor reversal, undo/redo and history isolation, parent value replacement, same-current-value preservation, unrelated prop rerenders, readonly/disabled transitions, and Tab entry. Multiline cases distinguish visual rows from newline-delimited lines and assert intermediate caret positions.

The preview host supplies fallback focus and Tab routing; these are not a final application-wide focus manager. The native test window is explicitly activated and pending tasks are drained before rendering: an inactive window suppresses Base's selection highlight even when the observed range is correct. Checkpoints normally suppress the caret on both sides; `caret: 'visible'` explicitly captures the real caret and verifies an actual narrow pixel change against a hidden-caret frame without changing editing state. The browser waits for its actual visible blink phase (no synthetic overlay), and native capture uses an explicit visibility flag. Transparent native caret colour was not sufficient: Base resolved it to foreground, a now-regression-tested harness bug. The interactive native window retains normal blinking and the thinner centered caret. Logical focus/selection and visible selection paint are independently tested; live caret behavior still needs real-window review.

The bundled Latin Inter variable font is losslessly decompressed from the project's `@fontsource-variable/inter` WOFF2 asset. Its internal native family name is `Inter`; the browser CSS family is `Inter Variable`. The font license is in `assets/Inter-LICENSE.txt`. Menlo is used for deterministic macOS monospaced probes. System fallback glyphs and rasterization remain platform-dependent.

## Follow-up from visual review

- Disabled button colours are composited once, rather than accumulating opacity where GPUI background/border overlap. Disabled input text no longer receives both Base's dimming and the control opacity.
- Disabled selection stays visibly painted without focus or editability. Carets are 1 logical px and centered on the insertion boundary; repeated focus notifications no longer hide their initial blink phase.
- Native text opts out of CoreGraphics stroke dilation, like Vue's `-webkit-font-smoothing: antialiased`. No lighter font weight or colour substitution was used. In fixed interior text regions of the saved light/dark placeholder and seeded input, mean RGB error decreased by roughly 29–46%; exact raster equality is not claimed.
- The emoji probe matches WebKit/macOS's painted glyph size, while Chromium uses a smaller glyph. The follow-up includes the WebKit image to make that distinction explicit. A small raster/baseline difference remains; no guessed scaling or screenshot resampling was applied.
- Hover after keyboard input remains a review choice: GPUI suppresses hover until real pointer movement; Vue retains it. The harness now captures before keys, after keys, and after pointer movement rather than masking the difference with synthetic motion.

The focused report has 14 representative checkpoints per theme, notes tied to the feedback, visible caret labels, and links to the full results. Equivalent disabled/focus cases are not repeated. Accepted focus styling is not presented as a new rejection simply because exact pixels differ.

## Review blockers

The current WebKit run fails **17 checkpoints per theme** in three groups. These failures are visible in JSON/HTML and in the command's exit status, not normalized away:

1. **Pointer button focus (15 checkpoints).** Chromium and Base focus mouse-pressed buttons; macOS WebKit does not. Keyboard activation is separately tested. Choose the intended macOS behavior before accepting the button, especially where pressing a button must preserve editor focus.
2. **Tab after disable/re-enable (one checkpoint).** In the isolated WebKit document, the first Tab after the disabled phase does not re-enter the now-readonly control. Chromium and the native preview host do. This needs a multi-control focus-order probe and actual WKWebView evidence, not just an assertion change.
3. **Backspace over a combining accent (one checkpoint).** After `e + combining acute`, Chromium/Base delete the accent alone; WebKit deletes the whole grapheme. The horizontal-navigation patch deliberately does not change deletion policy. Resolve which behavior Tau should preserve rather than claiming Unicode parity from simple emoji cases.

WebKit is especially relevant because the shipping macOS app uses WKWebView. A Chromium state pass is therefore useful implementation evidence, **not a completed input acceptance gate**.

Other known boundaries:

- The exported component prop surface is not complete: password/masking, maxlength, arbitrary native HTML attributes, form submission, labels/accessibility, and all validation behaviors are not ported.
- The textarea is a feasibility probe, not a completed textarea/composer port. Resize handles, scrolling, Tab/completion semantics, large documents, and streaming anchoring need separate work.
- OS IME/dead-key composition, native clipboard permissions/formats, complete undo grouping, VoiceOver, real window focus restoration, live caret/selection visibility, and performance are unproven here.
- GPUI Base/GPUI snapshot APIs and the local patches are a maintenance risk; this is not a production dependency decision.
- CI Metal availability, signed distribution, updater trust, and all Tau session/runtime lifecycle invariants are outside this prototype.

The next review should assess both the port **and the harness's ability to expose defects**. Resolve the WebKit policies, review the visual candidates, run real-window input checks, and prove capture on the intended CI runner before widening the rewrite.
