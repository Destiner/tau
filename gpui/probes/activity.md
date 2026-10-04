# Native activity rows probe

Run `cargo run --manifest-path gpui/Cargo.toml --bin activity-probe -- --capture /tmp/tau-activity-light` (add `--dark` for dark mode). This is a synthetic, isolated GPUI window, not a production transcript.

Click a header or focus it and press Space/Return to toggle details. Drag over the 190px bounded Base `TextView` to select output; wheel over it to scroll independently. Running / Complete / Fail change the first record's fixture status; Append extends the second and adds a row. Load Many adds 300 rows; First Rows, Far Rows, Latest and Reset control the measured list.

The capture takes 22 Metal screenshots per theme and records `checks.json`. Checks inspect measured collapsed/expanded row height and detail bounds, real pointer selection, the detail's own scroll position, independent status/disclosure state, measured first-row position while appending as a reader, tail-to-viewport gap, and the selected text after wheel-driven offscreen exit/re-entry. IDs and text-view entities are retained per record rather than recycled; these assertions exercise fixture state plus visible bounds, not a production persistence contract. Jump buttons establish the far-away viewport; a subsequent wheel exit/re-entry exercises selection retention. Loading 300 synthetic rows is a virtual-list behavior fixture, **not** a responsiveness or performance benchmark.

No Pi RPC, processes, filesystem, networking, persistent window, production `ActivityRow`, transcript integration, accessibility audit, or sustained large-session performance is covered. Button focus may clear a native selection; the selection retention check uses wheel scrolling instead. Detail output uses fenced Markdown, not production `<pre>`.
