export type CommandSource = "extension" | "prompt" | "skill";

export interface CommandOption {
  name: string;
  description?: string;
  source: CommandSource;
  sourceInfo?: {
    path: string;
    source: string;
    scope: "user" | "project" | "temporary";
    origin: "package" | "top-level";
    baseDir?: string;
  };
}

const MENU_GAP = 5;
const MENU_MAX_HEIGHT = 300;
const MENU_MIN_HEIGHT = 96;

export type CommandMenuPlacement = "above" | "below";

export type CommandMenuLayout = {
  placement: CommandMenuPlacement;
  maxHeight: number;
  /** Distance from the composer top, used by the "below" placement. */
  offset: number;
};

export type CommandMenuGeometry = {
  /** Height the menu wants, including its own border and padding. */
  contentHeight: number;
  /** Viewport offset of the box the menu is positioned against. */
  composerTop: number;
  /** Viewport offset of the first line of draft text. */
  textTop: number;
  /** Height of one line of draft text. */
  textLineHeight: number;
  /** Lowest viewport offset the menu may cover, i.e. the header bottom. */
  topBoundary: number;
  /** Highest viewport offset the menu may cover, i.e. the window bottom. */
  bottomBoundary: number;
};

/**
 * Places the menu above the composer when it fits there, and below the draft
 * text otherwise. The menu always overlays its surroundings so that opening it
 * never shifts the composer.
 */
export function commandMenuLayout({
  contentHeight,
  composerTop,
  textTop,
  textLineHeight,
  topBoundary,
  bottomBoundary,
}: CommandMenuGeometry): CommandMenuLayout {
  const desiredHeight = Math.min(contentHeight, MENU_MAX_HEIGHT);
  const belowTop = textTop + textLineHeight + MENU_GAP;
  const spaceAbove = composerTop - MENU_GAP - topBoundary;
  const spaceBelow = bottomBoundary - MENU_GAP - belowTop;
  const below = desiredHeight > spaceAbove && spaceBelow > spaceAbove;
  const available = below ? spaceBelow : spaceAbove;

  return {
    placement: below ? "below" : "above",
    maxHeight: Math.round(
      Math.max(MENU_MIN_HEIGHT, Math.min(desiredHeight, available)),
    ),
    offset: Math.round(belowTop - composerTop),
  };
}

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

export function commandInvocation(command: CommandOption): string {
  return `/${command.name}`;
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
