# Local observability

Tau continuously writes content-free OpenTelemetry JSONL under its profile data directory:

- development: `tau-dev/telemetry/`
- production: `tau/telemetry/`
- signals: `traces.jsonl`, `logs.jsonl`, and `metrics.jsonl`, plus rotated segments

Retention is bounded to seven days and 32 MiB. Telemetry failure does not fail product operations. Records contain operation names, lifecycle state, timings, outcomes, counts, sanitized source basenames, and relevant session/runtime identifiers. They do not contain prompts, transcripts, drafts, tool or extension payloads, file/clipboard contents, paths, SSH commands, connection strings, stderr, credentials, or raw errors.

## User issue reports

Reports submitted from the sidebar are stored separately from content-free telemetry:

- development: `tau-dev/feedback/issues.jsonl`
- production: `tau/feedback/issues.jsonl`

Each JSON line contains the user's description, its submission time, and the current Pi session ID only when the user explicitly includes it. Reports stay local and are retained until the file is removed.

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
