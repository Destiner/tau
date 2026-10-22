interface Request {
  source: string;
  language: string;
}

self.onmessage = async (event: MessageEvent<Request>): Promise<void> => {
  const { source, language } = event.data;
  if (
    source.length > 512 * 1024 ||
    !source ||
    source.includes('\0') ||
    !language
  ) {
    self.postMessage({ type: 'error' });
    return;
  }
  try {
    // Import inside the handler so WASM/import failures follow the same protocol.
    const { default: loadHighlighter } = await import('./highlight-engine');
    const core = await loadHighlighter();
    const { header, lines } = core.render(source, language);
    self.postMessage({ type: 'start', header });
    for (let index = 0; index < lines.length;) {
      let end = index;
      let length = 0;
      while (end < lines.length && end - index < 250) {
        const next = lines[end]!.html.length;
        if (end > index && length + next > 32_000) break;
        length += next;
        end += 1;
      }
      self.postMessage({
        type: 'chunk',
        html: lines
          .slice(index, end)
          .map((line) => line.html)
          .join(''),
        offset: lines[end - 1]!.offset,
      });
      index = end;
    }
    self.postMessage({ type: 'done' });
  } catch {
    self.postMessage({ type: 'error' });
  }
};
