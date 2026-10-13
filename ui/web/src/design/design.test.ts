// Component tests of the Design mode on the fake document (src/dev/fake-document.ts).
import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';

import App from '../App.svelte';
import { FakeService } from '../dev/fake-service';
import { projections } from '../start';
import { documentSummary } from '../state/document.svelte';

async function mountDesign(service = new FakeService()) {
  const props = projections(service);
  await Promise.all([
    props.machine.refresh(),
    props.view.refresh(),
    props.settings.refresh(),
    props.doc.refresh(),
  ]);
  render(App, props);
  await service.dispatch('view.set_mode', { mode: 'design' });
  await waitFor(() => expect(document.documentElement.dataset.mode).toBe('design'));
  const summary = async () => documentSummary(await service.dispatch('session.snapshot', {}));
  return { service, props, doc: service, summary };
}

const docActions = (d: { calls: { action: string }[] }) => d.calls.map((c) => c.action);

describe('Design mode', () => {
  it('tools are the view state; undo and redo follow the document', async () => {
    const { service, doc } = await mountDesign();
    await fireEvent.click(document.querySelector('[data-tool="rect"]')!);
    await waitFor(() => expect(service.view.tool).toBe('rect'));
    expect(document.querySelector('[data-tool="rect"]')!.getAttribute('aria-pressed')).toBe('true');
    const undo = document.querySelector<HTMLButtonElement>('[data-tool="undo"]')!;
    expect(undo.disabled).toBe(true);
    await doc.dispatch('document.add_shape', { shape: 'rect', x: 0, y: 0, width: 10, height: 10 });
    await waitFor(() => expect(undo.disabled).toBe(false));
    await fireEvent.click(undo);
    await waitFor(() => expect(docActions(doc)).toContain('history.undo'));
    await fireEvent.click(document.querySelector('[data-tool="import"]')!);
    await screen.findByTestId('dialog-import');
  });

  it('keyboard: copy, cut and paste on the canvas, never while typing', async () => {
    const { doc, summary } = await mountDesign();
    const { id } = await doc.dispatch('document.add_shape', {
      shape: 'rect',
      x: 10,
      y: 10,
      width: 10,
      height: 10,
    });
    await doc.dispatch('document.select', { ids: [id], mode: 'replace' });
    await waitFor(async () => expect((await summary()).selection).toEqual([id]));
    await fireEvent.keyDown(window, { key: 'c', ctrlKey: true });
    await waitFor(async () => expect((await summary()).clipboard).toBe(1));
    await fireEvent.keyDown(window, { key: 'v', ctrlKey: true });
    await waitFor(async () => expect((await summary()).objects).toHaveLength(2));
    await fireEvent.keyDown(window, { key: 'x', ctrlKey: true });
    await waitFor(async () => expect((await summary()).objects).toHaveLength(1));
    // a text field keeps Ctrl+V for itself
    const input = document.createElement('input');
    document.body.appendChild(input);
    await fireEvent.keyDown(input, { key: 'v', ctrlKey: true });
    input.remove();
    await new Promise((r) => setTimeout(r, 50));
    expect((await summary()).objects).toHaveLength(1);
    await fireEvent.keyDown(window, { key: 'v', ctrlKey: true });
    await waitFor(async () => expect((await summary()).objects).toHaveLength(2));
  });

  it('keyboard: tools, select all, duplicate, delete, undo, nudge, Esc', async () => {
    const { service, doc, summary } = await mountDesign();
    await doc.dispatch('document.add_shape', { shape: 'rect', x: 10, y: 10, width: 10, height: 10 });
    await doc.dispatch('document.add_shape', { shape: 'rect', x: 30, y: 10, width: 10, height: 10 });
    await fireEvent.keyDown(window, { key: 'e' });
    await waitFor(() => expect(service.view.tool).toBe('ellipse'));
    await fireEvent.keyDown(window, { key: 'Escape' });
    await waitFor(() => expect(service.view.tool).toBe('select'));
    await fireEvent.keyDown(window, { key: 'a', ctrlKey: true });
    await waitFor(async () => expect((await summary()).selection).toHaveLength(2));
    await fireEvent.keyDown(window, { key: 'ArrowRight', shiftKey: true });
    await waitFor(async () => expect((await summary()).objects[0]!.bounds![0]).toBeCloseTo(20));
    await fireEvent.keyDown(window, { key: 'd', ctrlKey: true });
    await waitFor(async () => expect((await summary()).objects).toHaveLength(4));
    await fireEvent.keyDown(window, { key: 'Delete' });
    await waitFor(async () => expect((await summary()).objects).toHaveLength(2));
    await fireEvent.keyDown(window, { key: 'z', ctrlKey: true });
    await waitFor(async () => expect((await summary()).objects).toHaveLength(4));
    await fireEvent.keyDown(window, { key: 'Escape' });
    await waitFor(async () => expect((await summary()).selection).toEqual([]));
  });

  it('layers panel: rows, current layer, show/hide, edit, order', async () => {
    const { doc, summary } = await mountDesign();
    await doc.dispatch('document.add_shape', { shape: 'rect', x: 0, y: 0, width: 10, height: 10 });
    await doc.dispatch('document.layer_for_color', { color: '#10A37F' });
    const list = await screen.findByTestId('layer-list');
    await waitFor(() => expect(list.querySelectorAll('.layer')).toHaveLength(2));
    const rows = list.querySelectorAll<HTMLElement>('.layer');
    expect(rows[0]!.textContent).toMatch(/^\s*00 Line/);
    expect(rows[0]!.textContent).toContain('1 obj.');
    // the row makes the layer current
    await fireEvent.click(rows[1]!.querySelector('.layer-row')!);
    await waitFor(async () => expect((await summary()).current_layer).toBe(Number(rows[1]!.dataset.layer)));
    // hide
    await fireEvent.click(within(rows[0]!).getByLabelText(/Show on canvas/));
    await waitFor(async () => expect((await summary()).layers[0]!.visible).toBe(false));
    // expand and edit speed and mode
    await fireEvent.click(within(rows[0]!).getByLabelText('Edit layer settings'));
    const speed = within(rows[0]!).getByTestId('layer-speed') as HTMLInputElement;
    speed.value = '2500';
    await fireEvent.change(speed);
    await waitFor(async () => expect((await summary()).layers[0]!.speed_mm_min).toBe(2500));
    // lines per mm only for fillings
    expect(within(rows[0]!).queryByTestId('layer-lines-per-mm')).toBeNull();
    await fireEvent.click(within(rows[0]!).getByRole('radio', { name: 'Fill' }));
    await waitFor(async () => expect((await summary()).layers[0]!.mode).toBe('fill'));
    const density = (await within(rows[0]!).findByTestId('layer-lines-per-mm')) as HTMLInputElement;
    expect(density.value).toBe('10');
    expect(density.closest('label')!.title).toContain('0.10 mm');
    density.value = '5';
    await fireEvent.change(density);
    await waitFor(async () => expect((await summary()).layers[0]!.fill.lines_per_mm).toBe(5));
    const pattern = within(rows[0]!).getByTestId('layer-pattern') as HTMLSelectElement;
    expect(pattern.value).toBe('horizontal');
    expect(pattern.options).toHaveLength(12);
    await fireEvent.change(pattern, { target: { value: 'diagonal_grid' } });
    await waitFor(async () => expect((await summary()).layers[0]!.fill.pattern).toBe('diagonal_grid'));
    const power = within(rows[0]!).getByTestId('layer-power') as HTMLInputElement;
    power.value = '75';
    await fireEvent.change(power);
    await waitFor(async () => expect((await summary()).layers[0]!.power).toBe(750));
    // M3 / M4: a layer following the machine shows M3
    const laser = within(rows[0]!).getByTestId('layer-laser');
    expect(within(laser).getByRole('radio', { name: 'M3 constant' }).getAttribute('aria-checked')).toBe(
      'true',
    );
    await fireEvent.click(within(laser).getByRole('radio', { name: 'M4 dynamic' }));
    await waitFor(async () => expect((await summary()).layers[0]!.laser_mode).toBe('dynamic'));
    await waitFor(() => expect(rows[0]!.querySelector('.layer-params')!.textContent).toContain('M4'));
    await fireEvent.click(within(rows[0]!).getByLabelText('Move down in the output order'));
    await waitFor(async () => expect((await summary()).layers[1]!.color).toBe('#4C8DFF'));
  });

  it('palette: with a selection moves it to the colour layer, without one sets the current layer', async () => {
    const { doc, summary, props } = await mountDesign();
    const { id } = await doc.dispatch('document.add_shape', {
      shape: 'rect',
      x: 0,
      y: 0,
      width: 10,
      height: 10,
    });
    await waitFor(() => expect(props.doc.selection).toEqual([id]));
    const palette = await screen.findByTestId('palette');
    await fireEvent.click(palette.querySelector('[data-color="#E8622C"]')!);
    await waitFor(async () => {
      const s = await summary();
      expect(s.layers.find((l) => l.id === s.objects[0]!.layer)!.color).toBe('#E8622C');
    });
    await waitFor(() =>
      expect(palette.querySelector('[data-color="#E8622C"]')!.getAttribute('aria-pressed')).toBe('true'),
    );
    await doc.dispatch('document.select', { ids: [], mode: 'replace' });
    await waitFor(() => expect(props.doc.selection).toEqual([]));
    await fireEvent.click(palette.querySelector('[data-color="#10A37F"]')!);
    await waitFor(async () => {
      const s = await summary();
      expect(s.layers.find((l) => l.id === s.current_layer)!.color).toBe('#10A37F');
    });
    expect((await summary()).objects.find((o) => o.id === id)!.layer).not.toBe(
      (await summary()).current_layer,
    );
  });

  it('inspector: position, size with proportions, rotation, text, align, duplicate, delete', async () => {
    const { service, doc, summary } = await mountDesign();
    await service.dispatch('view.open_panel', { name: 'object' });
    const inspector = await screen.findByTestId('inspector');
    expect(inspector.textContent).toContain('Nothing selected');
    await doc.dispatch('document.add_shape', { shape: 'rect', x: 0, y: 0, width: 20, height: 10 });
    const x = (await screen.findByTestId('obj-x')) as HTMLInputElement;
    expect(Number(x.value)).toBe(10);
    x.value = '50';
    await fireEvent.change(x);
    await waitFor(async () => expect((await summary()).objects[0]!.bounds![0]).toBeCloseTo(40));
    const w = screen.getByTestId('obj-w') as HTMLInputElement;
    w.value = '40';
    await fireEvent.change(w);
    await waitFor(async () => {
      const b = (await summary()).objects[0]!.bounds!;
      expect(b[2] - b[0]).toBeCloseTo(40);
      expect(b[3] - b[1]).toBeCloseTo(20); // proportions kept
    });
    await fireEvent.click(screen.getByTestId('obj-lock'));
    const h = screen.getByTestId('obj-h') as HTMLInputElement;
    h.value = '5';
    await fireEvent.change(h);
    await waitFor(async () => {
      const b = (await summary()).objects[0]!.bounds!;
      expect(b[2] - b[0]).toBeCloseTo(40);
      expect(b[3] - b[1]).toBeCloseTo(5);
    });
    const rot = screen.getByTestId('obj-rot') as HTMLInputElement;
    rot.value = '90';
    await fireEvent.change(rot);
    await waitFor(() => expect(Number((screen.getByTestId('obj-rot') as HTMLInputElement).value)).toBe(90));
    await fireEvent.click(screen.getByTestId('obj-duplicate'));
    await waitFor(async () => expect((await summary()).objects).toHaveLength(2));
    await waitFor(() => expect(screen.getByTestId('obj-sub').textContent).toContain('Rectangle'));
    await doc.dispatch('document.select_all', {});
    await waitFor(() => expect(screen.getByTestId('obj-sub').textContent).toContain('2 objects'));
    await fireEvent.click(screen.getByLabelText('Align left'));
    await waitFor(async () => {
      const [a, b] = (await summary()).objects;
      expect(a!.bounds![0]).toBeCloseTo(b!.bounds![0]);
    });
    await fireEvent.click(screen.getByTestId('obj-delete'));
    await waitFor(async () => expect((await summary()).objects).toHaveLength(0));
    // text
    await doc.dispatch('document.add_text', { text: 'Hi', x: 0, y: 0, height_mm: 10 });
    const ta = (await screen.findByLabelText('Text')) as HTMLTextAreaElement;
    ta.value = 'Hello';
    await fireEvent.change(ta);
    await waitFor(async () => expect((await summary()).objects[0]!.text!.text).toBe('Hello'));
  });

  it('combine, offset and array from the inspector and the rail', async () => {
    const { service, summary } = await mountDesign();
    await service.dispatch('view.open_panel', { name: 'object' });
    const a = (
      await service.dispatch('document.add_shape', { shape: 'rect', x: 0, y: 0, width: 10, height: 10 })
    ).id;
    const weld = document.querySelector<HTMLButtonElement>('[data-tool="weld"]')!;
    await waitFor(() => expect(weld.disabled).toBe(true));
    // offset of one object
    const distance = (await screen.findByTestId('obj-offset-distance')) as HTMLInputElement;
    distance.value = '2';
    await fireEvent.change(distance);
    await fireEvent.click(screen.getByTestId('obj-offset-go'));
    await waitFor(async () => expect((await summary()).objects).toHaveLength(2));
    expect(service.calls.find((c) => c.action === 'document.offset')!.params).toEqual({
      ids: [a],
      distance_mm: 2,
    });
    // two selected: weld from the rail
    await service.dispatch('document.select_all', {});
    await waitFor(() => expect(weld.disabled).toBe(false));
    await fireEvent.click(weld);
    await waitFor(async () => expect((await summary()).objects).toHaveLength(1));
    // array of the result, 3 × 1
    const cols = screen.getByTestId('obj-array-columns') as HTMLInputElement;
    cols.value = '3';
    await fireEvent.change(cols);
    const rows = screen.getByTestId('obj-array-rows') as HTMLInputElement;
    rows.value = '1';
    await fireEvent.change(rows);
    await fireEvent.click(screen.getByTestId('obj-array-go'));
    await waitFor(async () => expect((await summary()).objects).toHaveLength(3));
    // the subtract button needs two
    expect((screen.getByTestId('obj-difference') as HTMLButtonElement).disabled).toBe(false);
    await fireEvent.click(screen.getByTestId('obj-intersection'));
    await waitFor(() => expect(service.calls.some((c) => c.action === 'document.boolean')).toBe(true));
  });

  it('raster dialog: method, dithering, preview, apply as one step', async () => {
    const { service, summary } = await mountDesign();
    await service.dispatch('view.open_panel', { name: 'object' });
    const { objects } = await service.dispatch('document.import_data', {
      format: 'image',
      data: '',
      name: 'photo.png',
    });
    await service.dispatch('document.select', { ids: objects });
    await fireEvent.click(await screen.findByTestId('obj-image-open'));
    const dialog = await screen.findByTestId('dialog-raster');
    await within(dialog).findByTestId('raster-preview');
    expect(dialog.textContent).toContain('120 × 90 px');
    expect(within(dialog).queryByTestId('raster-dither')).toBeNull();
    await fireEvent.click(dialog.querySelector('[data-tool-mode="dithering"]')!);
    const dither = (await within(dialog).findByTestId('raster-dither')) as HTMLSelectElement;
    dither.value = 'atkinson';
    await fireEvent.change(dither);
    const lines = within(dialog).getByTestId('raster-lines') as HTMLInputElement;
    lines.value = '5';
    await fireEvent.change(lines);
    await waitFor(() => expect(dialog.textContent).toContain('200 × 150 px'), { timeout: 2000 });
    // nothing changed before Apply
    expect((await summary()).objects[0]!.image).toMatchObject({ tool: 'line_to_line' });
    await fireEvent.click(within(dialog).getByTestId('raster-apply'));
    await waitFor(() => expect(screen.queryByTestId('dialog-raster')).toBeNull());
    expect((await summary()).objects[0]!.image).toEqual({
      tool: 'dithering',
      dithering: 'atkinson',
      lines_per_mm: 5,
    });
    expect((await service.dispatch('session.snapshot', {})).undo.at(-1)).toBe('image settings');
  });

  it('raster dialog without an image says so', async () => {
    const { service } = await mountDesign();
    await service.dispatch('view.open_dialog', { name: 'raster' });
    await screen.findByTestId('raster-none');
    expect((screen.getByTestId('raster-apply') as HTMLButtonElement).disabled).toBe(true);
  });

  it('material test: a grid of cells on layers of their own, from the rail', async () => {
    const { service, summary } = await mountDesign();
    await fireEvent.click(document.querySelector('[data-tool="test"]')!);
    const dialog = await screen.findByTestId('dialog-material-test');
    expect(within(dialog).getByTestId('mtest-cells').textContent).toContain('25 cells');
    const steps = within(dialog).getByTestId('mtest-power-steps') as HTMLInputElement;
    steps.value = '3';
    await fireEvent.change(steps);
    const speedSteps = within(dialog).getByTestId('mtest-speed-steps') as HTMLInputElement;
    speedSteps.value = '2';
    await fireEvent.change(speedSteps);
    await waitFor(() => expect(within(dialog).getByTestId('mtest-cells').textContent).toContain('6 cells'));
    await fireEvent.click(dialog.querySelector('[data-mode="line"]')!);
    await fireEvent.click(within(dialog).getByTestId('mtest-go'));
    await waitFor(() => expect(screen.queryByTestId('dialog-material-test')).toBeNull());
    expect(service.calls.find((c) => c.action === 'document.material_test')!.params).toEqual({
      mode: 'line',
      power_pct: { from: 10, to: 100, steps: 3 },
      speed_mm_min: { from: 500, to: 6000, steps: 2 },
      cell_mm: 10,
      max_s: 1000,
    });
    const s = await summary();
    expect(s.objects).toHaveLength(6);
    expect(s.layers.filter((l) => l.mode === 'line')).toHaveLength(7);
    await waitFor(() => expect(service.view.toasts.at(-1)!.message).toContain('6 cells'));
  });

  it('import dialog: a file, keep or unify colours, a toast with the report', async () => {
    const { service, summary } = await mountDesign();
    await service.dispatch('view.open_dialog', { name: 'import' });
    const dialog = await screen.findByTestId('dialog-import');
    const go = within(dialog).getByTestId('import-go') as HTMLButtonElement;
    expect(go.disabled).toBe(true);
    const svg =
      '<svg xmlns="http://www.w3.org/2000/svg" height="20"><rect width="5" height="5" stroke="#E8622C"/></svg>';
    const file = new File([svg], 'logo.svg', { type: 'image/svg+xml' });
    // jsdom's File has no arrayBuffer in some versions
    if (!file.arrayBuffer)
      Object.assign(file, { arrayBuffer: async () => new TextEncoder().encode(svg).buffer });
    await fireEvent.change(within(dialog).getByTestId('import-file'), { target: { files: [file] } });
    await fireEvent.click(within(dialog).getByLabelText('Put everything on the current layer'));
    await fireEvent.click(go);
    await waitFor(async () => expect((await summary()).objects).toHaveLength(1));
    await waitFor(() => expect(screen.queryByTestId('dialog-import')).toBeNull());
    expect(service.calls.find((c) => c.action === 'document.import_data')!.params).toEqual({
      format: 'svg',
      data: btoa(svg),
      name: 'logo.svg',
      colors: 'unify',
    });
    await waitFor(() => expect(service.view.toasts.at(-1)!.message).toContain('Imported 1 objects'));
  });

  it('welcome: migration from LaserGRBL, read-only, with a report', async () => {
    const { service } = await mountDesign();
    await service.dispatch('view.open_dialog', { name: 'settings' });
    await fireEvent.click(await screen.findByText('Show the welcome again'));
    const dialog = await screen.findByTestId('dialog-welcome');
    await fireEvent.click(within(dialog).getByTestId('welcome-migrate'));
    const items = await within(dialog).findByTestId('migrate-items');
    expect(items.textContent).toContain('14 custom buttons');
    // untick the materials
    await fireEvent.click(within(items).getByLabelText(/User materials/));
    await fireEvent.click(within(dialog).getByTestId('migrate-go'));
    const report = await within(dialog).findByTestId('migrate-report');
    expect(report.textContent).not.toContain('materials');
    expect(service.calls.find((c) => c.action === 'migrate.run')!.params).toEqual({
      items: ['connection', 'preferences', 'buttons', 'counters'],
    });
    await fireEvent.click(within(dialog).getByText('Start'));
    await waitFor(() => expect(screen.queryByTestId('dialog-welcome')).toBeNull());
  });

  it('welcome: opens by itself on the first run, and closing it records it', async () => {
    const service = new FakeService();
    service.settings.ui!.welcomed = false;
    const props = projections(service);
    await Promise.all([props.view.refresh(), props.settings.refresh(), props.doc.refresh()]);
    render(App, props);
    const dialog = await screen.findByTestId('dialog-welcome');
    await fireEvent.click(within(dialog).getByLabelText('Close'));
    await waitFor(() => expect(service.settings.ui!.welcomed).toBe(true));
    await waitFor(() => expect(screen.queryByTestId('dialog-welcome')).toBeNull());
  });

  it('welcome: nothing to migrate, and explore connects the emulator', async () => {
    const { service } = await mountDesign();
    service.document.migration.found = false;
    await service.dispatch('view.open_dialog', { name: 'welcome' });
    const dialog = await screen.findByTestId('dialog-welcome');
    await fireEvent.click(within(dialog).getByTestId('welcome-migrate'));
    await within(dialog).findByTestId('migrate-none');
    expect((within(dialog).getByTestId('migrate-go') as HTMLButtonElement).disabled).toBe(true);
    await fireEvent.click(within(dialog).getByText('Back'));
    await fireEvent.click(within(dialog).getByText('Explore with the simulator'));
    await waitFor(() => expect(service.calls.some((c) => c.action === 'machine.connect')).toBe(true));
  });
});
