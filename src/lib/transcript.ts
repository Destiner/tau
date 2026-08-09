import type { TranscriptEntry } from "../types";

type JsonRecord = Record<string, unknown>;

export function hydrateTranscript(messages: unknown[]): TranscriptEntry[] {
  const entries: TranscriptEntry[] = [];
  let sequence = 0;
  const nextId = (prefix: string) => `${prefix}-${sequence++}`;

  for (const value of messages) {
    const message = asRecord(value);
    if (!message) continue;
    const role = stringValue(message.role);

    if (role === "user") {
      const text = contentText(message.content);
      if (text) entries.push({ id: nextId("user"), kind: "user", text });
      continue;
    }

    if (role === "assistant") {
      const content = Array.isArray(message.content) ? message.content : [];
      for (const partValue of content) {
        const part = asRecord(partValue);
        if (!part) continue;
        const type = stringValue(part.type);
        if (type === "text") {
          const text = stringValue(part.text);
          if (text)
            entries.push({ id: nextId("assistant"), kind: "assistant", text });
        } else if (type === "thinking") {
          const text = stringValue(part.thinking);
          if (text)
            entries.push({ id: nextId("thinking"), kind: "thinking", text });
        } else if (type === "toolCall") {
          const toolCallId = stringValue(part.id);
          const toolName = stringValue(part.name) || "tool";
          entries.push({
            id: nextId("tool"),
            kind: "tool",
            text: toolSummary(toolName, part.arguments),
            toolCallId,
            toolName,
            toolRunning: true,
            toolErrored: false,
          });
        }
      }
      continue;
    }

    if (role === "toolResult") {
      const toolCallId = stringValue(message.toolCallId);
      const existing = [...entries]
        .reverse()
        .find(
          (entry) => entry.kind === "tool" && entry.toolCallId === toolCallId,
        );
      if (existing) {
        existing.toolRunning = false;
        existing.toolErrored = message.isError === true;
      } else {
        const toolName = stringValue(message.toolName) || "tool";
        entries.push({
          id: nextId("tool"),
          kind: "tool",
          text: toolName,
          toolCallId,
          toolName,
          toolRunning: false,
          toolErrored: message.isError === true,
        });
      }
      continue;
    }

    if (role === "bashExecution") {
      const command = stringValue(message.command);
      entries.push({
        id: nextId("tool"),
        kind: "tool",
        text: command ? `bash ${command}` : "bash",
        toolName: "bash",
        toolRunning: false,
        toolErrored: numberValue(message.exitCode) !== 0,
      });
    }
  }
  return entries;
}

export function toolSummary(name: string, args: unknown): string {
  const record = asRecord(args);
  const detail =
    stringValue(record?.command) ||
    stringValue(record?.path) ||
    stringValue(record?.file_path) ||
    stringValue(record?.query) ||
    compactJson(args);
  return detail ? `${name} ${detail}` : name;
}

export function contentText(value: unknown): string {
  if (typeof value === "string") return value;
  if (!Array.isArray(value)) return "";
  return value
    .map((part) => {
      const record = asRecord(part);
      return record && record.type === "text" ? stringValue(record.text) : "";
    })
    .filter(Boolean)
    .join("\n");
}

export function asRecord(value: unknown): JsonRecord | undefined {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    ? (value as JsonRecord)
    : undefined;
}

export function stringValue(value: unknown): string {
  return typeof value === "string" ? value : "";
}

function numberValue(value: unknown): number | undefined {
  return typeof value === "number" ? value : undefined;
}

function compactJson(value: unknown): string {
  if (value === undefined) return "";
  try {
    const text = JSON.stringify(value);
    return text.length > 220 ? `${text.slice(0, 217)}…` : text;
  } catch {
    return "";
  }
}
