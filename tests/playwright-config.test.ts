/// <reference types="node" />

import { readFileSync } from 'node:fs';

import { describe, expect, test } from 'vitest';

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

describe('Playwright scheduling', () => {
  test('runs functional browsers at the configured capacity', () => {
    expect(playwrightConfig.workers).toBe(2);

    for (const name of ['chromium', 'webkit']) {
      expect(projectNamed(name).testIgnore).toBe(performanceSpec);
    }
  });

  test('runs both benchmarks in an isolated Chromium phase', () => {
    const project = projectNamed('chromium-performance');

    expect(project.testMatch).toBe(performanceSpec);
    expect(project.workers).toBe(1);
    expect(project.dependencies).toEqual(['chromium', 'webkit']);
    expect(project.use?.browserName).toBe('chromium');
  });

  test('provides a dependency-free isolated performance command', () => {
    const packageJson = JSON.parse(
      readFileSync(new URL('../package.json', import.meta.url), 'utf8'),
    ) as { scripts?: Record<string, string> };

    expect(packageJson.scripts?.['test:e2e:performance']).toBe(
      'playwright test --project=chromium-performance --no-deps',
    );
  });
});
