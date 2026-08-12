import { describe, expect, it } from "vitest";
import type { TranscriptEntry } from "../types";
import { hydrateTranscript, toolArgument } from "./transcript";

describe("hydrateTranscript", () => {
  it("keeps assistant content order and resolves tool outcomes", () => {
    const result = hydrateTranscript([
      { role: "user", content: "Inspect the project" },
      {
        role: "assistant",
        content: [
          { type: "thinking", thinking: "I should list it." },
          {
            type: "toolCall",
            id: "call-1",
            name: "bash",
            arguments: { command: "ls" },
          },
          { type: "text", text: "Done." },
        ],
      },
      {
        role: "toolResult",
        toolCallId: "call-1",
        toolName: "bash",
        isError: false,
      },
    ]);

    expect(result.map((entry) => entry.kind)).toEqual([
      "user",
      "thinking",
      "tool",
      "assistant",
    ]);
    expect(result[2]).toMatchObject({
      text: "ls",
      toolName: "bash",
      toolRunning: false,
      toolErrored: false,
    });
  });

  it("accepts block-based user content", () => {
    const result = hydrateTranscript([
      { role: "user", content: [{ type: "text", text: "Hello" }] },
    ]);
    expect(result[0]?.text).toBe("Hello");
  });

  it("carries streamed ids into the settled turn", () => {
    const streamed: TranscriptEntry[] = [
      { id: "optimistic-1", kind: "user", text: "Inspect the project" },
      { id: "stream-thinking-0", kind: "thinking", text: "I should list" },
      {
        id: "stream-tool-1",
        kind: "tool",
        text: "ls",
        toolCallId: "call-1",
        toolName: "bash",
        toolRunning: true,
        toolErrored: false,
      },
      { id: "stream-assistant-2", kind: "assistant", text: "Do" },
    ];

    const settled = hydrateTranscript(
      [
        { role: "user", content: "Inspect the project" },
        {
          role: "assistant",
          content: [
            { type: "thinking", thinking: "I should list it." },
            {
              type: "toolCall",
              id: "call-1",
              name: "bash",
              arguments: { command: "ls" },
            },
            { type: "text", text: "Done." },
          ],
        },
        {
          role: "toolResult",
          toolCallId: "call-1",
          toolName: "bash",
          isError: false,
        },
      ],
      streamed,
    );

    expect(settled.map((entry) => entry.id)).toEqual([
      "optimistic-1",
      "stream-thinking-0",
      "stream-tool-1",
      "stream-assistant-2",
    ]);
    expect(settled[3]?.text).toBe("Done.");
  });

  it("gives rows the settled turn adds ids of their own", () => {
    const streamed: TranscriptEntry[] = [
      { id: "stream-assistant-0", kind: "assistant", text: "Done." },
    ];

    const settled = hydrateTranscript(
      [
        {
          role: "assistant",
          content: [
            { type: "text", text: "Done." },
            { type: "text", text: "One more thing." },
          ],
        },
      ],
      streamed,
    );

    expect(settled[0]?.id).toBe("stream-assistant-0");
    expect(settled[1]?.id).not.toBe("stream-assistant-0");
    expect(new Set(settled.map((entry) => entry.id)).size).toBe(2);
  });

  it("keeps a reordered tool row from adopting another call's id", () => {
    const streamed: TranscriptEntry[] = [
      {
        id: "stream-tool-0",
        kind: "tool",
        text: "ls",
        toolCallId: "call-1",
        toolName: "bash",
        toolRunning: true,
        toolErrored: false,
      },
    ];

    const settled = hydrateTranscript(
      [
        {
          role: "assistant",
          content: [
            {
              type: "toolCall",
              id: "call-2",
              name: "bash",
              arguments: { command: "pwd" },
            },
          ],
        },
      ],
      streamed,
    );

    expect(settled[0]?.id).not.toBe("stream-tool-0");
  });
});

describe("toolArgument", () => {
  it("prefers a recognizable path", () => {
    expect(toolArgument({ path: "src/main.ts" })).toBe("src/main.ts");
  });
});
