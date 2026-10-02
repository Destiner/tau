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
  for (const project of projects) {
    if (project.collapsed) continue;
    const session = sessions(project).find(
      (item) => !item.archived && sessionPickerId(project.path, item.id) === id,
    );
    if (session) return { project, session };
  }
  return undefined;
}

export {
  preferredSessionPickerId,
  resolveSessionPickerTarget,
  sessionPickerEntries,
  sessionPickerId,
};
export type { SessionPickerEntry };
