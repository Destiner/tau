import type { ProjectSummary, SessionSummary } from '../composables/state';

type SessionPickerEntry = {
  id: string;
  projectPath: string;
  sessionId: string;
  projectName: string;
  session: SessionSummary;
};

function sessionPickerId(projectPath: string, sessionId: string): string {
  return JSON.stringify([projectPath, sessionId]);
}

function sessionPickerEntries(
  projects: ProjectSummary[],
  sessions: (project: ProjectSummary) => SessionSummary[],
): SessionPickerEntry[] {
  return projects.flatMap((project) =>
    project.collapsed
      ? []
      : sessions(project)
          .filter((session) => !session.archived)
          .map((session) => ({
            id: sessionPickerId(project.path, session.id),
            projectPath: project.path,
            sessionId: session.id,
            projectName: project.name,
            session,
          })),
  );
}

function preferredSessionPickerId(
  entries: SessionPickerEntry[],
  activeProjectPath: string,
  activeSessionId: string,
): string | null {
  return (
    entries.find(
      (entry) =>
        entry.projectPath === activeProjectPath &&
        entry.sessionId === activeSessionId,
    )?.id ??
    entries[0]?.id ??
    null
  );
}

function resolveSessionPickerTarget(
  id: string,
  projects: ProjectSummary[],
  sessions: (project: ProjectSummary) => SessionSummary[],
): { project: ProjectSummary; session: SessionSummary } | undefined {
  // Rebuild from the current workspace instead of trusting a row from a prior render.
  const entry = sessionPickerEntries(projects, sessions).find(
    (item) => item.id === id,
  );
  const project = projects.find((item) => item.path === entry?.projectPath);
  const session =
    project &&
    sessions(project).find(
      (item) => item.id === entry?.sessionId && !item.archived,
    );
  return project && session ? { project, session } : undefined;
}

export {
  preferredSessionPickerId,
  resolveSessionPickerTarget,
  sessionPickerEntries,
  sessionPickerId,
};
export type { SessionPickerEntry };
