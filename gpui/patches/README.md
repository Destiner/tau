# GPUI Base 0.7.0 patches

These are prototype-only fixes, not an approved long-lived fork. They apply to the exact published `gpui-base` 0.7.0 crate, whose SHA-256 is:

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

Six added upstream tests cover available wrapping width, horizontal overflow, plain-textarea row boundaries, preservation of editor behavior, and preserving the visual column when moving vertically after End. The last case also exposed an incorrect line-end column affinity shared by textarea/editor movement. `textarea-wrapped-rows` checks actual Metal-shaped text against fixed Menlo/width constraints and browser observations; it does not infer expected offsets from whichever renderer runs.

### `disabled-focus.patch`

**Signal:** disabled Base input and textarea states could still take mouse focus and change selection; they also remained in the Tab order.

Disabled editors no longer register focus tracking, place pointer selections, or honor the public `focus()` helper. Readonly editors remain focusable. Two added upstream tests exercise pointer focus, Tab focus, reenabling, and readonly for both input types.

`set_disabled` has no window argument. The owning Tau prototype therefore moves focus away when disabling a currently focused control. No broad upstream API change is made for that policy.

## Validation and maintenance

The combined patched Base library passed **1,235 upstream unit tests** locally, including the ten added tests. These use Base's test platform; they do not replace the native Metal and browser harness tests.

Optional upstream check, after preparation, from the repository root:

```sh
CARGO_TARGET_DIR="$PWD/gpui/target" cargo test \
  --manifest-path gpui/.cache/crates/gpui-base/Cargo.toml --lib
```

This optional upstream test run resolves the published crate's separate development dependencies; the prototype itself is built against the committed `gpui/Cargo.lock` with `--locked`.

For an upgrade, inspect the upstream fixes, remove superseded hunks, replace the archive checksum deliberately, regenerate the prototype lockfile, and rerun both upstream and rendered regressions. Do not let preparation accept a patch that no longer applies, edit the global registry, or keep a stale patched binary with `--skip-build`.

Upstream source/license: [GPUI Kit v0.7.0 Base](https://github.com/longbridge/gpui-kit/tree/v0.7.0/crates/base), Apache-2.0. The source archive retains its upstream license; only the focused patches are checked in here.
