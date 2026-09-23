type AdaptedMermaidSource =
  { kind: 'renderable'; source: string } | { kind: 'unsafe' };

type Delimiter = { opener: string; closer: string };

const DELIMITERS: readonly Delimiter[] = [
  { opener: '(((', closer: ')))' },
  { opener: '([', closer: '])' },
  { opener: '((', closer: '))' },
  { opener: '[[', closer: ']]' },
  { opener: '[(', closer: ')]' },
  { opener: '[/', closer: '\\]' },
  { opener: '[\\', closer: '/]' },
  { opener: '>', closer: ']' },
  { opener: '{{', closer: '}}' },
  { opener: '[', closer: ']' },
  { opener: '(', closer: ')' },
  { opener: '{', closer: '}' },
];

const FLOWCHART_HEADER = /^(?:graph|flowchart)\s+(?:TD|TB|LR|BT|RL)\s*$/i;
const NON_NODE_STATEMENT =
  /^(?:subgraph[ \t]+|end\s*$|direction[ \t]+|classDef[ \t]+|class[ \t]+|style[ \t]+|linkStyle[ \t]+)/;
const ARROW = /^(?:<)?(?:-->|-\.->|==>|---|-\.-|===)(?:\|[^|]*\|)?/;
const TEXT_ARROW = /^(?:<)?(?:--|-\.|==)\s+.+?\s+(?:-->|---|\.->|-\.-|==>|===)/;

function isIdentifierCharacter(character: string | undefined): boolean {
  return character !== undefined && /[\w-]/.test(character);
}

function lineEnd(source: string, start: number): number {
  const end = source.indexOf('\n', start);
  return end === -1 ? source.length : end;
}

function lineEndingLength(source: string, end: number): number {
  return end < source.length ? 1 : 0;
}

function firstBodyOffset(source: string): number | null {
  let offset = 0;
  while (offset <= source.length) {
    const end = lineEnd(source, offset);
    const line = source.slice(offset, end).replace(/\r$/, '').trim();
    if (line !== '' && !line.startsWith('%%')) {
      if (!FLOWCHART_HEADER.test(line)) return null;
      return end + lineEndingLength(source, end);
    }
    if (end === source.length) return null;
    offset = end + 1;
  }
  return null;
}

function delimiterAt(source: string, offset: number): Delimiter | null {
  return (
    DELIMITERS.find(({ opener }) => source.startsWith(opener, offset)) ?? null
  );
}

function startsStatement(source: string, offset: number): boolean {
  const end = lineEnd(source, offset);
  const line = source.slice(offset, end).replace(/\r$/, '').trimStart();
  if (
    line.startsWith('%%') ||
    FLOWCHART_HEADER.test(line) ||
    NON_NODE_STATEMENT.test(line)
  ) {
    return true;
  }

  const id = /^[\w-]+/.exec(line)?.[0];
  if (!id) return false;
  const rest = line.slice(id.length);
  return (
    delimiterAt(rest, 0) !== null ||
    /^\s*(?:&|<?(?:-->|-\.->|==>|---|-\.-|===))/.test(rest)
  );
}

function scanLabel(
  source: string,
  contentOffset: number,
  delimiter: Delimiter,
): { end: number; replacements: number[] } | { unsafe: boolean } {
  const quoted = source[contentOffset] === '"';
  let offset = contentOffset + (quoted ? 1 : 0);
  const replacements: number[] = [];

  while (offset < source.length) {
    if (quoted) {
      if (
        source[offset] === '"' &&
        source.startsWith(delimiter.closer, offset + 1)
      ) {
        return {
          end: offset + 1 + delimiter.closer.length,
          replacements,
        };
      }
      if (source[offset] === '"' && replacements.length > 0) {
        return { unsafe: true };
      }
    } else if (source.startsWith(delimiter.closer, offset)) {
      return { end: offset + delimiter.closer.length, replacements };
    } else if (source[offset] === '"' && replacements.length > 0) {
      return { unsafe: true };
    }

    if (source[offset] === '\r' && source[offset + 1] === '\n') {
      if (!quoted && startsStatement(source, offset + 2)) {
        return { unsafe: true };
      }
      replacements.push(offset);
      offset += 2;
      continue;
    }
    if (source[offset] === '\n') {
      if (!quoted && startsStatement(source, offset + 1)) {
        return { unsafe: true };
      }
      replacements.push(offset);
    }
    offset += 1;
  }

  return { unsafe: replacements.length > 0 };
}

