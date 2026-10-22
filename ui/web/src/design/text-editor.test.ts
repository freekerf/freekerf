import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

import { Camera } from '../canvas/camera';
import type { ObjectSummary } from '../gen/api';
import TextEditor from './TextEditor.svelte';

const camera = Camera.fitted({ x: 0, y: 0, width: 200, height: 100 }, 800, 400, 0);
const object = {
  id: 7,
  layer: 1,
  name: '',
  kind: 'text',
  bounds: [10, 10, 50, 20],
  transform: [1, 0, 0, 1, 0, 0],
  text: { text: 'Hello', height_mm: 10 },
} as ObjectSummary;

function mount() {
  const onsubmit = vi.fn();
  const oncancel = vi.fn();
  render(TextEditor, { object, camera, onsubmit, oncancel });
  return { onsubmit, oncancel, input: screen.getByTestId<HTMLTextAreaElement>('text-editor') };
}

describe('text editor on the canvas', () => {
  it('sits over the text with the text selected, and Enter applies', async () => {
    const { onsubmit, oncancel, input } = mount();
    expect(input.value).toBe('Hello');
    expect(document.activeElement).toBe(input);
    expect([input.selectionStart, input.selectionEnd]).toEqual([0, 5]);
    // 4 px per mm: the bounds' top left (10, 20 mm) is at (40, 320 px)
    expect([input.style.left, input.style.top, input.style.height]).toEqual(['40px', '320px', '40px']);
    await fireEvent.input(input, { target: { value: 'Hi there' } });
    await fireEvent.keyDown(input, { key: 'Enter' });
    expect(onsubmit).toHaveBeenCalledWith('Hi there');
    // the blur that follows does not apply it twice
    await fireEvent.blur(input);
    expect(onsubmit).toHaveBeenCalledTimes(1);
    expect(oncancel).not.toHaveBeenCalled();
  });

  it('Esc, an empty text or no change keeps the text; leaving the field applies', async () => {
    let m = mount();
    await fireEvent.input(m.input, { target: { value: 'Changed' } });
    await fireEvent.keyDown(m.input, { key: 'Escape' });
    expect(m.oncancel).toHaveBeenCalled();
    expect(m.onsubmit).not.toHaveBeenCalled();
    document.body.innerHTML = '';

    m = mount();
    await fireEvent.input(m.input, { target: { value: '   ' } });
    await fireEvent.keyDown(m.input, { key: 'Enter' });
    expect(m.onsubmit).not.toHaveBeenCalled();
    expect(m.oncancel).toHaveBeenCalled();
    document.body.innerHTML = '';

    m = mount();
    await fireEvent.input(m.input, { target: { value: 'Bye' } });
    await fireEvent.blur(m.input);
    expect(m.onsubmit).toHaveBeenCalledWith('Bye');
  });

  it('Shift+Enter starts a new line instead of applying', async () => {
    const m = mount();
    const shifted = await fireEvent.keyDown(m.input, { key: 'Enter', shiftKey: true });
    // not prevented: the browser inserts the line break
    expect(shifted).toBe(true);
    expect(m.onsubmit).not.toHaveBeenCalled();
    await fireEvent.input(m.input, { target: { value: 'Hel\nlo' } });
    await fireEvent.keyDown(m.input, { key: 'Enter' });
    expect(m.onsubmit).toHaveBeenCalledWith('Hel\nlo');
  });
});
