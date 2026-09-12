import { describe, expect, it } from 'vitest';

import hasExpectedVolumeEntries from './release-layout';

const required = ['.VolumeIcon.icns', 'Applications', 'Tau.app'];

describe('release DMG layout', () => {
  it('accepts headless packaging without Finder metadata', () => {
    expect(hasExpectedVolumeEntries(required, 'Tau')).toBe(true);
  });

  it('accepts interactive packaging with Finder metadata', () => {
    expect(hasExpectedVolumeEntries([...required, '.DS_Store'], 'Tau')).toBe(
      true,
    );
  });

  it.each(required)('still requires %s', (missing) => {
    expect(
      hasExpectedVolumeEntries(
        required.filter((entry) => entry !== missing),
        'Tau',
      ),
    ).toBe(false);
  });

  it('rejects unexpected files', () => {
    expect(hasExpectedVolumeEntries([...required, 'extra.sh'], 'Tau')).toBe(
      false,
    );
  });
});
