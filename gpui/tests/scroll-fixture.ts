import type { Step, Suite, Theme } from '../contract';

export function scrollFixture(theme: Theme): Suite {
  const value = Array.from(
    { length: 80 },
    (_, n) => `Row ${String(n).padStart(2, '0')} — Scroll precision`,
  ).join('\n');
  const checkpoint = (name: string): Step => ({
    action: 'checkpoint',
    name,
    expected: { value, selection: [0, 0], changes: 0, focused: true },
  });
  return {
    schemaVersion: 1,
    theme,
    scale: 2,
    viewport: { width: 640, height: 320 },
    scenarios: [
      {
        id: 'textarea-scroll-precision',
        component: 'textarea',
        props: { value, width: 360, height: 120 },
        steps: [
          { action: 'focus' },
          { action: 'select', start: 0, end: 0 },
          checkpoint('top'),
          { action: 'wheel', deltaY: 32 },
          checkpoint('down'),
          { action: 'wheel', deltaY: -32 },
          checkpoint('returned'),
          ...Array.from({ length: 8 }, (): Step => ({
            action: 'wheel',
            deltaY: 0.5,
          })),
          checkpoint('fractional'),
          { action: 'pointer', phase: 'move', x: 377, y: 60 },
          checkpoint('hover'),
          { action: 'pointer', phase: 'down', x: 377, y: 60 },
          checkpoint('dragging'),
          { action: 'pointer', phase: 'move', x: 377, y: 80 },
          checkpoint('dragged'),
          { action: 'pointer', phase: 'up', x: 377, y: 80 },
          { action: 'pointer', phase: 'move', x: 200, y: 80 },
          { action: 'wheel', deltaY: -10000 },
          checkpoint('clamped-top'),
        ],
      },
    ],
  };
}
