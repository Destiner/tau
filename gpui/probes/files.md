# Files · isolated native probe

Run from the repository root:

```sh
cargo run --manifest-path gpui/Cargo.toml --bin files-probe
cargo run --manifest-path gpui/Cargo.toml --bin files-probe -- --list-checks
cargo run --manifest-path gpui/Cargo.toml --bin files-probe -- --capture /tmp/tau-files-light
cargo run --manifest-path gpui/Cargo.toml --bin files-probe -- --dark --capture /tmp/tau-files-dark
```

Open a synthetic fixture from the list. **Complete Pending** resolves the active fixture; reopen the same fixture before completion and use **Complete Superseded** to exercise the request-identity guard, then switch to another fixture and repeat. Missing text always fails locally; **Retry** starts another controlled attempt. **Copy Source** copies the entire source, not merely visible text. Select text with the pointer or keyboard in the readonly Base textarea, scroll the long file with the wheel, close with Escape or **Close Preview**. Closing returns focus to the fixture that opened the viewer. No user's files, OS picker, network, Pi runtime, or production UI are accessed. The bundled 320 × 180 PNG is generated synthetic pixel data.

The captures write `checks.json` and a real Metal frame per check. Checks cover loading, superseded completion, text selection/read-only input, full-source clipboard, long wrapped text and native wheel scroll, embedded PNG signature/dimensions, image element visibility/bounds/aspect ratio, local failure and repeated retry, unsupported type, Escape and focus restoration, and Tab/Shift-Tab cycling through viewer controls and readonly text without escaping. Clipboard writes only occur when **Copy Source** is clicked; capture uses TestPlatform's local clipboard. The long-file bounds are clipped by the viewer; the Base textarea wraps unbroken lines instead of scrolling horizontally.

This is a fixture-driven presentation prototype, not a file transport implementation. Loading is explicitly completed by the user rather than a timed async task; there is no real IO, cancellation API, truncation, progress timeout, Markdown rendering, code syntax colors, PDF, video, audio, diagrams, or accessibility audit. Image display is deliberately fixed to a 640 × 360 contained frame at the probe's 900 × 700 reference size; responsive resizing is future work. The capture confirms the bundled PNG header and element geometry, not decoded pixel colors or behavior at smaller OS window sizes. The production viewer and extension/asset resolution are unchanged.
