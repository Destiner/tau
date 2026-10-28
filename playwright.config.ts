import { defineConfig, devices } from '@playwright/test';

const port = process.env.TAU_PLAYWRIGHT_PORT ?? '1420';
const mode = process.env.TAU_PLAYWRIGHT_MODE ?? 'chromium';
if (!['chromium', 'webkit', 'ci', 'performance'].includes(mode)) {
  throw new Error(`Unknown TAU_PLAYWRIGHT_MODE: ${mode}`);
}
const performance = mode === 'performance';

export default defineConfig({
  testDir: './tests/e2e',
  testMatch: '**/*.e2e.ts',
  timeout: 30_000,
  expect: {
    timeout: 10_000,
  },
  fullyParallel: true,
  workers: performance ? 1 : 2,
  reporter: 'line',
  use: {
    baseURL: `http://127.0.0.1:${port}`,
    trace: performance
      ? 'off'
      : process.env.CI
        ? 'on-first-retry'
        : 'retain-on-failure',
  },
  webServer: {
    command: `bun run build:e2e && bun run preview:e2e -- --host 127.0.0.1 --port ${port} --strictPort`,
    url: `http://127.0.0.1:${port}`,
    reuseExistingServer: false,
    timeout: 120_000,
  },
  projects: [
    {
      name: 'chromium',
      testIgnore: '**/*-performance.e2e.ts',
      use: { ...devices['Desktop Chrome'] },
    },
    {
      name: 'webkit',
      testIgnore: '**/*-performance.e2e.ts',
      use: { ...devices['Desktop Safari'] },
    },
    {
      name: 'chromium-performance',
      testMatch: '**/*-performance.e2e.ts',
      workers: 1,
      use: { ...devices['Desktop Chrome'], browserName: 'chromium' as const },
    },
  ].filter((project) =>
    mode === 'ci'
      ? project.name !== 'chromium-performance'
      : project.name === (performance ? 'chromium-performance' : mode),
  ),
});
