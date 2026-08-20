import type { Page } from '@playwright/test';

import { expect, test } from './fixtures';

const scenarioUrl = '/?test-scenario=saved-session-stale-generation';
const beforeStaleGate = 'before-stale-generation-output';
const prompt = 'Explain the fixture';
const reply = 'Deterministic reply.';
const sentinel = 'STALE_GENERATION_SENTINEL';

interface VisibleSessionSnapshot {
  transcript: string;
  selectedSessionCount: number;
  selectedSessionText: string;
  heading: string;
  composerEnabled: boolean;
  sendVisible: boolean;
  stopVisible: boolean;
  statuses: string[];
  workingIndicators: number;
  unreadIndicators: number;
}

function expectActiveSessionTranscript(snapshot: VisibleSessionSnapshot): void {
  expect(snapshot).toMatchObject({
    selectedSessionCount: 1,
    heading: 'Main',
    composerEnabled: true,
    unreadIndicators: 0,
  });
  expect(snapshot.selectedSessionText).toContain('Main');
  expect(snapshot.transcript).toContain(prompt);
  expect(snapshot.transcript).toContain(reply);
  expect(snapshot.transcript).not.toContain(sentinel);
}

function expectWorkingActiveSession(snapshot: VisibleSessionSnapshot): void {
  expectActiveSessionTranscript(snapshot);
  expect(snapshot).toMatchObject({
    sendVisible: false,
    stopVisible: true,
    statuses: [],
    workingIndicators: 1,
  });
}

async function snapshotVisibleSession(
  page: Page,
): Promise<VisibleSessionSnapshot> {
  return page.evaluate(() => {
    const selected = [...document.querySelectorAll('.session-row.selected')];
    const composer = document.querySelector<HTMLTextAreaElement>(
      'textarea[aria-label="Message Pi"]',
    );
    const visible = (selector: string): boolean => {
      const element = document.querySelector<HTMLElement>(selector);
      return Boolean(element && element.getClientRects().length > 0);
    };
    return {
      transcript:
        document.querySelector('[aria-label="Tau transcript"]')?.textContent ??
        '',
      selectedSessionCount: selected.length,
      selectedSessionText: selected[0]?.textContent ?? '',
      heading: document.querySelector('h1')?.textContent ?? '',
      composerEnabled: Boolean(composer && !composer.disabled),
      sendVisible: visible('button[aria-label="Send message"]'),
      stopVisible: visible('button[aria-label="Stop Pi"]'),
      statuses: [...document.querySelectorAll('[role="status"]')].map(
        (element) =>
          element.getAttribute('aria-label') ?? element.textContent ?? '',
      ),
      workingIndicators: document.querySelectorAll(
        '[role="img"][aria-label="Working"]',
      ).length,
      unreadIndicators: document.querySelectorAll(
        '[role="img"][aria-label="Unread"]',
      ).length,
    };
  });
}

test('ignores gated output from a stale runtime generation', async ({
  page,
}) => {
  await page.goto(scenarioUrl);

  const composer = page.getByRole('textbox', { name: 'Message Pi' });
  await expect(composer).toBeEnabled();
  await composer.fill(prompt);
  await page.getByRole('button', { name: 'Send message' }).click();

  await page.evaluate(async (gateName) => {
    const scenario = window.__TAU_PI_SCENARIO__;
    if (!scenario) throw new Error('Pi scenario API is unavailable.');
    await scenario.waitForGate(gateName);
  }, beforeStaleGate);
  const beforeRelease = await snapshotVisibleSession(page);

  await page.evaluate(async (gateName) => {
    const scenario = window.__TAU_PI_SCENARIO__;
    if (!scenario) throw new Error('Pi scenario API is unavailable.');
    await scenario.releaseGate(gateName);
  }, beforeStaleGate);
  const afterRelease = await snapshotVisibleSession(page);

  expectWorkingActiveSession(beforeRelease);
  expect(afterRelease).toEqual(beforeRelease);

  const completed = await page.evaluate(() => {
    const scenario = window.__TAU_PI_SCENARIO__;
    if (!scenario) throw new Error('Pi scenario API is unavailable.');
    return {
      scenario: scenario.scenario(),
      gates: scenario.gates(),
      verification: scenario.verify(),
      timeline: scenario.timeline(),
    };
  });
  expect(completed.scenario).toMatchObject({
    name: 'saved-session-stale-generation',
    schemaVersion: 1,
  });
  expect(completed.gates).toEqual([
    {
      name: beforeStaleGate,
      required: true,
      reached: true,
      released: true,
    },
  ]);
  expect(completed.verification.ok).toBe(true);
  expect(completed.timeline.slice(-2)).toEqual([
    expect.objectContaining({
      kind: 'gate-released',
      gate: beforeStaleGate,
    }),
    expect.objectContaining({
      kind: 'output',
      generation: 1,
      output: 'event main@1 message_update',
    }),
  ]);
});
