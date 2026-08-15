import { readText, writeText } from '@tauri-apps/plugin-clipboard-manager';

import type { UiMenuItem } from '../components/ui/UiMenu.vue';

type TextField = HTMLInputElement | HTMLTextAreaElement;

async function copyField(
  field: TextField,
  start: number,
  end: number,
  cut: boolean,
): Promise<void> {
  const text = field.value.slice(start, end);
  if (!text) return;
  try {
    await writeText(text);
  } catch {
    return;
  }
  if (cut) replaceFieldRange(field, start, end, '');
}

async function pasteField(
  field: TextField,
  start: number,
  end: number,
): Promise<void> {
  let text: string;
  try {
    text = await readText();
  } catch {
    return;
  }
  if (text) replaceFieldRange(field, start, end, text);
}

/**
 * Fields are bound with v-model, so the edit is announced with an input event
 * rather than written to the reactive state each field happens to use.
 */
function replaceFieldRange(
  field: TextField,
  start: number,
  end: number,
  text: string,
): void {
  field.value = field.value.slice(0, start) + text + field.value.slice(end);
  const caret = start + text.length;
  field.dispatchEvent(new Event('input', { bubbles: true }));
  field.focus();
  field.setSelectionRange(caret, caret);
}

/**
 * The cut/copy/paste menu a text field carries, replacing the webview's. The
 * range is read when the menu opens, while the field still holds it: the menu
 * takes focus next, and the items hand that range back on the way out.
 */
function textFieldItems(field: () => TextField | undefined): UiMenuItem[] {
  const element = field();
  const start = element?.selectionStart ?? 0;
  const end = element?.selectionEnd ?? 0;
  const selected = start !== end;
  const editable = element ? !element.readOnly && !element.disabled : false;
  if (!element) return [];
  return [
    {
      label: 'Cut',
      disabled: !selected || !editable,
      run: () => void copyField(element, start, end, editable),
    },
    {
      label: 'Copy',
      disabled: !selected,
      run: () => void copyField(element, start, end, false),
    },
    {
      label: 'Paste',
      disabled: !editable,
      run: () => void pasteField(element, start, end),
    },
  ];
}

export default textFieldItems;
