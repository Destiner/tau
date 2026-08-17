/*
 * Drains the bounded queue and hands each batch to the native ingest
 * command. Every failure here is swallowed: a broken exporter must never be
 * visible to the product, per "telemetry failure must never fail a user
 * operation." This module must never itself be span-instrumented — doing so
 * would enqueue a record about sending a batch, which would only reach disk
 * by calling this same command again, recursing forever.
 */
import { invoke } from '@tauri-apps/api/core';

import type { BoundedQueue } from './queue';
import type { FrontendQueueRecord } from './tracer';

/** Keeps a single native call small; the queue's own bound is the real
 * backstop against unbounded growth. */
const MAX_BATCH_SIZE = 20;
/** Coalesces spans that finish within a short window into one native call
 * instead of one call per span. */
const FLUSH_DELAY_MS = 50;

async function sendBatch(records: FrontendQueueRecord[]): Promise<void> {
  try {
    await invoke('ingest_telemetry', { records });
  } catch {
    // Swallowed intentionally: the frontend exporter must have no
    // product-visible effect.
  }
}

async function flushAll(
  queue: BoundedQueue<FrontendQueueRecord>,
): Promise<void> {
  for (;;) {
    const batch = queue.drain(MAX_BATCH_SIZE);
    if (batch.length === 0) return;
    await sendBatch(batch);
  }
}

function createFlushScheduler(queue: BoundedQueue<FrontendQueueRecord>): {
  scheduleFlush: () => void;
  flushNow: () => Promise<void>;
} {
  let timer: ReturnType<typeof setTimeout> | undefined;
  let inFlight: Promise<void> | undefined;

  function flushNow(): Promise<void> {
    if (timer !== undefined) {
      clearTimeout(timer);
      timer = undefined;
    }
    if (!inFlight) {
      inFlight = flushAll(queue).finally(() => {
        inFlight = undefined;
        if (queue.length > 0) scheduleFlush();
      });
    }
    return inFlight;
  }

  function scheduleFlush(): void {
    if (timer !== undefined) return;
    timer = setTimeout(() => {
      timer = undefined;
      void flushNow();
    }, FLUSH_DELAY_MS);
  }

  return { scheduleFlush, flushNow };
}

export { createFlushScheduler, MAX_BATCH_SIZE };
