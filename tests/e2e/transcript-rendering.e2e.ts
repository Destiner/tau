import waitForShowcaseRendering from './fixture-readiness';
import { expect, test } from './fixtures';

const fixtureUrl = '/?fixture=long-transcript&showcase=true';

declare global {
  interface Window {
    __TAU_VIEWER_ESCAPE_HANDLER_CALLS__?: number;
  }
}

test.beforeEach(async ({ page }) => {
  await page.goto(fixtureUrl);
  await expect(page.getByTestId('fixture-count')).toHaveText('13 messages');
  await expect(page.locator('[data-index="12"]')).toBeVisible();
  await waitForShowcaseRendering(page);
});

for (const width of [480, 1440]) {
  test(`fits user bubbles to content without overflow at ${width}px`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 900 });
    for (const id of ['fixture-short-user', 'fixture-markdown-showcase-user']) {
      const message = page.locator(`[data-message-id="${id}"]`);
      const bubble = message.locator('.user-bubble');
      await bubble.scrollIntoViewIfNeeded();
      const geometry = await bubble.evaluate((element) => {
        const box = element.getBoundingClientRect();
        const parent = element.closest('.message')!.getBoundingClientRect();
        const paragraph = element.querySelector('p')!;
        const range = document.createRange();
        range.selectNodeContents(paragraph);
        const style = getComputedStyle(element);
        const paragraphStyle = getComputedStyle(paragraph);
        return {
          width: box.width,
          available: parent.width,
          left: box.left,
          right: box.right,
          parentLeft: parent.left,
          parentRight: parent.right,
          scrollWidth: element.scrollWidth,
          clientWidth: element.clientWidth,
          textWidth: range.getBoundingClientRect().width,
          inset:
            parseFloat(style.paddingLeft) +
            parseFloat(style.paddingRight) +
            parseFloat(paragraphStyle.marginLeft) +
            parseFloat(paragraphStyle.marginRight),
        };
      });
      expect(geometry.left).toBeCloseTo(geometry.parentLeft, 1);
      expect(geometry.right).toBeLessThanOrEqual(geometry.parentRight + 1);
      expect(geometry.scrollWidth).toBeLessThanOrEqual(
        geometry.clientWidth + 1,
      );
      if (id === 'fixture-short-user') {
        expect(geometry.width).toBeCloseTo(
          geometry.textWidth + geometry.inset,
          0,
        );
        expect(geometry.width).toBeLessThan(geometry.available - 20);
      } else {
        expect(geometry.width).toBeCloseTo(geometry.available, 0);
      }
    }
  });

  test(`masks only visible activity controls without shifting geometry at ${width}px`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 900 });
    for (const state of ['running', 'failed', 'done']) {
      const row = page.locator(`[data-message-id="fixture-tool-${state}"]`);
      const header = row.locator('.activity-header');
      await header.scrollIntoViewIfNeeded();
      await page.mouse.move(0, 0);
      const rest = await header.evaluate(activityGeometry);
      expect(rest.disclosureWidth).toBe(0);
      expect(rest.argumentOverflow).toBe(true);
      expect(rest.argument.right).toBeGreaterThanOrEqual(rest.end.right);
      expect(rest.endOnTop).toBe(state !== 'done');
      expect(rest.end.width).toBeCloseTo(rest.mark.width + 3, 1);
      if (state !== 'done') {
        expect(rest.background).not.toBe('rgba(0, 0, 0, 0)');
        expect(rest.end.right).toBeCloseTo(rest.mark.right, 1);
      } else {
        expect(rest.background).toBe('rgba(0, 0, 0, 0)');
      }

      const assertRevealed = async (): Promise<void> => {
        const active = await header.evaluate(activityGeometry);
        expect(active.disclosureWidth).toBeGreaterThan(0);
        expect(active.end.width).toBeCloseTo(
          rest.end.width + active.disclosureWidth + (state === 'done' ? 0 : 6),
          1,
        );
        expect(active.backgroundImage).toContain('linear-gradient');
        expect(active.endOnTop).toBe(true);
        expect(active.end.right).toBeCloseTo(rest.end.right, 1);
        expect(active.mark).toEqual(rest.mark);
        expect(active.header).toEqual(rest.header);
        expect(active.name).toEqual(rest.name);
        expect(active.argument).toEqual(rest.argument);
      };
      await header.hover();
      await assertRevealed();
      await page.mouse.move(0, 0);
      // Tab establishes keyboard modality before programmatically selecting this row.
      await page.keyboard.press('Tab');
      await header.focus();
      await expect(header).toBeFocused();
      expect(
        await header.evaluate((element) => element.matches(':focus-visible')),
      ).toBe(true);
      await assertRevealed();
      await page.keyboard.press('Enter');
      await expect(header).toHaveAttribute('aria-expanded', 'true');
      await assertRevealed();
      await page.keyboard.press('Enter');
      await expect(header).toHaveAttribute('aria-expanded', 'false');
      await assertRevealed();
      await header.evaluate((element) => element.blur());
      expect(await header.evaluate(activityGeometry)).toEqual(rest);
    }
  });
}

