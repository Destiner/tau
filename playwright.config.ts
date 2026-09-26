import { defineConfig, devices } from '@playwright/test';

const port = process.env.TAU_PLAYWRIGHT_PORT ?? '1420';

export default defineConfig({
  testDir: './tests/e2e',
  testMatch: '**/*.e2e.ts',
  timeout: 30_000,
  expect: {
    timeout: 10_000,
  },
  fullyParallel: false,
  workers: 2,
  reporter: 'line',
  use: {
    baseURL: `http://127.0.0.1:${port}`,
    trace: 'retain-on-failure',
  },
  webServer: {
    command: `bun run dev -- --host 127.0.0.1 --port ${port} --strictPort`,
    url: `http://127.0.0.1:${port}`,
    reuseExistingServer: !process.env.CI,
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
      dependencies: ['chromium', 'webkit'],
      use: { ...devices['Desktop Chrome'], browserName: 'chromium' },
    },
  ],
});
