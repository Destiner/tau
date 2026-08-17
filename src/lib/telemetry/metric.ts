/*
 * The frontend metric transport: a Stage 5 addition for the two raw
 * measurements that exist only on the frontend and have no native span/log
 * counterpart to derive a metric from (event-loop lag, long-task duration).
 * Every other Stage 5 metric (invoke/RPC/action duration histograms, RPC
 * failure counts, process start/exit counts, queue/pending gauges) is
 * derived natively from an already-ingested span or log instead — see
 * `src-tauri/src/telemetry/ingest.rs`'s `record_span_metrics`/
 * `record_log_metrics`. This file only defines the wire shape for the two
 * that cannot be.
 */

/** The wire shape a raw frontend metric value is sent to Rust as, mirroring
 * `src-tauri/src/telemetry/ingest.rs`'s `FrontendMetricRecord`. `family`
 * selects the native instrument directly (`ingest.rs`'s `record_metric`),
 * the same way a log record's `family` selects its fixed event name and
 * severity. Structurally disjoint from both a span record (no `traceId`/
 * `spanId`/`sampled`/`startTimeUnixNano`/`endTimeUnixNano`) and a log record
 * (`value` is required here and rejected by a log record's
 * `deny_unknown_fields`), so `ingest_one` can try all three shapes in order
 * without an explicit discriminant field. */
interface FrontendMetricRecord {
  family: string;
  value: number;
  timeUnixNano: string;
  attributes: Record<string, string | number>;
}

/** Hard upper bound on a reported metric value (24 hours in milliseconds).
 * Neither an event-loop-lag reading nor a long-task duration can genuinely
 * reach this; it exists only to reject a corrupted or nonsensical value
 * before it crosses IPC, matching the catalog's other hard bounds. */
const MAX_METRIC_VALUE_MS = 24 * 60 * 60 * 1000;

export type { FrontendMetricRecord };
export { MAX_METRIC_VALUE_MS };
