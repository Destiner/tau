import type { TranscriptEntry } from '../lib/pi/transcript';

const assistantParagraphs = [
  'The visible transcript stays responsive because only nearby rows are mounted, while every message remains directly reachable through the scrollbar.',
  'Variable-height markdown, tool calls, and thinking blocks are measured after rendering so the viewport does not jump when estimates are corrected.',
  'New output follows the end only while the reader is already there. Scrolling into history leaves the viewport anchored to the same content.',
];

/*
 * Every list shape the markdown theme has a rule for, in one message: tight
 * and loose items, nesting, ordered lists, and task lists mixed with plain
 * items. It is placed near the end of the fixture so `bun run repro` opens on
 * it, and repeated in the user bubble and the thinking block because both
 * restyle markdown around it.
 */
const listShowcase = `- A tight item, whose marker should be quiet next to the text
- An item with a sublist
  - Nested one level
    - And two, still tightly spaced
- [ ] An unchecked task, with no bullet beside the box
- [x] A checked task, readable rather than struck through
- A plain item after the tasks, back on the marker column

1. Ordered items number from the same column
2. Second
   1. Nested ordering

- A loose item

  with a second paragraph inside it

- The item after a loose one`;

/*
 * Both table shapes the theme has to hold: every column alignment markdown can
 * ask for, and one wide enough to scroll inside the message column instead of
 * stretching it.
 */
const tableShowcase = `| Default | Left | Center | Right |
| ------- | :--- | :----: | ----: |
| unaligned | left | center | right |
| second row | a | b | 1,024 |

| Session | Working directory | Extension | Last message |
| --- | --- | --- | ---: |
| tau-desktop | /Users/someone/code/tau/src-tauri | filesystem, telemetry, ssh | 3,204 |
| pi-runtime | /Users/someone/code/pi/packages/runtime | filesystem | 87 |`;

/*
 * The elements that had no rules of their own until the theme grew some:
 * every heading level, a rule, a struck run, a key, nested quotes, and an
 * image wider than the message column it has to stay inside.
 */
const longTailShowcase = `# Heading one
## Heading two directly under it
### Heading three
#### Heading four
##### Heading five
###### Heading six

A paragraph with ~~a struck run that recedes~~ and a shortcut: press <kbd>Cmd</kbd> + <kbd>K</kbd>.

---

> A quote, ruled rather than boxed.
>
> > And one nested inside it.

![An image wider than the message column](data:image/svg+xml;utf8,%3Csvg%20xmlns%3D%27http%3A%2F%2Fwww.w3.org%2F2000%2Fsvg%27%20width%3D%271200%27%20height%3D%27160%27%3E%3Crect%20width%3D%271200%27%20height%3D%27160%27%20fill%3D%27%236e757f%27%2F%3E%3C%2Fsvg%3E)`;

/*
 * Fenced blocks in the languages the highlighter holds a grammar for, plus the
 * two cases it does not highlight: a language it has no grammar for, and a
 * fence that named none. The label a block wears is the tag as written, which
 * is why one of these is fenced `console` rather than `bash`.
 */
const codeShowcase = `\`\`\`ts
export async function load(id: string): Promise<Session | null> {
  // A comment, italic in both schemes
  const session = await invoke<Session>('load_session', { id });
  return session ?? null;
}
\`\`\`

\`\`\`rust
#[tauri::command]
fn load_session(id: &str) -> Result<Session, String> {
    let path = format!("sessions/{id}.jsonl");
    std::fs::read_to_string(path).map_err(|error| error.to_string())
}
\`\`\`

\`\`\`console
$ bun run test --reporter=dot 2>&1 | tail -3
\`\`\`

\`\`\`json
{ "sessions": [{ "id": "tau", "archived": false, "turns": 12 }] }
\`\`\`

\`\`\`diff
-const idle = 30_000;
+const idle = 60_000;
\`\`\`

\`\`\`swift
let unknownToTheHighlighter = 1 < 2
\`\`\`

\`\`\`
A fence that named no language at all.
\`\`\``;

/*
 * The diagram kinds a transcript is likely to be sent, plus the two fences
 * that stay source: one the parser cannot read, and one that is still arriving
 * and has not reached its closing fence.
 */
const diagramShowcase = `\`\`\`mermaid
graph TD
  Prompt[Prompt] --> Runtime{Session live?}
  Runtime -->|Yes| Send[Send over RPC]
  Runtime -->|No| Start[Start Pi]
  Start --> Send
  Send --> Stream[Stream the answer]
\`\`\`

\`\`\`mermaid
sequenceDiagram
  Tau->>Pi: prompt
  Pi-->>Tau: delta
  Pi-->>Tau: done
\`\`\`

\`\`\`mermaid
not a diagram the parser can read
\`\`\`

\`\`\`mermaid
graph TD
  Arriving[Still streaming] --> Unclosed[No closing fence yet]`;

function createLongTranscript(count = 5_000): TranscriptEntry[] {
  return Array.from({ length: count }, (_, index) => createMessage(index));
}

