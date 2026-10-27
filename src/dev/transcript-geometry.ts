import type { TranscriptEntry as TranscriptMessage } from '../lib/pi/transcript';

const inlineGeometry = `A [documentation link](https://example.com/tau/transcript?view=geometry) sits beside \`src/components/TranscriptView.vue:418\`, /Users/agent/Code/tau/.worktrees/geometry-review, and SUPERCALIFRAGILISTICEXPIALIDOCIOUS-UNBREAKABLE-IDENTIFIER-0123456789.`;

const proseGeometry = `# Transcript geometry

## A nearby heading

Paragraphs should preserve a readable measure at every window width. **Strong**, *emphasis*, ~~de-emphasis~~, and \`inline code\` share one line without changing its rhythm.

${inlineGeometry}

---

> A quoted observation should align to the quote rule, rather than create a second horizontal layout.
>
> > A nested quote keeps its own indentation and available width.`;

const listGeometry = `- Tight item one
- Tight item two
  - Nested child
    - Nested grandchild
- [ ] Unchecked task aligns with the marker column
- [x] Checked task leaves the following item aligned

- Loose item with a second paragraph.

  This paragraph belongs to the loose item and tests the indent after a blank line.

1. First ordered item
2. Second ordered item
   1. Nested ordered item`;

const codeGeometry = `Code borders and prose share the same rails.

\`\`\`ts
const indented = {
  leading: '  two spaces remain after the quote',
  trailing: 'spaces before this fence should remain visible  ',
};

function preserveWhitespace(value: string) {
  return value.replace(/^ +| +$/g, '·');
}
\`\`\`

\`\`\`
THIS_IS_A_DELIBERATELY_UNBREAKABLE_CODE_LINE_FOR_HORIZONTAL_SCROLL_REVIEW_0123456789_abcdefghijklmnopqrstuvwxyz_ABCDEFGHIJKLMNOPQRSTUVWXYZ_/Users/agent/Code/tau/src/components/TranscriptView.vue:418
\`\`\``;

const smallMermaid = `\`\`\`mermaid
flowchart LR
  A[Prompt] --> B[Reply]
\`\`\``;

const wideMermaid = `\`\`\`mermaid
flowchart LR
  A[Collect transcript geometry] --> B[Measure intrinsic content] --> C[Constrain message column] --> D[Preserve local overflow] --> E[Review at narrow viewport] --> F[Review at wide viewport] --> G[Record regression]
\`\`\``;

const smallTable = `| State | Width |
| --- | ---: |
| narrow | 320 px |
| wide | 1,440 px |`;

const wideTable = `| ${Array.from({ length: 12 }, (_, index) => `Column ${index + 1}`).join(' | ')} |
| ${Array(12).fill('---').join(' | ')} |
| ${Array.from({ length: 12 }, (_, index) => `geometry-column-${index + 1}-unbreakable-value`).join(' | ')} |`;

const smallImage =
  '![A deterministic small blue rectangle](data:image/svg+xml;utf8,%3Csvg%20xmlns%3D%22http%3A%2F%2Fwww.w3.org%2F2000%2Fsvg%22%20width%3D%2296%22%20height%3D%2248%22%20viewBox%3D%220%200%2096%2048%22%3E%3Crect%20width%3D%2296%22%20height%3D%2248%22%20fill%3D%22%234f79a8%22%2F%3E%3Ctext%20x%3D%2248%22%20y%3D%2230%22%20fill%3D%22white%22%20font-size%3D%2212%22%20text-anchor%3D%22middle%22%3E96%20x%2048%3C%2Ftext%3E%3C%2Fsvg%3E)';

const oversizedImage =
  '![A deterministic oversized purple rectangle](data:image/svg+xml;utf8,%3Csvg%20xmlns%3D%22http%3A%2F%2Fwww.w3.org%2F2000%2Fsvg%22%20width%3D%221600%22%20height%3D%22180%22%20viewBox%3D%220%200%201600%20180%22%3E%3Crect%20width%3D%221600%22%20height%3D%22180%22%20fill%3D%22%2372529f%22%2F%3E%3Ctext%20x%3D%22800%22%20y%3D%22105%22%20fill%3D%22white%22%20font-size%3D%2240%22%20text-anchor%3D%22middle%22%3Eintrinsic%201600%20x%20180%3C%2Ftext%3E%3C%2Fsvg%3E)';

