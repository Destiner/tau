import type { CommandOption } from "../types";

export function slashCommandQuery(draft: string): string | null {
  const match = /^\/([^\s/]*)$/.exec(draft);
  return match?.[1] ?? null;
}

export function filterCommands(
  commands: readonly CommandOption[],
  query: string,
): CommandOption[] {
  if (!query) return [...commands];

  return commands
    .map((command, index) => ({
      command,
      index,
      score: fuzzyScore(command.name, query),
    }))
    .filter(
      (
        match,
      ): match is {
        command: CommandOption;
        index: number;
        score: number;
      } => match.score !== null,
    )
    .sort((left, right) => left.score - right.score || left.index - right.index)
    .map(({ command }) => command);
}

export function commandCompletion(command: CommandOption): string {
  return `/${command.name} `;
}

function fuzzyScore(value: string, query: string): number | null {
  const candidate = value.toLowerCase();
  const needle = query.toLowerCase();
  let queryIndex = 0;
  let firstMatch = -1;
  let previousMatch = -1;
  let gaps = 0;

  for (
    let candidateIndex = 0;
    candidateIndex < candidate.length && queryIndex < needle.length;
    candidateIndex += 1
  ) {
    if (candidate[candidateIndex] !== needle[queryIndex]) continue;
    if (firstMatch < 0) firstMatch = candidateIndex;
    if (previousMatch >= 0) gaps += candidateIndex - previousMatch - 1;
    previousMatch = candidateIndex;
    queryIndex += 1;
  }

  if (queryIndex !== needle.length) return null;
  return firstMatch * 8 + gaps * 2 + candidate.length - needle.length;
}
