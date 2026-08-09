import { describe, expect, it } from "vitest";
import { hydrateTranscript, toolSummary } from "./transcript";

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
      text: "bash ls",
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
});

describe("toolSummary", () => {
  it("prefers a recognizable path", () => {
    expect(toolSummary("read", { path: "src/main.ts" })).toBe(
      "read src/main.ts",
    );
  });
});
