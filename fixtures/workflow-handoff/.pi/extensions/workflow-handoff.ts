import type {
  ExtensionAPI,
  ExtensionCommandContext,
  ReplacedSessionContext,
} from "@earendil-works/pi-coding-agent";
import { Type } from "typebox";
import { readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";

const SESSION_MARKER = "mock-handoff-session";
const STATE_FILE = ".mock-handoff.json";
const PHASES = ["plan", "implement", "review"] as const;

type Phase = (typeof PHASES)[number] | "complete";

interface HandoffState {
  workflowId: string;
  phase: Phase;
  completed: string[];
}

interface SessionMarker {
  workflowId: string;
  phase: Phase;
}

/**
 * Pi hands out a session-opening context only inside a command handler or a
 * `withSession` callback, so a workflow that opens the next phase itself has to
 * hold that context until the phase ends. The context lives in this process and
 * nowhere else, which is the whole point of this fixture: restart the runtime
 * mid-phase and the handoff is gone, exactly as it is for a real workflow.
 */
interface PhaseCoordinator {
  ctx: ReplacedSessionContext | ExtensionCommandContext;
  phase: Phase;
  advancing: boolean;
}

const COORDINATORS_KEY = Symbol.for("tau-mock-handoff.coordinators");
const handoffGlobal = globalThis as typeof globalThis & {
  [COORDINATORS_KEY]?: Map<string, PhaseCoordinator>;
};
const coordinators =
  handoffGlobal[COORDINATORS_KEY] ?? new Map<string, PhaseCoordinator>();
handoffGlobal[COORDINATORS_KEY] = coordinators;

function statePath(cwd: string): string {
  return join(cwd, STATE_FILE);
}

async function loadState(cwd: string): Promise<HandoffState | undefined> {
  try {
    return JSON.parse(await readFile(statePath(cwd), "utf8")) as HandoffState;
  } catch {
    return undefined;
  }
}

async function saveState(cwd: string, state: HandoffState): Promise<void> {
  await writeFile(statePath(cwd), JSON.stringify(state, null, 2) + "\n");
}

function nextPhase(phase: Phase): Phase {
  const index = PHASES.indexOf(phase as (typeof PHASES)[number]);
  if (index === -1) return "complete";
  return PHASES[index + 1] ?? "complete";
}

function phaseLabel(phase: Phase): string {
  return phase.charAt(0).toUpperCase() + phase.slice(1);
}

function findMarker(entries: readonly unknown[]): SessionMarker | undefined {
  for (const entry of [...entries].reverse()) {
    if (!entry || typeof entry !== "object") continue;
    const candidate = entry as {
      type?: unknown;
      customType?: unknown;
      data?: unknown;
    };
    if (
      candidate.type === "custom" &&
      candidate.customType === SESSION_MARKER &&
      candidate.data &&
      typeof candidate.data === "object"
    ) {
      return candidate.data as SessionMarker;
    }
  }
  return undefined;
}

function phasePrompt(phase: Phase): string {
  return [
    `Mock workflow, ${phaseLabel(phase)} phase. Use no tools in this turn.`,
    `Reply with exactly "${phaseLabel(phase)} ready — reply go to finish it." and end your turn.`,
    `When the operator replies, call mock_handoff_complete_phase with phase="${phase}"`,
    "and a one-line summary, then end your turn.",
  ].join(" ");
}

export default function (pi: ExtensionAPI) {
  async function startPhase(
    ctx: ExtensionCommandContext,
    state: HandoffState,
  ): Promise<void> {
    if (state.phase === "complete") {
      ctx.ui.notify("Mock workflow complete", "info");
      return;
    }
    const phase = state.phase;
    const marker: SessionMarker = { workflowId: state.workflowId, phase };
    const parentSession = ctx.sessionManager.getSessionFile();
    const result = await ctx.newSession({
      parentSession,
      setup: async (sessionManager) => {
        sessionManager.appendCustomEntry(SESSION_MARKER, marker);
        sessionManager.appendSessionInfo(`Mock · ${phaseLabel(phase)}`);
      },
      withSession: async (replacementCtx) => {
        coordinators.set(state.workflowId, {
          ctx: replacementCtx,
          phase,
          advancing: false,
        });
        replacementCtx.ui.setStatus(
          "mock-handoff",
          `${phaseLabel(phase)} · armed`,
        );
        await replacementCtx.sendUserMessage(phasePrompt(phase));
      },
    });
    if (result.cancelled)
      ctx.ui.notify(`${phaseLabel(phase)} cancelled`, "warning");
  }

  /** Open the next phase once this one settles. False when the handoff is gone. */
  function openNextPhase(
    cwd: string,
    workflowId: string,
    phase: Phase,
  ): boolean {
    const coordinator = coordinators.get(workflowId);
    if (!coordinator || coordinator.phase !== phase || coordinator.advancing) {
      return false;
    }
    coordinator.advancing = true;

    void (async () => {
      try {
        await coordinator.ctx.waitForIdle();
        coordinators.delete(workflowId);
        const state = await loadState(cwd);
        if (!state) throw new Error("Mock handoff state is missing");
        await startPhase(coordinator.ctx, state);
      } catch (error) {
        coordinators.delete(workflowId);
        coordinator.ctx.ui.notify(
          `Next session failed: ${error instanceof Error ? error.message : String(error)}`,
          "error",
        );
      }
    })();

    return true;
  }

  pi.on("session_start", (_event, ctx) => {
    const branch = ctx.sessionManager.getBranch();
    const marker = findMarker(branch);
    if (!marker) return;

    // A phase session that already holds messages is being reopened rather
    // than created, and a session this process created has armed itself by
    // now, so this reports only on runtimes that inherited someone's phase.
    const reopened = branch.some(
      (entry) => (entry as { type?: unknown }).type === "message",
    );
    if (!reopened || coordinators.has(marker.workflowId)) return;
    ctx.ui.setStatus("mock-handoff", `${phaseLabel(marker.phase)} · lost`);
    ctx.ui.notify(
      `Mock handoff lost: ${phaseLabel(marker.phase)} reopened in a fresh runtime, so it cannot open the next session.`,
      "warning",
    );
  });

  pi.registerCommand("mock-handoff", {
    description: "Run a mock two-session workflow handoff",
    handler: async (_args, ctx) => {
      const existing = await loadState(ctx.cwd);
      const state: HandoffState = existing?.phase
        ? existing
        : {
            workflowId: crypto.randomUUID(),
            phase: PHASES[0],
            completed: [],
          };
      if (!existing) await saveState(ctx.cwd, state);
      await startPhase(ctx, state);
    },
  });

  pi.registerCommand("mock-handoff-reset", {
    description: "Reset the mock workflow state",
    handler: async (_args, ctx) => {
      coordinators.clear();
      await saveState(ctx.cwd, {
        workflowId: crypto.randomUUID(),
        phase: PHASES[0],
        completed: [],
      });
      ctx.ui.notify("Mock workflow reset", "info");
    },
  });

  pi.registerTool({
    name: "mock_handoff_complete_phase",
    label: "Complete mock phase",
    description:
      "Record a mock workflow phase as complete and open the next one",
    parameters: Type.Object({
      phase: Type.String({ description: "Phase being completed" }),
      summary: Type.String({ description: "One-line summary" }),
    }),
    async execute(_toolCallId, params, _signal, _onUpdate, ctx) {
      const marker = findMarker(ctx.sessionManager.getBranch());
      if (!marker) throw new Error("This session is not a mock phase session");

      const state = (await loadState(ctx.cwd)) ?? {
        workflowId: marker.workflowId,
        phase: marker.phase,
        completed: [],
      };
      state.completed = [
        ...state.completed,
        `${params.phase}: ${params.summary}`,
      ];
      state.phase = nextPhase(marker.phase);
      await saveState(ctx.cwd, state);

      const advancing = openNextPhase(ctx.cwd, state.workflowId, marker.phase);
      if (!advancing && state.phase !== "complete") {
        ctx.ui.notify(
          `${phaseLabel(marker.phase)} complete, but the handoff was lost. Run /mock-handoff to continue.`,
          "warning",
        );
        ctx.ui.setEditorText("/mock-handoff");
      }

      return {
        content: [
          {
            type: "text",
            text:
              state.phase === "complete"
                ? "Mock workflow complete. End this turn."
                : advancing
                  ? `${phaseLabel(marker.phase)} complete. End this turn so the ${phaseLabel(state.phase)} session can open.`
                  : `${phaseLabel(marker.phase)} complete. End this turn, then run /mock-handoff.`,
          },
        ],
        details: { advancing, phase: marker.phase, nextPhase: state.phase },
      };
    },
  });
}
