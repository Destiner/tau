import { mkdtempSync, realpathSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { childEnvironment, systemRunner } from './release';

if (process.platform !== 'darwin')
  throw new Error('The signing helper pure tests require macOS');
const root = fileURLToPath(new URL('../../', import.meta.url));
const output = mkdtempSync(join(realpathSync(tmpdir()), 'tau-keychain-check-'));
try {
  const source = join(root, 'scripts/macos/signing-keychain.swift');
  const tests = join(output, 'signing-keychain-tests');
  const commands = [
    [
      'xcrun',
      'swiftc',
      '-O',
      '-parse-as-library',
      source,
      '-o',
      join(output, 'signing-keychain'),
    ],
    [
      'xcrun',
      'swiftc',
      '-O',
      '-D',
      'KEYCHAIN_TESTS',
      '-parse-as-library',
      source,
      join(root, 'scripts/macos/signing-keychain-tests.swift'),
      '-o',
      tests,
    ],
    [tests],
  ];
  for (const args of commands) {
    const result = systemRunner({
      args,
      env: childEnvironment(),
      timeout: 120_000,
    });
    if (result.stdout) process.stdout.write(result.stdout);
    if (result.stderr) process.stderr.write(result.stderr);
    if (result.status !== 0)
      throw new Error('Signing helper pure checks failed');
  }
} finally {
  rmSync(output, { recursive: true, force: true });
}
