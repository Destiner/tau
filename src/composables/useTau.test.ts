import { describe, expect, it } from "vitest";
import type { ProjectSummary, WorkspaceSnapshot } from "../types";
import { useTau } from "./useTau";

describe("session drafts", () => {
  it("keeps non-empty phantom sessions and removes empty ones when switching", async () => {
    const project: ProjectSummary = {
      path: "/tmp/tau-draft-test",
      name: "tau-draft-test",
      collapsed: false,
      selected: true,
      sessions: [],
    };
    const workspace: WorkspaceSnapshot = {
      activeProjectPath: project.path,
      piPath: "/usr/local/bin/pi",
      sdkAvailable: true,
      projects: [project],
    };
    const {
      state,
      newSession,
      selectSession,
      projectSessions,
      sessionIndicator,
    } = useTau();

    state.activeProjectPath = "";
    state.activeSessionId = "";
    state.draft = "";
    state.ephemeralSessions.splice(0);
    state.workspace = workspace;
    state.streaming = false;
    state.stopping = false;
    state.startingSession = false;
    state.switchingSession = false;

    newSession(project);
    const first = projectSessions(project)[0];
    expect(first?.title).toBe("New session");

    state.draft = "  Keep this draft\nwith its session  ";
    expect(first?.title).toBe("Keep this draft with its session");

    newSession(project);
    expect(projectSessions(project)).toHaveLength(2);
    expect(first && sessionIndicator(project, first)).toBe("draft");

    const empty = projectSessions(project)[0];
    expect(empty?.title).toBe("New session");
    if (!first) throw new Error("Expected the first phantom session");
    await selectSession(project, first);

    expect(projectSessions(project)).toEqual([first]);
    expect(state.draft).toBe("  Keep this draft\nwith its session  ");
  });
});
