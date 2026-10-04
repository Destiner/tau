# Menus probe

`cargo run --bin menus-probe -- --list-checks` lists the isolated checks. Capture without opening a persistent app:

```sh
cd gpui
cargo run --bin menus-probe -- --capture /tmp/tau-menus-light
cargo run --bin menus-probe -- --capture /tmp/tau-menus-dark --dark
cargo clippy --bin menus-probe -- -D warnings
```

The 16 checks cover opening the dropdown, inert disabled row, outside-click dismissal, pointer hover, keyboard selection, Tab/Shift-Tab cycling enabled rows without losing focus, Enter activation, a second context surface above an open dropdown, one-layer-at-a-time Escape with focus restoration, right-click activation, an above-trigger popup near the bottom edge, and reset. Every check records actual surface visibility, focus handle, counters, selection and (while open) viewport-contained bounds in `checks.json`; the edge check additionally compares trigger and popup bounds. The stacked check uses a right-click on a dropdown row to open the context menu as a second surface; it is **not** a submenu or proof of arbitrary modal stacking.

The menu is deliberately application-owned: a small `Item` specification and shared `menu(layer, window, cx)` row/surface builder serve both dropdown and context menus. Base `Popup` owns trigger measurement, deferred paint and window-edge clamping; `Button` supplies real pointer/keyboard semantics and disabled behavior. Focus and highlighted index stay in `MenusProbe`, with Escape explicitly closing only the focused top layer. This is a useful starting point for picker and palette rows, not a generic menu framework.

Only synthetic counters change. There is no Pi/runtime integration, clipboard, filesystem operation, macOS system menu, typeahead, submenu, native accessibility parity audit, or assertion of full Reka/Vue behavior. The context menu is anchored to its row rather than the exact pointer coordinate; the normal context check compares measured row and popup geometry. The edge toggle deliberately places the dropdown trigger low and selects an above-trigger anchor; dynamic auto-flipping for arbitrary window resizing is not established by this probe. The edge layout puts the context trigger below the viewport; the edge check only exercises the dropdown.
