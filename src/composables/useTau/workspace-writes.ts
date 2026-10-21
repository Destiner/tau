import { invokeTraced } from '../../lib/telemetry';
import { type ProjectSummary, type SessionSummary } from '../state';

// Serialize workspace writes without making navigation wait for disk I/O.
let workspaceWrite: Promise<void> = Promise.resolve();
const foldIntent = new WeakMap<
  ProjectSummary,
  { revision: number; confirmed: boolean }
>();
const archiveIntent = new WeakMap<SessionSummary, { confirmed: boolean }>();
let orderRevision = 0;
function nextOrderRevision(): number {
  return ++orderRevision;
}
function isCurrentOrderRevision(revision: number): boolean {
  return orderRevision === revision;
}
function writeWorkspace<T = void>(
  command:
    | 'import_project'
    | 'import_remote_project'
    | 'set_project_collapsed'
    | 'reorder_projects'
    | 'remove_project'
    | 'rename_project'
    | 'archive_session'
    | 'unarchive_session',
  args: Record<string, unknown>,
): Promise<T> {
  const write = workspaceWrite
    .catch(() => undefined)
    .then(() => invokeTraced<T>(command, args));
  workspaceWrite = write.then(
    () => undefined,
    () => undefined,
  );
  return write;
}

export {
  foldIntent,
  archiveIntent,
  nextOrderRevision,
  isCurrentOrderRevision,
  writeWorkspace,
};