interface ActivityRect {
  left: number;
  right: number;
  top: number;
  width: number;
  height: number;
}

interface ActivityGeometry {
  argumentOverflow: boolean;
  endOnTop: boolean;
  header: { width: number; height: number };
  name: ActivityRect;
  argument: ActivityRect;
  end: ActivityRect;
  mark: ActivityRect;
  disclosureWidth: number;
  background: string;
  backgroundImage: string;
}

function activityGeometry(element: HTMLElement): ActivityGeometry {
  const rect = (selector: string): ActivityRect => {
    const box = element.querySelector(selector)?.getBoundingClientRect();
    const header = element.getBoundingClientRect();
    return box
      ? {
          left: box.left - header.left,
          right: box.right - header.left,
          top: box.top - header.top,
          width: box.width,
          height: box.height,
        }
      : { left: 0, right: 0, top: 0, width: 0, height: 0 };
  };
  const end = element.querySelector('.activity-end')!;
  const style = getComputedStyle(end);
  const endBox = end.getBoundingClientRect();
  const argument = element.querySelector('.activity-argument')!;
  const topmost = document.elementFromPoint(
    endBox.left + 1,
    endBox.top + endBox.height / 2,
  );
  return {
    argumentOverflow: argument.scrollWidth > argument.clientWidth,
    endOnTop: topmost === end || (topmost !== null && end.contains(topmost)),
    header: {
      width: element.getBoundingClientRect().width,
      height: element.getBoundingClientRect().height,
    },
    name: rect('.activity-name'),
    argument: rect('.activity-argument'),
    end: rect('.activity-end'),
    mark: rect('.activity-mark'),
    disclosureWidth: rect('.activity-disclosure').width,
    background: style.backgroundColor,
    backgroundImage: style.backgroundImage,
  };
}

