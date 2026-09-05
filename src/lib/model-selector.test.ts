import { describe, expect, it } from 'vitest';

import {
  filterModels,
  flattenModelGroups,
  groupModels,
  providerLabel,
} from './model-selector';
import type { ModelOption } from './pi/model-scope';

const models: ModelOption[] = [
  {
    provider: 'openai-codex',
    id: 'gpt-5.6-sol',
    name: 'GPT-5.6 Sol',
    reasoning: true,
  },
  {
    provider: 'openrouter',
    id: 'deepseek/deepseek-v4-flash-0731',
    name: 'DeepSeek V4 Flash',
    reasoning: true,
  },
  {
    provider: 'pi-claude',
    id: 'claude-opus-5',
    name: 'Claude Opus 5',
    reasoning: true,
  },
  {
    provider: 'openai-codex',
    id: 'gpt-6-astra',
    name: 'GPT-6 Astra',
    reasoning: true,
  },
];

describe('model selector catalogue', () => {
  it('filters by model name and id but not provider', () => {
    expect(filterModels(models, 'astra')).toEqual([models[3]]);
    expect(filterModels(models, 'deepseek-v4')).toEqual([models[1]]);
    expect(filterModels(models, 'openai-codex')).toEqual([]);
  });

  it('groups providers without changing the rendered traversal order', () => {
    const groups = groupModels(models);

    expect(groups.map((group) => group.provider)).toEqual([
      'openai-codex',
      'openrouter',
      'pi-claude',
    ]);
    expect(
      flattenModelGroups(groups).map(
        (model) => `${model.provider}/${model.id}`,
      ),
    ).toEqual([
      'openai-codex/gpt-5.6-sol',
      'openai-codex/gpt-6-astra',
      'openrouter/deepseek/deepseek-v4-flash-0731',
      'pi-claude/claude-opus-5',
    ]);
  });

  it('uses reviewed provider names and a readable fallback', () => {
    expect(providerLabel('openai-codex')).toBe('OpenAI Codex');
    expect(providerLabel('team-models')).toBe('Team Models');
  });
});
