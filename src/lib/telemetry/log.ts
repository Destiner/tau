/*
 * The frontend log transport: a Stage 4 addition alongside the span
 * transport `tracer.ts` already provides. Logs need no trace/span
 * identity — they are point-in-time events with a fixed, native-assigned
 * event name and severity per family (see `fixedLogMetadata` in
 * `src-tauri/src/telemetry/ingest.rs`) — so this file only defines the wire
 * shape and a timestamp encoder, not an OTel SDK integration.
 */

/** The wire shape a frontend log record is sent to Rust as, mirroring
 * `src-tauri/src/telemetry/ingest.rs`'s `FrontendLogRecord`. No event name
 * or severity: native code assigns both from `family` alone, so an
 * untrusted caller can never choose either. */
interface FrontendLogRecord {
  family: string;
  timeUnixNano: string;
  attributes: Record<string, string | number>;
}

/** Encodes the current time as decimal nanoseconds since the epoch, built
 * from `Date.now()`'s millisecond precision via string concatenation. A
 * plain multiplication (`Date.now() * 1e6`) would round-trip through a JS
 * `number`, which cannot represent an epoch nanosecond timestamp without
 * losing precision — the same reason `tracer.ts`'s `hrTimeToNanosString`
 * avoids one. */
function nowUnixNanoString(): string {
  const millis = Date.now();
  const seconds = Math.floor(millis / 1000);
  const nanosRemainder = (millis % 1000) * 1_000_000;
  return `${seconds}${String(nanosRemainder).padStart(9, '0')}`;
}

export type { FrontendLogRecord };
export { nowUnixNanoString };
