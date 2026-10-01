# Keyboard and command palette

> **Status: implemented keymap.** Commands are shown only when their current
> target and app state allow them. Platform-native behavior and the explicit
> exclusions remain owned by their existing controls.

## Conventions

- `⌘` means Command on macOS and Control on other platforms; `Ctrl` is literal
  Control on every platform. Shortcut labels use the local platform notation.
- Commands share the same guarded action as their button or menu item. A
  shortcut never targets a hovered row: row-local actions use the focused row,
  otherwise the active session or project.
- No bare-letter global shortcuts. Text editing, IME/dead keys, standard
  editing shortcuts, media keys, and Pi slash completion keep their owners.
- A local shortcut belongs only to its focused, topmost surface. One key event
  executes at most one action. Escape dismisses only the topmost safely
  dismissible layer.
- `Activate` means ordinary Enter or Space on a focused semantic control, not
  a new global binding.

## Palette

`⌘K` opens and closes the global command palette over app-owned transient UI.
It preserves the underlying surface, draft, selection, and caret. Escape closes
only the palette and restores valid origin focus. Commands that are unsafe in
the current dirty, busy, or confirmation state are **not shown**; the palette
has no disabled command rows.

On home (with no projects), **Show Update Status** and **Check for Updates**
are omitted from the palette. They become available in a project workspace.

The palette uses a stable fuzzy filter: matching only narrows the list; results
remain in their original order as the query changes. It searches command
labels and reviewed aliases locally; query text is transient and is never
telemetry.

- **Switch Session** (`⌘P`) opens a searchable list of non-archived sessions in
  the active project only.
- **Switch Project** (`⌘⇧P`) opens imported projects in their existing order.
- Model and effort selection are embedded palette lists, not handoffs to
  separate selectors. Their options are filtered and chosen inside the palette.
- In a project/session/model/effort sublist, `⌘[` or the Back button returns to
  the root. Backspace or Delete on an empty query also returns; otherwise those
  keys edit the query normally.
- Up/Down changes highlight and Enter activates it. Highlighting alone never
  changes state. No-results and empty-list states are distinct.

The launcher is a compact ghost icon in the top-right application chrome. Its
shortcut is available through its tooltip rather than duplicated as visible
chrome text. Home shows the Tau version as a static label, not an update control.

## Global target keymap

The following are intended global commands, subject to the availability and
safety rules above. State pairs share one binding.

| Area       | Command                           | Shortcut               |
| ---------- | --------------------------------- | ---------------------- |
| App        | Command palette                   | `⌘K`                   |
| Session    | New session                       | `⌘N`                   |
| Project    | Open project                      | `⌘O`                   |
| Project    | Open local project                | `⌘⇧O`                  |
| Project    | Open remote project               | `⌘⌥O`                  |
| Session    | Switch session                    | `⌘P`                   |
| Project    | Switch project                    | `⌘⇧P`                  |
| Session    | Previous / next session           | `Ctrl⇧Tab` / `CtrlTab` |
| Session    | Rename session                    | `⌘⇧R`                  |
| Session    | Archive / unarchive session       | `⌘⇧A`                  |
| Session    | Mark read / unread                | `⌘⇧U`                  |
| Sidebar    | Show archived / active sessions   | `⌘⇧H`                  |
| Project    | Expand / collapse focused project | `→` / `←`              |
| Project    | Move focused project up / down    | `⌘⌥↑` / `⌘⌥↓`          |
| Focus      | Composer / sidebar / transcript   | `⌘L` / `⌘⇧L` / `⌘⇧T`   |
| Composer   | Choose model / thinking effort    | `⌘⇧M` / `⌘⇧E`          |
| Composer   | Send or queue steering message    | Enter in composer      |
| Composer   | Queue follow-up                   | `⌘Enter` in composer   |
| Composer   | Stop Pi                           | `⌘.`                   |
| Recovery   | Review unsent messages            | `⌘⇧D`                  |
| Transcript | Load earlier messages             | `⌘⇧B`                  |
| Remote     | Reconnect active session          | `⌘⇧C`                  |
| Admin      | Open issue reporter               | `⌘⇧I`                  |
| App        | Quit (native-owned)               | `⌘Q`                   |

Palette-only target commands are **Remove Project**, **Clear Queued Messages**,
**Show Update Status**, and **Check for Updates**. They have no destructive or
ambiguous global chord. Session/project pickers expose their dynamic results
only within their sublists; individual rows are not root commands.

## Local surfaces

These remain scoped to their existing surface and are excluded from the global
palette unless their launcher appears above.

| Surface                       | Target bindings                                                                                                                           |
| ----------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- |
| Rename                        | Enter or `⌘Enter` commits; Escape cancels. Opening the palette does not blur-commit.                                                      |
| Menus and context menus       | Arrows, Enter, and their existing typeahead; Escape dismisses that menu.                                                                  |
| Remote project dialog         | Enter/`⌘Enter` submits where applicable; `⌘↑` goes to parent; Escape closes only when dismissible.                                        |
| Pi prompts                    | Existing arrows/Enter selection; `⌘Enter` primary submit; `⌘⇧Enter` negative confirmation; Escape cancels only that prompt when eligible. |
| Model and effort lists        | Existing list navigation and Enter selection; Escape closes the current list.                                                             |
| Queue and unsent review       | Escape dismisses the focused status/review; `⌘Enter` restores a focused draft when eligible.                                              |
| Feedback, quit, and update UI | `⌘Enter` activates the eligible primary action; `⌘⇧Enter` skips an update; Escape closes only a dismissible surface.                      |
| Issue reporter                | `⌘⇧S` toggles Include Current Session within the report; `⌘Enter` submits; Escape retains unsent text.                                    |

## Discovery and tooltip policy

Shortcut hints use the same formatter as the palette and accurately identify
any contextual scope. Informational path, status, and session tooltips remain
informational rather than acquiring invented commands.

The approved compact tooltip treatment is **C**: an outline, secondary, small
shortcut treatment, with **Enter** spelled out (not a return-glyph substitute).
It must not steal menu/popover anchors or create fake tab stops.

## Explicit exclusions

The following retain ordinary native, browser, Pi, or content-local keyboard
behavior rather than becoming app commands: standard editing and window
shortcuts; sidebar resizing; transcript scrolling/selection; media and viewer
controls; per-message disclosures; code/file/link/context-menu actions; drag
reordering; Pi slash filtering, completion, and execution; and the undisclosed
admin unlock code. Native About, Hide, window management, and OS file panels
also remain native-owned.

## Keeping this inventory current

Update this document with any changed command, scope, target, or shortcut in
the same change that implements it. Do not add a palette entry or shortcut by
accident, and do not describe target keymap entries as shipped before their
coverage lands.
