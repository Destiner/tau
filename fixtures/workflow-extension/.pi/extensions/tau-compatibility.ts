import { StringEnum } from "@earendil-works/pi-ai";
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { Type } from "typebox";

const SESSION_MARKER = "tau-compatibility-session";
const BLOCKED_BASH_MARKER = "tau-compatibility-blocked";
const TOOL_NAME = "tau_compatibility_dialog";

export default function (pi: ExtensionAPI) {
  pi.on("tool_call", (event) => {
    if (event.toolName !== "bash") return;

    const command = (event.input as { command?: unknown }).command;
    if (typeof command === "string" && command.includes(BLOCKED_BASH_MARKER)) {
      return {
        block: true,
        reason: `Tau compatibility fixture blocked ${BLOCKED_BASH_MARKER}`,
      };
    }
  });

  pi.registerTool({
    name: TOOL_NAME,
    label: "Tau compatibility dialog",
    description:
      "Show a Tau confirmation dialog during tool execution and return the user's choice",
    parameters: Type.Object({
      message: Type.String({ description: "Message shown in the dialog" }),
      outcome: StringEnum(["continue", "terminate"] as const),
    }),
    async execute(_toolCallId, params, signal, onUpdate, ctx) {
      onUpdate?.({
        content: [{ type: "text", text: "Waiting for Tau confirmation…" }],
        details: { stage: "waiting" },
      });

      const confirmed = await ctx.ui.confirm(
        "Tau tool execution dialog",
        params.message,
        { signal },
      );
      const result = confirmed ? "confirmed" : "cancelled";

      ctx.ui.notify(`Tool dialog ${result}.`, confirmed ? "info" : "warning");

      return {
        content: [{ type: "text", text: `Tau tool dialog was ${result}.` }],
        details: { confirmed, result },
        terminate: params.outcome === "terminate",
      };
    },
  });

  pi.registerCommand("tau-compat-ui", {
    description: "Exercise Tau's supported extension UI methods",
    handler: async (_args, ctx) => {
      ctx.ui.notify("Starting Tau extension UI checks.", "info");

      const firstChoice = await ctx.ui.select("Back-to-back select 1 of 2", [
        "Alpha",
        "Beta",
      ]);
      const secondChoice = await ctx.ui.select("Back-to-back select 2 of 2", [
        "Gamma",
        "Delta",
      ]);
      const confirmed = await ctx.ui.confirm(
        "Tau confirmation",
        "Choose either response to continue the fixture.",
      );
      const input = await ctx.ui.input(
        "Tau single-line input",
        "Enter any value or cancel",
      );
      const editor = await ctx.ui.editor(
        "Tau multi-line editor",
        "Edit or submit this prefilled text.\nA second line exercises multi-line input.",
      );

      const completed = [
        `selects=${firstChoice ?? "cancelled"}/${secondChoice ?? "cancelled"}`,
        `confirm=${confirmed}`,
        `input=${input === undefined ? "cancelled" : "submitted"}`,
        `editor=${editor === undefined ? "cancelled" : "submitted"}`,
      ].join(", ");

      ctx.ui.notify(`Tau extension UI checks complete: ${completed}`, "info");
      ctx.ui.setEditorText("/tau-compat-session");
    },
  });

  pi.registerCommand("tau-compat-session", {
    description:
      "Replace the active Pi session and run a tool from withSession",
    handler: async (_args, ctx) => {
      const parentSession = ctx.sessionManager.getSessionFile();
      const result = await ctx.newSession({
        parentSession,
        setup: async (sessionManager) => {
          sessionManager.appendCustomEntry(SESSION_MARKER, {
            createdAt: new Date().toISOString(),
            parentSession,
          });
          sessionManager.appendSessionInfo("Tau compatibility replacement");
        },
        withSession: async (replacementCtx) => {
          replacementCtx.ui.notify(
            "Tau compatibility replacement session started.",
            "info",
          );
          await replacementCtx.sendUserMessage(
            `Call ${TOOL_NAME} exactly once with {"message":"Confirm the replacement-session tool call.","outcome":"terminate"}. Do not call any other tool.`,
          );
          replacementCtx.ui.setEditorText("/tau-compat-state");
        },
      });

      if (result.cancelled) {
        ctx.ui.notify(
          "Tau compatibility session replacement cancelled.",
          "warning",
        );
      }
    },
  });

  pi.registerCommand("tau-compat-state", {
    description: "Verify the replacement session's persistent custom entry",
    handler: async (_args, ctx) => {
      const marker = ctx.sessionManager
        .getEntries()
        .find(
          (entry) =>
            entry.type === "custom" && entry.customType === SESSION_MARKER,
        );
      const message = marker
        ? "Replacement session marker found."
        : "Replacement session marker missing.";

      ctx.ui.notify(message, marker ? "info" : "error");
      ctx.ui.setEditorText(
        `Use the bash tool to run exactly: echo ${BLOCKED_BASH_MARKER}`,
      );
    },
  });
}
