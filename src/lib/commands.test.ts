import { describe, expect, it } from "vitest";
import type { CommandOption } from "../types";
import {
  commandInvocation,
  filterCommands,
  slashCommandQuery,
} from "./commands";

const commands: CommandOption[] = [
  {
    name: "session-name",
    description: "Name the session",
    source: "extension",
  },
  { name: "summarize", description: "Summarize the session", source: "prompt" },
  { name: "skill:source-check", description: "Check a claim", source: "skill" },
];

describe("slash command completion", () => {
  it("extracts a query only from a leading command token", () => {
    expect(slashCommandQuery("/")).toBe("");
    expect(slashCommandQuery("/ssn")).toBe("ssn");
    expect(slashCommandQuery(" /ssn")).toBeNull();
    expect(slashCommandQuery("/session-name ")).toBeNull();
    expect(slashCommandQuery("/session-name value")).toBeNull();
    expect(slashCommandQuery("/session\n")).toBeNull();
  });

  it("keeps Pi's command order until a query is entered", () => {
    expect(filterCommands(commands, "")).toEqual(commands);
  });

  it("fuzzy matches command names and ranks tighter matches first", () => {
    expect(filterCommands(commands, "ssn").map(({ name }) => name)).toEqual([
      "session-name",
    ]);
    expect(filterCommands(commands, "sum").map(({ name }) => name)).toEqual([
      "summarize",
    ]);
    expect(filterCommands(commands, "sc").map(({ name }) => name)).toEqual([
      "skill:source-check",
    ]);
  });

  it("formats a command for immediate invocation", () => {
    expect(commandInvocation(commands[0])).toBe("/session-name");
  });
});
