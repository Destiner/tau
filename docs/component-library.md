# Component library refactor

A plan for building Tau's reusable component library on [reka-ui](https://reka-ui.com)
and clearing component-specific CSS out of `src/styles.css`, keeping only the
theme and global defaults there. Strictly a refactor: no UI/UX changes.

## Inventory

### Primitives on reka-ui (reka provides behavior + a11y; Tau provides the ayu styling)

| Component                     | Replaces                                      | API                                                                                                                                                                                    |
| ----------------------------- | --------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `UiMenu` (DropdownMenu)       | `.project-menu`                               | `items: { label, disabled?, run? }[]`, `v-model:open`, `side?` (default `top`), `align?` (default `start`), `sideOffset?` (default 5), `minWidth?`; `#trigger` slot                    |
| `UiContextMenu` (ContextMenu) | `.context-menu` (session rows, text fields)   | `items: { label, disabled?, run? }[]`, `minWidth?` (default 152); default slot = trigger via `asChild`                                                                                 |
| `UiDialog` (Dialog)           | `.dialog-layer` + `.remote-dialog`            | `v-model:open`, `title` (aria), `width?: 'sm' \| 'md'` (440/480px), `busy?: boolean` (aria-busy); default slot = panel body                                                            |
| `UiSelect` (Select)           | `.composer-selector` (model + effort pickers) | `modelValue: string`, `options: { value, label }[]`, `placeholder?`, `fallbackLabel?` (shown when the value is not in `options`), `disabled?`, `maxWidth?` (default 210), `ariaLabel?` |
| `UiCollapsible` (Collapsible) | `.tool-call` expand/collapse                  | `v-model:open`, `disabled?`; `#trigger` + `#content` slots                                                                                                                             |

### Plain primitives (no reka — pure markup and state)

| Component      | Replaces                                                                                                           | API                                                                                                                                                                                                                                                |
| -------------- | ------------------------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `UiButton`     | `.extension-dialog-button.primary/.secondary`                                                                      | `variant: 'primary' \| 'secondary'`, `size?: 'sm' \| 'md'`                                                                                                                                                                                         |
| `UiIconButton` | `.icon-button` (28px), `.row-action`/`.session-archive` (22px), `.send-button` (20px), notification dismiss (16px) | `size: 'lg' \| 'md' \| 'sm' \| 'xs'` (28/22/20/16), `variant: 'fade' \| 'fill'`, `tone?: 'default' \| 'danger'`, `label` (aria, required), `title?`, `disabled?`; icon slot. Row-reveal hover rules live in the row's own stylesheet via `:deep()` |
| `UiInput`      | `.extension-dialog-input`, `.remote-dialog input`, `.session-name-input`                                           | `modelValue`, `variant: 'bordered' \| 'mono' \| 'bare'`, `error?: boolean`; everything else via `$attrs`                                                                                                                                           |
| `UiTextarea`   | `.extension-dialog-editor`                                                                                         | `modelValue`, `variant: 'bordered' \| 'bare'`; rest via `$attrs`                                                                                                                                                                                   |
| `UiStatusDot`  | `.session-indicator` (new/draft/working), `.tool-status` (working)                                                 | `tone?: 'new' \| 'draft' \| 'working'`, `label?` (aria; without it the dot is aria-hidden)                                                                                                                                                         |

Kept as-is and moved under `ui/`: `UiIcon`, `UiSpinner`, `MarkdownText`. The
`.markdown` typography moves into `MarkdownText.vue` as a **non-scoped** style
block — required because `v-html` output carries no scoped attributes.

### App-level components extracted from `App.vue`

| Component               | Contents                                                                                                                                                     |
| ----------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `ProjectSidebar.vue`    | titlebar, sortable project list, session rows, footer `UiMenu`, resize handle. Owns sortablejs init, sidebar-width state (v-model), localStorage persistence |
| `SessionHeader.vue`     | session title, rename field, new-session button; exposes `cancelRename()`                                                                                    |
| `CommandMenu.vue`       | slash-command popover; keeps its placement math                                                                                                              |
| `ComposerBar.vue`       | status line, command menu, growing textarea, model/effort `UiSelect`s, send/stop button; exposes `focus()`                                                   |
| `ExtensionDialog.vue`   | the in-flow workflow prompt (select / input / editor / confirm). Stays in-flow by design — not a modal                                                       |
| `RemoteDialog.vue`      | connection + directory browser inside `UiDialog`                                                                                                             |
| `NotificationStack.vue` | fixed bottom-right notification stack                                                                                                                        |

### Stays hand-rolled (reka's model does not fit the interaction)

- **Composer textarea** — `field-sizing: content` multi-line field the command
  menu is anchored to; reka Autocomplete/Combobox expect a single-line input.
- **Command menu + the two option lists** (extension options, remote directory
  list) — keyboard navigation is driven by an external input or wraps around,
  which conflicts with reka Listbox's roving focus. No `UiListbox` is built.
- **Project group collapse** — the collapsed state is server-persisted
  (`project.collapsed`), so it stays a plain button + `v-if` rather than
  `UiCollapsible`.
- **Transcript virtualizer** — built on `@tanstack/vue-virtual`.

Built in the follow-up: `UiTooltip`, `UiToast` (extension notifications), and
`UiSplitter` (the sidebar resize, via reka Splitter with px sizing and
`autoSaveId` persistence).

## CSS ownership map

`src/styles.css` keeps only: theme tokens (`:root` + dark scheme), global
defaults (box-sizing, font stack, cursor/user-select policy, `::selection`,
disabled opacity, `accent-color`). Everything else moves to the owning SFC:

