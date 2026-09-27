import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

import { resolveConfig } from 'vite';
import { describe, expect, test, vi } from 'vitest';

import playwrightConfig from '../playwright.config';

const performanceSpec = '**/*-performance.e2e.ts';

function projectNamed(
  name: string,
): NonNullable<typeof playwrightConfig.projects>[number] {
  const project = playwrightConfig.projects?.find(
    (candidate) => candidate.name === name,
  );
  expect(project, `Playwright project ${name}`).toBeDefined();
  return project!;
}

describe('Archive E2E error guard', () => {
  test('archive suites use the shared browser-error fixture', () => {
    for (const name of ['archive-window', 'archive-performance']) {
      const source = readFileSync(
        new URL(`./e2e/${name}.e2e.ts`, import.meta.url),
        'utf8',
      );
      expect(source).toMatch(/import \{ expect, test \} from '\.\/fixtures';/);
    }
  });
});

describe('Bundled E2E server isolation', () => {
  test('enables fixture routes only in a separate browser-test build', async () => {
    const configPath = (name: string): string =>
      fileURLToPath(new URL(`../${name}`, import.meta.url));
    const production = await resolveConfig(
      { configFile: configPath('vite.config.ts') },
      'build',
    );
    const e2e = await resolveConfig(
      { configFile: configPath('vite.e2e.config.ts') },
      'build',
    );

    expect(production.build.outDir).toBe('dist');
    expect(production.define?.['import.meta.env.DEV']).toBeUndefined();
    expect(e2e.build.outDir).toBe('node_modules/.cache/tau-e2e');
    expect(e2e.define?.['import.meta.env.DEV']).toBe('true');
    const { scripts } = JSON.parse(
      readFileSync(new URL('../package.json', import.meta.url), 'utf8'),
    ) as { scripts: Record<string, string> };
    expect(scripts['build:e2e']).toBe('vite build --config vite.e2e.config.ts');
    expect(scripts['preview:e2e']).toBe(
      'vite preview --config vite.e2e.config.ts',
    );
    expect(
      e2e.plugins.filter((plugin) => plugin.name === 'vite:vue').length,
    ).toBe(
      production.plugins.filter((plugin) => plugin.name === 'vite:vue').length,
    );
  });

  test('builds before previewing on the configured port', async () => {
    vi.stubEnv('TAU_PLAYWRIGHT_PORT', '1432');
    vi.resetModules();
    try {
      const { default: config } = await import('../playwright.config');
      expect(config.webServer).toMatchObject({
        command:
          'bun run build:e2e && bun run preview:e2e -- --host 127.0.0.1 --port 1432 --strictPort',
        url: 'http://127.0.0.1:1432',
      });
      expect(config.use?.baseURL).toBe('http://127.0.0.1:1432');
    } finally {
      vi.unstubAllEnvs();
    }
  });
});

describe('Playwright scheduling', () => {
  test('selects tracing for both CI modes without changing retries', async () => {
    for (const [ci, trace] of [
      ['1', 'on-first-retry'],
      ['', 'retain-on-failure'],
    ]) {
      vi.stubEnv('CI', ci);
      vi.resetModules();
      try {
        const { default: config } = await import('../playwright.config');
        expect(config.use?.trace).toBe(trace);
        expect(config.retries ?? 0).toBe(0);
      } finally {
        vi.unstubAllEnvs();
      }
    }
  });

  test('schedules functional browsers and isolates Chromium benchmarks', () => {
    expect(playwrightConfig.fullyParallel).toBe(true);
    expect(playwrightConfig.workers).toBe(2);

    for (const name of ['chromium', 'webkit']) {
      expect(projectNamed(name).testIgnore).toBe(performanceSpec);
    }

    const project = projectNamed('chromium-performance');

    expect(project.testMatch).toBe(performanceSpec);
    expect(project.workers).toBe(1);
    expect(project.dependencies).toEqual(['chromium', 'webkit']);
    expect(project.use?.browserName).toBe('chromium');

    const packageJson = JSON.parse(
      readFileSync(new URL('../package.json', import.meta.url), 'utf8'),
    ) as { scripts?: Record<string, string> };

    expect(packageJson.scripts?.['test:e2e:performance']).toBe(
      'playwright test --project=chromium-performance --no-deps',
    );
  });
});
