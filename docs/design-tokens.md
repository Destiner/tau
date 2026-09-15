# Design Tokens

Every visual value in the UI comes from a token in `src/styles.css`. This
document is the meaning of each one: which token to reach for, and what
choosing it says about the element. The palette itself (ayu, light and dark)
is explained in the header comment of `src/styles.css` and is not repeated
here.

The rule: **a component never writes a bare length for control height, font
size, or corner radius, and never writes a bare color.** Two exemptions —
the glyph size inside an icon box (`font-size` on `UiIconButton`, which scales
`UiIcon`'s `1em` box), and one-off layout geometry that is not a control
(padding, gaps, a resize handle's width, a status dot's diameter).

## Scales

### Control height

Four rungs. The rung is chosen by how dense the surrounding surface is, not by
how important the control is — importance is carried by tone, not size.

| Token          | Value | Used for                                                                                  |
| -------------- | ----- | ----------------------------------------------------------------------------------------- |
| `--control-xs` | 16px  | An affordance inside a line of text: the copy button on a code block.                     |
| `--control-sm` | 22px  | Dense chrome that sits beside content: the composer toolbar, a list group head.           |
| `--control-md` | 24px  | The default control: buttons, menu items, and actions revealed on a list row.             |
| `--control-lg` | 28px  | Dialog-weight controls: fields and actions in a dialog, popover triggers, footer buttons. |

Shared buttons use the same height for every variant. Their focus indication is
inset so keyboard focus does not visually enlarge one action beside another.

Nothing else is a control height. A control that does not fit a rung is a sign
the surface's density is undecided, not that the scale needs a fifth value.

### Type

Four rungs. The compact UI scale runs from `--text-xs` through `--text-lg`.
A transcript notice's type label alone uses `--text-notice-label`: it is
metadata within an already labelled message, so it must stay quieter than any
other label.

| Token                 | Value | Used for                                                                         |
| --------------------- | ----- | -------------------------------------------------------------------------------- |
| `--text-xs`           | 10px  | Labels and meta: button text, a select's value, a group head, a row's timestamp. |
| `--text-sm`           | 11px  | Default UI text: menu items, field values, tool details.                         |
| `--text-md`           | 12px  | Titles and notice copy: a session name, an inline-edited name.                   |
| `--text-lg`           | 13px  | Body and prose: the root size, transcript content.                               |
| `--text-notice-label` | 9px   | The type label in a transcript notice only.                                      |

Line height is `--leading-tight` (1.25) for a single line that may wrap tightly,
`--leading-ui` (1.45) for anything read as a sentence.

### Radius

Radius follows what a thing _is_, not how tall it is. A control keeps the same
corner whether it is a 22px toolbar select or a 28px dialog field, which is what
makes a screen full of controls read as one set.

| Token         | Value | Used for                                                                     |
| ------------- | ----- | ---------------------------------------------------------------------------- |
| `--radius-xs` | 3px   | A mark: the checkbox box, and anything smaller than a control.               |
| `--radius-sm` | 4px   | Any control: button, field, select trigger, icon button, and the tooltip.    |
| `--radius-md` | 6px   | A row the reader picks from, a menu item, and an inline block like a notice. |
| `--radius-lg` | 8px   | Anything that floats over the app: dialog, menu, popover, command menu.      |

The tooltip is the one floating surface that does not take `--radius-lg`: it is
two words on 3px of padding, and at that size an 8px corner reads as a bubble
rather than as part of the same set as the controls it labels.

## Color

### Surfaces

A surface token answers "how far from the canvas is this?", not "what color is
this?". They stack in one order and are never used out of it.

| Token            | Meaning                                                                    |
| ---------------- | -------------------------------------------------------------------------- |
| `--sunk`         | Inset _into_ a surface: a code block, a well, a read-only body of text.    |
| `--canvas`       | The app background, and the background of a field the user types into.     |
| `--panel`        | Chrome attached to the canvas: the sidebar, a header.                      |
| `--panel-raised` | Detached from the app and floating over it: dialog, menu, popover, notice. |
| `--user`         | The reader's own message. A single-purpose surface, not a rung.            |

`--scrim` dims the app behind a dialog. `--shadow-soft` lifts a menu or popover;
`--shadow-strong` lifts a dialog. Nothing that is not floating has a shadow.

### State washes

Washes are translucent, so they compose over whichever surface they land on.

| Token                 | Meaning                                                                              |
| --------------------- | ------------------------------------------------------------------------------------ |
| `--hover`             | The pointer is over something actionable. Never indicates state.                     |
| `--selected`          | The row the user is on, in a surface that has focus.                                 |
| `--selected-inactive` | That same row while focus is elsewhere — still the answer to "where am I?", quieter. |
| `--selection`         | Text the user has selected. Set once, globally; components do not use it.            |

### Text

| Token     | Meaning                                                            |
| --------- | ------------------------------------------------------------------ |
| `--text`  | Content, and any label the user acts on.                           |
| `--muted` | Secondary text and chrome: labels, meta, a control's resting text. |
| `--faint` | Tertiary: placeholder, empty state, a disabled or absent value.    |

### Meaning

These tokens carry meaning and are never decoration.

| Token         | Meaning                                                                             |
| ------------- | ----------------------------------------------------------------------------------- |
| `--accent`    | Attention: the caret, an unread session, a warning tone, or a primary button fill.  |
| `--on-accent` | Text and focus indication on accent fills; dark in both color schemes for contrast. |
| `--link`      | A destination the user can open.                                                    |
| `--danger`    | Destructive, or failed.                                                             |
| `--border`    | A boundary between two things. Not a highlight.                                     |

`--status-new` and `--status-working` name the session indicator's colors where
it is read rather than inferred from a palette color. A draft reuses
`--status-working` at 30% opacity, retaining its state relationship without a
separate color token.

## Shared surfaces

Shared interaction styling lives in `src/components/ui/surface.css`, global
because reka portals menus to the body where a scoped attribute never reaches
them:

- `.ui-surface` / `.ui-menu` / `.ui-menu-item` — the floating surface and its
  row. A menu has one cursor, so hover and keyboard highlight are one rule.
- `.ui-selector-trigger` / `.ui-selector-value` / `.ui-selector-option` — the
  common selector language. The value in force has the accent stripe; pointer
  or keyboard traversal has only the hover wash. Filtering and grouping remain
  component-specific.
- `.ui-pick-row` — a row in a list the reader is choosing from. `:hover` says
  the pointer is here; `[data-cursor]` says the keyboard is, and adds a 2px
  accent rail. The two must never look alike, or the keyboard cursor disappears
  under the pointer. Selection in place (a sidebar row, an archived session)
  keeps `--selected` with no rail: it answers "where am I", not "what will Enter
  take".

## Migration

The primitives in `src/components/ui/` are on the tokens, as are the components
that were reworked with them: the command menu, the extension prompt, the remote
dialog, the archived list, the issue report, and the transcript notice. The rest
still carry the pixel values they were written with, including the retired `9px`
and `10px` labels; each is migrated as its part of the UI is reworked. New code
uses the tokens.
