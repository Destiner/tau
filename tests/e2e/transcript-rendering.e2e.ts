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

      await expect(block.locator('pre')).toHaveClass(/shiki/);

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
    await expect(diagram.locator('.pele-node')).toHaveCount(11);
    await expect(diagram.locator('.pele-edge')).toHaveCount(11);
    await expect(diagram.locator('.pele-shape-diam')).toHaveCount(3);
    await expect(diagram.locator('.pele-node[data-id="F"] tspan')).toHaveText([
      'Route exists',
      'for both sides?',
    ]);
    await expect(diagram.locator('.pele-node[data-id="I"] tspan')).toHaveText([
      'Delivery only',
      '+ dynamic preview',
      '+ no result?',
    ]);
    for (const label of ['F', 'I', 'for']) {
      await expect(
        diagram
          .locator('.pele-label')
          .filter({ hasText: new RegExp(`^${label}$`) }),
      ).toHaveCount(0);
    }
    const codeDiagram = showcase.locator('.diagram').filter({
      has: page.locator('.pele-node[data-id="A"]', {
        hasText: 'metadata.calls =',
      }),
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
      const node = codeSvg.locator(`.pele-node[data-id="${id}"]`);
      await expect(node).toHaveClass(
        new RegExp(`pele-shape-${shape === 'diamond' ? 'diam' : 'rect'}`),
      );
      await expect
        .poll(() =>
          node.locator('text').evaluate((text) => {
            const spans = Array.from(text.querySelectorAll('tspan'));
            const label = spans.length
              ? spans.map((span) => span.textContent).join(' ')
              : text.textContent;
            return label?.replace(/\s+/g, ' ').trim();
          }),
        )
        .toBe(label);
    }
    await expect(codeSvg.locator('.pele-edge')).toHaveCount(4);
    const syntheticDiagram = showcase.locator('.diagram').filter({
      has: page.locator('.pele-node[data-id="Complete"]'),
    });
    await expect(syntheticDiagram).toHaveCount(1);
    const syntheticSvg = syntheticDiagram.locator('svg');
    await expect(syntheticSvg.locator('.pele-node')).toHaveCount(7);
    await expect(syntheticSvg.locator('.pele-edge')).toHaveCount(8);
    const compactDiagram = showcase.locator('.diagram').filter({
      has: page.locator('.pele-node[data-id="S"]'),
    });
    await expect(compactDiagram).toHaveCount(1);
    const compactSvg = compactDiagram.locator('svg');
    await expect(compactSvg.locator('.pele-node')).toHaveCount(11);
    await expect(compactSvg.locator('.pele-edge')).toHaveCount(13);
    await expect(compactSvg.locator('.pele-node[data-id="R"]')).toHaveClass(
      /pele-shape-diam/,
    );
    const compactEdge = compactSvg.locator('.pele-edge[data-id="L_R_T_0"]');
    await expect(
      compactSvg.locator('.pele-edge-label[data-id="L_R_T_0"]'),
    ).toHaveText('remaining-deadline fires');
    await expect(compactEdge.locator('path').first()).toHaveAttribute(
      'stroke-dasharray',
      /.+/,
    );
    await expect(compactEdge.locator('.pele-marker')).toHaveCount(1);
    // Stable edge identifiers preserve fan-in, cycle, and dotted endpoint semantics.
    for (const [from, to] of [
      ['P', 'W'],
      ['P2', 'W'],
      ['E', 'W'],
      ['Z', 'R'],
      ['W', 'T'],
    ]) {
      await expect(
        compactSvg.locator(`.pele-edge[data-id="L_${from}_${to}_0"]`),
      ).toHaveCount(1);
    }
    await expect(compactDiagram.locator('.diagram-expand')).toHaveCount(1);
    await expect(
      syntheticSvg.locator('.pele-node[data-id="Complete"]'),
    ).toHaveClass(/pele-shape-lean-r/);
    await expect(
      syntheticSvg.locator(
        '.pele-edge-label[data-id="L_Regression_Topology_0"] tspan',
      ),
    ).toHaveText(['two', 'lines']);
    await expect(
      syntheticSvg.locator('.pele-edge-label[data-id="L_Topology_Viewer_0"]'),
    ).toHaveText('count ≤ 7');
    await expect(
      syntheticSvg.locator('.pele-edge-label[data-id="L_Viewer_Complete_0"]'),
    ).toHaveText('count > 7');
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
      has: page.locator('.pele-node[data-id="Complete"]'),
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
    await expect(vector.locator('.pele-node')).toHaveCount(7);
    await expect(vector.locator('.pele-edge')).toHaveCount(8);
    await expect(vector.locator('.pele-node[data-id="Complete"]')).toHaveClass(
      /pele-shape-lean-r/,
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
      has: page.locator('.pele-node[data-id="S"]'),
    });
    const button = figure.locator('.diagram-expand');
    await button.click({ force: true });
    const viewer = page.locator('.diagram-viewer');
    await expect(viewer.locator('.pele-node')).toHaveCount(11);
    await expect(viewer.locator('.pele-edge')).toHaveCount(13);
    await expect(viewer.locator('.pele-edge[data-id="L_R_T_0"]')).toHaveCount(
      1,
    );
    await expect(
      viewer.locator('.pele-edge-label[data-id="L_R_T_0"]'),
    ).toHaveText('remaining-deadline fires');
    await page.keyboard.press('Escape');
    await expect(viewer).toHaveCount(0);
    await expect(button).toBeFocused();
  });
});

/** The distinct colours a highlighted block's tokens are drawn in. */
function tokenColors(element: HTMLElement): string[] {
  const spans = Array.from(element.querySelectorAll<HTMLElement>('pre span'));
  return [...new Set(spans.map((span) => getComputedStyle(span).color))];
}

function labelOpacity(element: HTMLElement): string {
  return getComputedStyle(element, '::before').opacity;
}

function labelContent(element: HTMLElement): string {
  return getComputedStyle(element, '::before').content;
}
