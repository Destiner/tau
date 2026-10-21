interface ConfirmationEntry {
  content: HTMLElement;
  onEscape: () => void;
}

const confirmations: ConfirmationEntry[] = [];

function isVisible(element: HTMLElement): boolean {
  const style = window.getComputedStyle(element);
  return style.display !== 'none' && style.visibility !== 'hidden';
}

function topmostAlertDialog(): HTMLElement | undefined {
  return [...document.querySelectorAll<HTMLElement>('[role="alertdialog"]')]
    .filter(isVisible)
    .at(-1);
}

function handleDocumentKeydown(event: KeyboardEvent): void {
  if (event.key !== 'Escape' || event.isComposing) return;

  const confirmation = confirmations.at(-1);
  if (!confirmation || topmostAlertDialog() !== confirmation.content) {
    return;
  }

  event.preventDefault();
  event.stopImmediatePropagation();
  confirmation.onEscape();
}

/**
 * Routes Escape to the latest compact confirmation only when it is the
 * document's topmost alert dialog. This leaves Reka dialogs above it in charge.
 */
export default function registerConfirmation(
  content: HTMLElement,
  onEscape: () => void,
): () => void {
  const entry = { content, onEscape };
  const wasEmpty = confirmations.length === 0;
  confirmations.push(entry);
  if (wasEmpty)
    document.addEventListener('keydown', handleDocumentKeydown, true);

  return () => {
    const index = confirmations.indexOf(entry);
    if (index !== -1) confirmations.splice(index, 1);
    if (confirmations.length === 0) {
      document.removeEventListener('keydown', handleDocumentKeydown, true);
    }
  };
}
