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
has no disabled command rows. An inline Pi question blocks ordinary commands,
but New Session, Switch Session, Switch Project, and previous/next session
navigation remain available without answering or cancelling the question. Its
answer draft stays with its session. Global dialogs, fullscreen viewers, and
other blocking surfaces still prevent navigation.

On home (with no projects), **Show Update Status** and **Check for Updates**
are omitted from the palette. They become available in a project workspace.

The palette uses a stable fuzzy filter: matching only narrows the list; results
remain in their original order as the query changes. It searches command
labels and reviewed aliases locally; query text is transient and is never
telemetry.

- **Switch Session** (`⌘P`) lists non-archived sessions from all expanded workspace
  projects, grouped in sidebar order (including idle and remote projects). Collapsed
  projects are excluded even from search. The active session is highlighted on open
  when eligible, otherwise the first listed session. Search matches project names
  and session titles; headings are not keyboard stops. An empty workspace list says
  “No sessions”; a search without results says “No matches”. Palette-only
  **Previous Session** and **Next Session** navigate within the active project.
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

| Area       | Command                           | Shortcut             |
| ---------- | --------------------------------- | -------------------- |
| App        | Command palette                   | `⌘K`                 |
| Session    | New session                       | `⌘N`                 |
| Project    | Open project                      | `⌘O`                 |
| Project    | Open local project                | `⌘⇧O`                |
| Project    | Open remote project               | `⌘⌥O`                |
| Session    | Switch session                    | `⌘P`                 |
| Project    | Switch project                    | `⌘⇧P`                |
| Session    | Rename session                    | `⌘⇧R`                |
| Session    | Archive / unarchive session       | `⌘⇧A`                |
| Session    | Mark read / unread                | `⌘⇧U`                |
| Sidebar    | Show archived / active sessions   | `⌘⇧H`                |
| Project    | Expand / collapse focused project | `→` / `←`            |
| Composer   | Choose model / thinking effort    | `⌘⇧M` / `⌘⇧E`        |
| Composer   | Send or queue steering message    | Enter in composer    |
| Composer   | Queue follow-up                   | `⌘Enter` in composer |
| Composer   | Stop Pi                           | `⌘.`                 |
| Recovery   | Review unsent messages            | `⌘⇧D`                |
| Transcript | Find in transcript                | `⌘F`                 |
| Transcript | Load earlier messages             | `⌘⇧B`                |
| Remote     | Reconnect active session          | `⌘⇧C`                |
| Admin      | Open issue reporter               | `⌘⇧I`                |
| App        | Quit (native-owned)               | `⌘Q`                 |

Palette-only target commands are **Focus Composer**, **Focus Sidebar**,
**Focus Transcript**, **Move Project Up**, **Move Project Down**,
**Previous Session**, **Next Session**, **Rename Project**, **Remove Project**, **Clear Queued Messages**,
**Show Update Status**, and **Check for Updates**. They have no global shortcut. Session/project pickers expose their dynamic results
only within their sublists; individual rows are not root commands.

## Project display names

**Rename Project…** is in each project row's context menu, between **New Session**
and **Remove Project**. It targets that row without switching projects or sessions.
The palette's **Rename Project** targets the active project and shows its name and
location to distinguish duplicate labels. It has no new shortcut; `⌘⇧R` still
renames the session.

The compact dialog selects the current name (or that project's retained draft).
Enter or `⌘Enter` saves; IME composition does not submit. Cancel or Escape keeps
unfinished text for reopening and restores valid origin focus or the composer.
Names are trimmed, nonblank, and limited to 240 UTF-16 code units; duplicate
labels, Unicode, punctuation, and internal spaces are allowed. A display-name
change does not move the folder or change project, session, or runtime identity.

While saving, repeated submission and dismissal are blocked. An inline failure
keeps the draft and offers **Retry**. After 10 seconds without confirmation,
**Close** permits leaving the dialog while the write finishes; it does not cancel
the save, and another save for that project is blocked until it settles.

## Local surfaces

Activating a focused project header makes that project active and expands it. Its
separate chevron controls folding only; it does not activate the project. Right
expands and Left collapses the project when either control has focus.

These remain scoped to their existing surface and are excluded from the global
palette unless their launcher appears above.

| Surface                       | Target bindings                                                                                                                                      |
| ----------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- |
| Transcript Find               | Enter / Shift+Enter selects next / previous occurrence and wraps while Find has focus; Escape closes only when Find is the topmost eligible surface. |
| Session rename                | Enter or `⌘Enter` commits; Escape cancels. Opening the palette does not blur-commit.                                                                 |
| Menus and context menus       | Arrows, Enter, and their existing typeahead; Escape dismisses that menu.                                                                             |
| Remote project dialog         | Enter/`⌘Enter` submits where applicable; `⌘↑` goes to parent; Escape closes only when dismissible.                                                   |
| Pi prompts                    | Existing arrows/Enter selection; `⌘Enter` primary submit; `⌘⇧Enter` negative confirmation; Escape cancels only that prompt when eligible.            |
| Model and effort lists        | Existing list navigation and Enter selection; Escape closes the current list.                                                                        |
| Queue and unsent review       | Escape dismisses the focused status/review; `⌘Enter` restores a focused draft when eligible.                                                         |
| Feedback, quit, and update UI | `⌘Enter` activates the eligible primary action; `⌘⇧Enter` skips an update; Escape closes only a dismissible surface.                                 |
| Issue reporter                | `⌘⇧S` toggles Include Current Session within the report; `⌘Enter` submits; Escape retains unsent text.                                               |

An inline Pi prompt blocks global palette and shortcut actions. Explicit sidebar
row actions to mark a session read/unread or archive an eligible session remain
available, including for another project; other blocking dialogs and workspace
lockouts still apply. Escape from a row menu dismisses only the menu, not the
prompt.

## Transcript Find

**Find in Transcript** opens a floating bar for the active transcript. Repeating
`⌘F` focuses and selects the retained query; it never toggles the bar closed.
On macOS this is Command-F, not literal Control-F; elsewhere it is Control-F.
The command is unavailable on home, during initial session loading, or behind a
rename, blocking dialog, fullscreen viewer, or inline Pi prompt.

Find matches literal, case-insensitive occurrences in readable user and agent
text, including formatted prose and code. It searches all **loaded** messages,
including offscreen virtualized rows, not tool output, thinking, skill instructions,
drafts, titles, image/diagram internals, or hidden Markdown markup. It does not
fetch compacted history. “Loaded messages only” identifies an earlier-history
boundary; explicitly loading earlier messages adds them to the search.

Queries, open state, and active occurrences stay local to each live session and
are not persisted or recorded in telemetry. Query edits select the first result;
streaming refreshes counts without jumping. Closing removes highlights and keeps
the reader's current position and query. The palette can open over Find; its first
Escape restores Find rather than dismissing both surfaces.

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
