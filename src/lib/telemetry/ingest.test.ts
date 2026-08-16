import { invoke } from '@tauri-apps/api/core';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { createFlushScheduler, MAX_BATCH_SIZE } from './ingest';
import { createBoundedQueue } from './queue';
import type { FrontendSpanRecord } from './tracer';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(async () => ({ accepted: 0, rejected: 0 })),
}));

function sampleRecord(index: number): FrontendSpanRecord {
  return {
    family: 'tauri.invoke',
    traceId: '4bf92f3577b34da6a3ce929d0e0e4736',
    spanId: index.toString(16).padStart(16, '0'),
    sampled: true,
    startTimeUnixNano: '1000000000',
    endTimeUnixNano: '1000005000',
    attributes: { 'tau.invoke.command': 'load_workspace' },
  };
}

const mockInvoke = vi.mocked(invoke);

beforeEach(() => {
  mockInvoke.mockClear();
  mockInvoke.mockResolvedValue({ accepted: 0, rejected: 0 });
});

describe('flushNow', () => {
  it('sends everything queued, split into batches no larger than the configured max', async () => {
    const queue = createBoundedQueue<FrontendSpanRecord>(200);
    for (let index = 0; index < MAX_BATCH_SIZE + 5; index += 1)
      queue.push(sampleRecord(index));
    const { flushNow } = createFlushScheduler(queue);

    await flushNow();

    expect(queue.length).toBe(0);
    expect(mockInvoke).toHaveBeenCalledTimes(2);
    expect(mockInvoke.mock.calls[0]?.[1]).toMatchObject({
      records: expect.arrayContaining([expect.any(Object)]),
    });
    const [firstBatchArgs, secondBatchArgs] = mockInvoke.mock.calls.map(
      (call) => (call[1] as { records: unknown[] }).records.length,
    );
    expect(firstBatchArgs).toBe(MAX_BATCH_SIZE);
    expect(secondBatchArgs).toBe(5);
  });

  it('serializes concurrent flush requests', async () => {
    let release: (() => void) | undefined;
    mockInvoke.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          release = (): void =>
            resolve({ accepted: MAX_BATCH_SIZE, rejected: 0 });
        }),
    );
    const queue = createBoundedQueue<FrontendSpanRecord>(200);
    for (let index = 1; index <= MAX_BATCH_SIZE + 5; index += 1)
      queue.push(sampleRecord(index));
    const { flushNow } = createFlushScheduler(queue);

    const first = flushNow();
    const second = flushNow();
    expect(first).toBe(second);
    expect(mockInvoke).toHaveBeenCalledTimes(1);

    release?.();
    await first;
    expect(mockInvoke).toHaveBeenCalledTimes(2);
  });

  it('does nothing when the queue is empty', async () => {
    const queue = createBoundedQueue<FrontendSpanRecord>(200);
    const { flushNow } = createFlushScheduler(queue);
    await flushNow();
    expect(mockInvoke).not.toHaveBeenCalled();
  });

  it('never enqueues anything about sending the batch itself', async () => {
    const queue = createBoundedQueue<FrontendSpanRecord>(200);
    queue.push(sampleRecord(0));
    const { flushNow } = createFlushScheduler(queue);

    await flushNow();

    expect(queue.length).toBe(0);
    expect(queue.dropped).toBe(0);
  });

  it('swallows a rejected invoke call so it never surfaces to the caller', async () => {
    mockInvoke.mockRejectedValueOnce(new Error('ipc unavailable'));
    const queue = createBoundedQueue<FrontendSpanRecord>(200);
    queue.push(sampleRecord(0));
    const { flushNow } = createFlushScheduler(queue);

    await expect(flushNow()).resolves.toBeUndefined();
  });
});

describe('scheduleFlush', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('coalesces spans finishing within the delay window into one flush', async () => {
    const queue = createBoundedQueue<FrontendSpanRecord>(200);
    const { scheduleFlush } = createFlushScheduler(queue);

    queue.push(sampleRecord(0));
    scheduleFlush();
    queue.push(sampleRecord(1));
    scheduleFlush();

    await vi.runAllTimersAsync();

    expect(mockInvoke).toHaveBeenCalledTimes(1);
    expect(
      (mockInvoke.mock.calls[0]?.[1] as { records: unknown[] }).records,
    ).toHaveLength(2);
  });
});
