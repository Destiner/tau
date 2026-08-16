import { describe, expect, it } from 'vitest';

import { formatTraceContext, parseTraceContext } from './trace-context';

const VALID = '00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01';

describe('parseTraceContext', () => {
  it('parses a valid traceparent', () => {
    const result = parseTraceContext(VALID);
    expect(result).toEqual({
      ok: true,
      context: {
        traceId: '4bf92f3577b34da6a3ce929d0e0e4736',
        spanId: '00f067aa0ba902b7',
        sampled: true,
      },
    });
  });

  it('rejects an all-zero trace id', () => {
    const value = '00-00000000000000000000000000000000-00f067aa0ba902b7-01';
    expect(parseTraceContext(value)).toEqual({ ok: false, error: 'trace-id' });
  });

  it('rejects an all-zero span id', () => {
    const value = '00-4bf92f3577b34da6a3ce929d0e0e4736-0000000000000000-01';
    expect(parseTraceContext(value)).toEqual({ ok: false, error: 'span-id' });
  });

  it('rejects the wrong number of segments', () => {
    expect(
      parseTraceContext('00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7'),
    ).toEqual({ ok: false, error: 'format' });
    expect(parseTraceContext(`${VALID}-extra`)).toEqual({
      ok: false,
      error: 'format',
    });
  });

  it('rejects an unsupported version', () => {
    const value = 'ff-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01';
    expect(parseTraceContext(value)).toEqual({ ok: false, error: 'version' });
  });

  it('rejects uppercase hex', () => {
    const value = '00-4BF92F3577B34DA6A3CE929D0E0E4736-00f067aa0ba902b7-01';
    expect(parseTraceContext(value)).toEqual({ ok: false, error: 'trace-id' });
  });
});

describe('formatTraceContext', () => {
  it('round-trips a parsed context', () => {
    const result = parseTraceContext(VALID);
    if (!result.ok) throw new Error('expected a valid traceparent');
    expect(formatTraceContext(result.context)).toBe(VALID);
  });

  it('round-trips the unsampled flag', () => {
    const unsampled = '00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-00';
    const result = parseTraceContext(unsampled);
    if (!result.ok) throw new Error('expected a valid traceparent');
    expect(result.context.sampled).toBe(false);
    expect(formatTraceContext(result.context)).toBe(unsampled);
  });
});
