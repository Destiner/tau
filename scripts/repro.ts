import { spawn } from 'node:child_process';

import { createReproLaunch, parseReproArgs } from './repro-command';

const parsed = parseReproArgs(process.argv.slice(2));

if (parsed.kind === 'print') {
  console.log(parsed.text);
} else if (parsed.kind === 'error') {
  console.error(parsed.text);
  process.exitCode = 1;
} else {
  const launch = createReproLaunch(parsed.scenario.name);
  console.log(
    `Opening ${parsed.scenario.name}\n${parsed.scenario.purpose}\nPress Ctrl-C to stop the development server.`,
  );
  const child = spawn(launch.command, launch.args, { stdio: 'inherit' });
  child.once('error', (error) => {
    console.error(`Could not launch the reproduction: ${error.message}`);
    process.exitCode = 1;
  });
  child.once('exit', (code, signal) => {
    if (signal) {
      process.kill(process.pid, signal);
      return;
    }
    process.exitCode = code ?? 1;
  });
}
