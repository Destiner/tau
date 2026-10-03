import '@fontsource-variable/inter';
import '../../src/styles.css';
import './scene.css';
import {
  createApp,
  defineComponent,
  h,
  nextTick,
  reactive,
  ref,
  type VNode,
} from 'vue';
import UiButton from '../../src/components/ui/UiButton.vue';
import UiInput from '../../src/components/ui/UiInput.vue';
import UiTextarea from '../../src/components/ui/UiTextarea.vue';
import {
  defaultProps,
  validateSelection,
  type Component,
  type Observation,
  type Props,
  type Theme,
} from '../contract';

const scene = reactive<{
  component: Component;
  props: Props;
  model: string;
  clicks: number;
  changes: number;
}>({
  component: 'input',
  props: { ...defaultProps },
  model: '',
  clicks: 0,
  changes: 0,
});
const element = ref<HTMLElement>();
const generation = ref(0);
const App = defineComponent({
  setup(): () => VNode {
    return (): VNode => {
      const p = scene.props;
      const common = {
        key: generation.value,
        ref: (value: unknown): void => {
          element.value = (value as { $el?: HTMLElement } | null)?.$el;
        },
      };
      const control =
        scene.component === 'button'
          ? h(
              UiButton,
              {
                ...common,
                variant: (p.variant || 'secondary') as
                  'primary' | 'secondary' | 'ghost',
                size: p.size as 'sm' | 'md',
                disabled: p.disabled,
                onClick: () => {
                  scene.clicks++;
                },
              },
              () => p.label,
            )
          : scene.component === 'input'
            ? h(UiInput, {
                ...common,
                modelValue: scene.model,
                'onUpdate:modelValue': (v: string) => {
                  scene.model = v;
                },
                onInput: () => {
                  scene.changes++;
                },
                variant: (p.variant || 'bordered') as
                  'bordered' | 'mono' | 'bare',
                error: p.error,
                placeholder: p.placeholder,
                disabled: p.disabled,
                readonly: p.readonly,
              })
            : h(UiTextarea, {
                ...common,
                modelValue: scene.model,
                'onUpdate:modelValue': (v: string) => {
                  scene.model = v;
                },
                onInput: () => {
                  scene.changes++;
                },
                variant: (p.variant || 'bordered') as 'bordered' | 'bare',
                placeholder: p.placeholder,
                disabled: p.disabled,
                readonly: p.readonly,
                rows: p.rows,
                'data-monospaced': p.monospaced ? 'true' : undefined,
              });
      return h('div', { id: 'canvas' }, [
        h(
          'div',
          {
            id: 'subject-wrap',
            style:
              scene.component === 'button'
                ? undefined
                : { width: `${p.width}px` },
          },
          [control],
        ),
      ]);
    };
  },
});
createApp(App).mount('#app');

function subject(): HTMLButtonElement | HTMLInputElement | HTMLTextAreaElement {
  if (!element.value) throw new Error('subject not mounted');
  return element.value as
    HTMLButtonElement | HTMLInputElement | HTMLTextAreaElement;
}
function observation(): Observation {
  const el = subject();
  const text =
    el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement
      ? el
      : null;
  return {
    value: text?.value ?? el.textContent ?? '',
    selection: text ? [text.selectionStart ?? 0, text.selectionEnd ?? 0] : null,
    focused: document.activeElement === el,
    disabled: el.disabled,
    readonly: text?.readOnly ?? scene.props.readonly,
    clicks: scene.clicks,
    changes: scene.changes,
  };
}
async function reset(
  component: Component,
  props: Partial<Props>,
  theme: Theme,
): Promise<void> {
  if (document.activeElement instanceof HTMLElement)
    document.activeElement.blur();
  generation.value++;
  scene.component = component;
  scene.props = { ...defaultProps, ...props };
  scene.model = scene.props.value;
  scene.clicks = 0;
  scene.changes = 0;
  document.documentElement.dataset.theme = theme;
  await nextTick();
}
async function patch(p: Partial<Props>): Promise<void> {
  scene.props = { ...scene.props, ...p };
  if (p.value !== undefined) scene.model = p.value;
  await nextTick();
}
declare global {
  interface Window {
    tauReference: {
      reset: typeof reset;
      patch: typeof patch;
      observe: typeof observation;
      select: (start: number, end: number) => void;
      focus: () => void;
      blur: () => void;
    };
  }
}
window.tauReference = {
  reset,
  patch,
  observe: observation,
  select: (start, end): void => {
    const el = subject();
    if (!(el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement))
      throw new Error('button has no selection');
    validateSelection(el.value, start, end);
    el.setSelectionRange(start, end);
  },
  focus: (): void => subject().focus(),
  blur: (): void => subject().blur(),
};