function prefixLines(text: string, prefix: string): string {
  return text
    .split('\n')
    .map((line) => `${prefix}${line}`)
    .join('\n');
}

/** A bounded, representative transcript for horizontal-geometry review. */
export default function createGeometryTranscript(): TranscriptMessage[] {
  return [
    {
      id: 'geometry-user-prose',
      kind: 'user',
      text: proseGeometry,
    },
    {
      id: 'geometry-assistant-prose',
      kind: 'assistant',
      text: proseGeometry,
    },
    {
      id: 'geometry-user-lists',
      kind: 'user',
      text: `## List alignment\n\n${listGeometry}`,
    },
    {
      id: 'geometry-assistant-lists',
      kind: 'assistant',
      text: `## List alignment\n\n${listGeometry}`,
    },
    {
      id: 'geometry-user-code',
      kind: 'user',
      text: `## Code width and whitespace\n\n${codeGeometry}`,
    },
    {
      id: 'geometry-assistant-code',
      kind: 'assistant',
      text: `## Code width and whitespace\n\n${codeGeometry}`,
    },
    {
      id: 'geometry-user-mermaid',
      kind: 'user',
      text: `## Mermaid diagrams\n\n${smallMermaid}\n\n${wideMermaid}`,
    },
    {
      id: 'geometry-assistant-mermaid',
      kind: 'assistant',
      text: `## Mermaid diagrams\n\n${smallMermaid}\n\n${wideMermaid}`,
    },
    {
      id: 'geometry-user-tables',
      kind: 'user',
      text: `## Natural tables\n\n${smallTable}\n\n${wideTable}`,
    },
    {
      id: 'geometry-assistant-tables',
      kind: 'assistant',
      text: `## Natural tables\n\n${smallTable}\n\n${wideTable}`,
    },
    {
      id: 'geometry-user-images',
      kind: 'user',
      text: `## Intrinsic SVG images\n\n${smallImage}\n\n${oversizedImage}`,
    },
    {
      id: 'geometry-assistant-images',
      kind: 'assistant',
      text: `## Intrinsic SVG images\n\n${smallImage}\n\n${oversizedImage}`,
    },
    {
      id: 'geometry-user-structured-list',
      kind: 'user',
      text: `## Structured content in a list\n\n- Code remains inside this item:\n\n  \`\`\`ts\n  const listWidth = 'contained by the list item';\n  \`\`\`\n\n- Diagram remains inside this item:\n\n${prefixLines(smallMermaid, '  ')}\n\n- Table remains inside this item:\n\n${prefixLines(smallTable, '  ')}\n\n- Image remains inside this item:\n\n  ${smallImage}`,
    },
    {
      id: 'geometry-assistant-structured-quote',
      kind: 'assistant',
      text: `## Structured content in a quote\n\n> Code remains inside the quote:\n>\n> \`\`\`ts\n> const quoteWidth = 'contained by the quote';\n> \`\`\`\n>\n> Diagram remains inside the quote:\n>\n${prefixLines(smallMermaid, '> ')}\n>\n> Table remains inside the quote:\n>\n${prefixLines(smallTable, '> ')}\n>\n> Image remains inside the quote:\n>\n> ${smallImage}`,
    },
    {
      id: 'geometry-tool-secondary-read',
      kind: 'tool',
      text: 'src/dev/transcript-geometry.ts',
      toolCallId: 'geometry-call-secondary-read',
      toolName: 'read',
      toolRunning: false,
      toolErrored: false,
      toolArguments: '{\n  "path": "src/dev/transcript-geometry.ts"\n}',
      toolResult: 'Read the bounded horizontal-geometry fixture.',
    },
    {
      id: 'geometry-notice-secondary',
      kind: 'notice',
      noticeType: 'warning',
      text: 'Secondary review: inspect horizontal overflow at narrow and wide widths.',
    },
    {
      id: 'geometry-error-secondary',
      kind: 'error',
      errorLabel: 'Preview unavailable',
      text: 'The secondary renderer could not measure an intentionally malformed preview.',
    },
    {
      id: 'geometry-compaction-boundary',
      kind: 'compaction',
      text: '',
      historyAvailable: true,
      historyLoading: false,
    },
  ];
}
