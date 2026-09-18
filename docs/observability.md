# Local observability

## Admin mode

Telemetry and the issue reporter are off in an ordinary run: nothing is recorded, no telemetry directory is created, and the sidebar shows no report button. Both are unlocked together by admin mode, which is turned on and off by typing `iddqd` anywhere outside a text field.

The setting is persisted natively in `preferences.json` beside the telemetry and feedback directories (the disposable profile in development, application-data `tau/` in production) and is read before the app records anything, so a run outside admin mode observes nothing at all rather than recording and discarding. Toggling it takes effect immediately: enabling starts the run's telemetry where a launch in admin mode would have begun it, and disabling flushes what was recorded, drops what the frontend still had queued, and stops.

When admin mode is on, Tau continuously writes content-free OpenTelemetry JSONL under its profile data directory:

- development: `<temporary-root>/tau/telemetry/`
- production: `tau/telemetry/`
- signals: `traces.jsonl`, `logs.jsonl`, and `metrics.jsonl`, plus rotated segments

Retention is bounded to seven days and 256 MiB. Telemetry failure does not fail product operations. Records contain operation names, lifecycle state, timings, outcomes, counts, sanitized source basenames, and relevant session/runtime identifiers. Pi frontend ownership records distinguish initial claims, replacements, rejected claims, and cleanup failures, including only the stale direct-child count. Children stopped during replacement have a matching `pi.process.stopped` record with the fixed `ownership_replaced` reason. Owner tokens and native failure text are never recorded. Records do not contain prompts, transcripts, drafts, tool or extension payloads, file/clipboard contents, paths, SSH commands, connection strings, stderr, credentials, or raw errors.

## User issue reports

The report button in the sidebar footer is part of admin mode and is not shown otherwise. Reports submitted from it are stored separately from content-free telemetry:

- development: `<temporary-root>/tau/feedback/issues.jsonl`
- production: `tau/feedback/issues.jsonl`

Each JSON line contains the user's description, its submission time, and the current Pi session ID only when the user explicitly includes it. Reports stay local and are retained until the file is removed. Native development reports are disposable along with the workspace: copy them before closing the app if needed. The startup terminal output identifies the current profile; see [development storage](development-storage.md).

`issues.jsonl` is the inbox and Tau only ever appends to it. Reports that have been dealt with are moved by hand to `done.jsonl` beside it, keeping their original fields and gaining `resolvedAt`, a `resolution` of `fixed`, `deferred` or `not-an-issue`, the `commit` that fixed it where there is one, and a short `note`. Nothing reads either file: the split is a convention for working through reports, so an empty `issues.jsonl` means none are outstanding.

## Optional development OTLP export

External export is disabled by default and no collector is bundled. Build with the opt-in feature:

```sh
cargo run --manifest-path src-tauri/Cargo.toml --features otlp_export
```

Configure a plain HTTP/JSON OTLP endpoint using standard variables:

```sh
OTEL_EXPORTER_OTLP_ENDPOINT=http://127.0.0.1:4318 \
OTEL_EXPORTER_OTLP_PROTOCOL=http/json \
  cargo run --manifest-path src-tauri/Cargo.toml --features otlp_export
```

Signal-specific endpoint and protocol variables are also supported, such as `OTEL_EXPORTER_OTLP_TRACES_ENDPOINT` and `OTEL_EXPORTER_OTLP_TRACES_PROTOCOL`. `OTEL_EXPORTER_OTLP_HEADERS` supplies comma-separated headers. HTTPS, gRPC, and protobuf transport are intentionally unsupported by this lightweight development path; use a local Collector HTTP receiver.

External delivery is best-effort through a bounded background queue. Local persistence remains authoritative and continues if the endpoint is unavailable.
