import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';

import { describe, expect, test } from 'vitest';
import { parse } from 'yaml';

const workflow = parse(
  readFileSync(new URL('../.github/workflows/ci.yml', import.meta.url), 'utf8'),
) as {
  on: Record<string, unknown>;
  permissions: Record<string, string>;
  concurrency: { group: string; 'cancel-in-progress': boolean };
  jobs: Record<
    'frontend' | 'browser' | 'native' | 'check',
    {
      needs?: string[];
      if?: string;
      'runs-on': string;
      'continue-on-error'?: boolean;
      steps: Array<{
        'continue-on-error'?: boolean;
        uses?: string;
        run?: string;
        with?: Record<string, string | boolean>;
        env?: Record<string, string>;
      }>;
    }
  >;
};

const lanes = ['frontend', 'browser', 'native'] as const;

function commands(job: keyof typeof workflow.jobs): string {
  return workflow.jobs[job].steps.map((step) => step.run ?? '').join('\n');
}

describe('PR CI workflow', () => {
  test('runs independent lanes and retains a single fail-closed check', () => {
    expect(Object.keys(workflow.jobs).sort()).toEqual(
      [...lanes, 'check'].sort(),
    );
    expect(workflow.on).toHaveProperty('pull_request');
    for (const lane of lanes) {
      expect(workflow.jobs[lane].needs).toBeUndefined();
      expect(workflow.jobs[lane]['runs-on']).toBe('macos-15');
      expect(workflow.jobs[lane].if).toBeUndefined();
      expect(workflow.jobs[lane]['continue-on-error']).toBeUndefined();
      for (const step of workflow.jobs[lane].steps) {
        expect(step['continue-on-error']).toBeUndefined();
      }
    }

    const check = workflow.jobs.check;
    expect(check.needs?.sort()).toEqual([...lanes].sort());
    expect(check.if).toMatch(/always\(\)/);
    expect(
      check.steps.some(
        (step) =>
          step.env?.RESULTS?.includes('needs.*.result') &&
          step.run?.includes('"success,success,success"') &&
          step.run?.includes('exit 1'),
      ),
    ).toBe(true);
  });

  test('the aggregate shell rejects any non-successful lane', () => {
    const gate = workflow.jobs.check.steps.find((step) => step.env?.RESULTS);
    expect(gate?.run).toBeDefined();
    const results = ['success', 'failure', 'cancelled', 'skipped'];
    const states = results.join(' ');
    const result = spawnSync(
      'bash',
      [
        '-e',
        '-c',
        `
        for frontend in ${states}; do
          for browser in ${states}; do
            for native in ${states}; do
              export RESULTS="$frontend,$browser,$native"
              if (${gate!.run!}) >/dev/null; then
                echo "$RESULTS:pass"
              else
                echo "$RESULTS:fail"
              fi
            done
          done
        done
      `,
      ],
      { encoding: 'utf8' },
    );
    expect(result.status, result.stderr).toBe(0);
    const expected = results.flatMap((frontend) =>
      results.flatMap((browser) =>
        results.map((native) => {
          const statuses = [frontend, browser, native];
          const pass = statuses.every((status) => status === 'success');
          return `${statuses.join(',')}:${pass ? 'pass' : 'fail'}`;
        }),
      ),
    );
    expect(result.stdout.trim().split('\n')).toEqual(expected);
  });

  test('keeps every required validation command', () => {
    for (const command of ['bun run lint', 'bun run build', 'bun run test']) {
      expect(commands('frontend')).toContain(command);
    }
    expect(commands('browser')).toContain('playwright install chromium webkit');
    for (const project of ['chromium', 'webkit']) {
      expect(commands('browser')).toContain(`--project=${project}`);
    }
    expect(commands('browser')).toMatch(/--retries=[01]\b/);
    expect(commands('native')).toContain('bun x vite build');
    expect(commands('native').indexOf('bun x vite build')).toBeLessThan(
      commands('native').indexOf('cargo clippy'),
    );
    for (const command of ['cargo fmt --check', 'cargo clippy', 'cargo test']) {
      expect(commands('native')).toContain(command);
    }
  });

  test('uses scoped cancellation, safe checkout and frozen Bun installs', () => {
    expect(workflow.permissions).toEqual({ contents: 'read' });
    expect(workflow.concurrency.group).toContain(
      'github.event.pull_request.number',
    );
    expect(workflow.concurrency['cancel-in-progress']).toBe(true);
    for (const lane of lanes) {
      const checkout = workflow.jobs[lane].steps.find((step) =>
        step.uses?.startsWith('actions/checkout@'),
      );
      expect(checkout?.with?.['persist-credentials']).toBe(false);
      expect(
        workflow.jobs[lane].steps.find((step) =>
          step.uses?.startsWith('oven-sh/setup-bun@'),
        )?.with?.['bun-version'],
      ).toBe('1.3.0');
    }
    for (const lane of lanes) {
      const steps = workflow.jobs[lane].steps;
      expect(commands(lane)).toContain('bun install --frozen-lockfile');
      const cache = steps.find((step) =>
        step.uses?.startsWith('actions/cache@'),
      );
      expect(cache?.with?.path).toContain('.bun/install/cache');
      expect(cache?.with?.key).toContain("hashFiles('bun.lock')");
    }
    const rustCache = workflow.jobs.native.steps.find((step) =>
      step.uses?.startsWith('Swatinem/rust-cache@'),
    );
    expect(rustCache?.with?.workspaces).toContain('src-tauri');
    expect(rustCache?.with?.key).toContain('Cargo.lock');
  });
});
