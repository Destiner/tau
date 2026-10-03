# Vue reference fixture

This fixture imports the production `UiButton`, `UiInput`, `UiTextarea`, and `src/styles.css`; it does not import the development catalogue. An isolated, loopback-only Vite server hosts the fixture. Playwright supplies keyboard and pointer events; focus, selection, and prop changes are explicit setup operations. Observations read the DOM control and real event counters, never authored expected values.

Each scenario mounts a fresh control, resets counters and model state, and clears a held pointer before starting. The fixture verifies the bundled browser font is loaded. Page/console errors and attempted external requests fail the run.

Run from the repository root:

```sh
bun gpui/scripts/run.ts --reference-only
bun gpui/scripts/run.ts --component input --browser webkit
```

Captures are 1280×640 physical pixels at scale 2 for a 640×320 logical viewport. Caret blinking is hidden in images; UTF-16 selection is independently observed. Setup selections inside a surrogate pair or outside the current value are rejected.

Reference-only runs assert expectations and fail on mismatches. Full comparisons retain mismatching reference observations so the joint report can show Vue, GPUI, and the authored expectation; the comparator still fails. Current WebKit differences are **not waived**. See the [prototype README](../README.md) for commands, coverage, current results, review blockers, and limitations.
