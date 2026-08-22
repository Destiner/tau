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
| `--control-xs` | 16px  | An affordance inside a line of text: the copy button on a tool row.                       |
| `--control-sm` | 22px  | Dense chrome that sits beside content: the composer toolbar, a list group head.           |
| `--control-md` | 26px  | The default control: buttons, and actions revealed on a list row.                         |
| `--control-lg` | 30px  | Dialog-weight controls: fields and actions in a dialog, popover triggers, footer buttons. |

Nothing else is a control height. A control that does not fit a rung is a sign
the surface's density is undecided, not that the scale needs a fifth value.

### Type

Four rungs. `9px` and `10px` labels are retired; the smallest text in the app
is `--text-xs`.

| Token       | Value | Used for                                                                         |
| ----------- | ----- | -------------------------------------------------------------------------------- |
| `--text-xs` | 11px  | Labels and meta: button text, a select's value, a group head, a row's timestamp. |
| `--text-sm` | 12px  | Default UI text: menu items, field values, tool details.                         |
| `--text-md` | 13px  | Titles: a session name, an inline-edited name.                                   |
| `--text-lg` | 14px  | Body and prose: the root size, transcript content.                               |

Line height is `--leading-tight` (1.25) for a single line that may wrap tightly,
`--leading-ui` (1.45) for anything read as a sentence.

### Radius

Radius follows the element's role, and for controls it follows the height rung,
so the two scales stay in step.

| Token         | Value | Used for                                                              |
| ------------- | ----- | --------------------------------------------------------------------- |
| `--radius-sm` | 4px   | An `xs` or `sm` control, and inline marks: chips, small icon buttons. |
| `--radius-md` | 6px   | An `md` or `lg` control, and a list row or menu item.                 |
| `--radius-lg` | 9px   | Anything that floats over the app: dialog, menu, popover, notice.     |

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

These four carry meaning and are never decoration.

| Token      | Meaning                                                                     |
| ---------- | --------------------------------------------------------------------------- |
| `--accent` | Attention: the caret, an unread session, a warning tone. Not a button fill. |
| `--link`   | A destination the user can open.                                            |
| `--danger` | Destructive, or failed.                                                     |
| `--border` | A boundary between two things. Not a highlight.                             |

`--status-new`, `--status-draft`, and `--status-working` are the session
indicator's three states, aliased so the indicator's meaning is named where it
is read rather than inferred from a palette color.

## Migration

The primitives in `src/components/ui/` are on the tokens. App components still
carry the pixel values they were written with, including the retired `9px` and
`10px` labels; each is migrated as its part of the UI is reworked. New code uses
the tokens.