function createMessage(index: number): TranscriptEntry {
  if (index === 4_998) {
    return {
      id: 'fixture-skill-4998',
      kind: 'skill',
      skillName: 'desktop-app-native-feel',
      skillPrompt: 'Focus on keyboard behavior and perceived performance',
      text: `# Native feel audit

Inspect selection, scrolling, keyboard behavior, window behavior, and perceived performance.`,
    };
  }

  if (index === 4_997) {
    return {
      id: 'fixture-markdown-showcase',
      kind: 'assistant',
      text: `**Markdown showcase** — lists, task lists, and tables.\n\n${listShowcase}\n\n${tableShowcase}\n\n${longTailShowcase}\n\n${codeShowcase}\n\n${diagramShowcase}`,
    };
  }
  if (index === 4_996) {
    return {
      id: 'fixture-markdown-showcase-user',
      kind: 'user',
      text: `The same markdown in a user bubble:\n\n${listShowcase}\n\n${tableShowcase}\n\n${longTailShowcase}\n\n${codeShowcase}`,
    };
  }
  // One of each activity state, at a fixed index, so a test can name it.
  if (index === 4_994) {
    return {
      id: 'fixture-tool-running',
      kind: 'tool',
      text: 'bun run test -- transcript',
      toolCallId: 'fixture-call-running',
      toolName: 'bash',
      toolRunning: true,
      toolErrored: false,
      toolArguments: '{\n  "command": "bun run test -- transcript"\n}',
    };
  }
  if (index === 4_993) {
    return {
      id: 'fixture-tool-failed',
      kind: 'tool',
      text: 'src/components/TranscriptView.vue',
      toolCallId: 'fixture-call-failed',
      toolName: 'edit',
      toolRunning: false,
      toolErrored: true,
      toolArguments: '{\n  "path": "src/components/TranscriptView.vue"\n}',
      toolResult: 'Error: no match found for the replacement anchor.',
    };
  }
  if (index === 4_992) {
    return {
      id: 'fixture-tool-done',
      kind: 'tool',
      text: 'src/lib/pi/runtime.ts',
      toolCallId: 'fixture-call-done',
      toolName: 'read',
      toolRunning: false,
      toolErrored: false,
      toolArguments: '{\n  "path": "src/lib/pi/runtime.ts"\n}',
      toolResult: 'Read 120 lines from the runtime.',
    };
  }
  if (index === 4_991) {
    return {
      id: 'fixture-thinking-trace',
      kind: 'thinking',
      text: 'The adoption walk stops at the first local error row, which would strand every row below it.',
    };
  }

  if (index === 4_995) {
    return {
      id: 'fixture-markdown-showcase-thinking',
      kind: 'thinking',
      text: `The same markdown at the thinking block's smaller size:\n\n${listShowcase}\n\n${tableShowcase}\n\n${longTailShowcase}\n\n${codeShowcase}`,
    };
  }

  const variant = index % 6;
  if (variant === 0) {
    return {
      id: `fixture-user-${index}`,
      kind: 'user',
      text: `Can you investigate transcript item ${index}? ${'Please preserve my reading position. '.repeat(index % 4)}`,
    };
  }
  if (variant === 2) {
    return {
      id: `fixture-thinking-${index}`,
      kind: 'thinking',
      text: `${assistantParagraphs[index % assistantParagraphs.length]}\n\n${'Checking another possibility. '.repeat((index % 5) + 1)}`,
    };
  }
  if (variant === 3) {
    const path = `/tmp/tau-fixture/session-${index}/a-very-long-command-name-${index}.jsonl`;
    return {
      id: `fixture-tool-${index}`,
      kind: 'tool',
      text: path,
      toolCallId: `fixture-call-${index}`,
      toolName: index % 12 === 3 ? 'read' : 'bash',
      toolRunning: index % 30 === 3,
      toolErrored: index % 42 === 3,
      toolArguments: JSON.stringify({ path, limit: 200 }, null, 2),
      toolResult:
        index % 30 === 3
          ? undefined
          : `Read ${index} lines from the fixture file.\n${'Result line for the expanded call. '.repeat((index % 6) + 1)}`,
    };
  }

  const detail = assistantParagraphs[index % assistantParagraphs.length];
  const extra = Array.from(
    { length: index % 5 },
    (_, paragraph) =>
      `Paragraph ${paragraph + 1}: ${assistantParagraphs[(index + paragraph + 1) % assistantParagraphs.length]}`,
  ).join('\n\n');
  const code =
    index % 17 === 1
      ? `\n\n\`\`\`ts\nconst messageIndex = ${index};\nconsole.log({ messageIndex });\n\`\`\``
      : '';

  return {
    id: `fixture-assistant-${index}`,
    kind: 'assistant',
    text: `**Message ${index}** — ${detail}${extra ? `\n\n${extra}` : ''}${code}`,
  };
}

export default createLongTranscript;
