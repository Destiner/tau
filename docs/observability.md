# Cloud observability and feedback

## Admin mode

Telemetry and the issue reporter are off in an ordinary run. Typing `iddqd` outside a text field unlocks both; the choice is persisted in native `preferences.json`. Admin mode is also enforced by every native cloud command, not just by hiding the reporter.

Admin mode alone does not collect telemetry. A configured Grafana connection is also required. The frontend starts its heartbeat and long-task observer only after the native side confirms effective telemetry is enabled. Disabling admin mode stops collection and discards queued records. Disconnecting Grafana does the same without disabling the issue reporter. Requests already in flight may finish; no new queued delivery crosses a disable, disconnect, or connection-change boundary.

There are no local telemetry segments, run markers, report files, or persistent upload queues. Old `telemetry/` and `feedback/` files are neither read, migrated, nor deleted. Their presence does not indicate new collection. There is no automatic historical backfill or unclean-exit detection based on a local marker.

## One-time setup

In admin mode, open **Report an Issue** in the sidebar footer:

1. Enter a **Linear personal API key** and connect. The key needs access to read teams, projects, and issues, and to create issues. Choose a team and optionally a project, then save the destination. Destination discovery currently supports up to 100 teams and 100 projects per team; larger responses fail explicitly rather than silently hiding options.
2. Optionally connect **Grafana Cloud** using its OpenTelemetry connection tile's base OTLP endpoint, numeric instance ID, and token. For example, the endpoint has the form `https://otlp-gateway-<region>.grafana.net/otlp`. Use your stack's actual values, not a guessed hostname. The token needs `logs:write`, `traces:write`, and `metrics:write`, scoped to the intended stack. Do not enter signal-specific `/v1/logs`, `/v1/traces`, or `/v1/metrics` URLs. Setup validates all three ingestion endpoints without requiring read/query permissions.
3. Submit the report. Grafana is optional and its setup/export failures never prevent submitting to Linear.

Subsequent openings go straight to the report. **Connections** inside the reporter is the only configuration surface. The **×** beside a provider removes its configuration and credential; its entry fields become empty. It does not affect the other provider or the report draft. No connection controls appear outside admin mode, and successful reporting closes the popover without a banner.

Grafana must be connected before reproducing a problem to capture its diagnostics. Connecting while reporting cannot recover earlier telemetry.

## Configuration and credentials

Only nonsecret provider configuration and immutable Keychain entry references are written to `cloud-connections.json` in the native profile. Production macOS credentials are stored in Keychain under the `app.tau.cloud` service, never in preferences, the webview's persistent storage, or telemetry. Setup/network operations and Keychain access run natively; network requests use HTTPS, bounded response sizes and timeouts, and do not follow redirects carrying credentials. Provider errors are mapped to fixed, reviewed categories rather than displaying their raw responses.

Development profiles use in-memory credentials and disposable configuration; they never read or modify production Keychain entries. A native development restart requires reconnecting. Browser fixtures use invented in-memory connections with no external requests. Production secure credential storage is currently implemented for macOS; other platforms reject setup rather than persisting plaintext secrets.

Agent-side Linear/Grafana MCP authentication is separate. Tau's ingestion tokens are not shared with the coding sandbox. Prefer read-only Grafana access for investigation and grant only the Linear access needed for the triage workflow.

## Issue reports

Reports go directly to Linear. Each contains the description, a stable report UUID, the original timestamp in UTC and Unix nanoseconds, Tau version, environment, and the Pi session ID only when explicitly included. The description stays separate from telemetry. The generated metadata footer starts with `Tau issue report` so the triage workflow can identify these reports in the selected destination.

Submission is online-only. A failure preserves the draft in memory for retry; closing the popover, editing connections, or toggling admin mode does not discard it. There is no disk outbox or report-draft persistence across app restarts. Successful submission clears the draft. Retrying an unchanged report reuses its UUID and original session context; Linear is queried for that UUID before creating it, including archived issues. Matching metadata and destination are required before an existing issue counts as delivered. A timed-out create is not blindly retried.

Linear owns issue status, findings, and resolution. Tau neither mirrors that state locally nor automatically closes issues. A merged fix is not the same as a user-confirmed resolution; see `.agents/skills/triage/SKILL.md`.

## Telemetry transport and privacy

Tau sends the existing privacy-validated OpenTelemetry mapping directly to Grafana's HTTPS OTLP endpoints using HTTP/JSON with Basic authentication. Grafana supports this encoding for low-volume usage; no collector is required. The previous development-only plain-HTTP exporter and `OTEL_EXPORTER_OTLP_*` configuration path have been removed.

Delivery is best-effort and memory-only: at most 256 queued records, each at most 64 KiB, drained in batches of up to 32. Native requests have an eight-second overall timeout. Full queues, failed delivery, shutdown, and process crashes may lose telemetry; there is no offline replay. Metric aggregation is reset across destination/admin epochs, and stale spans or frontend records cannot leak into a newly enabled connection. Cloud retention and usage limits are controlled by the Grafana plan, not by Tau.

Records contain operation names, lifecycle state, timings, outcomes, counts, sanitized source basenames, and reviewed session/runtime identifiers. Pi frontend ownership records distinguish initial claims, replacements, rejected claims, and cleanup failures, including only the stale direct-child count. Children stopped during replacement have a matching `pi.process.stopped` record with the fixed `ownership_replaced` reason.

Records never contain prompts, transcripts, drafts, tool or extension payloads, report descriptions, file/clipboard contents, paths, SSH commands, connection strings, stderr, credentials, owner tokens, or raw errors. Session/runtime IDs are log/span context, not high-cardinality metric labels. External export goes through the same native allowlist validation as frontend ingestion.

## Action milestones

`startActionMilestones(name, scope?)` from `src/lib/telemetry` returns `{ span, mark, afterRender, cancel }`. Use `span.context` for child invokes/RPCs and end `span` as usual. Selection/new-session actions watch the originating controller for `ready`, `readable_memory`/`readable_saved`/`readable_rpc`, and `hydrated`; they mark `persisted` only when their selection write settles successfully, or `persistence_failed` on failure. Superseded views cancel pending measurements, including frames and delayed writes. After applying the UI mutation, `await nextTick(); milestones.afterRender()` schedules two animation frames. Do not await frames to show a hidden window. Call `cancel()` on replacement/unmount/failure to discard pending frame work; it does not end the action span.

A milestone is a once-only, content-free `action.milestone` log linked to the action's trace/span, with allowlisted action name, fixed milestone kind (`ready`, `persisted`, `persistence_failed`, `readable_memory`, `readable_saved`, `readable_rpc`, `hydrated`, `paint_opportunity`, `paint_unavailable`), bounded elapsed milliseconds and optional reviewed scope IDs. `paint_opportunity` is **not** proof of compositing or pixels on screen. Hidden windows and a five-second frame timeout report `paint_unavailable`, never a fabricated paint. Disabling telemetry invalidates pending milestones; without effective telemetry they record nothing and schedule no frames. Missing readiness/persistence markers are not success signals: emit them only at the confirmed boundary. No free-form labels, paths, or content may be passed as dimensions.

## Provider references

- [Linear API authentication, GraphQL errors, and issue creation](https://linear.app/developers/graphql)
- [Grafana Cloud OTLP connection setup](https://grafana.com/docs/grafana-cloud/send-data/otlp/send-data-otlp/)
- [Grafana OTLP encoding and field mapping](https://grafana.com/docs/grafana-cloud/send-data/otlp/otlp-format-considerations/)