test('renders the showcase activity, Markdown and code contract', async ({
  page,
}) => {
  await test.step('renders expandable skill, thinking, and tool activity rows', async () => {
    {
      const skill = page.locator('[data-message-id="fixture-skill-4998"]');
      const header = skill.locator('.activity-header');

      await expect(skill.locator('.activity-name')).toHaveText('skill');
      await expect(skill.locator('.activity-argument')).toHaveText(
        'desktop-app-native-feel',
      );
      await expect(header).not.toContainText(
        'Focus on keyboard behavior and perceived performance',
      );
      await expect(header).toHaveAttribute('aria-expanded', 'false');
      await expect(skill.locator('.activity-details')).toHaveCount(0);

      await header.click();

      await expect(header).toHaveAttribute('aria-expanded', 'true');
      await expect(skill.locator('.activity-detail-label')).toHaveText([
        'Prompt',
        'Instructions',
      ]);
      await expect(skill.locator('.activity-details')).toContainText(
        'Focus on keyboard behavior and perceived performance',
      );
      await expect(skill.locator('.activity-details')).toContainText(
        'Inspect selection, scrolling, keyboard behavior',
      );
    }
    {
      const thought = page.locator(
        '[data-message-id="fixture-thinking-trace"]',
      );
      const header = thought.locator('.activity-header');

      await expect(thought.locator('.activity-name')).toHaveText('thinking');
      await expect(thought).not.toContainText('The adoption walk stops');
      await expect(header).toHaveAttribute('aria-expanded', 'false');

      await header.click();

      await expect(header).toHaveAttribute('aria-expanded', 'true');
      await expect(thought.locator('.activity-details')).toContainText(
        'The adoption walk stops at the first local error row',
      );
    }
    {
      const thought = page.locator(
        '[data-message-id="fixture-thinking-sections"]',
      );
      await thought.locator('.activity-header').click();

      const paragraphs = thought.locator('.activity-thinking-prose p');
      const emphasis = paragraphs.locator('strong');
      await expect(paragraphs).toHaveCount(2);
      await expect(emphasis).toHaveCount(2);
    }
    {
      const running = page.locator('[data-message-id="fixture-tool-running"]');
      const failed = page.locator('[data-message-id="fixture-tool-failed"]');
      const done = page.locator('[data-message-id="fixture-tool-done"]');

      const runningMark = running.locator('.activity-mark.running');
      const failedMark = failed.locator('svg.activity-mark.failed');
      await expect(runningMark).toHaveCount(1);
      await expect(failedMark).toHaveCount(1);
      await expect(done.locator('.activity-mark')).toHaveCount(0);

      await failed.locator('.activity-header').click();

      await expect(failed.locator('.activity-detail-label')).toHaveText([
        'Arguments',
        'Error',
      ]);
      await expect(
        failed.locator('.activity-detail-body.failed'),
      ).toContainText('no match found for the replacement anchor');
    }
  });

  await test.step('opens a running tool from its end control', async () => {
    const tool = page.locator('[data-message-id="fixture-tool-running"]');
    const header = tool.locator('.activity-header');
    const end = tool.locator('.activity-end');

    await expect(tool.locator('.activity-mark.running')).toBeVisible();
    await end.click();
    await expect(header).toHaveAttribute('aria-expanded', 'true');
  });

  await test.step('renders Markdown table headers and alignment', async () => {
    const headers = page
      .locator('[data-message-id="fixture-markdown-showcase"] table')
      .first()
      .locator('thead th');

    await expect(headers.nth(0)).toHaveCSS('text-align', 'left');
    await expect(headers.nth(1)).toHaveCSS('text-align', /^(?:-webkit-)?left$/);
    await expect(headers.nth(2)).toHaveCSS(
      'text-align',
      /^(?:-webkit-)?center$/,
    );
    await expect(headers.nth(3)).toHaveCSS(
      'text-align',
      /^(?:-webkit-)?right$/,
    );

    const tables = page.locator(
      '[data-message-id="fixture-markdown-showcase"] table',
    );
    const emptyHeader = tables.nth(2);
    const partialHeader = tables.nth(3);

    await expect(emptyHeader.locator('thead')).toHaveCount(0);
    await expect(emptyHeader.locator('tbody td').first()).toHaveCSS(
      'text-align',
      /^(?:-webkit-)?left$/,
    );
    await expect(emptyHeader.locator('tbody td').nth(1)).toHaveCSS(
      'text-align',
      /^(?:-webkit-)?right$/,
    );
    await expect(partialHeader.locator('thead th')).toHaveText(['', 'Kept']);
  });

  await test.step('keeps Markdown media inside the message column', async () => {
    const showcase = page.locator(
      '[data-message-id="fixture-markdown-showcase"]',
    );
    const image = showcase.locator('img');
    await expect(image).toHaveJSProperty('naturalWidth', 1_200);
    await expect(image).toBeVisible();
    const [drawn, column] = await Promise.all([
      image.boundingBox(),
      showcase.locator('.markdown').first().boundingBox(),
    ]);
    expect(drawn?.width ?? 0).toBeLessThanOrEqual(column?.width ?? 0);
    expect(drawn?.width ?? 0).toBeGreaterThan(0);
  });

  await test.step('preserves inline shortcut HTML in sanitized Markdown', async () => {
    const showcase = page.locator(
      '[data-message-id="fixture-markdown-showcase"]',
    );
    await expect(showcase.locator('kbd').first()).toHaveText('Cmd');
  });

  await test.step('highlights fenced code across schemes and labels its source language', async () => {
    {
      const block = page
        .locator('[data-message-id="fixture-markdown-showcase"] .code-block')
        .first();

      await expect(block.locator('[data-tau-highlight]')).toBeVisible();

      const light = await block.evaluate(tokenColors);
      expect(light.length).toBeGreaterThan(3);

      // Both schemes ride along on every token, so the dark palette is a repaint
      // rather than another pass over the markdown.
      await page.emulateMedia({ colorScheme: 'dark' });
      const dark = await block.evaluate(tokenColors);
      expect(dark.length).toBeGreaterThan(3);
      expect(dark).not.toEqual(light);
      await page.emulateMedia({ colorScheme: 'light' });
    }
    {
      const showcase = page.locator(
        '[data-message-id="fixture-markdown-showcase"]',
      );
      const block = showcase.locator('.code-block').first();

      // The tag as written, not the grammar it resolves to: `console` is the label
      // even though a shell grammar draws it.
      await expect(block).toHaveAttribute('data-tau-lang', 'ts');
      await expect(
        showcase.locator('.code-block[data-tau-lang="console"]'),
      ).toHaveCount(1);
      await expect(
        showcase.locator('.code-block:not([data-tau-lang])'),
      ).toHaveCount(1);

      // The label keeps the copy button's terms: shown to a reader who is pointing
      // at the block, and out of the way otherwise.
      await expect.poll(() => block.evaluate(labelOpacity)).toBe('0');
      await block.hover();
      await expect.poll(() => block.evaluate(labelOpacity)).not.toBe('0');
      expect(await block.evaluate(labelContent)).toContain('ts');
    }
  });
});

