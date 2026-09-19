import { convertFileSrc } from '@tauri-apps/api/core';

const TEXT_PREVIEW_LIMIT = 512 * 1024;

interface FilePreviewDescriptor {
  id: string;
  assetPath: string;
  filename: string;
  directory: string;
  byteLength: number;
}

type FilePreviewKind = 'text' | 'image' | 'pdf' | 'video' | 'audio' | 'object';

interface FilePreviewType {
  kind: FilePreviewKind;
  language?: string;
  mediaType: string;
}

const TEXT_EXTENSIONS = new Map<string, string | undefined>([
  ['bash', 'bash'],
  ['cjs', 'javascript'],
  ['css', 'css'],
  ['csv', undefined],
  ['diff', 'diff'],
  ['go', 'go'],
  ['htm', 'html'],
  ['html', 'html'],
  ['ini', undefined],
  ['js', 'javascript'],
  ['json', 'json'],
  ['jsonl', 'json'],
  ['jsx', 'tsx'],
  ['log', undefined],
  ['md', 'markdown'],
  ['mdx', 'markdown'],
  ['mjs', 'javascript'],
  ['patch', 'diff'],
  ['properties', undefined],
  ['py', 'python'],
  ['rs', 'rust'],
  ['sh', 'bash'],
  ['sql', undefined],
  ['toml', 'toml'],
  ['ts', 'typescript'],
  ['tsx', 'tsx'],
  ['txt', undefined],
  ['vue', 'vue'],
  ['xml', undefined],
  ['yaml', 'yaml'],
  ['yml', 'yaml'],
  ['zsh', 'bash'],
]);

const TEXT_NAMES = new Map<string, string | undefined>([
  ['dockerfile', 'bash'],
  ['gemfile', undefined],
  ['license', undefined],
  ['makefile', undefined],
  ['readme', 'markdown'],
  ['.editorconfig', undefined],
  ['.env', undefined],
  ['.gitattributes', undefined],
  ['.gitignore', undefined],
  ['.npmrc', undefined],
]);

const IMAGE_TYPES = new Map([
  ['avif', 'image/avif'],
  ['bmp', 'image/bmp'],
  ['gif', 'image/gif'],
  ['ico', 'image/x-icon'],
  ['jpeg', 'image/jpeg'],
  ['jpg', 'image/jpeg'],
  ['png', 'image/png'],
  ['svg', 'image/svg+xml'],
  ['webp', 'image/webp'],
]);

const VIDEO_TYPES = new Map([
  ['m4v', 'video/x-m4v'],
  ['mov', 'video/quicktime'],
  ['mp4', 'video/mp4'],
  ['ogv', 'video/ogg'],
  ['webm', 'video/webm'],
]);

const AUDIO_TYPES = new Map([
  ['aac', 'audio/aac'],
  ['flac', 'audio/flac'],
  ['m4a', 'audio/mp4'],
  ['mp3', 'audio/mpeg'],
  ['oga', 'audio/ogg'],
  ['ogg', 'audio/ogg'],
  ['wav', 'audio/wav'],
]);

function extension(filename: string): string {
  const index = filename.lastIndexOf('.');
  return index > 0 ? filename.slice(index + 1).toLowerCase() : '';
}

/** Classifies by the provided display name; file contents are never sniffed. */
function classifyFilePreview(filename: string): FilePreviewType {
  const name = filename.toLowerCase();
  const ext = extension(name);

  if (TEXT_NAMES.has(name)) {
    return {
      kind: 'text',
      language: TEXT_NAMES.get(name),
      mediaType: 'text/plain;charset=utf-8',
    };
  }
  if (TEXT_EXTENSIONS.has(ext)) {
    return {
      kind: 'text',
      language: TEXT_EXTENSIONS.get(ext),
      mediaType: 'text/plain;charset=utf-8',
    };
  }
  const image = IMAGE_TYPES.get(ext);
  if (image) return { kind: 'image', mediaType: image };
  if (ext === 'pdf') return { kind: 'pdf', mediaType: 'application/pdf' };
  const video = VIDEO_TYPES.get(ext);
  if (video) return { kind: 'video', mediaType: video };
  const audio = AUDIO_TYPES.get(ext);
  if (audio) return { kind: 'audio', mediaType: audio };
  return { kind: 'object', mediaType: 'application/octet-stream' };
}

function isAbsolutePath(path: string): boolean {
  return (
    path.startsWith('/') || /^[a-z]:\//i.test(path) || path.startsWith('//')
  );
}

/** Normalizes `.` and `..` lexically, without consulting the local filesystem. */
function normalizePreviewPath(value: string): string {
  const path = value.replaceAll('\\', '/');
  const drive = /^[a-z]:/i.exec(path)?.[0] ?? '';
  const absolute = isAbsolutePath(path);
  const prefix = drive
    ? `${drive}/`
    : path.startsWith('//')
      ? '//'
      : absolute
        ? '/'
        : '';
  const body = drive ? path.slice(drive.length) : path;
  const parts: string[] = [];

  for (const part of body.split('/')) {
    if (!part || part === '.') continue;
    if (part === '..') {
      if (parts.length && parts.at(-1) !== '..') parts.pop();
      else if (!absolute) parts.push(part);
    } else {
      parts.push(part);
    }
  }

  const normalized = `${prefix}${parts.join('/')}`;
  return normalized || (absolute ? prefix : '.');
}