| Current CSS                                                                 | Destination                                                   |
| --------------------------------------------------------------------------- | ------------------------------------------------------------- |
| `.app-shell`, `.window-inactive`, `.resizing-sidebar`                       | `App.vue` (scoped)                                            |
| `.sidebar*`, `.project-*`, `.session-*`, `.empty-*`                         | `ProjectSidebar.vue` (+ `UiStatusDot`, `UiIconButton`)        |
| `.session-pane`, `.session-loading`                                         | `App.vue`                                                     |
| `.session-header`, `.session-name*`, `.session-new-button`                  | `SessionHeader.vue`                                           |
| `.transcript*`, `.message*`, `.user-bubble`, `.thinking-*`, `.stream-state` | `TranscriptView.vue`                                          |
| `.tool-*`                                                                   | `ToolCall.vue` (via `UiCollapsible`)                          |
| `.error-*`                                                                  | `ErrorNotice.vue`                                             |
| `.composer*`, `.send-button`, `.status`                                     | `ComposerBar.vue`                                             |
| `.command-*`                                                                | `CommandMenu.vue`                                             |
| `.composer-selector`                                                        | `UiSelect.vue`                                                |
| `.icon-button`, `.row-action`                                               | `UiIconButton.vue` (+ row reveal rules in `ProjectSidebar`)   |
| `.project-menu` / `.context-menu`                                           | `UiMenu.vue` / `UiContextMenu.vue`                            |
| `.extension-notification*`                                                  | `NotificationStack.vue`                                       |
| `.extension-composer`, `.extension-dialog-*`                                | `ExtensionDialog.vue` (+ `UiInput`, `UiTextarea`, `UiButton`) |
| `.dialog-layer`, `.remote-dialog*`, `.remote-directory-*`                   | `RemoteDialog.vue` (+ `UiDialog`, `UiInput`)                  |
| `.markdown*`                                                                | `MarkdownText.vue` (non-scoped block)                         |

One technical note: reka portals (menu/dialog content) mount into `body`, and the
scoped `data-v` attribute does **not** reach the portaled node, so the primitives
style their portaled parts with `:global()` on namespaced classes (`.ui-menu`,
`.ui-dialog-content`, …). Regular child-component roots do carry the parent's
scoped attribute, so non-portaled descendants are styled plainly.

## Phases

Checks after every phase: `bun run lint && bun run typecheck && bun run test
&& bun run test:e2e && bun run build`.

0. **Foundation** — add `reka-ui`; create `src/components/ui/`; move `UiIcon`,
   `UiSpinner`, `MarkdownText` there and update imports.
1. **Simple primitives** — `UiButton`, `UiIconButton`, `UiInput`, `UiTextarea`,
   `UiStatusDot`; migrate their call sites; move their CSS out.
2. **Overlay primitives** — `UiMenu`, `UiContextMenu`, `UiDialog`, `UiSelect`,
   `UiCollapsible`; migrate project menu, context menus (and drop the
   document-level `contextmenu` opener), remote dialog, composer pickers, tool
   calls; move their CSS out.
3. **App extraction** — `SessionHeader`, `CommandMenu`, `ComposerBar`,
   `ExtensionDialog`, `RemoteDialog`, `ProjectSidebar`, `NotificationStack`;
   `App.vue` shrinks to an orchestrator.
4. **CSS final sweep** — reduce `styles.css` to theme + global defaults; move
   `.markdown` into `MarkdownText`; verify no orphaned rules.

**Class-name contract** (Playwright asserts on these): `.message`,
`.tool-call`, `.tool-header`, `.tool-details`, `.tool-detail-label`,
`data-index`, `data-message-id`.

A dev playground (`src/dev/ComponentPlayground.vue`, loaded via
`?fixture=playground`) exists for visual evaluation of the primitives in both
ayu schemes. It stays **uncommitted** until evaluated.

## Status

Implemented end to end. `App.vue` went from ~1610 lines to ~480; `styles.css`
from ~1580 lines to the theme and global defaults only. Two deviations from the
inventory above, both noted there: no `UiListbox` was built (the two option
lists are driven by an external input's keyboard, which reka Listbox's roving
focus fights), and the `.status` line lives inside `ComposerBar` rather than
being its own component.

Follow-up: the three deferred primitives shipped. `UiTooltip` replaces native
`title` on the sidebar and header actions (long informational titles — drag
handle, connection details — stay native). Extension notifications render
through reka Toast (`UiToast` provider + viewport + `NotificationToast`) with
enter/exit animations, swipe-to-dismiss, and the same persistent lifetime.
The sidebar resize is driven by reka Splitter (`SplitterGroup`/`SplitterPanel`
with px sizing + `autoSaveId` persistence + the resize handle's `dragging`
event for the resize cursor); the hand-rolled pointer/keyboard/persistence
machinery and `lib/sidebar-width.ts` were removed.

A dev playground (`src/dev/ComponentPlayground.vue`, loaded via
`?fixture=playground`) exercises every primitive in both ayu schemes, with a
manual light/dark toggle, plus tooltip, toast, menu, and dialog demos. It stays
**uncommitted** until evaluated.

## Commits

1. `docs: plan the reka-ui component library refactor`
2. `chore: add reka-ui and move shared components under ui/`
3. `feat(ui): add button, icon-button, input, textarea, and status-dot primitives`
4. `refactor: migrate simple call sites to the ui primitives`
5. `feat(ui): add menu, context-menu, dialog, select, and collapsible primitives`
6. `refactor: adopt the overlay primitives across the app`
7. `refactor: extract the session header into its own component`
8. `refactor: extract composer, dialogs, and notifications from App.vue`
9. `refactor: extract the project sidebar and reduce styles.css to theme and defaults`