test('draws diagrams and opens their pan-and-zoom viewer', async ({ page }) => {
  await test.step('draws fenced diagrams and preserves their graph content', async () => {
    const showcase = page.locator(
      '[data-message-id="fixture-markdown-showcase"]',
    );
    const diagram = showcase.locator('.diagram svg').first();
    await expect(showcase.locator('.diagram')).toHaveCount(5, {
      timeout: 20_000,
    });
    await expect(
      diagram.locator('text', { hasText: 'Session live?' }),
    ).toHaveCount(1);
    await expect(diagram.locator('g.node')).toHaveCount(11);
    await expect(diagram.locator('polyline.edge')).toHaveCount(11);
    await expect(diagram.locator('g.node[data-shape="diamond"]')).toHaveCount(
      3,
    );
    await expect(diagram.locator('g.node[data-id="F"]')).toHaveAttribute(
      'data-label',
      'Route exists\nfor both sides?',
    );
    await expect(diagram.locator('g.node[data-id="I"]')).toHaveAttribute(
      'data-label',
      'Delivery only\n+ dynamic preview\n+ no result?',
    );
    for (const label of ['F', 'I', 'for']) {
      await expect(
        diagram.locator(`g.node[data-label="${label}"]`),
      ).toHaveCount(0);
    }
    const codeDiagram = showcase.locator('.diagram').filter({
      has: page.locator(
        'g.node[data-id="A"][data-label="metadata.calls = request.tasks ?? []"]',
      ),
    });
    await expect(codeDiagram).toHaveCount(1);
    const codeSvg = codeDiagram.locator('svg');
    for (const [id, label, shape] of [
      ['A', 'metadata.calls = request.tasks ?? []', 'rectangle'],
      ['B', 'execute([bundle])', 'rectangle'],
      ['C', 'execute([parent, ...children])', 'rectangle'],
      ['D', 'root authority', 'diamond'],
      ['E', 'submit {proofs: []}, sponsored: true', 'rectangle'],
    ] as const) {
      const node = codeSvg.locator(`g.node[data-id="${id}"]`);
      await expect(node).toHaveAttribute('data-label', label);
      await expect(node).toHaveAttribute('data-shape', shape);
      await expect(node.locator('text')).toHaveText(label);
    }
    await expect(codeSvg.locator('polyline.edge')).toHaveCount(4);
    const syntheticDiagram = showcase.locator('.diagram').filter({
      has: page.locator(
        'g.node[data-id="Complete"][data-label="Ready to ship"]',
      ),
    });
    await expect(syntheticDiagram).toHaveCount(1);
    const syntheticSvg = syntheticDiagram.locator('svg');
    await expect(syntheticSvg.locator('g.node')).toHaveCount(7);
    await expect(syntheticSvg.locator('polyline.edge')).toHaveCount(8);
    const compactDiagram = showcase.locator('.diagram').filter({
      has: page.locator('g.node[data-id="S"][data-label="Start; α"]'),
    });
    await expect(compactDiagram).toHaveCount(1);
    const compactSvg = compactDiagram.locator('svg');
    await expect(compactSvg.locator('g.node')).toHaveCount(11);
    await expect(compactSvg.locator('polyline.edge')).toHaveCount(13);
    await expect(compactSvg.locator('g.node[data-id="R"]')).toHaveAttribute(
      'data-shape',
      'diamond',
    );
    const compactEdge = compactSvg.locator(
      'polyline.edge[data-from="R"][data-to="T"]',
    );
    await expect(compactEdge).toHaveAttribute(
      'data-label',
      'remaining-deadline fires',
    );
    await expect(compactEdge).toHaveAttribute('stroke-dasharray', /.+/);
    await expect(compactEdge).toHaveAttribute('marker-end', /url\(#.+\)/);
    await expect(compactEdge).not.toHaveAttribute('marker-start', /.+/);
    await expect(compactDiagram.locator('.diagram-expand')).toHaveCount(1);
    await expect(
      syntheticSvg.locator('g.node[data-id="Complete"]'),
    ).toHaveAttribute('data-shape', 'parallelogram');
    const sourceBlocks = showcase.locator(
      '.code-block[data-tau-lang="mermaid"]',
    );
    await expect(sourceBlocks).toHaveCount(4);
    const incomplete = sourceBlocks.filter({ hasText: 'An incomplete label' });
    await expect(incomplete).toContainText('B --> C');
    await expect(incomplete.locator('.diagram-expand')).toHaveCount(0);
    const malformed = sourceBlocks.filter({ hasText: 'trailing garbage' });
    await expect(malformed).toContainText('A --> B trailing garbage');
    await expect(malformed.locator('.diagram-expand')).toHaveCount(0);
    const width = await diagram.evaluate(
      (svg) =>
        svg.getBoundingClientRect().width /
        (svg.closest('.markdown')?.getBoundingClientRect().width ?? 1),
    );
    expect(width).toBeLessThanOrEqual(1);
  });

  await test.step('expands a drawn diagram into a fullscreen pan-and-zoom viewer', async () => {
    const showcase = page.locator(
      '[data-message-id="fixture-markdown-showcase"]',
    );
    const figure = showcase.locator('.diagram').filter({
      has: page.locator(
        'g.node[data-id="Complete"][data-label="Ready to ship"]',
      ),
    });
    const button = figure.locator('.diagram-expand');
    await expect(figure).toHaveCount(1, { timeout: 20_000 });
    await expect(showcase.locator('.diagram')).toHaveCount(5);
    await expect(showcase.locator('.diagram-expand')).toHaveCount(5);
    await expect
      .poll(() =>
        button.evaluate((element) => getComputedStyle(element).opacity),
      )
      .toBe('0');
    await figure.hover();
    await expect
      .poll(() =>
        button.evaluate((element) => getComputedStyle(element).opacity),
      )
      .not.toBe('0');
    await button.click();
    const viewer = page.locator('.diagram-viewer');
    await expect(viewer).toBeVisible();
    await expect(viewer).toHaveRole('dialog');
    await expect(viewer).toHaveAccessibleName('Diagram');
    const drawing = viewer.locator('.viewer-drawing');
    const vector = drawing.locator(':scope > svg');
    await expect(
      vector.locator('text', { hasText: 'Ready to ship' }),
    ).toHaveCount(1);
    await expect(vector.locator('g.node')).toHaveCount(7);
    await expect(vector.locator('polyline.edge')).toHaveCount(8);
    await expect(vector.locator('g.node[data-id="Complete"]')).toHaveAttribute(
      'data-shape',
      'parallelogram',
    );
    await expect(vector).toHaveAttribute('width', '100%');
    await expect(vector).toHaveAttribute('height', '100%');
    await expect(vector).toHaveAttribute('preserveAspectRatio', 'none');
    const compositingStyles = await vector.evaluate((element) => {
      const styles: Array<{ transform: string; willChange: string }> = [];
      for (
        let current: Element | null = element;
        current;
        current = current.parentElement
      ) {
        const style = getComputedStyle(current);
        styles.push({
          transform: style.transform,
          willChange: style.willChange,
        });
      }
      return styles;
    });
    expect(
      compositingStyles.every(({ transform }) => transform === 'none'),
    ).toBe(true);
    expect(
      compositingStyles.every(({ willChange }) => willChange === 'auto'),
    ).toBe(true);
    const canvas = viewer.locator('.viewer-canvas');
    const canvasBox = (await canvas.boundingBox())!;
    const viewBox = (): Promise<[number, number, number, number]> =>
      vector.evaluate((element) => {
        const box = (element as SVGSVGElement).viewBox.baseVal;
        return [box.x, box.y, box.width, box.height];
      });
    const scaleOf = async (): Promise<number> => {
      const box = await viewBox();
      return canvasBox.width / box[2];
    };
    const fitted = await scaleOf();
    const zoomPoint = { x: 400, y: 300 };
    const beforeZoom = await viewBox();
    const worldAtPointer = {
      x: beforeZoom[0] + (zoomPoint.x / canvasBox.width) * beforeZoom[2],
      y: beforeZoom[1] + (zoomPoint.y / canvasBox.height) * beforeZoom[3],
    };
    await canvas.dispatchEvent('wheel', {
      deltaY: -200,
      ctrlKey: true,
      clientX: zoomPoint.x,
      clientY: zoomPoint.y,
    });
    await expect.poll(scaleOf).toBeGreaterThan(fitted);
    const afterZoom = await viewBox();
    expect(
      afterZoom[0] + (zoomPoint.x / canvasBox.width) * afterZoom[2],
    ).toBeCloseTo(worldAtPointer.x, 3);
    expect(
      afterZoom[1] + (zoomPoint.y / canvasBox.height) * afterZoom[3],
    ).toBeCloseTo(worldAtPointer.y, 3);
    const beforePan = await viewBox();
    await canvas.dispatchEvent('wheel', { deltaX: 40, deltaY: 60 });
    await expect.poll(viewBox).not.toEqual(beforePan);
    expect(await scaleOf()).toBeGreaterThan(fitted);
    const box = (await canvas.boundingBox())!;
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
    await page.mouse.down();
    await page.mouse.move(box.x + box.width / 2 + 120, box.y + box.height / 2);
    await page.mouse.up();
    await expect(viewer).toBeVisible();
    await canvas.click({ position: { x: 40, y: 40 } });
    await expect(viewer).toHaveCount(0);
    await expect(button).toBeFocused();
    await page.keyboard.press('Enter');
    await expect(viewer).toBeVisible();
    await page.evaluate(() => {
      window.__TAU_VIEWER_ESCAPE_HANDLER_CALLS__ = 0;
      document.addEventListener('keydown', (event) => {
        if (event.key === 'Escape') {
          window.__TAU_VIEWER_ESCAPE_HANDLER_CALLS__! += 1;
        }
      });
    });
    await page.keyboard.press('Escape');
    await expect(viewer).toHaveCount(0);
    await expect(button).toBeFocused();
    expect(
      await page.evaluate(() => window.__TAU_VIEWER_ESCAPE_HANDLER_CALLS__),
    ).toBe(0);
  });

  await test.step('opens the compact dotted diagram with all edges intact', async () => {
    const showcase = page.locator(
      '[data-message-id="fixture-markdown-showcase"]',
    );
    const figure = showcase.locator('.diagram').filter({
      has: page.locator('g.node[data-id="S"][data-label="Start; α"]'),
    });
    const button = figure.locator('.diagram-expand');
    await button.click({ force: true });
    const viewer = page.locator('.diagram-viewer');
    await expect(viewer.locator('g.node')).toHaveCount(11);
    await expect(viewer.locator('polyline.edge')).toHaveCount(13);
    await expect(
      viewer.locator('polyline.edge[data-from="R"][data-to="T"]'),
    ).toHaveAttribute('data-label', 'remaining-deadline fires');
    await page.keyboard.press('Escape');
    await expect(viewer).toHaveCount(0);
    await expect(button).toBeFocused();
  });
});

/** The distinct colours a highlighted block's tokens are drawn in. */
function tokenColors(element: HTMLElement): string[] {
  const spans = Array.from(
    element.querySelectorAll<HTMLElement>('[data-tau-highlight] span'),
  );
  return [...new Set(spans.map((span) => getComputedStyle(span).color))];
}

function labelOpacity(element: HTMLElement): string {
  return getComputedStyle(element, '::before').opacity;
}

function labelContent(element: HTMLElement): string {
  return getComputedStyle(element, '::before').content;
}
