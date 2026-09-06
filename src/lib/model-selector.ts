const providerLabels: Record<string, string> = {
  'amazon-bedrock': 'Amazon Bedrock',
  'github-copilot': 'GitHub Copilot',
  'google-vertex': 'Google Vertex',
  'openai-codex': 'OpenAI Codex',
  opencode: 'OpenCode',
  'opencode-go': 'OpenCode Go',
  openrouter: 'OpenRouter',
  'pi-claude': 'Pi Claude',
  xai: 'xAI',
};

export default function providerLabel(provider: string): string {
  return (
    providerLabels[provider] ??
    provider
      .replaceAll('-', ' ')
      .replace(/\b\w/g, (letter) => letter.toUpperCase())
  );
}