/** Uses a project-relative directory only when it is lexically inside it. */
function filePreviewDirectoryLabel(
  directory: string,
  projectDirectory?: string,
): string {
  const normalized = normalizePreviewPath(directory);
  if (!projectDirectory || !isAbsolutePath(normalized)) return normalized;

  const normalizedProject = normalizePreviewPath(projectDirectory);
  const project =
    normalizedProject.endsWith('/') &&
    normalizedProject !== '/' &&
    !/^[a-z]:\/$/i.test(normalizedProject)
      ? normalizedProject.slice(0, -1)
      : normalizedProject;
  const caseInsensitive = /^[a-z]:\//i.test(project);
  const comparedDirectory = caseInsensitive
    ? normalized.toLowerCase()
    : normalized;
  const comparedProject = caseInsensitive ? project.toLowerCase() : project;

  if (comparedDirectory === comparedProject) return '.';
  const prefix = `${comparedProject}/`;
  return comparedDirectory.startsWith(prefix)
    ? normalized.slice(project.length + 1)
    : normalized;
}

/** Labels a file's parent, relative only when the file is inside the project. */
function filePreviewDirectoryForPath(
  filePath: string,
  projectDirectory?: string,
): string {
  const normalized = normalizePreviewPath(filePath);
  const separator = normalized.lastIndexOf('/');
  const directory =
    separator < 0
      ? '.'
      : separator === 0
        ? '/'
        : normalized.slice(0, separator);
  return filePreviewDirectoryLabel(directory, projectDirectory);
}

function isBrowserPreviewUrl(value: string): boolean {
  try {
    const protocol = new URL(value).protocol.toLowerCase();
    return (
      protocol === 'data:' || protocol === 'http:' || protocol === 'https:'
    );
  } catch {
    return false;
  }
}

/** Converts snapshots for Tauri while keeping deterministic browser fixtures usable. */
function filePreviewAssetUrl(assetPath: string): string {
  return isBrowserPreviewUrl(assetPath) ? assetPath : convertFileSrc(assetPath);
}

function escapePreviewHtml(value: string): string {
  return value
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&#39;');
}

interface TextPreview {
  text: string;
  truncated: boolean;
}

/** Reads at most one preview window even when an asset server ignores Range. */
async function fetchTextPreview(
  url: string,
  byteLength: number,
  signal?: AbortSignal,
): Promise<TextPreview> {
  if (byteLength === 0) return { text: '', truncated: false };

  const response = await fetch(url, {
    headers: { Range: `bytes=0-${TEXT_PREVIEW_LIMIT - 1}` },
    signal,
  });
  if (!response.ok) throw new Error('Preview request failed');

  const reader = response.body?.getReader();
  if (!reader) {
    const bytes = new Uint8Array(await response.arrayBuffer());
    return {
      text: new TextDecoder().decode(bytes.slice(0, TEXT_PREVIEW_LIMIT)),
      truncated:
        bytes.byteLength > TEXT_PREVIEW_LIMIT || byteLength > bytes.byteLength,
    };
  }

  const chunks: Uint8Array[] = [];
  let received = 0;
  let exceeded = false;
  while (received <= TEXT_PREVIEW_LIMIT) {
    const { done, value } = await reader.read();
    if (done) break;
    if (received + value.byteLength > TEXT_PREVIEW_LIMIT) {
      chunks.push(value.slice(0, TEXT_PREVIEW_LIMIT - received));
      received = TEXT_PREVIEW_LIMIT;
      exceeded = true;
      await reader.cancel();
      break;
    }
    chunks.push(value);
    received += value.byteLength;
  }

  const bytes = new Uint8Array(received);
  let offset = 0;
  for (const chunk of chunks) {
    bytes.set(chunk, offset);
    offset += chunk.byteLength;
  }

  const contentRange = response.headers.get('content-range');
  const total = contentRange ? Number(contentRange.split('/').at(-1)) : 0;
  return {
    text: new TextDecoder().decode(bytes),
    truncated:
      exceeded ||
      byteLength > received ||
      (Number.isFinite(total) && total > received),
  };
}

export {
  TEXT_PREVIEW_LIMIT,
  classifyFilePreview,
  escapePreviewHtml,
  fetchTextPreview,
  filePreviewAssetUrl,
  filePreviewDirectoryForPath,
  filePreviewDirectoryLabel,
  isBrowserPreviewUrl,
  normalizePreviewPath,
};

export type {
  FilePreviewDescriptor,
  FilePreviewKind,
  FilePreviewType,
  TextPreview,
};
