# Tau Quality Rubric

This document defines what "polished and reliable" means for tau. It is a
set of rules that quality work is judged against — not a backlog, not a
record of individual decisions. When a concrete question comes up (should
X spin, where does Y render, can Z animate), the answer should be derivable
from a rule here. If it isn't, the rubric is missing a rule, not an entry.

**How it's used:**

- Audit passes check the app against one section at a time.
- Every rule carries a verification tag:
  - `[e2e]` — enforced or enforceable by an end-to-end test
  - `[unit]` — enforced or enforceable by a unit test
  - `[audit]` — checkable by an agent reading code against the rule
  - `[manual]` — requires a human using the app
- The ratchet rule: a bug that violates a rule here is fixed together with
  a regression test.
- New components and flows inherit every applicable rule.
- Concrete violations found in audits go to the backlog/journal, not here.

## Principles

- **Minimal core.** Adding anything costs; removing anything gains. Extra
  capability belongs in extensions. Empty surfaces avoid hints, tips, and
  decorative filler; concise factual copy is useful when blankness would be
  ambiguous.
- **Locality.** Everything renders where its context lives: a message
  belonging to a session appears in that session's transcript; session
  state lives in that session's indicators; a dialog's failure lands in
  that dialog. Foreground operation failures without an owning surface use
  the app-level fallback dialog; background-session failures wait until their
  session is selected. No other feedback pops over the UI or steals attention.
- **Never lose the user's work.** Typed text is sacred; no action, error,
  or race destroys it.
- **Keyboard-first (north star).** The long-term goal is that nearly all
  actions are doable by keyboard alone. An aspiration guiding new work,
  not a current requirement.
- **Motion is a tool, not decoration.** Default is no animation.
- **Perceptible slowness is a bug** — journaled and fixed like any other
  defect, in place of numeric performance budgets.

---

## 1. Feedback

- Every interaction is acknowledged visibly and near-instantly, or shows
  progress. `[manual]`
- Fast operations never flash a loading indicator; indicators appear after
  a delay tuned to the context of each surface. `[audit]`
- No surface is ever silently blank while work is in flight — every async
  wait has a visible state. `[e2e]`
- Cheap mutations are optimistic: the UI reflects the change immediately
  and reconciles in the background. `[unit]`
- Disabled controls look disabled and are inert — no dead clicks that
  swallow input. `[audit]`

## 2. Input integrity

- The user's draft survives everything: switching context and returning,
  a failed operation, transient UI appearing or erroring, and an app
  restart. `[unit]`
- Dismissing or cancelling never destroys typed text without an obvious
  way back. Explicitly cancelling a tool or extension prompt, including with
  Escape while it is focused, may discard that prompt's unfinished answer;
  ordinary composer text, failed operations, and unrelated context changes
  still preserve their drafts. `[audit]`

## 3. Failure

- Every failure is either surfaced to the user or deliberately classified
  as cosmetic; only purely cosmetic operations may fail silently, and once
  structured logging exists, every failure is logged regardless. `[audit]`
- Failures surface by locality: in the turn, session, or dialog they belong
  to. A foreground failure with no dedicated surface uses one fallback dialog;
  a background session's failure waits behind its status indicator until the
  user selects that session. `[audit]`
- Every error has reviewed, operation-specific plain-language copy stating
  what happened and what to do next. Raw technical details never reach the UI.
  `[audit]`
- Failure is distinguishable from slowness: within bounded time the user
  learns an operation failed, and why. `[unit]`
- Where retry makes sense, it is offered in place — a failure is never a
  dead end. `[unit]`
- One session's failure never corrupts, blocks, or leaks into another.
  `[unit]`
- The native side never panics on a user-reachable path; errors cross the
  bridge as values, not crashes. `[audit]`

## 4. Transcript

- The reader's position is physically stable: content streaming in never
  moves what they are reading. When they are at the end, the view follows
  the end. `[e2e]`
- Per-message UI state (expanded/collapsed) survives scrolling and
  re-rendering. `[e2e]`
- Very large sessions stay fully responsive to scroll and input. `[e2e]`
- An interrupted stream leaves a coherent transcript: partial content is
  preserved and visibly final. `[unit]`
- Rendered content is sanitized, and links open outside the app — the
  window never navigates away. `[audit]`

## 5. State correctness

Concurrency bugs are the worst UX bugs.

- Stale async results never overwrite newer state — late responses to
  superseded requests are dropped. `[unit]`
- Concurrent sessions are fully isolated: activity in one (including
  streaming in the background) never disturbs another's state, and
  returning to a session finds it intact. `[unit]`
- Every transitional state is bounded: nothing can remain "starting",
  "stopping", or "pending" forever — each resolves to success or a
  surfaced failure within a known window. `[unit]`
- Repeat submission is impossible while an equivalent operation is
  pending. `[e2e]`
- Background resource management (process reuse, eviction) never touches
  what the user is actively using. `[unit]`

## 6. Keyboard and focus

- When transient UI closes, focus returns to a still-valid originating
  control or the active composer — never behind another modal or onto the
  document body. `[e2e]`
- Escape dismisses only the topmost dismissible transient layer. If the
  topmost surface cannot be safely dismissed, Escape does nothing. Focused
  tool and extension prompts are explicitly cancellable under the input
  integrity exception above. `[e2e]`
- The current accessibility level (semantics, roles, live regions) is the
  bar: maintained in changed code, neither regressed nor expanded. `[audit]`

## 7. Visual stability and motion

- Nothing shifts layout after first paint; asynchronous content reserves
  its space. `[manual]`
- Streaming growth never causes horizontal shift or scrollbar flicker.
  `[manual]`
- Window chrome behavior follows the platform's conventions. `[manual]`
- Animation is justified only when it carries information the user would
  otherwise miss — a spatial or state relationship. Never on
  high-frequency interactions: anything done many times a day responds
  instantly, because even fast animation is a tax on every repetition.
  When used: short and interruptible. `[audit]`

## 8. Copy

- Voice is terse. Controls, menu items, tooltips, and headings use title case;
  statuses, validation, errors, placeholders, and explanatory prose use
  sentence case. No exclamation marks or anthropomorphizing. `[audit]`
- Minimal: if a sentence can be removed and the UI still makes sense,
  remove it. `[audit]`
- Status messages describe current state, not history. `[audit]`
- Empty states are concise and intentional. A short factual label is useful
  when blankness could be mistaken for loading, failure, or missing content;
  an action appears only when there is a useful next step. `[audit]`

## 9. Performance

- Existing end-to-end performance invariants are regression guards and
  must keep passing. `[e2e]`
- Changes may not introduce perceptible lag to typing, navigation, or
  scrolling. `[manual]`
