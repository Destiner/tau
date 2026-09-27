import { describe, expect, it } from 'vitest';

import { formatTraceContext, parseTraceContext } from './trace-context';

const VALID = '00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01';

describe('parseTraceContext', () => {
  it('parses valid context and rejects invalid identifiers and syntax', () => {
    const result = parseTraceContext(VALID);
    expect(result).toEqual({
      ok: true,
      context: {
        traceId: '4bf92f3577b34da6a3ce929d0e0e4736',
        spanId: '00f067aa0ba902b7',
        sampled: true,
      },
    });
    expect(
      parseTraceContext(
        '00-00000000000000000000000000000000-00f067aa0ba902b7-01',
      ),
    ).toEqual({
      ok: false,
      error: 'trace-id',
    });
    expect(
      parseTraceContext(
        '00-4bf92f3577b34da6a3ce929d0e0e4736-0000000000000000-01',
      ),
    ).toEqual({
      ok: false,
      error: 'span-id',
    });
    expect(
      parseTraceContext('00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7'),
    ).toEqual({ ok: false, error: 'format' });
    expect(parseTraceContext(`${VALID}-extra`)).toEqual({
      ok: false,
      error: 'format',
    });
    expect(
      parseTraceContext(
        'ff-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01',
      ),
    ).toEqual({
      ok: false,
      error: 'version',
    });
    expect(
      parseTraceContext(
        '00-4BF92F3577B34DA6A3CE929D0E0E4736-00f067aa0ba902b7-01',
      ),
    ).toEqual({
      ok: false,
      error: 'trace-id',
    });
  });
});

describe('formatTraceContext', () => {
  it('round-trips sampled and unsampled flags', () => {
    const result = parseTraceContext(VALID);
    if (!result.ok) throw new Error('expected a valid traceparent');
    expect(formatTraceContext(result.context)).toBe(VALID);
    const unsampled = '00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-00';
    const unsampledResult = parseTraceContext(unsampled);
    if (!unsampledResult.ok) throw new Error('expected a valid traceparent');
    expect(unsampledResult.context.sampled).toBe(false);
    expect(formatTraceContext(unsampledResult.context)).toBe(unsampled);
  });
});
