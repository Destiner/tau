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
      const prose = thought.locator('.activity-thinking-prose');
      await expect(thought.locator('.activity-details')).toContainText(
        'The adoption walk stops at the first local error row',
      );
      await expect(prose).toHaveCSS('color', 'rgb(110, 117, 127)');
      await expect(prose).toHaveCSS('font-size', '11px');
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
      await expect(paragraphs.first()).toHaveCSS('margin-top', '0px');
      await expect(paragraphs.first()).toHaveCSS('margin-bottom', '0px');
      await expect(paragraphs.last()).toHaveCSS('margin-top', '0px');
      await expect(paragraphs.last()).toHaveCSS('margin-bottom', '0px');
      await expect(emphasis.first()).toHaveCSS('font-weight', '400');
      await expect(emphasis.first()).toHaveCSS('color', 'rgb(110, 117, 127)');
      await expect(emphasis.first()).toHaveCSS('font-size', '11px');
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
      await expect(runningMark).toHaveCSS('width', '11px');
      await expect(failedMark).toHaveCSS('width', '10px');
      await expect(failedMark).toHaveCSS('height', '10px');
      await expect(failedMark).toHaveCSS('font-size', '10px');

      const runningBox = await runningMark.boundingBox();
      const failedBox = await failedMark.boundingBox();
      expect(runningBox).not.toBeNull();
      expect(failedBox).not.toBeNull();
      const runningEnd = (runningBox?.x ?? 0) + (runningBox?.width ?? 0);
      const failedEnd = (failedBox?.x ?? 0) + (failedBox?.width ?? 0);
      expect(Math.abs(runningEnd - failedEnd)).toBeLessThan(1);

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

  await test.step('renders task markers, tables, and message bubble geometry', async () => {
    {
      const showcase = page.locator(
        '[data-message-id="fixture-markdown-showcase"]',
      );
      const task = showcase
        .locator('li', { has: page.locator('input[type="checkbox"]') })
        .first();
      const plain = showcase.locator('li').first();

      // The checkbox is the marker: a disc beside it would be a second one.
      await expect(task).toHaveCSS('list-style-type', 'none');
      await expect(plain).toHaveCSS('list-style-type', 'disc');

      // Task boxes are disabled inputs, which the app fades everywhere else.
      await expect(task.locator('input[type="checkbox"]')).toHaveCSS(
        'opacity',
        '1',
      );

      // The box hangs into the marker column, leaving both items on one text column.
      const [checkbox, taskItem, plainItem] = await Promise.all([
        task.locator('input[type="checkbox"]').boundingBox(),
        task.boundingBox(),
        plain.boundingBox(),
      ]);
      expect(taskItem?.x).toBeCloseTo(plainItem?.x ?? 0, 0);
      expect(checkbox?.x ?? 0).toBeLessThan(taskItem?.x ?? 0);
    }
    {
      const headers = page
        .locator('[data-message-id="fixture-markdown-showcase"] table')
        .first()
        .locator('thead th');

      await expect(headers.nth(0)).toHaveCSS('text-align', 'left');
      await expect(headers.nth(1)).toHaveCSS(
        'text-align',
        /^(?:-webkit-)?left$/,
      );
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

      const bubble = page.locator(
        '[data-message-id="fixture-markdown-showcase-user"] .user-bubble',
      );

      const [header, bubbleBackground] = await Promise.all([
        bubble.locator('thead th').first().evaluate(backgroundColor),
        bubble.evaluate(backgroundColor),
      ]);
      expect(header).not.toBe(bubbleBackground);
    }
    {
      const shortMessage = page.locator(
        '[data-message-id="fixture-short-user"]',
      );
      const shortBubble = shortMessage.locator('.user-bubble');
      const longMessage = page.locator(
        '[data-message-id="fixture-markdown-showcase-user"]',
      );
      const longBubble = longMessage.locator('.user-bubble');

      await expect(shortBubble).toHaveCSS(
        'background-color',
        'rgb(235, 238, 240)',
      );

      const [shortMessageBox, shortBubbleBox, longMessageBox, longBubbleBox] =
        await Promise.all([
          shortMessage.boundingBox(),
          shortBubble.boundingBox(),
          longMessage.boundingBox(),
          longBubble.boundingBox(),
        ]);
      expect(shortBubbleBox?.width ?? 0).toBeLessThan(
        (shortMessageBox?.width ?? 0) * 0.9,
      );
      expect(longBubbleBox?.width ?? 0).toBeCloseTo(
        (longMessageBox?.width ?? 0) * 0.9,
        0,
      );
    }
  });

  await test.step('keeps notices compact and Markdown media inside the message column', async () => {
    await expect(page.locator('html')).toHaveCSS('font-size', '13px');
    for (const index of [4990]) {
      const notice = page.locator(
        `[data-message-id="fixture-notice-${index}"]`,
      );
      await expect(notice.locator('.notice-label')).toHaveCSS(
        'font-size',
        '9px',
      );
      await expect(notice.locator('.notice-message')).toHaveCSS(
        'font-size',
        '12px',
      );
      await expect(notice.locator('.notice-message')).toHaveCSS(
        'line-height',
        '18px',
      );
    }
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

  await test.step('renders sanitized Markdown typography, keys, and prose widths', async () => {
    {
      const showcase = page.locator(
        '[data-message-id="fixture-markdown-showcase"]',
      );

      // Inline HTML the agent writes for a shortcut, and a themed `hr`: both are
      // dropped by a sanitizer that does not allow them.
      await expect(showcase.locator('kbd').first()).toHaveText('Cmd');
      await expect(showcase.locator('hr')).toHaveCSS('border-top-width', '1px');
    }
    {
      const showcase = page.locator(
        '[data-message-id="fixture-markdown-showcase"]',
      );

      await expect(showcase.locator('em')).toHaveCSS('font-style', 'italic');
      await expect(showcase.locator('strong').first()).toHaveCSS(
        'font-weight',
        '600',
      );
      await expect(showcase.locator('h1')).toHaveCSS('font-weight', '650');

      const italicLoaded = await page.evaluate(async () => {
        await document.fonts.ready;
        let found = false;
        document.fonts.forEach((face) => {
          if (
            face.family.includes('Inter Variable') &&
            face.style === 'italic'
          ) {
            found = true;
          }
        });
        return found;
      });
      expect(italicLoaded).toBe(true);
    }
    {
      const showcase = page.locator(
        '[data-message-id="fixture-markdown-showcase"]',
      );
      const markdown = showcase.locator('.markdown').first();
      const prose = markdown.locator('> p').first();
      const block = markdown.locator('.code-block').first();
      await expect(prose).toBeVisible();
      await expect(block).toBeVisible();

      const [markdownBox, proseBox, blockBox] = await Promise.all([
        markdown.boundingBox(),
        prose.boundingBox(),
        block.boundingBox(),
      ]);
      expect(proseBox?.width ?? 0).toBeLessThan(markdownBox?.width ?? 0);
      expect(blockBox?.width ?? 0).toBeCloseTo(markdownBox?.width ?? 0, 0);

      const leading = await block.locator('code').evaluate((code) => {
        const style = getComputedStyle(code);
        return (
          Number.parseFloat(style.lineHeight) /
          Number.parseFloat(style.fontSize)
        );
      });
      expect(leading).toBeCloseTo(1.5, 2);
    }
  });

  await test.step('highlights fenced code across schemes and labels its source language', async () => {
    {
      const block = page
        .locator('[data-message-id="fixture-markdown-showcase"] .code-block')
        .first();

      await expect(block.locator('pre')).toHaveClass(/shiki/);

      // A comment is a scope of its own, and the one place the palette leans on
      // italics: a grammar that failed to load would leave the block one colour.
      const comment = block.locator('span', { hasText: 'A comment' }).last();
      await expect(comment).toHaveCSS('font-style', 'italic');

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
      expect(
        await block.evaluate(
          (element) => getComputedStyle(element, '::before').fontSize,
        ),
      ).toBe('10px');
    }
  });
});

