# Host contract

Create one `Entity<DiagramBlock>` for each **real parsed, closed** Mermaid fence.
Subscribe to the unit `DiagramChanged` event to invalidate the owning TextView's
layout; return `Some(entity)` from the prepared Markdown body hook only while
`ready()`. Otherwise return `None` and leave the original fenced code in place.
The parent owns its existing 0.7em bottom gap. Do not constrain figures to the
80ch prose measure. `set_theme` and `set_source` keep the entity/focus handles;
errors and queue saturation retain the parent's source fallback.

A fullscreen layer is deferred in window coordinates, priority 30, and does not
replace the root, install a modal dialog, or consume conversation flex space.
Escape closes only when its own focus handle is focused; an upper menu/dialog
must take focus. Hosts must not globally intercept Escape ahead of focused child
layers. Focus restoration is weak and only runs if focus is still in this viewer.
Unmounting the entity removes its layer, releases an owned capture, and cancels
its operation. Scheme changes keep the last ready reservation mounted while
preparing the new scheme, rather than briefly replacing the viewer with code.

Preparation, local font initialization, rasterization, PNG demultiplication and
GPUI BGRA decoding run on two shared worker threads. Sixteen queued jobs contain
only bounded source/diagram data and channels, never entities. Camera changes
coalesce behind the active job. Until a sharper zoom raster arrives, the last
fullscreen crop stays visible with both its origin and dimensions projected
through the current camera. Source/theme revisions cancel and discard stale
results. UI polling checks the five-second operation deadline independently of
the worker reply, so even a nonpreemptible renderer phase cannot keep a view
pending indefinitely. This does not terminate that phase: hard worker wall-clock
termination still requires a separate helper process (see the renderer README).
Oversized sources fail before Arc duplication or queue admission; the parent,
not the rejected entity, retains the original fallback code. The prepared cache is FIFO/LRU-hit-promoted, at most 50 entries and 16 MiB
of source+SVG (semantic data also has renderer limits). Each entity keeps one
inline and one active image; raster dimensions cap at 4096 and 16,777,216 pixels,
backing scale at 4. The image is rasterized for the intersected actual camera
viewport, not a whole zoomed diagram. Empty intersections paint only the grid.

GPUI exposes native `PinchEvent` (macOS Magnify); the viewer handles it. Its
mouse event stream has no DOM pointer-cancel event. The viewer captures the
native GPUI hitbox and migrates capture across redraws. Release outside, focus
loss, window deactivation, gesture cancellation, and moves without a pressed
left button clear pan state. OS capture/window-deactivation behavior still
requires native-window input review; TestApp input is not AX/OS certification.
`components::markdown::MarkdownPresentation` integrates these blocks through the
prepared Markdown hook in the production app. The host still owns OS-input
acceptance; component tests alone do not establish it.

## Checks

```sh
caffeinate -i cargo test --locked --manifest-path gpui/Cargo.toml --lib components::diagram_view
caffeinate -i cargo clippy --locked --manifest-path gpui/Cargo.toml --lib -- -D warnings
caffeinate -i cargo test --locked --manifest-path gpui/Cargo.toml --test diagram-paint
```

`diagram-paint` is a registered Cargo integration test with `harness = false`,
like `activity-paint`, so its entry point runs on the process main thread. It
requires macOS and an available Metal renderer; the non-macOS entry point fails
explicitly rather than reporting unexecuted checks as passing. Cargo links the
current library and GPUI dependencies directly. Component initialization loads
the local font assets. It runs offline in both schemes using hidden headless
windows and in-memory captures, without saving screenshot files.

The paint test checks inline ink, geometry-derived first-click expansion, and
centered fullscreen ink outside the inline bounds across flowchart, sequence,
and state diagrams. Repeated wheel and native pinch events are captured both
before and after worker settling. Text-colored ink is compared at identical
pixel coordinates against rasters of the actual production SVG, with color
tolerances and a retained-ink threshold; this is not byte-exact full-frame parity,
and background dots cannot satisfy the check. Exact cropped-pixel comparisons
check expand and Markdown copy controls across idle, group hover, button hover,
and keyboard focus, including Escape focus restoration for the viewer.

These are actual Metal paint regressions, not Vue/source-fixture parity or
OS-input certification. The separate Root unit tests drive GPUI key down/up,
mouse, scroll and native pinch events; check nonmodal bounds, draft retention,
capture/cancel, weak focus, source/theme revision behavior and camera invariants.
Worker tests check malformed fallback, bounded cache eviction, shared cancellation
and premultiplied RGBA conversion.
