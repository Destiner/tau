/* eslint-disable @typescript-eslint/explicit-function-return-type */
// Reproducible discovery and size audit. Baseline artifacts in node_modules are inputs to
// the one-time manifest only; --check always re-reads the pinned Git revision.
import { execFileSync } from 'node:child_process';
import { existsSync, lstatSync, readFileSync } from 'node:fs';
import { dirname, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import * as ts from 'typescript';
import { parse } from 'vue/compiler-sfc';

import baseline from './deslop-baseline.json';
import mappings from './deslop-mappings.json';

const root = resolve(fileURLToPath(new URL('..', import.meta.url)));
const git = (...args: string[]) =>
  execFileSync('git', args, {
    cwd: root,
    encoding: 'utf8',
    maxBuffer: 32 * 1024 * 1024,
  });
const run = (program: string, args: string[]) =>
  execFileSync(program, args, {
    cwd: root,
    encoding: 'utf8',
    maxBuffer: 32 * 1024 * 1024,
  });
const show = (path: string) => git('show', `${baseline.revision}:${path}`);
const uniq = (items: string[]) => [...new Set(items)];
const chars = (text: string) => [...text].length;
const sorted = (items: string[]) => uniq(items).sort();

function markdownPaths(tracked: string[], added: string[] = []) {
  return sorted(
    [...tracked, ...added].filter(
      (path) =>
        path.endsWith('.md') &&
        !/(^|\/)(node_modules|dist|target|build|\.cache)\//.test(path),
    ),
  );
}

function codeComments(text: string, extension: string): number {
  if (extension === '.vue') {
    const { descriptor } = parse(text);
    const parts = [descriptor.script, descriptor.scriptSetup].filter(
      (x) => x != null,
    );
    return (
      parts.reduce((n, part) => n + codeComments(part!.content, '.ts'), 0) +
      (descriptor.template?.content.match(/<!--[\s\S]*?-->/g) ?? []).reduce(
        (n, x) => n + chars(x),
        0,
      ) +
      descriptor.styles.reduce(
        (n, part) => n + codeComments(part.content, '.css'),
        0,
      )
    );
  }
  if (
    extension === '.ts' ||
    extension === '.tsx' ||
    extension === '.js' ||
    extension === '.mjs'
  ) {
    const ast = ts.createSourceFile(
      `source${extension}`,
      text,
      ts.ScriptTarget.Latest,
      true,
    );
    const ranges = new Map<number, number>();
    function visit(node: ts.Node) {
      for (const range of [
        ...(ts.getLeadingCommentRanges(text, node.getFullStart()) ?? []),
        ...(ts.getTrailingCommentRanges(text, node.end) ?? []),
      ])
        ranges.set(range.pos, range.end);
      for (const child of node.getChildren(ast)) visit(child);
    }
    visit(ast);
    return [...ranges].reduce(
      (total, [start, end]) => total + chars(text.slice(start, end)),
      0,
    );
  }
  if (extension === '.css' || extension === '.rs')
    return lexicalComments(text, extension === '.rs');
  return 0;
}

// Rust raw strings and nested block comments need different handling from JS/CSS.
function lexicalComments(text: string, rust: boolean): number {
  let count = 0;
  for (let i = 0; i < text.length;) {
    const tail = text.slice(i);
    if (rust) {
      const raw = /^(?:b)?r(#+)?"/.exec(tail);
      if (raw) {
        const end = text.indexOf('"' + (raw[1] ?? ''), i + raw[0].length);
        i = end < 0 ? text.length : end + 1 + (raw[1]?.length ?? 0);
        continue;
      }
    }
    if (tail.startsWith('/*')) {
      const start = i;
      let depth = 1;
      i += 2;
      while (i < text.length && depth) {
        if (text.startsWith('/*', i) && rust) {
          depth++;
          i += 2;
        } else if (text.startsWith('*/', i)) {
          depth--;
          i += 2;
        } else i++;
      }
      count += chars(text.slice(start, i));
    } else if (rust && tail.startsWith('//')) {
      const end = text.indexOf('\n', i);
      const next = end < 0 ? text.length : end;
      count += chars(text.slice(i, next));
      i = next;
    } else if (
      text[i] === '"' ||
      (text[i] === "'" && (!rust || /^'(?:\\.|[^'\\])'/.test(tail)))
    ) {
      const quote = text[i++];
      while (i < text.length) {
        if (text[i] === '\\') i += 2;
        else if (text[i++] === quote) break;
      }
    } else i++;
  }
  return count;
}

function rustTests(text: string): string[] {
  return sorted(
    text
      .split('\n')
      .filter((line) => line.endsWith(': test'))
      .map((line) => line.slice(0, -6)),
  );
}

type PlaywrightSuite = {
  title: string;
  specs?: { file: string; title: string; tests?: { projectName: string }[] }[];
  suites?: PlaywrightSuite[];
};

function playwrightTests(report: {
  suites: PlaywrightSuite[];
}): Record<string, string[]> {
  const projects: Record<string, string[]> = {};
  function walk(suites: PlaywrightSuite[], titles: string[] = []) {
    for (const suite of suites) {
      const path = [...titles, suite.title];
      for (const spec of suite.specs ?? [])
        for (const test of spec.tests ?? []) {
          (projects[test.projectName] ??= []).push(
            ['tests/e2e/' + spec.file, ...path.slice(1), spec.title].join(
              ' > ',
            ),
          );
        }
      walk(suite.suites ?? [], path);
    }
  }
  walk(report.suites);
  for (const name of Object.keys(projects))
    projects[name] = sorted(projects[name] ?? []);
  return projects;
}

function resolveCoverage(
  original: string[],
  current: string[],
  entries: { from: string[]; to: string[]; reason: string }[],
) {
  const graph = new Map<string, string[]>();
  for (const entry of entries)
    for (const id of uniq(entry.from)) {
      if (!entry.reason.trim() || !entry.to.length)
        throw Error(`Missing rationale/target for ${id}`);
      if (graph.has(id)) throw Error(`Duplicate mapping for ${id}`);
      graph.set(id, entry.to);
    }
  const live = new Set(current);
  const remaining = new Map<string, number>();
  for (const id of current) remaining.set(id, (remaining.get(id) ?? 0) + 1);
  const uncovered: string[] = [];
  for (const id of original) {
    const count = remaining.get(id) ?? 0;
    if (count > 0) {
      remaining.set(id, count - 1);
      continue;
    }
    const seen = new Set<string>();
    function reaches(node: string): boolean {
      if (live.has(node)) return true;
      if (seen.has(node)) throw Error(`Mapping cycle at ${node}`);
      seen.add(node);
      return (graph.get(node) ?? []).some(reaches);
    }
    if (!(graph.get(id) ?? []).some(reaches)) uncovered.push(id);
  }
  return uncovered;
}

function sourcePaths() {
  const tracked = git('ls-files', '-z').split('\0').filter(Boolean);
  const added = git('ls-files', '--others', '--exclude-standard', '-z')
    .split('\0')
    .filter(Boolean);
  return {
    files: sorted([...tracked, ...added]).filter(
      (path) =>
        existsSync(resolve(root, path)) &&
        !lstatSync(resolve(root, path)).isSymbolicLink(),
    ),
  };
}

function directives(text: string, path: string): string[] {
  if (path.endsWith('.rs'))
    return (text.match(/#\s*\[\s*ignore\b[^\]]*\]/g) ?? []).sort();
  if (!/\.(?:ts|tsx|js|mjs|vue)$/.test(path)) return [];
  const parts = path.endsWith('.vue')
    ? (() => {
        const d = parse(text).descriptor;
        return [d.script?.content, d.scriptSetup?.content].filter(
          (x): x is string => x != null,
        );
      })()
    : [text];
  const found: string[] = [];
  for (const part of parts) {
    const ast = ts.createSourceFile(path, part, ts.ScriptTarget.Latest, true);
    const visit = (node: ts.Node) => {
      if (
        ts.isPropertyAccessExpression(node) &&
        ['skip', 'only', 'fixme', 'todo'].includes(node.name.text)
      ) {
        const receiver = node.expression.getText(ast);
        if (/^(?:test|it|describe)(?:\.|$)/.test(receiver))
          found.push(node.getText(ast));
      }
      ts.forEachChild(node, visit);
    };
    visit(ast);
  }
  return found.sort();
}

function audit() {
  const check = process.argv.includes('--check');
  if (!check && !process.argv.includes('--report'))
    throw Error('Usage: bun scripts/deslop-audit.ts --check|--report');
  const errors: string[] = [];
  const assert = (ok: boolean, message: string) => {
    if (!ok) errors.push(message);
  };
  assert(
    baseline.revision === 'cfb596fd2ea0dc9db3f017301fc39c8b1d43184f',
    'Baseline revision changed',
  );
  assert(
    baseline.baseline.e2e === 160 &&
      baseline.e2e.chromium.length === 158 &&
      baseline.e2e.webkit.length === 158 &&
      baseline.e2e['chromium-performance'].length === 2,
    'E2E manifest count mismatch',
  );
  assert(
    JSON.stringify(baseline.e2e.chromium) ===
      JSON.stringify(baseline.e2e.webkit),
    'Baseline functional browsers disagree',
  );
  const { files } = sourcePaths();
  const baselineFiles = git('ls-tree', '-r', '--name-only', baseline.revision)
    .split('\n')
    .filter(Boolean);
  const baselineSet = new Set(baselineFiles);
  const docs = markdownPaths(files);
  const baselineDocs = markdownPaths(baselineFiles);
  const originalDocs = Object.fromEntries(
    baselineDocs.map((path) => [path, chars(show(path))]),
  );
  const baseTotal = Object.values(originalDocs).reduce((a, b) => a + b, 0);
  assert(
    baseTotal === baseline.baseline.docs,
    `Baseline docs mismatch: ${baseTotal}`,
  );
  assert(
    baseline.caps.docs === Math.floor(baseTotal * 0.8) &&
      baseline.caps.e2e === 32,
    'Retention caps changed',
  );
  const currentDocs = docs.reduce(
    (n, path) => n + chars(readFileSync(resolve(root, path), 'utf8')),
    0,
  );
  for (const path of docs) {
    const text = readFileSync(resolve(root, path), 'utf8').replace(
      /^```[^\n]*\n[\s\S]*?^```/gm,
      '',
    );
    for (const link of text.matchAll(
      /\]\(([^\s)]+)(?:\s+"[^"]*")?\)|^\[[^\]]+\]:\s*(\S+)/gm,
    )) {
      const target = (link[1] ?? link[2] ?? '')
        .replace(/^<|>$/g, '')
        .split('#')[0]
        ?.split('?')[0];
      if (!target || /^[a-z]+:/i.test(target)) continue;
      assert(
        existsSync(resolve(root, dirname(path), decodeURIComponent(target))),
        `Broken documentation link: ${path} -> ${target}`,
      );
    }
  }
  assert(
    currentDocs <= baseline.caps.docs,
    `Docs ${currentDocs} > ${baseline.caps.docs}`,
  );
  console.log(
    `docs: ${currentDocs}/${baseline.caps.docs} (Git baseline ${baseTotal})`,
  );

  const source = (path: string) =>
    /\.(?:ts|tsx|js|mjs|vue|rs|css)$/.test(path) &&
    !path.startsWith('node_modules/');
  let initialComments = 0,
    currentComments = 0;
  for (const path of baselineFiles.filter(source)) {
    initialComments += codeComments(show(path), '.' + path.split('.').at(-1));
  }
  for (const path of files.filter(source)) {
    const text = readFileSync(resolve(root, path), 'utf8');
    currentComments += codeComments(text, '.' + path.split('.').at(-1));
    const oldFlags = baselineSet.has(path) ? directives(show(path), path) : [];
    const newFlags = directives(text, path);
    for (const flag of oldFlags) {
      const index = newFlags.indexOf(flag);
      if (index >= 0) newFlags.splice(index, 1);
    }
    assert(
      newFlags.length === 0,
      `New skip/only/ignore directive in ${path}: ${newFlags.join(', ')}`,
    );
  }
  for (const path of ['playwright.config.ts', 'vite.config.ts']) {
    const discovery = (text: string) =>
      [
        ...text.matchAll(
          /\b(?:testDir|testMatch|testIgnore|grep|grepInvert|exclude|include|ignored)\s*:\s*([^,\n]+)/g,
        ),
      ].map((match) => match[0]);
    assert(
      JSON.stringify(discovery(show(path))) ===
        JSON.stringify(discovery(readFileSync(resolve(root, path), 'utf8'))),
      `Discovery config changed: ${path}`,
    );
  }
  console.log(
    `comments (TS/JS/Vue/CSS/Rust, Unicode characters): ${currentComments}/${initialComments} (${Math.round((100 * currentComments) / initialComments)}% of baseline; goal 50%)`,
  );
  assert(currentComments <= initialComments / 2, `Comments exceed 50% target`);

  const v = JSON.parse(run('bun', ['x', 'vitest', 'list', '--json']));
  const vitest = v.map(
    (x: { file: string; name: string }) =>
      `${relative(root, x.file).replaceAll('\\', '/')} > ${x.name}`,
  );
  assert(
    baseline.vitest.length === baseline.baseline.vitest,
    'Vitest manifest count mismatch',
  );
  console.log(`vitest: ${vitest.length} (baseline ${baseline.vitest.length})`);

  const rust = (all: boolean) =>
    rustTests(
      run('cargo', [
        'test',
        '--manifest-path',
        'src-tauri/Cargo.toml',
        ...(all ? ['--all-features'] : []),
        '--',
        '--list',
      ]),
    );
  const rustDefault = rust(false),
    rustAll = rust(true);
  const docTests = run('cargo', [
    'test',
    '--manifest-path',
    'src-tauri/Cargo.toml',
    '--all-features',
    '--doc',
    '--',
    '--list',
  ]);
  assert(
    /0 tests, 0 benchmarks/.test(docTests) && rustTests(docTests).length === 0,
    'Rust doctest discovery changed',
  );
  const ignored = rustTests(
    run('cargo', [
      'test',
      '--manifest-path',
      'src-tauri/Cargo.toml',
      '--all-features',
      '--',
      '--ignored',
      '--list',
    ]),
  );
  assert(
    baseline.rustAll.length === 302 && baseline.rustDefault.length === 292,
    'Rust manifest count mismatch',
  );

  assert(
    rustDefault.every((id) => rustAll.includes(id)),
    'Default-feature tests missing from all-features',
  );
  // Cargo --list does not annotate ignored tests; source attributes must stay unchanged.
  assert(
    JSON.stringify(ignored) === JSON.stringify(sorted(baseline.rustIgnored)),
    `Rust ignored set changed: ${ignored.join(', ')}`,
  );
  console.log(
    `rust: default ${rustDefault.length}, all-features ${rustAll.length}/${baseline.caps.rustAll} (${ignored.length} ignored, ${rustAll.length - ignored.length} active)`,
  );

  const nonE2e = vitest.length + rustAll.length - ignored.length;
  const nonE2eCap = Math.floor(
    (baseline.vitest.length +
      baseline.rustAll.length -
      baseline.rustIgnored.length) /
      2,
  );
  console.log(`non-e2e: ${nonE2e} (original target ${nonE2eCap}, waived)`);

  const e2e = playwrightTests(
    JSON.parse(
      run('bun', ['x', 'playwright', 'test', '--list', '--reporter=json']),
    ),
  );
  const functional = sorted([...(e2e.chromium ?? []), ...(e2e.webkit ?? [])]);
  const performance = e2e['chromium-performance'] ?? [];
  assert(
    JSON.stringify(e2e.chromium) === JSON.stringify(e2e.webkit),
    'Functional E2E not discovered in both engines',
  );
  assert(
    JSON.stringify(performance) ===
      JSON.stringify(baseline.e2e['chromium-performance']),
    'Performance E2E discovery changed',
  );
  console.log(
    `e2e: ${functional.length + performance.length} (original target ${baseline.caps.e2e}, waived; ${functional.length} functional in both engines + ${performance.length} performance)`,
  );

  const currentIds = [...vitest, ...rustAll, ...functional, ...performance];
  for (const [name, original] of [
    ['vitest', baseline.vitest],
    ['native', baseline.rustAll],
    [
      'e2e',
      sorted([
        ...baseline.e2e.chromium,
        ...baseline.e2e['chromium-performance'],
      ]),
    ],
  ] as const) {
    const uncovered = resolveCoverage(original, currentIds, mappings[name]);
    console.log(
      `${name} coverage: ${original.length - uncovered.length}/${original.length}`,
    );
    if (uncovered.length) {
      assert(
        false,
        `${name} uncovered original IDs (${uncovered.length}): ${uncovered.slice(0, 8).join('; ')}`,
      );
    }
  }
  if (errors.length) {
    for (const error of errors) console.error(`FAIL ${error}`);
    if (check) process.exitCode = 1;
  }
}

if (import.meta.main) audit();

export {
  codeComments,
  markdownPaths,
  playwrightTests,
  resolveCoverage,
  rustTests,
};
