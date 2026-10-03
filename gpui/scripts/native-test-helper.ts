import { spawn } from 'node:child_process';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { join, resolve } from 'node:path';
import type { Capture, Suite } from '../contract';
import { suite } from '../scenarios';

const root = resolve(import.meta.dir, '..');
export const binary = join(root, 'target/debug/tau-gpui-prototype');

export async function exec(
  command: string,
  args: string[],
  cwd = root,
): Promise<{ code: number; output: string }> {
  return new Promise((ok, fail) => {
    const child = spawn(command, args, {
      cwd,
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    let output = '';
    child.stdout.on('data', (data: Buffer) => {
      output += data.toString();
    });
    child.stderr.on('data', (data: Buffer) => {
      output += data.toString();
    });
    child.on('error', fail);
    child.on('close', (code) => ok({ code: code ?? -1, output }));
  });
}

export async function native(
  request: unknown,
  dir: string,
  fault?: string,
): Promise<{ code: number; output: string; captures?: Capture[] }> {
  await mkdir(dir, { recursive: true });
  const file = join(dir, 'request.json');
  await writeFile(file, JSON.stringify(request));
  const result = await exec(binary, [
    '--suite',
    file,
    '--out',
    dir,
    ...(fault ? ['--fault', fault] : []),
  ]);
  if (result.code !== 0) return result;
  return {
    ...result,
    captures: JSON.parse(
      await readFile(join(dir, 'observations.json'), 'utf8'),
    ) as Capture[],
  };
}

export function pick(theme: Suite['theme'], ...ids: string[]): Suite {
  const full = suite(theme);
  const scenarios = ids.map((id) => {
    const match = full.scenarios.find((item) => item.id === id);
    if (!match) throw new Error(`unknown scenario ${id}`);
    return structuredClone(match);
  });
  return { ...full, scenarios };
}
