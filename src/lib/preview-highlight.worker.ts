import loadHighlighter from './highlight-engine';

interface Request {
  source: string;
  language: string;
}

self.onmessage = async (event: MessageEvent<Request>): Promise<void> => {
  const { source, language } = event.data;
  if (source.length > 512 * 1024 || !source || !language) {
    self.postMessage({ type: 'error' });
    return;
  }
  try {
    const core = await loadHighlighter();
    if (!core.getLoadedLanguages().includes(language))
      throw new Error('Unknown grammar');
    const html = core.codeToHtml(source, {
      lang: language,
      themes: { light: 'ayu-light', dark: 'ayu-dark' },
      defaultColor: false,
      cssVariablePrefix: '--tau-code-',
      transformers: [
        {
          pre(node): void {
            delete node.properties.tabindex;
          },
        },
      ],
    });
    const codeStart = html.indexOf('<code>') + 6;
    const codeEnd = html.lastIndexOf('</code></pre>');
    if (codeStart < 6 || codeEnd < codeStart)
      throw new Error('Invalid highlight output');
    const lines = html.slice(codeStart, codeEnd).split('\n');
    const sourceLines = source.split('\n');
    if (lines.length !== sourceLines.length)
      throw new Error('Invalid highlight lines');
    self.postMessage({ type: 'start', header: html.slice(0, codeStart) });
    let offset = 0;
    for (let index = 0; index < lines.length;) {
      let end = index;
      let length = 0;
      while (end < lines.length && end - index < 250) {
        const next = lines[end]!.length + 1;
        if (end > index && length + next > 32_000) break;
        length += next;
        end += 1;
      }
      const chunk =
        lines.slice(index, end).join('\n') + (end < lines.length ? '\n' : '');
      for (let line = index; line < end; line += 1) {
        offset +=
          sourceLines[line]!.length + (line < sourceLines.length - 1 ? 1 : 0);
      }
      self.postMessage({ type: 'chunk', html: chunk, offset });
      index = end;
    }
    self.postMessage({ type: 'done' });
  } catch {
    self.postMessage({ type: 'error' });
  }
};
