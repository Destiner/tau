export default function confirmedSlowFrameRatio(
  ratios: number[],
  limit: number,
): number {
  if (ratios.length !== 1 && ratios.length !== 3) {
    throw new Error('Expected one sweep or three confirmation sweeps');
  }
  if (ratios.length === 1 && ratios[0]! >= limit) {
    throw new Error('Confirm a slow sweep before deciding whether it failed');
  }
  return [...ratios].sort((left, right) => left - right)[
    Math.floor(ratios.length / 2)
  ]!;
}
