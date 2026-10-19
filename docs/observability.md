# Local observability

## Admin mode and privacy

Telemetry and the issue reporter are off by default. Outside admin mode nothing is
recorded, no telemetry directory is created and no report button is shown. Typing
`iddqd` outside text fields toggles both together. The shared native setting is
persisted in `preferences.json` and read before telemetry initialization. Enabling
starts collection; disabling flushes recorded work, stops collection and invalidates
outstanding measurement tokens.

`crates/tau-observability` owns preferences, feedback append, telemetry validation,
retention and optional OTLP export. The GPUI service worker uses those services and
formats. Initialize an explicit `tau-core` profile before `Telemetry::init`;
initialization never silently opens production storage.

Telemetry is bounded, content-free and best-effort. Its failure never fails product
operations. Reviewed catalog names and fixed outcomes describe operations, lifecycle
state, timings and counts, with relevant session/runtime identifiers. Never record
prompts, transcripts, drafts, tool/extension payloads, file/clipboard contents, paths,
SSH commands/connection strings, stderr, credentials or raw errors. Do not introduce
free-form diagnostic labels or a parallel reporting schema.

## Local files

Under the selected profile's Tau data directory:

- `telemetry/traces.jsonl`, `logs.jsonl`, `metrics.jsonl` and rotated segments
- `feedback/issues.jsonl` for user reports; `feedback/done.jsonl` for manual resolutions

Production uses application-data `tau/` (on macOS,
`~/Library/Application Support/tau/`). An explicit isolated profile uses
`<profile>/tau/`. Isolated profiles persist until explicitly removed; no automatic
per-process temporary cleanup applies. See [development storage](development-storage.md).
Telemetry retention is bounded to seven days and 256 MiB. Local persistence remains
authoritative even if optional external delivery is unavailable.

## Reporter acknowledgement and triage

The retained reporter is available only in admin mode. A report contains the user's
description, submission time and current Pi session ID **only when explicitly opted
in**; it is separate from content-free telemetry. Reports remain local until manually
removed. Submission waits for actual append acknowledgement even when the reporter
is hidden or admin mode changes. Delay is not proof of failure and must not admit a
duplicate retry. Preference-write failure uses fixed copy and reconciles with the
backend's actual live gate.

Tau only appends to `issues.jsonl`. Resolutions are moved manually to `done.jsonl`,
preserving original fields and adding `resolvedAt`, `resolution` (`fixed`, `deferred`
or `not-an-issue`), a fixing `commit` where known and a short `note`. This is a human
convention, not a runtime consumer. Re-read immediately before editing the inbox to
preserve concurrent appends; only resolve with user confirmation. See the
[triage skill](../.agents/skills/triage/SKILL.md).

## Optional development OTLP export

Export is disabled by default; no collector is bundled. The shared observability
crate has an opt-in `otlp_export` feature. Do not assume the app forwards that feature:
verify current Cargo feature wiring before enabling an export build.

The lightweight exporter supports plain HTTP/JSON OTLP via
`OTEL_EXPORTER_OTLP_ENDPOINT` and `OTEL_EXPORTER_OTLP_PROTOCOL=http/json`, with
signal-specific endpoint/protocol variables and optional `OTEL_EXPORTER_OTLP_HEADERS`.
HTTPS, gRPC and protobuf are not supported by this development path; use a local
Collector HTTP receiver. Keep headers out of logs, source and visible arguments.
Delivery uses a bounded best-effort background queue and remains subject to the same
admin gate and content-free policy. No GUI telemetry viewer is promised.
