// Exact Swift CLI vocabulary only. Never echo a partially matching line.
const helperMessages = new Set([
  'Cannot read credential input',
  'Credential input exceeds limit',
  'Expected credential JSON on stdin',
  'Invalid base64 certificate',
  'Unsafe directory',
  'Protected directory',
  'Unsafe directory ancestor',
  'Writable directory ancestor',
  'Unsafe directory parent',
  'Missing directory parent',
  'Directory parent is not owned',
  'Missing owned directory',
  'Unsafe owned directory',
  'Invalid ownership marker',
  'Cannot inspect owned directory',
  'Unexpected owned directory contents',
  'Cannot inspect owned file',
  'Unsafe owned file',
  'Invalid keychain lock file',
  'Directory must be fresh and creatable',
  'Cannot inspect new directory',
  'Cannot create ownership marker',
  'Cannot write ownership marker',
  'Cannot remove owned file',
  'Cannot remove ownership marker',
  'Cannot remove owned directory',
  'Recovery state exceeds limit',
  'Cannot protect recovery state',
  'Missing keychain recovery state',
  'Owned keychain deletion incomplete',
  'Creation failed; owned cleanup or search-list restoration incomplete',
  'Required Security access API unavailable',
  'Required Security format API unavailable',
  'Legacy keychain format has no partition ACLs',
  'Unsupported keychain format',
  'Missing key access',
  'Missing key ACLs',
  'Expected exactly one partition ACL',
  'Expected exactly one imported identity',
  'Missing certificate subject',
  'Empty certificate team OU',
  'Missing search list',
  'Missing user search list',
  'Legacy search-list restoration unavailable',
  'Missing owned keychain',
  'Missing search-list keychain',
  'Missing created keychain',
  'Missing codesign application',
  'Missing import access',
  'Missing import results',
  'Missing imported key',
  'Imported key outside owned keychain',
  'Activation requires active owned keychain',
  'Activation search-list verification failed',
  'Usage: signing-keychain create|activate|remove <absolute owned-directory>',
  'Do not run as root',
  'Unknown command',
  'Signing keychain operation failed',
]);
const securityOperations = new Set([
  'Read key access',
  'Read key ACLs',
  'Read partition ACL',
  'Configure partition ACL',
  'Save key access',
  'Read imported certificate',
  'Capture search list',
  'Capture user search list',
  'Read keychain path',
  'Open owned keychain',
  'Delete owned keychain',
  'Open search-list keychain',
  'Restore search list',
  'Restore user search list',
  'Restore legacy merged search list',
  'Generate keychain password',
  'Create keychain',
  'Configure keychain',
  'Read keychain format',
  'Unlock keychain',
  'Trust codesign',
  'Create import access',
  'Import PKCS12',
  'Read imported key',
  'Check imported key scope',
  'Disable keychain interaction',
]);
export function boundedInteger(value: number): boolean {
  return Number.isInteger(value) && value >= -2147483648 && value <= 2147483647;
}
export function helperDiagnostic(
  stderr: string | undefined,
): string | undefined {
  if (stderr === undefined || stderr.length > 256) return;
  const line = stderr.endsWith('\n') ? stderr.slice(0, -1) : stderr;
  if (helperMessages.has(line)) return line;
  const match = /^(.+) failed \(OSStatus (-?(?:0|[1-9][0-9]{0,9}))\)$/.exec(
    line,
  );
  if (
    match &&
    match[0] === line &&
    securityOperations.has(match[1]!) &&
    match[2] === String(Number(match[2])) &&
    boundedInteger(Number(match[2]))
  )
    return `${match[1]} failed (OSStatus ${Number(match[2])})`;
}
