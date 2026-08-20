const timelineSectionPattern = /^Timeline:(?:$|\s)/m;

function formatScenarioMismatch(
  error: string | undefined,
  timeline: readonly unknown[],
): string {
  const prefix = 'Pi scenario mismatch:';
  if (error && timelineSectionPattern.test(error)) return `${prefix}\n${error}`;

  const entries = timeline.map((entry) => JSON.stringify(entry)).join('\n');
  return `${prefix}\n${error ?? 'Unknown scenario failure.'}\nTimeline:${entries ? `\n${entries}` : ' (empty)'}`;
}

export default formatScenarioMismatch;
