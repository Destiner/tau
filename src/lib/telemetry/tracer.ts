/*
 * The Tau-owned tracer: a minimal `TracerProvider` exporting into a bounded
 * queue instead of over the network. This is the only file that touches
 * `@opentelemetry/sdk-trace` directly; product code calls `./index`'s
 * `startCommandSpan` instead, keeping browser SDK usage behind one adapter
 * per the design principles.
 */
import { TraceFlags, type Tracer } from '@opentelemetry/api';
import type { ExportResult } from '@opentelemetry/core';
import { ExportResultCode } from '@opentelemetry/core';
import {
  AlwaysOnSampler,
  SimpleSpanProcessor,
  TracerProvider,
  type ReadableSpan,
  type SpanExporter,
} from '@opentelemetry/sdk-trace';

import { validateAttribute } from './attributes';
import type { BoundedQueue } from './queue';

/** The wire shape a frontend span is sent to Rust as, mirroring
 * src-tauri/src/telemetry/ingest.rs's `FrontendSpanRecord`. Only string and
 * integer attribute values are allowed, matching the Stage 0 catalog; Rust
 * revalidates every field before persisting it, so nothing here needs to be
 * trusted on its own. */
interface FrontendSpanRecord {
  family: string;
  traceId: string;
  spanId: string;
  parentSpanId?: string;
  sampled: boolean;
  startTimeUnixNano: string;
  endTimeUnixNano: string;
  attributes: Record<string, string | number>;
}

/** Converts an `[seconds, nanoseconds]` `HrTime` to a decimal
 * nanoseconds-since-epoch string without going through a JS `number`, which
 * cannot represent a nanosecond epoch timestamp without losing precision. */
function hrTimeToNanosString(time: readonly [number, number]): string {
  const [seconds, nanoseconds] = time;
  return `${seconds}${nanoseconds.toString().padStart(9, '0')}`;
}

/** Drops an attribute the catalog does not allow for this span's family
 * rather than rejecting the whole span; native re-validates regardless. */
function sanitizedAttributes(
  family: string,
  attributes: ReadableSpan['attributes'],
): Record<string, string | number> {
  const sanitized: Record<string, string | number> = {};
  for (const [key, value] of Object.entries(attributes)) {
    if (typeof value !== 'string' && typeof value !== 'number') continue;
    if (!validateAttribute(family, key, value).valid) continue;
    sanitized[key] = value;
  }
  return sanitized;
}

function toFrontendSpanRecord(span: ReadableSpan): FrontendSpanRecord {
  const context = span.spanContext();
  return {
    family: span.name,
    traceId: context.traceId,
    spanId: context.spanId,
    parentSpanId: span.parentSpanContext?.spanId,
    sampled: (context.traceFlags & TraceFlags.SAMPLED) !== 0,
    startTimeUnixNano: hrTimeToNanosString(span.startTime),
    endTimeUnixNano: hrTimeToNanosString(span.endTime),
    attributes: sanitizedAttributes(span.name, span.attributes),
  };
}

/** An exporter that only ever enqueues; it never touches Tauri IPC itself,
 * so `initTelemetry`/`startCommandSpan` cannot recurse into the ingest
 * command through this path. */
function createQueueExporter(
  queue: BoundedQueue<FrontendSpanRecord>,
): SpanExporter {
  return {
    export(spans, resultCallback: (result: ExportResult) => void): void {
      for (const span of spans) {
        queue.push(toFrontendSpanRecord(span));
      }
      resultCallback({ code: ExportResultCode.SUCCESS });
    },
    shutdown(): Promise<void> {
      return Promise.resolve();
    },
  };
}

/** Builds a tracer that exports every span (no sampling, per the design
 * principles) into `queue`. */
function createTracer(queue: BoundedQueue<FrontendSpanRecord>): Tracer {
  const provider = new TracerProvider({
    sampler: new AlwaysOnSampler(),
    spanProcessors: [
      new SimpleSpanProcessor({ exporter: createQueueExporter(queue) }),
    ],
  });
  return provider.getTracer('tau');
}

export type { FrontendSpanRecord };

export { createTracer, hrTimeToNanosString };
