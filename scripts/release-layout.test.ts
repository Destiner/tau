import { describe, expect, it } from 'vitest';

import hasExpectedVolumeEntries from './release-layout';

const required = ['.VolumeIcon.icns', 'Applications', 'Tau.app'];

describe('release DMG layout', () => {
  it('accepts headless and interactive packaging', () => {
    expect(hasExpectedVolumeEntries(required, 'Tau')).toBe(true);
    expect(hasExpectedVolumeEntries([...required, '.DS_Store'], 'Tau')).toBe(
      true,
    );
  });

  it('rejects missing or unexpected entries', () => {
    for (const missing of required) {
      expect(
        hasExpectedVolumeEntries(
          required.filter((entry) => entry !== missing),
          'Tau',
        ),
      ).toBe(false);
    }
    expect(hasExpectedVolumeEntries([...required, 'extra.sh'], 'Tau')).toBe(
      false,
    );
  });
});
