// Colour layers in the UI: the palette, layer codes (C00–C11) and the palette
// action (`document.layer_for_color`, which makes the layer current, then
// `document.move_to_layer` with a selection).

import type { Color, LayerMode } from '../gen/api';
import type { DocumentProjection, Layer } from '../state/document.svelte';

/** The design palette, C00..C11 (`fk-color-layer-c00..c11`, design/tokens). */
export const PALETTE_COLORS: Color[] = [
  '#4C8DFF',
  '#E8622C',
  '#10A37F',
  '#E8C21F',
  '#B36AE2',
  '#52B8E8',
  '#E0559A',
  '#F08C1A',
  '#8FA31E',
  '#7F8EA3',
  '#A0693A',
  '#25C2B5',
];

/** `C00`… for a palette colour, else the colour itself. */
export function layerCode(color: Color): string {
  const i = PALETTE_COLORS.indexOf(color.toUpperCase());
  return i < 0 ? color.toUpperCase() : `C${String(i).padStart(2, '0')}`;
}

/** The modes the layer editor offers (the others come with M2's planner). */
export const MODES: LayerMode[] = ['line', 'fill', 'fill_and_line', 'cut'];

/** The i18n id of a mode. */
export function modeLabel(m: LayerMode): string {
  return m === 'line' ? 'layers-mode-line' : m === 'ignore' ? 'layers-mode-tool' : `layers-mode-${m}`;
}

/** One line of layer parameters: `1000 mm/min · 50% · 1×`. */
export function layerParams(l: Layer, maxPower = 1000): string {
  const pct = Math.round((100 * l.power) / maxPower);
  // the laser command only when it is not the machine's default (M3)
  const m4 = l.laser_mode === 'dynamic' ? ' · M4' : '';
  return `${l.speed_mm_min} mm/min · ${pct}% · ${l.passes}×${m4}`;
}

/** The palette action. */
export async function assignColor(doc: DocumentProjection, color: Color): Promise<void> {
  const { layer } = await doc.service.dispatch('document.layer_for_color', { color });
  const ids = doc.selection;
  if (ids.length) await doc.service.dispatch('document.move_to_layer', { ids, layer });
}
