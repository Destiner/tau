type HighlightMessage =
  | { type: 'start'; header: string }
  | { type: 'chunk'; html: string; offset: number }
  | { type: 'done' | 'error' };

/** Enrich an already readable preview without blocking its first paint. */
function highlightPreview(
  host: HTMLElement,
  source: string,
  language: string,
): () => void {
  if (source.length > 512 * 1024) return () => {};
  let worker: Worker;
  try {
    worker = new Worker(
      new URL('./preview-highlight.worker.ts', import.meta.url),
      {
        type: 'module',
      },
    );
  } catch {
    return () => {};
  }
  const scroller = host.parentElement;
  const pending: HighlightMessage[] = [];
  let code: HTMLElement | undefined;
  let remainder: Text | undefined;
  let consumed = 0;
  let lineIndex = 0;
  const sourceLines = source.split('\n');
  let stopped = false;
  let draining = false;
  const deadline = window.setTimeout(stop, 15_000);

  function stop(): void {
    if (stopped) return;
    stopped = true;
    worker.terminate();
    window.clearTimeout(deadline);
    document.removeEventListener('selectionchange', resume);
    pending.length = 0;
  }

  function selected(): boolean {
    const selection = window.getSelection();
    return Boolean(
      selection &&
      !selection.isCollapsed &&
      Array.from({ length: selection.rangeCount }, (_, index) =>
        selection.getRangeAt(index),
      ).some((range) => range.intersectsNode(host)),
    );
  }

  function resume(): void {
    if (!selected()) void drain();
  }

  async function drain(): Promise<void> {
    if (draining || stopped || selected()) return;
    draining = true;
    while (pending.length && !stopped && !selected()) {
      const message = pending.shift()!;
      if (message.type === 'error') {
        stop();
        break;
      }
      if (message.type === 'done') {
        stop();
        break;
      }
      const top = scroller?.scrollTop ?? 0;
      const left = scroller?.scrollLeft ?? 0;
      if (message.type === 'start') {
        const template = document.createElement('template');
        template.innerHTML = `${message.header}</code></pre>`;
        const pre = template.content.firstElementChild;
        code = pre?.querySelector('code') ?? undefined;
        if (!pre || !code) {
          stop();
          break;
        }
        remainder = document.createTextNode(source);
        code.append(remainder);
        host.replaceChildren(pre);
      } else if (message.type === 'chunk' && code && remainder) {
        const template = document.createElement('template');
        template.innerHTML = message.html;
        // HTML parsing normalizes CRLF; add each original CR as a text node.
        for (const line of template.content.querySelectorAll('.line')) {
          if (sourceLines[lineIndex]?.endsWith('\r'))
            line.after(document.createTextNode('\r'));
          lineIndex += 1;
        }
        code.insertBefore(template.content, remainder);
        remainder.deleteData(0, message.offset - consumed);
        consumed = message.offset;
      }
      if (scroller) scroller.scrollTo(left, top);
      // Let input and paint run before inserting the next batch.
      await new Promise<void>((resolve) => window.setTimeout(resolve, 0));
    }
    draining = false;
  }

  document.addEventListener('selectionchange', resume);
  worker.onmessage = (event: MessageEvent<HighlightMessage>): void => {
    if (stopped) return;
    if (event.data.type === 'done') window.clearTimeout(deadline);
    pending.push(event.data);
    void drain();
  };
  worker.onerror = (event): void => {
    event.preventDefault();
    stop();
  };
  try {
    worker.postMessage({ source, language });
  } catch {
    stop();
  }
  return stop;
}

export default highlightPreview;
