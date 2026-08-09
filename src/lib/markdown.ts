import DOMPurify from "dompurify";
import { marked } from "marked";

marked.use({ gfm: true, breaks: true });

export function renderMarkdown(source: string): string {
  return DOMPurify.sanitize(marked.parse(source, { async: false }) as string);
}
