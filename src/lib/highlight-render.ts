import type { TokensResult } from '@pierre/highlights';

function escape(value: string): string {
  return value.replace(
    /[&<>"'\r]/g,
    (character) =>
      ({
        '&': '&amp;',
        '<': '&lt;',
        '>': '&gt;',
        '"': '&quot;',
        "'": '&#39;',
        '\r': '&#13;',
      })[character]!,
  );
}

interface HighlightLine {
  html: string;
  /** End of this fragment in the original UTF-16 source, including its separator. */
  offset: number;
}

/** Both renderers own their markup; lexer output never becomes raw HTML. */
function renderHighlight(
  source: string,
  result: TokensResult,
  tag: string,
): {
  header: string;
  lines: HighlightLine[];
} {
  const sourceLines = source.split('\n');
  if (sourceLines.length !== result.tokens.length)
    throw new Error('Invalid highlight lines');
  let start = 0;
  const lines = result.tokens.map((tokens, index) => {
    const line = sourceLines[index]!;
    const end = start + line.length;
    let cursor = start;
    let html = '<span class="line">';
    for (const token of tokens) {
      const tokenEnd = token.offset + token.content.length;
      if (
        token.offset < cursor ||
        tokenEnd > end ||
        source.slice(token.offset, tokenEnd) !== token.content
      )
        throw new Error('Invalid highlight offsets');
      html += escape(source.slice(cursor, token.offset));
      const style = Object.entries(token.htmlStyle ?? {})
        .map(([key, value]) => `${key}:${value}`)
        .join(';');
      html += style
        ? `<span style="${escape(style)}">${escape(token.content)}</span>`
        : escape(token.content);
      cursor = tokenEnd;
    }
    // The lexer omits CRLF separators; the source remains the authority for gaps.
    html += `${escape(source.slice(cursor, end))}</span>`;
    const hasNewline = index < sourceLines.length - 1;
    if (hasNewline) html += '\n';
    start = end + (hasNewline ? 1 : 0);
    return { html, offset: start };
  });
  return {
    header: `<pre class="tau-highlight" data-tau-highlight style="${escape(result.rootStyle ?? '')}"><code class="language-${escape(tag)}">`,
    lines,
  };
}

function highlightHtml(rendered: ReturnType<typeof renderHighlight>): string {
  return `${rendered.header}${rendered.lines.map((line) => line.html).join('')}</code></pre>`;
}

export { type HighlightLine, renderHighlight, highlightHtml };
