import { ROOT_CONTEXT } from '@opentelemetry/api';
import { describe, expect, it } from 'vitest';

import { createBoundedQueue } from './queue';
import {
  createTracer,
  hrTimeToNanosString,
  type FrontendSpanRecord,
} from './tracer';

describe('hrTimeToNanosString', () => {
  it('combines seconds and nanoseconds without losing precision', () => {
    expect(hrTimeToNanosString([1, 5])).toBe('1000000005');
    expect(hrTimeToNanosString([1700000000, 123456789])).toBe(
      '1700000000123456789',
    );
  });
});

describe('createTracer', () => {
  it('exports a root span with only catalog-approved attributes', () => {
    const queue = createBoundedQueue<FrontendSpanRecord>(10);
    const tracer = createTracer(queue);

    const span = tracer.startSpan('tauri.invoke', undefined, ROOT_CONTEXT);
    span.setAttribute('tau.invoke.command', 'load_workspace');
    span.setAttribute('does.not.exist', 'value');
    span.end();

    expect(queue.length).toBe(1);
    const [record] = queue.drain(1);
    expect(record?.family).toBe('tauri.invoke');
    expect(record?.traceId).toMatch(/^[0-9a-f]{32}$/);
    expect(record?.spanId).toMatch(/^[0-9a-f]{16}$/);
    expect(record?.parentSpanId).toBeUndefined();
    expect(record?.attributes).toEqual({
      'tau.invoke.command': 'load_workspace',
    });
    expect(Number(record?.startTimeUnixNano)).toBeGreaterThan(0);
    expect(Number(record?.endTimeUnixNano)).toBeGreaterThanOrEqual(
      Number(record?.startTimeUnixNano),
    );
  });

  it('drops an oversized attribute value', () => {
    const queue = createBoundedQueue<FrontendSpanRecord>(10);
    const tracer = createTracer(queue);

    const span = tracer.startSpan('tauri.invoke', undefined, ROOT_CONTEXT);
    span.setAttribute('tau.invoke.command', 'x'.repeat(129));
    span.end();

    const [record] = queue.drain(1);
    expect(record?.attributes).toEqual({});
  });
});
