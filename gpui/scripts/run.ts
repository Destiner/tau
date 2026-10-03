import { mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { spawn } from 'node:child_process';
import {
  assertCaptures,
  validateSuite,
  type Capture,
  type Component,
  type Suite,
} from '../contract';
import { suite } from '../scenarios';
import { runBrowser } from './browser';
import { compare } from './compare';
import { prepare } from './prepare';

const projectRoot = resolve(import.meta.dir, '../..');
function exec(command: string, args: string[]): Promise<void> {
  return new Promise((ok, fail) => {
    const child = spawn(command, args, { stdio: 'inherit' });
    child.on('error', fail);
    child.on('exit', (code) =>
      code === 0 ? ok() : fail(new Error(`${command} exited ${code}`)),
    );
  });
}
export function options(args: string[]): {
  component?: Component;
  browser: 'chromium' | 'webkit';
  out: string;
  referenceOnly: boolean;
  skipBuild: boolean;
  strictVisual: boolean;
} {
  const flags = new Set([
    '--reference-only',
    '--skip-build',
    '--strict-visual',
  ]);
  const values = new Set(['--component', '--browser', '--out']);
  const o: {
    component?: Component;
    browser: 'chromium' | 'webkit';
    out: string;
    referenceOnly: boolean;
    skipBuild: boolean;
    strictVisual: boolean;
  } = {
    browser: 'chromium',
    out: resolve(projectRoot, 'gpui/results/review'),
    referenceOnly: false,
    skipBuild: false,
    strictVisual: false,
  };
  for (let i = 0; i < args.length; i++) {
    const arg = args[i]!;
    if (flags.has(arg)) {
      if (arg === '--reference-only') o.referenceOnly = true;
      if (arg === '--skip-build') o.skipBuild = true;
      if (arg === '--strict-visual') o.strictVisual = true;
      continue;
    }
    if (!values.has(arg) || !args[i + 1])
      throw new Error(`unknown or incomplete option: ${arg}`);
    const value = args[++i]!;
    if (arg === '--component') {
      if (!['button', 'input', 'textarea'].includes(value))
        throw new Error(`unknown component ${value}`);
      o.component = value as Component;
    }
    if (arg === '--browser') {
      if (!['chromium', 'webkit'].includes(value))
        throw new Error(`unknown browser ${value}`);
      o.browser = value as typeof o.browser;
    }
    if (arg === '--out') o.out = value;
  }
  return o;
}
export async function run(args: string[]): Promise<void> {
  const o = options(args),
    base = resolve(o.out);
  if (!o.referenceOnly && !o.skipBuild) {
    await prepare();
    await exec('cargo', [
      'build',
      '--manifest-path',
      resolve(projectRoot, 'gpui/Cargo.toml'),
      '--locked',
    ]);
  }
  const failures: string[] = [];
  for (const theme of ['light', 'dark'] as const) {
    try {
      const s: Suite = suite(theme, o.component);
      validateSuite(s);
      const folder = resolve(base, `${theme}-${o.browser}`),
        vue = resolve(folder, 'vue'),
        native = resolve(folder, 'native'),
        report = resolve(folder, 'report');
      await mkdir(folder, { recursive: true });
      await rm(report, { recursive: true, force: true });
      await rm(resolve(native, 'observations.json'), { force: true });
      const request = resolve(folder, 'request.json');
      await writeFile(request, JSON.stringify(s, null, 2));
      const browser = await runBrowser(s, vue, o.browser, undefined, {
        deferStateAssertions: !o.referenceOnly,
      });
      if (o.referenceOnly) {
        console.log(
          `Vue ${theme}: STATE PASS (${browser.length} captures); VISUAL REVIEW REQUIRED — candidates in ${vue}`,
        );
        continue;
      }
      await mkdir(native, { recursive: true });
      await exec(resolve(projectRoot, 'gpui/target/debug/tau-gpui-prototype'), [
        '--suite',
        request,
        '--out',
        native,
      ]);
      const captures: Capture[] = JSON.parse(
        await readFile(resolve(native, 'observations.json'), 'utf8'),
      );
      assertCaptures(s, captures, 'native');
      await compare(s, browser, captures, vue, native, report, o.strictVisual);
      console.log(`${theme}: report ${resolve(report, 'report.html')}`);
    } catch (error) {
      failures.push(`${theme}: ${String(error)}`);
      console.error(`${theme}:`, error);
    }
  }
  if (failures.length) throw new Error(failures.join('\n'));
}
if (import.meta.main)
  run(process.argv.slice(2)).catch((error) => {
    console.error(error);
    process.exitCode = 1;
  });
