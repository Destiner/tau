# Bounded editor accessibility (20/80)

Named Base inputs (`accessibility_label`) expose a complete plain-text AccessKit
value when the document is at most **64 KiB of UTF-8 bytes**. This includes empty,
readonly and disabled non-password fields. Unnamed generic inputs do not project
values. Password/masked fields never project their text or advertise SetValue.
The existing text element adds no accessible plaintext descendants; delivered
whole-tree regressions check value, label and description for leaks.

Above the limit, value and SetValue are both absent. The role/name/status and
existing focus path remain. There is no truncated prefix pretending to be a
complete document, and large generic ropes are not converted to strings every
frame. Projection is opt-in and size-bounded, not a text-layout cache or a new
editor data model. Named bounded fields currently allocate a bounded value at
render time.

The one additional action is **SetValue** (whole-value replacement), supported
only for named bounded editable **InputState and TextareaState** outside IME
composition. Named **EditorState** code editors retain the same bounded readable
value, but never advertise or accept SetValue, even with auto-close disabled.
Their interactive pairing/indentation semantics are outside this Tau field scope;
whole-value accessibility operations must not silently turn `(` into `()`.
The pinned
`accesskit_macos 0.26.3` provider forwards `setAccessibilityValue:` with an
`ActionData::Value`; its selector allow-list recognizes SetValue without text
ranges. Existing Core `aria_value` and `on_a11y_action` are sufficient: **no Core
API/patch change is needed**.

Delivery checks the same input/textarea mode policy, live name opt-in,
editability, masked state, marked-text state,
both incoming and current-document sizes, and `active_focus_trap` containment.
Malformed/missing/numeric payloads are inert. IME composition is readable but
cannot be overwritten or committed by SetValue. SetValue does not steal focus;
Focus continues to use the existing editor handle and live trap/disabled guards.
A retained enabled listener also checks policy changes before any redraw.

Accepted replacements convert the entire existing byte range to UTF-16 and call
`EntityInputHandler::replace_text_in_range` with an Atomic undo intent. The same
editor owns normalization, validation, change events, selection, display updates
and history. There is no direct `set_value`, alternate mutation/clipboard path,
new shortcuts or global editor dispatcher. Single-line newline normalization and
validation remain authoritative. Undo restores the original text and Unicode
selection; subsequent typing and other editors have separate history steps.

## Focused verification

`tests/editor-values.rs` is a headless delivered-tree/action regression. It uses
Core TestPlatform's real activation/action callbacks, Base fields, the normal
input handler, genuine typing and undo/redo key dispatch. It checks exact values
and intermediate changed states (not insert/backspace net-zero observations),
Unicode/non-BMP/combining text, multiline values, independent histories,
selection restoration, IME commitment/atomic undo, disabled/readonly/masked
policy, both sides of the byte limit, empty values, generic opt-out, malformed
payloads, normalization/validation and active-trap rejection. A named empty code
editor with an explicit pairing language exposes exactly an empty value, withholds
SetValue, and remains empty after a delivered Value `(` action; normal interactive
input still produces `()`. Input and textarea advertise SetValue in the same tree.

Base's `input/base/accessibility_tests.rs` deliberately changes private live
policy without notifying or drawing. It first proves that the actually delivered
tree still advertises the enabled SetValue action, then delivers that retained
action and checks unchanged text/history/composition. This isolates live guards
from the simpler rejection caused by removing the handler at redraw.

## Deliberately deferred

No accessible cursor/selection API, InlineTextBox descendants, text ranges,
range geometry, layout projection, navigation/read-aloud infrastructure, rich
text, large-document AX editing, password AX editing, or live announcements.
Modal tree isolation and nested trap policy are a separate owner's work.

The tests establish the application-side tree and action contract only. Pinned
macOS source compatibility is not proof of OS delivery, VoiceOver reading or
editing, certification, IME candidate UI, physical keyboard behavior, clipboard
exchange, or permissions. No GUI is launched and no OS clipboard, permissions,
Pi runtime or real session is touched by this slice.
