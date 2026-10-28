import { nextTick, onBeforeUnmount, watch } from 'vue';

export function topModal(): HTMLElement | undefined {
  return [
    ...document.querySelectorAll<HTMLElement>(
      '[role="dialog"]:not([aria-modal="false"]), [role="alertdialog"]:not([aria-modal="false"])',
    ),
  ]
    .filter((element) => {
      const style = window.getComputedStyle(element);
      return style.display !== 'none' && style.visibility !== 'hidden';
    })
    .at(-1);
}

/** Repair only focus lost by disabling this surface, never a later user choice. */
export default function useDisabledFocusRepair(
  root: () => HTMLElement | undefined,
  disabled: () => boolean,
  fallback: () => void,
): void {
  let origin: HTMLElement | undefined;
  let revision = 0;

  function clear(): void {
    origin = undefined;
    revision += 1;
    document.removeEventListener('focusin', handleFocus);
  }

  function handleFocus(event: FocusEvent): void {
    if (event.target !== origin) clear();
  }

  watch(disabled, (blocked) => {
    if (blocked) {
      clear();
      const active = document.activeElement;
      if (active instanceof HTMLElement && root()?.contains(active)) {
        origin = active;
        document.addEventListener('focusin', handleFocus);
      }
      return;
    }

    const target = origin;
    const version = revision;
    const wasDisabled = target?.matches(':disabled');
    void nextTick(() => {
      if (version !== revision) return;
      const surface = root();
      const modal = topModal();
      const active = document.activeElement;
      const canRepair =
        wasDisabled &&
        surface?.isConnected &&
        (!modal || modal === surface) &&
        (active === document.body || active === target);
      clear();
      if (!canRepair) return;
      if (
        target?.isConnected &&
        surface.contains(target) &&
        !target.matches(':disabled')
      )
        target.focus({ preventScroll: true });
      else fallback();
    });
  });

  onBeforeUnmount(clear);
}