test('draws a themed diagram and opens its pan-and-zoom viewer', async ({
  page,
}) => {
  await test.step('draws a fenced diagram in the scheme around it', async () => {
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
    const block = showcase.locator('.diagram').first();
    await expect(block).toHaveCSS('border-top-width', '1px');
    await expect(block).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
    const width = await diagram.evaluate(
      (svg) =>
        svg.getBoundingClientRect().width /
        (svg.closest('.markdown')?.getBoundingClientRect().width ?? 1),
    );
    expect(width).toBeLessThanOrEqual(1);
    const light = await diagram.evaluate(diagramColors);
    await page.emulateMedia({ colorScheme: 'dark' });
    expect(await diagram.evaluate(diagramColors)).not.toEqual(light);
    await page.emulateMedia({ colorScheme: 'light' });
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
    const naturalSize = await figure.evaluate((element) => {
      const svg = element.querySelector('svg');
      return {
        width: Number.parseFloat(svg?.getAttribute('width') ?? ''),
        height: Number.parseFloat(svg?.getAttribute('height') ?? ''),
      };
    });
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
    expect(fitted).toBeCloseTo(
      Math.min(
        (canvasBox.width - 96) / naturalSize.width,
        (canvasBox.height - 96) / naturalSize.height,
        1.5,
      ),
      3,
    );
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

function backgroundColor(element: HTMLElement): string {
  return getComputedStyle(element).backgroundColor;
}

/** The distinct colours a highlighted block's tokens are drawn in. */
function tokenColors(element: HTMLElement): string[] {
  const spans = Array.from(element.querySelectorAll<HTMLElement>('pre span'));
  return [...new Set(spans.map((span) => getComputedStyle(span).color))];
}

/** The fills a drawn diagram is painted with, which its theme decides. */
function diagramColors(element: SVGElement): string[] {
  const painted = Array.from(
    element.querySelectorAll<SVGElement>('rect, text, polyline'),
  );
  return [
    ...new Set(painted.map((node) => getComputedStyle(node).fill)),
  ].sort();
}

function labelOpacity(element: HTMLElement): string {
  return getComputedStyle(element, '::before').opacity;
}

function labelContent(element: HTMLElement): string {
  return getComputedStyle(element, '::before').content;
}
