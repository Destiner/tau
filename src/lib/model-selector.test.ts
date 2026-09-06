import { describe, expect, it } from 'vitest';

import providerLabel from './model-selector';

describe('model selector catalogue', () => {
  it('uses reviewed provider names and a readable fallback', () => {
    expect(providerLabel('openai-codex')).toBe('OpenAI Codex');
    expect(providerLabel('team-models')).toBe('Team Models');
  });
});
