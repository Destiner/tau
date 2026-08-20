import type { PiScenarioMetadata } from '../tests/support/pi-scenario';
import {
  findPiScenario,
  piScenarioCatalogue,
} from '../tests/support/pi-scenario/catalogue';

interface ReproLaunch {
  command: string;
  args: readonly string[];
  scenarioUrl: string;
}

type ReproParseResult =
  | { kind: 'launch'; scenario: PiScenarioMetadata }
  | { kind: 'print'; text: string }
  | { kind: 'error'; text: string };

function availableScenarios(): string {
  return piScenarioCatalogue()
    .map(({ name, purpose }) => `  ${name}\n    ${purpose}`)
    .join('\n');
}

function reproUsage(): string {
  return [
    'Usage: bun run repro -- <scenario>',
    '       bun run repro -- --list',
    '',
    'Available scenarios:',
    availableScenarios(),
  ].join('\n');
}

function parseReproArgs(args: readonly string[]): ReproParseResult {
  if (args.length === 1 && (args[0] === '--help' || args[0] === '-h')) {
    return { kind: 'print', text: reproUsage() };
  }
  if (args.length === 1 && args[0] === '--list') {
    return {
      kind: 'print',
      text: `Available scenarios:\n${availableScenarios()}`,
    };
  }
  if (args.length === 0) {
    return { kind: 'error', text: `Missing scenario name.\n\n${reproUsage()}` };
  }
  if (args.length !== 1) {
    return {
      kind: 'error',
      text: `Expected exactly one scenario name.\n\n${reproUsage()}`,
    };
  }

  const scenario = findPiScenario(args[0] ?? '');
  if (!scenario) {
    return {
      kind: 'error',
      text: `Unknown scenario ${JSON.stringify(args[0])}.\n\n${reproUsage()}`,
    };
  }
  return { kind: 'launch', scenario: scenario.metadata };
}

function createReproLaunch(
  scenarioName: string,
  bunExecutable = process.execPath,
): ReproLaunch {
  const scenario = findPiScenario(scenarioName);
  if (!scenario) {
    throw new Error(
      `Cannot launch unknown scenario ${JSON.stringify(scenarioName)}.`,
    );
  }
  const scenarioUrl = `/?test-scenario=${encodeURIComponent(scenario.metadata.name)}`;
  return {
    command: bunExecutable,
    args: ['run', 'dev', '--', '--open', scenarioUrl],
    scenarioUrl,
  };
}

export {
  createReproLaunch,
  parseReproArgs,
  reproUsage,
  type ReproLaunch,
  type ReproParseResult,
};
