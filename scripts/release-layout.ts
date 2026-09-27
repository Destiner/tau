export default function hasExpectedVolumeEntries(
  entries: string[],
  productName: string,
): boolean {
  const required = ['.VolumeIcon.icns', 'Applications', `${productName}.app`];
  const actual = entries.filter((entry) => entry !== '.DS_Store').sort();
  return actual.join('\n') === required.sort().join('\n');
}
