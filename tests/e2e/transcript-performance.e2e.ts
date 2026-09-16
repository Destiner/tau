import { expect, test } from './fixtures';

const fixtureUrl = '/?fixture=long-transcript';

test.beforeEach(async ({ page }) => {
  await page.goto(fixtureUrl);
  await expect(page.getByTestId('fixture-count')).toHaveText('5000 messages');
  await expect(page.locator('[data-index="4999"]')).toBeVisible();
});

test('keeps frame delivery and mounted rows bounded during a full sweep', async ({
  page,
}, testInfo) => {
  const transcript = page.getByLabel('Transcript');
  const metrics = await transcript.evaluate(async (element) => {
    const duration = 2_500;
    const sampleFrames = async (sampleDuration: number): Promise<number[]> => {
      const samples: number[] = [];
      await new Promise<void>((resolve) => {
        let previousFrame: number | undefined;
        const startedAt = performance.now();
        const frame = (now: number): void => {
          if (previousFrame !== undefined) samples.push(now - previousFrame);
          previousFrame = now;
          if (now - startedAt < sampleDuration) requestAnimationFrame(frame);
          else resolve();
        };
        requestAnimationFrame(frame);
      });
      return samples;
    };
    const baselineIntervals = await sampleFrames(500);
    baselineIntervals.sort((left, right) => left - right);
    const baselineFrameInterval =
      baselineIntervals[Math.floor(baselineIntervals.length / 2)] ?? 16.67;
    const intervals: number[] = [];
    let maximumRows = 0;
    let longTaskDuration = 0;
    let previousFrame = performance.now();
    const startTop = element.scrollTop;

    const observer =
      typeof PerformanceObserver !== 'undefined' &&
      PerformanceObserver.supportedEntryTypes.includes('longtask')
        ? new PerformanceObserver((list) => {
            for (const entry of list.getEntries()) {
              longTaskDuration += entry.duration;
            }
          })
        : undefined;
    observer?.observe({ entryTypes: ['longtask'] });

    await new Promise<void>((resolve) => {
      const startedAt = performance.now();
      const frame = (now: number): void => {
        intervals.push(now - previousFrame);
        previousFrame = now;
        const progress = Math.min((now - startedAt) / duration, 1);
        const eased = progress * progress * (3 - 2 * progress);
        element.scrollTop = startTop * (1 - eased);
        maximumRows = Math.max(
          maximumRows,
          document.querySelectorAll('.message').length,
        );
        if (progress < 1) requestAnimationFrame(frame);
        else resolve();
      };
      requestAnimationFrame(frame);
    });

    observer?.disconnect();
    intervals.sort((left, right) => left - right);
    const percentile95 = intervals[Math.floor(intervals.length * 0.95)] ?? 0;
    const slowFrames = intervals.filter((interval) => interval > 50).length;

    return {
      baselineFrameInterval,
      frameCount: intervals.length,
      frameDeliveryRatio: intervals.length / (duration / baselineFrameInterval),
      longTaskDuration,
      maximumRows,
      percentile95,
      slowFrameRatio: slowFrames / intervals.length,
    };
  });

  await testInfo.attach('transcript-performance-metrics', {
    body: JSON.stringify(metrics, null, 2),
    contentType: 'application/json',
  });

  /*
   * Measured over a 2.5s sweep of 5000 rows: 110-117 frames at 60Hz, 24-25
   * mounted rows, a 50ms 95th percentile, 3-8% slow frames, and 600-780ms of
   * long tasks. Headless Chromium can be scheduled at 30Hz under load, so
   * delivery is compared with its idle cadence instead of assuming 60Hz. The
   * ratio and interval guard still catch a doubling in frame cost.
   */
  expect(metrics.frameDeliveryRatio).toBeGreaterThan(0.6);
  expect(metrics.maximumRows).toBeLessThan(50);
  expect(metrics.percentile95).toBeLessThan(67);
  expect(metrics.slowFrameRatio).toBeLessThan(0.12);
  expect(metrics.longTaskDuration).toBeLessThan(1_000);
  await expect(page.locator('[data-index="0"]')).toBeVisible();
});
