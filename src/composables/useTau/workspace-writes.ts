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
function writeWorkspace(
  command:
    | 'set_project_collapsed'
    | 'reorder_projects'
    | 'remove_project'
    | 'archive_session'
    | 'unarchive_session',
  args: Record<string, unknown>,
): Promise<void> {
  const write = workspaceWrite
    .catch(() => undefined)
    .then(() => invokeTraced<void>(command, args));
  workspaceWrite = write.catch(() => undefined);
  return write;
}

export {
  foldIntent,
  archiveIntent,
  nextOrderRevision,
  isCurrentOrderRevision,
  writeWorkspace,
};
