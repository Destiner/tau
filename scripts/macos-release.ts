import {
  cleanup,
  prepare,
  ReleaseError,
  sign,
  verify,
  verifyInput,
} from './macos/release';

function main(args: string[]): void {
  const command = args.shift();
  const allowed: Record<string, string[]> = {
    prepare: ['version', 'commit', 'output', 'target-dir'],
    'verify-input': ['version', 'commit', 'input'],
    sign: ['version', 'commit', 'input', 'output', 'work', 'keychain-helper'],
    verify: ['version', 'commit', 'input', 'work'],
    cleanup: ['work', 'keychain-helper'],
  };
  if (!command || !Object.hasOwn(allowed, command))
    throw new ReleaseError('Unknown release command');
  const values: Record<string, string> = {};
  while (args.length) {
    const flag = args.shift()!;
    const value = args.shift();
    const name = flag.slice(2);
    if (
      !flag.startsWith('--') ||
      !allowed[command]!.includes(name) ||
      Object.hasOwn(values, name) ||
      !value ||
      value.startsWith('--')
    )
      throw new ReleaseError('Invalid release arguments');
    values[name] = value;
  }
  for (const key of allowed[command]!)
    if (key !== 'target-dir' && !values[key])
      throw new ReleaseError('Missing release argument');
  const expected = { version: values.version!, commit: values.commit! };
  switch (command) {
    case 'prepare':
      prepare({
        ...expected,
        output: values.output!,
        targetDir: values['target-dir'],
      });
      break;
    case 'verify-input':
      verifyInput(values.input!, expected);
      break;
    case 'sign': {
      const result = sign({
        ...expected,
        input: values.input!,
        output: values.output!,
        work: values.work!,
        keychainHelper: values['keychain-helper']!,
      });
      console.log(
        `Accepted app request: ${result.appRequestId}\nAccepted DMG request: ${result.dmgRequestId}`,
      );
      break;
    }
    case 'verify':
      verify({ ...expected, input: values.input!, work: values.work! });
      break;
    case 'cleanup':
      cleanup(values.work!, values['keychain-helper']!);
      break;
  }
}
if (import.meta.main) {
  try {
    main(process.argv.slice(2));
    console.log('Release operation complete');
  } catch (error) {
    console.error(
      error instanceof ReleaseError
        ? error.message
        : 'Release operation failed',
    );
    process.exitCode = 1;
  }
}
