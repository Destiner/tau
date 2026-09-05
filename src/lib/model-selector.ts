import type { ModelOption } from './pi/model-scope';

interface ModelGroup {
  provider: string;
  label: string;
  models: ModelOption[];
}

const providerLabels: Record<string, string> = {
  'amazon-bedrock': 'Amazon Bedrock',
  'github-copilot': 'GitHub Copilot',
  'google-vertex': 'Google Vertex',
  'openai-codex': 'OpenAI Codex',
  opencode: 'OpenCode',
  'opencode-go': 'OpenCode Go',
  openrouter: 'OpenRouter',
  'pi-claude': 'Pi Claude',
  xai: 'xAI',
};

function filterModels(models: ModelOption[], query: string): ModelOption[] {
  const normalized = query.trim().toLocaleLowerCase();
  if (!normalized) return models;
  return models.filter((model) =>
    `${model.name} ${model.id}`.toLocaleLowerCase().includes(normalized),
  );
}

/** Preserves first-seen provider order and source order within each provider. */
function groupModels(models: ModelOption[], query = ''): ModelGroup[] {
  const groups = new Map<string, ModelOption[]>();
  for (const model of filterModels(models, query)) {
    const group = groups.get(model.provider) ?? [];
    group.push(model);
    groups.set(model.provider, group);
  }
  return [...groups].map(([provider, groupedModels]) => ({
    provider,
    label: providerLabel(provider),
    models: groupedModels,
  }));
}

function flattenModelGroups(groups: ModelGroup[]): ModelOption[] {
  return groups.flatMap((group) => group.models);
}

function providerLabel(provider: string): string {
  return (
    providerLabels[provider] ??
    provider
      .replaceAll('-', ' ')
      .replace(/\b\w/g, (letter) => letter.toUpperCase())
  );
}

export type { ModelGroup };

export { filterModels, flattenModelGroups, groupModels, providerLabel };