function hasLinkBoundary(source: string, offset: number): boolean {
  while (source[offset] === ' ' || source[offset] === '\t') offset += 1;
  if (
    offset >= source.length ||
    source[offset] === '\r' ||
    source[offset] === '\n' ||
    source[offset] === '&'
  ) {
    return true;
  }

  const rest = source.slice(offset, lineEnd(source, offset));
  return ARROW.test(rest) || TEXT_ARROW.test(rest);
}

function hasNodeBoundary(source: string, offset: number): boolean {
  if (source.startsWith(':::', offset)) {
    offset += 3;
    if (!/\w/.test(source[offset] ?? '')) return false;
    while (isIdentifierCharacter(source[offset])) offset += 1;
  }
  return hasLinkBoundary(source, offset);
}

function replaceLineEndings(
  source: string,
  offsets: readonly number[],
): string {
  if (offsets.length === 0) return source;

  let adapted = '';
  let start = 0;
  for (const offset of offsets) {
    adapted += `${source.slice(start, offset)}\\n`;
    start = offset + (source[offset] === '\r' ? 2 : 1);
  }
  return adapted + source.slice(start);
}

/**
 * beautiful-mermaid splits source into physical lines before parsing node
 * shapes. Translate only complete flowchart node labels into the literal `\\n`
 * form it supports; guessing at an incomplete label would produce a partial SVG.
 */
function adaptMermaidSource(source: string): AdaptedMermaidSource {
  const bodyOffset = firstBodyOffset(source);
  if (bodyOffset === null) return { kind: 'renderable', source };

  const replacements: number[] = [];
  let lineOffset = bodyOffset;

  while (lineOffset < source.length) {
    const end = lineEnd(source, lineOffset);
    const line = source.slice(lineOffset, end).replace(/\r$/, '');
    const trimmed = line.trimStart();
    if (
      trimmed !== '' &&
      !trimmed.startsWith('%%') &&
      !NON_NODE_STATEMENT.test(trimmed)
    ) {
      let offset = lineOffset + (line.length - trimmed.length);
      let expectsNode = true;

      while (offset < source.length && offset <= lineEnd(source, offset)) {
        while (source[offset] === ' ' || source[offset] === '\t') offset += 1;
        if (
          source[offset] === '\r' ||
          source[offset] === '\n' ||
          offset >= source.length
        )
          break;

        if (expectsNode) {
          const idStart = offset;
          while (isIdentifierCharacter(source[offset])) offset += 1;
          if (offset === idStart) break;

          const delimiter = delimiterAt(source, offset);
          if (delimiter) {
            const label = scanLabel(
              source,
              offset + delimiter.opener.length,
              delimiter,
            );
            if ('unsafe' in label) {
              if (label.unsafe) return { kind: 'unsafe' };
              break;
            }
            if (
              label.replacements.length > 0 &&
              !hasNodeBoundary(source, label.end)
            ) {
              return { kind: 'unsafe' };
            }
            replacements.push(...label.replacements);
            offset = label.end;
          }

          if (source.startsWith(':::', offset)) {
            offset += 3;
            while (isIdentifierCharacter(source[offset])) offset += 1;
          }
          expectsNode = false;
          continue;
        }

        if (source[offset] === '&') {
          offset += 1;
          expectsNode = true;
          continue;
        }

        const rest = source.slice(offset, lineEnd(source, offset));
        const arrow = ARROW.exec(rest)?.[0] ?? TEXT_ARROW.exec(rest)?.[0];
        if (!arrow) break;
        offset += arrow.length;
        expectsNode = true;
      }

      if (offset > end) {
        lineOffset = lineEnd(source, offset);
      }
    }

    const currentEnd = lineEnd(source, lineOffset);
    if (currentEnd === source.length) break;
    lineOffset = currentEnd + 1;
  }

  return {
    kind: 'renderable',
    source: replaceLineEndings(source, replacements),
  };
}

export { adaptMermaidSource, type AdaptedMermaidSource };
