import { describe, expect, it } from 'vitest';

import type { ModelOption } from './model-scope';
import { scopeModels } from './model-scope';

function model(provider: string, id: string, name = id): ModelOption {
  return { provider, id, name, reasoning: false };
}

const models: ModelOption[] = [
  model('anthropic', 'claude-opus-4-8', 'Claude Opus 4.8'),
  model('anthropic', 'claude-sonnet-4-5', 'Claude Sonnet 4.5'),
  model('anthropic', 'claude-sonnet-4-5-20250929', 'Claude Sonnet 4.5'),
  model('openai', 'gpt-5.5', 'GPT-5.5'),
  model('openrouter', 'deepseek/deepseek-v4-pro', 'DeepSeek V4 Pro'),
];

const ids = (scoped: ModelOption[]): string[] =>
  scoped.map((option) => `${option.provider}/${option.id}`);

describe('model scope', () => {
  it('keeps the catalogue when nothing is scoped', () => {
    expect(scopeModels(models, [])).toEqual(models);
  });

  it('resolves canonical references in the configured order', () => {
    expect(
      ids(scopeModels(models, ['openai/gpt-5.5', 'anthropic/claude-opus-4-8'])),
    ).toEqual(['openai/gpt-5.5', 'anthropic/claude-opus-4-8']);
  });

  it('resolves bare ids and drops duplicates', () => {
    expect(ids(scopeModels(models, ['gpt-5.5', 'openai/gpt-5.5']))).toEqual([
      'openai/gpt-5.5',
    ]);
  });

  it('ignores a trailing thinking level', () => {
    expect(ids(scopeModels(models, ['openai/gpt-5.5:high']))).toEqual([
      'openai/gpt-5.5',
    ]);
    expect(ids(scopeModels(models, ['anthropic/*:medium']))).toEqual([
      'anthropic/claude-opus-4-8',
      'anthropic/claude-sonnet-4-5',
      'anthropic/claude-sonnet-4-5-20250929',
    ]);
  });

  it('prefers an alias over a dated release when matching partially', () => {
    expect(ids(scopeModels(models, ['sonnet']))).toEqual([
      'anthropic/claude-sonnet-4-5',
    ]);
  });

  it('matches globs against both the reference and the bare id', () => {
    expect(ids(scopeModels(models, ['*sonnet*']))).toEqual([
      'anthropic/claude-sonnet-4-5',
      'anthropic/claude-sonnet-4-5-20250929',
    ]);
  });

  it('stops single-star globs at the provider boundary', () => {
    expect(ids(scopeModels(models, ['openai/*', 'openrouter/*']))).toEqual([
      'openai/gpt-5.5',
    ]);
    expect(ids(scopeModels(models, ['openrouter/**']))).toEqual([
      'openrouter/deepseek/deepseek-v4-pro',
    ]);
  });

  it('keeps the catalogue when no pattern matches', () => {
    expect(scopeModels(models, ['mistral/*', '  '])).toEqual(models);
  });
});
