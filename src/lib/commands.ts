type CommandSource = 'extension' | 'prompt' | 'skill';

interface CommandOption {
  name: string;
  description?: string;
  source: CommandSource;
  sourceInfo?: {
    path: string;
    source: string;
    scope: 'user' | 'project' | 'temporary';
    origin: 'package' | 'top-level';
    baseDir?: string;
  };
}

const MENU_GAP = 5;
const MENU_MAX_HEIGHT = 300;
const MENU_MIN_HEIGHT = 96;

type CommandMenuPlacement = 'above' | 'below';

type CommandMenuLayout = {
  placement: CommandMenuPlacement;
  maxHeight: number;

  offset: number;
};

type CommandSelection = {
  draft: string;
  submit: boolean;
};

type CommandMenuGeometry = {
  contentHeight: number;

  composerTop: number;

  textTop: number;

  textLineHeight: number;

  topBoundary: number;

  bottomBoundary: number;
};

/**
 * Places the menu above the composer when it fits there, and below the draft
 * text otherwise. The menu always overlays its surroundings so that opening it
 * never shifts the composer.
 */
function commandMenuLayout({
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
    placement: below ? 'below' : 'above',
    maxHeight: Math.round(
      Math.max(MENU_MIN_HEIGHT, Math.min(desiredHeight, available)),
    ),
    offset: Math.round(belowTop - composerTop),
  };
}

function slashCommandQuery(draft: string): string | null {
  const match = /^\/([^\s/]*)$/.exec(draft);
  return match?.[1] ?? null;
}

function filterCommands(
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

function commandInvocation(command: CommandOption): string {
  return `/${command.name}`;
}

function commandSelection(
  command: CommandOption,
  completeOnly = false,
): CommandSelection {
  const submit = !completeOnly;
  const invocation = commandInvocation(command);
  return {
    draft: submit ? invocation : `${invocation} `,
    submit,
  };
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

export type {
  CommandSource,
  CommandOption,
  CommandMenuPlacement,
  CommandMenuLayout,
  CommandSelection,
  CommandMenuGeometry,
};

export {
  commandMenuLayout,
  slashCommandQuery,
  filterCommands,
  commandInvocation,
  commandSelection,
};
