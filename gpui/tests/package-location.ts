import assert from 'node:assert/strict';
import { mkdirSync, symlinkSync } from 'node:fs';
import { homedir } from 'node:os';
import { basename, join } from 'node:path';
import { assertFresh, plan } from '../scripts/package';

const dir = process.argv[2];
assert(dir && basename(dir).startsWith('tau-package-location-'));
assert.equal(homedir(), dir);
mkdirSync(join(dir, 'Applications'));

for (const base of [
  '/Applications',
  '/System/Applications',
  join(homedir(), 'Applications'),
]) {
  const output = join(base, 'tau-preview-fresh-test', 'Tau.app');
  assert.throws(() => plan(['--output', output]), /Applications/);
  assert.throws(() => assertFresh(output), /Applications/);
  assert.throws(
    () =>
      plan([
        '--output',
        `${base}/../Applications/tau-preview-fresh-test/Tau.app`,
      ]),
    /Applications/,
  );
}

symlinkSync(join(dir, 'Applications'), join(dir, 'alias'));
const output = join(dir, 'alias', 'tau-preview-fresh-test', 'Tau.app');
assert.throws(() => plan(['--output', output]), /Applications/);
assert.throws(() => assertFresh(output), /Applications/);

symlinkSync(join(dir, 'missing'), join(dir, 'dangling'));
assert.throws(
  () => plan(['--output', join(dir, 'dangling', 'Tau.app')]),
  /symlink/,
);
mkdirSync(join(dir, 'safe'));
symlinkSync(join(dir, 'safe'), join(dir, 'safe-alias'));
assert.doesNotThrow(() =>
  assertFresh(join(dir, 'safe-alias', 'fresh', 'Tau.app')),
);
