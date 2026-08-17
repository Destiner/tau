import type { TranscriptEntry } from '../lib/pi/transcript';

const assistantParagraphs = [
  'The visible transcript stays responsive because only nearby rows are mounted, while every message remains directly reachable through the scrollbar.',
  'Variable-height markdown, tool calls, and thinking blocks are measured after rendering so the viewport does not jump when estimates are corrected.',
  'New output follows the end only while the reader is already there. Scrolling into history leaves the viewport anchored to the same content.',
];

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
