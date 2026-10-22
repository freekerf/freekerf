<script lang="ts">
  // Editing a text object on the canvas (double click, Enter, or right after the
  // text tool placed it): a field over the text. Enter or leaving the field applies
  // (`document.set_text`, one undo step), Shift+Enter starts a new line; Esc keeps
  // the text as it was.
  import { onMount } from 'svelte';

  import { Camera, type CameraState } from '../canvas/camera';
  import type { ObjectSummary } from '../gen/api';
  import { t } from '../i18n/index.svelte';

  let {
    object,
    camera,
    onsubmit,
    oncancel,
  }: {
    object: ObjectSummary;
    camera: CameraState;
    onsubmit: (text: string) => void;
    oncancel: () => void;
  } = $props();

  let input: HTMLTextAreaElement | undefined = $state();
  let done = false;

  // over the text's bounds, at least wide enough to type in
  const box = $derived.by(() => {
    const [x0, y0, x1, y1] = object.bounds ?? [0, 0, 0, 0];
    const [left, top] = Camera.toScreen(camera, x0, y1);
    const [right, bottom] = Camera.toScreen(camera, x1, y0);
    const lines = Math.max(1, (object.text?.text ?? '').split('\n').length);
    const h = Math.max(24 * lines, bottom - top);
    const font = Math.max(13, (h / lines) * 0.7);
    return { left, top, width: Math.max(160, right - left + h / lines), height: h, font };
  });

  onMount(() => {
    input?.focus();
    input?.select();
  });

  function finish(apply: boolean) {
    if (done) return;
    done = true;
    const v = input?.value ?? '';
    if (apply && v.trim() && v !== object.text?.text) onsubmit(v);
    else oncancel();
  }
</script>

<textarea
  class="text-editor"
  data-testid="text-editor"
  aria-label={t('obj-text')}
  bind:this={input}
  value={object.text?.text ?? ''}
  style:left={`${box.left}px`}
  style:top={`${box.top}px`}
  style:width={`${box.width}px`}
  style:height={`${box.height}px`}
  style:font-size={`${box.font}px`}
  rows={Math.max(1, (object.text?.text ?? '').split('\n').length)}
  onkeydown={(e) => {
    if (e.key === 'Enter' && !e.shiftKey) finish(true);
    else if (e.key === 'Escape' && !e.shiftKey) finish(false);
    else return;
    e.preventDefault();
  }}
  onblur={() => finish(true)}></textarea>
