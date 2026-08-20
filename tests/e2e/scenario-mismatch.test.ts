import { describe, expect, it } from 'vitest';

import formatScenarioMismatch from './scenario-mismatch';

const timeline = [
  { sequence: 1, kind: 'request', request: { type: 'get_state' } },
  { sequence: 2, kind: 'output', output: 'response main.state' },
];

describe('formatScenarioMismatch', () => {
  it('does not duplicate an engine error timeline', () => {
    const error = `scenario: unexpected request\nTimeline:\n001 first\n002 second`;

    const result = formatScenarioMismatch(error, timeline);

    expect(result).toBe(`Pi scenario mismatch:\n${error}`);
    expect(result.match(/^Timeline:/gm)).toHaveLength(1);
  });

  it('appends the ordered scenario timeline to adapter errors', () => {
    const result = formatScenarioMismatch(
      'register_session invocation count: expected 2, received 1.',
      timeline,
    );

    expect(result).toBe(
      `Pi scenario mismatch:\nregister_session invocation count: expected 2, received 1.\nTimeline:\n${JSON.stringify(timeline[0])}\n${JSON.stringify(timeline[1])}`,
    );
  });

  it('includes the ordered scenario timeline for unknown errors', () => {
    const result = formatScenarioMismatch(undefined, timeline);

    expect(result).toContain('Unknown scenario failure.');
    expect(result.indexOf(JSON.stringify(timeline[0]))).toBeLessThan(
      result.indexOf(JSON.stringify(timeline[1])),
    );
    expect(result.match(/^Timeline:/gm)).toHaveLength(1);
  });
});
