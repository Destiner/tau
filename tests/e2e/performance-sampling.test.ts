import { describe, expect, it } from 'vitest';

import confirmedSlowFrameRatio from './performance-sampling';

describe('slow-frame confirmation', () => {
  it('accepts a passing first sweep without repeating it', () => {
    expect(confirmedSlowFrameRatio([0.08], 0.12)).toBe(0.08);
  });

  it('does not turn one scheduling outlier into a regression', () => {
    expect(confirmedSlowFrameRatio([0.15, 0.06, 0.08], 0.12)).toBe(0.08);
  });

  it('retains the guard when the slowdown repeats', () => {
    expect(confirmedSlowFrameRatio([0.15, 0.06, 0.14], 0.12)).toBe(0.14);
  });

  it('requires confirmation of a failing first sweep', () => {
    expect(() => confirmedSlowFrameRatio([0.15], 0.12)).toThrow();
  });
});
