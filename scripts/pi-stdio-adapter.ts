#!/usr/bin/env bun

import { createInterface } from 'node:readline';

import { findPiScenario } from '../tests/support/pi-scenario/catalogue';
import { PiScenarioEngine } from '../tests/support/pi-scenario/index';

const SCENARIO_ENV = 'TAU_PI_TEST_SCENARIO';
const FAULT_ENV = 'TAU_PI_TEST_FAULT';
const PROJECT_ENV = 'TAU_PI_TEST_PROJECT_PATH';
const SESSION_ENV = 'TAU_PI_TEST_SESSION_PATH';

function fail(message: string): never {
  console.error(`Fake Pi adapter: ${message}`);
  process.exit(2);
}

function validateStartup(): void {
  const args = process.argv.slice(2);
  if (args[0] !== '--mode' || args[1] !== 'rpc') {
    fail('expected startup arguments --mode rpc.');
  }

  const remaining = args.slice(2);
  const sessionPath =
    remaining[0] === '--session' && remaining.length === 2
      ? remaining[1]
      : undefined;
  if (remaining.length > 0 && sessionPath === undefined) {
    fail('received unsupported startup arguments.');
  }

  const expectedProject = process.env[PROJECT_ENV];
  if (expectedProject !== undefined && process.cwd() !== expectedProject) {
    fail('working directory did not match the test configuration.');
  }
  const expectedSession = process.env[SESSION_ENV];
  if (expectedSession !== undefined && sessionPath !== expectedSession) {
    fail('session argument did not match the test configuration.');
  }
}

function writeAvailableOutputs(engine: PiScenarioEngine): void {
  for (let output = engine.takeOutput(); output; output = engine.takeOutput()) {
    if (output.kind === 'runtime-event') {
      if (output.value.kind !== 'started') {
        fail('scenario requested an unsupported runtime event.');
      }
      continue;
    }
    process.stdout.write(`${JSON.stringify(output.value)}\n`);
  }
}

async function main(): Promise<void> {
  validateStartup();

  const scenarioName = process.env[SCENARIO_ENV];
  const scenario = scenarioName ? findPiScenario(scenarioName) : undefined;
  if (!scenario) fail('test scenario is missing or unknown.');

  const fault = process.env[FAULT_ENV];
  if (fault === 'malformed-stdout') {
    process.stdout.write('not-json\n');
    return;
  }
  if (fault === 'nonzero-exit') {
    console.error('scripted transport failure');
    process.exit(23);
  }
  if (fault !== undefined) fail('test fault is unknown.');

  const engine = new PiScenarioEngine(scenario);
  engine.bindRuntime('main', 'stdio');
  writeAvailableOutputs(engine);

  const lines = createInterface({ input: process.stdin, crlfDelay: Infinity });
  for await (const line of lines) {
    if (!line) fail('received an empty request line.');
    let request: unknown;
    try {
      request = JSON.parse(line);
    } catch {
      fail('received malformed request JSONL.');
    }
    engine.consumeRequest('stdio', request);
    writeAvailableOutputs(engine);
    if (engine.isComplete()) {
      engine.verifyComplete();
      lines.close();
      return;
    }
  }

  engine.verifyComplete();
}

try {
  await main();
} catch (error) {
  fail(error instanceof Error ? error.message : 'scenario execution failed.');
}
