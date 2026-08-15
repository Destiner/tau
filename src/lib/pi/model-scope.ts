import type { ModelOption } from "../../types";

const thinkingLevels = new Set([
  "off",
  "minimal",
  "low",
  "medium",
  "high",
  "xhigh",
  "max",
]);

const globCharacters = /[*?[]/;
const datedVersion = /-\d{8}$/;
const regexpCharacters = /[.*+?^${}()|[\]\\]/g;

/**
 * Narrows the model catalogue to the scope Pi resolves from its `enabledModels`
 * setting, which is the same set `/scoped-models` edits and `/model` lists.
 * Patterns that match nothing leave the catalogue untouched, so a stale setting
 * never empties the picker.
 */
export function scopeModels(
  models: ModelOption[],
  patterns: string[],
): ModelOption[] {
  const scoped: ModelOption[] = [];
  for (const pattern of patterns) {
    for (const model of matchPattern(models, pattern.trim())) {
      if (!scoped.includes(model)) scoped.push(model);
    }
  }
  return scoped.length > 0 ? scoped : models;
}

function matchPattern(models: ModelOption[], pattern: string): ModelOption[] {
  if (!pattern) return [];
  if (!globCharacters.test(pattern)) {
    const model = matchReference(models, pattern);
    return model ? [model] : [];
  }

  const glob = withoutThinkingLevel(pattern);
  const exact = matchExactReference(models, glob);
  if (exact) return [exact];
  const expression = globExpression(glob);
  return models.filter(
    (model) =>
      expression.test(`${model.provider}/${model.id}`) ||
      expression.test(model.id),
  );
}

/**
 * Resolves one non-glob pattern the way Pi does: an exact reference first, then
 * a partial match that prefers an alias over a dated release. A trailing
 * `:<thinking level>` is only stripped once the full pattern fails to match,
 * because model ids may themselves contain colons.
 */
function matchReference(
  models: ModelOption[],
  pattern: string,
): ModelOption | undefined {
  const exact = matchExactReference(models, pattern);
  if (exact) return exact;

  const needle = pattern.toLowerCase();
  const matches = models.filter(
    (model) =>
      model.id.toLowerCase().includes(needle) ||
      model.name.toLowerCase().includes(needle),
  );
  if (matches.length > 0) {
    const aliases = matches.filter((model) => isAlias(model.id));
    const ranked = aliases.length > 0 ? aliases : matches;
    return [...ranked].sort((first, second) =>
      second.id.localeCompare(first.id),
    )[0];
  }

  const colon = pattern.lastIndexOf(":");
  return colon === -1
    ? undefined
    : matchReference(models, pattern.slice(0, colon));
}

/** Matches `provider/id` or a bare id, rejecting references shared by providers. */
function matchExactReference(
  models: ModelOption[],
  reference: string,
): ModelOption | undefined {
  const trimmed = reference.trim();
  if (!trimmed) return undefined;
  const normalized = trimmed.toLowerCase();

  const canonical = models.filter(
    (model) => `${model.provider}/${model.id}`.toLowerCase() === normalized,
  );
  if (canonical.length > 0) return single(canonical);

  const slash = trimmed.indexOf("/");
  if (slash !== -1) {
    const provider = trimmed.slice(0, slash).trim().toLowerCase();
    const id = trimmed
      .slice(slash + 1)
      .trim()
      .toLowerCase();
    if (provider && id) {
      const matches = models.filter(
        (model) =>
          model.provider.toLowerCase() === provider &&
          model.id.toLowerCase() === id,
      );
      if (matches.length > 0) return single(matches);
    }
  }

  return single(
    models.filter((model) => model.id.toLowerCase() === normalized),
  );
}

function single(models: ModelOption[]): ModelOption | undefined {
  return models.length === 1 ? models[0] : undefined;
}

function withoutThinkingLevel(pattern: string): string {
  const colon = pattern.lastIndexOf(":");
  if (colon === -1) return pattern;
  return thinkingLevels.has(pattern.slice(colon + 1))
    ? pattern.slice(0, colon)
    : pattern;
}

function isAlias(id: string): boolean {
  return id.endsWith("-latest") || !datedVersion.test(id);
}

/** `*` and `?` stop at provider boundaries, `**` crosses them, as in Pi. */
function globExpression(pattern: string): RegExp {
  let source = "";
  for (let index = 0; index < pattern.length; index += 1) {
    const character = pattern[index];
    if (character === "*") {
      const crossesProviders = pattern[index + 1] === "*";
      if (crossesProviders) index += 1;
      source += crossesProviders ? ".*" : "[^/]*";
      continue;
    }
    if (character === "?") {
      source += "[^/]";
      continue;
    }
    if (character === "[") {
      const end = pattern.indexOf("]", index + 1);
      if (end !== -1) {
        source += `[${pattern.slice(index + 1, end).replace(/^!/, "^")}]`;
        index = end;
        continue;
      }
    }
    source += character.replace(regexpCharacters, "\\$&");
  }
  return new RegExp(`^${source}$`, "i");
}
