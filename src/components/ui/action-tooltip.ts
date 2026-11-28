import { shallowRef } from 'vue';

import type { ShortcutInput } from '../../lib/app-commands/shortcut';

interface ActionTooltipOptions {
  text?: string;
  shortcut?: ShortcutInput;
  side?: 'top' | 'right' | 'bottom' | 'left';
  align?: 'start' | 'center' | 'end';
  delay?: number;
  contentClass?: string;
}

// False delegates ownership to a launcher; wrappers default to undefined to
// avoid Vue treating an absent Boolean-union prop as false.
type ActionTooltip = ActionTooltipOptions | false;

const ACTION_TOOLTIP_DELAY = 450;
const activeTooltipId = shallowRef<string>();

export { ACTION_TOOLTIP_DELAY, activeTooltipId, type ActionTooltip };
