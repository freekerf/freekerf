// A fake of the session's document actions (`session.snapshot`, `document.*`,
// `history.*`, `migrate.*`) for development and component tests: the real ones are
// in freekerf-session and freekerf-service. In memory: layers, objects (rectangles,
// ellipses, text, imported SVG shapes) with a 2×3 transform, selection, current
// layer, undo/redo, a tiny SVG importer and the migration scan. Never part of a
// production build (see fake-service.ts).

import type {
  ActionName,
  AlignHow,
  Color,
  LayerSummary,
  RasterSettings,
  ObjectSummary,
  Snapshot,
  TextSpec,
  TransformOp,
  ViewState,
} from '../gen/api';
import { encodeToolpath, type SegmentRecord } from '../canvas/toolpath';
import { PALETTE_COLORS } from '../design/layers';
import { ActionFailure, type Params, type Result } from '../service';

type Layer = Omit<LayerSummary, 'objects'>;
type ObjectKind = 'shape' | 'text' | 'image';
type Change = 'document' | 'selection' | 'history' | 'clipboard';

type Affine = [number, number, number, number, number, number];
const IDENTITY: Affine = [1, 0, 0, 1, 0, 0];

interface FakeObject {
  id: number;
  layer: number;
  name: string;
  kind: ObjectKind;
  /** Polylines in object coordinates (mm, Y up). */
  paths: [number, number][][];
  transform: Affine;
  text?: TextSpec;
  /** Images: the raster dialog's settings (the frame is the outline). */
  raster?: RasterSettings;
}

/** The Rust defaults of `RasterSettings`. */
export function defaultRaster(): RasterSettings {
  return {
    tool: 'line_to_line',
    direction: 'horizontal',
    lines_per_mm: 3,
    adjustments: {
      formula: 'simple_average',
      red: 100,
      green: 100,
      blue: 100,
      brightness: 100,
      contrast: 100,
      threshold: null,
      white_clip: 5,
    },
    dithering: 'floyd_steinberg',
    interpolation: 'high_quality_bicubic',
    vectorize: {
      spot_removal: null,
      smoothing: null,
      optimize: null,
      adaptive_quality: false,
      down_sampling: null,
      optimize_fast: false,
      fill: null,
      fill_lines_per_mm: 3,
    },
    centerline: { corner_threshold: 110, line_threshold: 10 },
    border_speed_mm_min: 1000,
    mark_speed_mm_min: 1000,
    min_power: 0,
    max_power: 1000,
    laser_on: 'M3',
    laser_off: 'M5',
  };
}

/** A 1 × 1 white PNG: the fake's preview. */
const WHITE_PNG =
  'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8/x8AAwMCAO+ip1sAAAAASUVORK5CYII=';

interface Doc {
  layers: Layer[];
  objects: FakeObject[];
  selection: number[];
  current_layer: number;
  next_id: number;
}

interface Step {
  label: string;
  doc: string;
}

function apply(m: Affine, x: number, y: number): [number, number] {
  return [m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]];
}

/** `a` after `b`. */
function compose(a: Affine, b: Affine): Affine {
  return [
    a[0] * b[0] + a[2] * b[1],
    a[1] * b[0] + a[3] * b[1],
    a[0] * b[2] + a[2] * b[3],
    a[1] * b[2] + a[3] * b[3],
    a[0] * b[4] + a[2] * b[5] + a[4],
    a[1] * b[4] + a[3] * b[5] + a[5],
  ];
}

function opMatrix(op: TransformOp): Affine {
  if ('translate' in op) return [1, 0, 0, 1, op.translate[0], op.translate[1]];
  if ('matrix' in op) return op.matrix;
  const [px, py] = op.pivot;
  const to: Affine = [1, 0, 0, 1, px, py];
  const from: Affine = [1, 0, 0, 1, -px, -py];
  let m: Affine;
  if ('scale' in op) m = [op.scale[0], 0, 0, op.scale[1], 0, 0];
  else {
    const a = (op.rotate_deg * Math.PI) / 180;
    m = [Math.cos(a), Math.sin(a), -Math.sin(a), Math.cos(a), 0, 0];
  }
  return compose(to, compose(m, from));
}

function rectPath(x: number, y: number, w: number, h: number): [number, number][] {
  return [
    [x, y],
    [x + w, y],
    [x + w, y + h],
    [x, y + h],
    [x, y],
  ];
}

function ellipsePath(cx: number, cy: number, rx: number, ry: number): [number, number][] {
  const n = 64;
  return Array.from({ length: n + 1 }, (_, i) => {
    const a = (i / n) * Math.PI * 2;
    return [cx + rx * Math.cos(a), cy + ry * Math.sin(a)] as [number, number];
  });
}

/** A text stand-in: one stroke per character cell (the real one uses Hershey fonts). */
function textPaths(text: string, x: number, y: number, h: number): [number, number][][] {
  const w = h * 0.6;
  const out: [number, number][][] = [];
  [...text].forEach((ch, i) => {
    if (ch === ' ') return;
    const x0 = x + i * w;
    out.push([
      [x0, y],
      [x0 + w * 0.4, y + h],
      [x0 + w * 0.8, y],
    ]);
    out.push([
      [x0 + w * 0.2, y + h * 0.4],
      [x0 + w * 0.6, y + h * 0.4],
    ]);
  });
  return out.length ? out : [rectPath(x, y, w, h)];
}

function newLayer(id: number, color: Color): Layer {
  return {
    id,
    color,
    name: '',
    mode: 'line',
    speed_mm_min: 1000,
    power: 1000,
    passes: 1,
    fill: { pattern: 'horizontal', lines_per_mm: 10 },
    visible: true,
    laser_mode: 'machine',
  };
}

/** Parses the shapes of a simple SVG (rect, circle, ellipse, line, polyline, polygon). */
export function parseSvg(text: string): { color: Color; path: [number, number][] }[] {
  const doc = new DOMParser().parseFromString(text, 'image/svg+xml');
  const out: { color: Color; path: [number, number][] }[] = [];
  const num = (el: Element, a: string) => Number(el.getAttribute(a) ?? 0);
  const height = Number(doc.documentElement.getAttribute('height')?.replace(/[a-z]+$/i, '') ?? 0);
  const flip = (p: [number, number][]) =>
    p.map(([x, y]) => [x, height ? height - y : -y] as [number, number]);
  for (const el of doc.querySelectorAll('rect, circle, ellipse, line, polyline, polygon')) {
    const color = (el.getAttribute('stroke') ?? el.getAttribute('fill') ?? '#000000').toUpperCase();
    let p: [number, number][] = [];
    switch (el.tagName) {
      case 'rect':
        p = rectPath(num(el, 'x'), num(el, 'y'), num(el, 'width'), num(el, 'height'));
        break;
      case 'circle':
        p = ellipsePath(num(el, 'cx'), num(el, 'cy'), num(el, 'r'), num(el, 'r'));
        break;
      case 'ellipse':
        p = ellipsePath(num(el, 'cx'), num(el, 'cy'), num(el, 'rx'), num(el, 'ry'));
        break;
      case 'line':
        p = [
          [num(el, 'x1'), num(el, 'y1')],
          [num(el, 'x2'), num(el, 'y2')],
        ];
        break;
      default: {
        const v = (el.getAttribute('points') ?? '')
          .trim()
          .split(/[\s,]+/)
          .map(Number);
        for (let i = 0; i + 1 < v.length; i += 2) p.push([v[i]!, v[i + 1]!]);
        if (el.tagName === 'polygon' && p.length) p.push(p[0]!);
      }
    }
    if (p.length > 1) out.push({ color: /^#[0-9A-F]{6}$/.test(color) ? color : '#000000', path: flip(p) });
  }
  return out;
}

type Handler = (params: unknown) => unknown;
const invalid = (action: string, message: string) =>
  new ActionFailure(action, { kind: 'invalid_params', message });

export class FakeDocument {
  private doc: Doc = {
    layers: [newLayer(1, PALETTE_COLORS[0]!)],
    objects: [],
    selection: [],
    current_layer: 1,
    next_id: 1,
  };
  private undoStack: Step[] = [];
  /** `document.copy` / `cut` (simplified: same document, the layers kept by id). */
  private clipboard: FakeObject[] = [];
  private pastes = 0;
  private redoStack: Step[] = [];
  private documentRevision = 0;
  readonly calls: { action: string; params: unknown }[] = [];
  /** What `migrate.detect` finds (tests change it). */
  migration = { found: true, path: '/home/user/.wine/drive_c/users/user/AppData/Roaming/LaserGRBL' };

  /** `changed` records a change and returns the new revision (the fake service's). */
  constructor(private readonly changed: (change: Change) => number) {}

  private emit(...changes: Change[]): void {
    for (const c of changes) {
      const r = this.changed(c);
      if (c === 'document') this.documentRevision = r;
    }
  }

  /** One undo step: `f` changes the document (and maybe the selection). */
  private edit<T>(label: string, f: (d: Doc) => T): T {
    const before = JSON.stringify(this.doc);
    const sel = JSON.stringify(this.doc.selection);
    let r: T;
    try {
      r = f(this.doc);
    } catch (e) {
      this.doc = JSON.parse(before) as Doc;
      throw e;
    }
    if (JSON.stringify(this.doc) === before) return r;
    this.undoStack.push({ label, doc: before });
    this.redoStack = [];
    this.emit('history', 'document');
    if (JSON.stringify(this.doc.selection) !== sel) this.emit('selection');
    return r;
  }

  private obj(id: number, action = 'document'): FakeObject {
    const o = this.doc.objects.find((x) => x.id === id);
    if (!o) throw invalid(action, `no object ${id}`);
    return o;
  }

  /** Ids given, or the selection. */
  private targets(ids: number[] | null | undefined): number[] {
    return ids ?? this.doc.selection;
  }

  private image(id: number, action: string): FakeObject {
    const o = this.obj(id, action);
    if (!o.raster) throw invalid(action, `object ${id} is not an image`);
    return o;
  }

  private currentLayer(): number {
    if (this.doc.layers.some((l) => l.id === this.doc.current_layer)) return this.doc.current_layer;
    const first = this.doc.layers[0];
    return first ? first.id : this.layerForColor('#000000');
  }

  private add(name: string, paths: [number, number][][], layer?: number | null, text?: TextSpec): number {
    if (layer != null && !this.doc.layers.some((l) => l.id === layer))
      throw invalid('document', `no layer ${layer}`);
    const id = this.doc.next_id++;
    this.doc.objects.push({
      id,
      layer: layer ?? this.currentLayer(),
      name,
      kind: text ? 'text' : 'shape',
      paths,
      transform: IDENTITY,
      text,
    });
    this.doc.selection = [id];
    return id;
  }

  private bounds(o: FakeObject): [number, number, number, number] | null {
    let b: [number, number, number, number] | null = null;
    for (const p of o.paths) {
      for (const [x, y] of p) {
        const [tx, ty] = apply(o.transform, x, y);
        b = b
          ? [Math.min(b[0], tx), Math.min(b[1], ty), Math.max(b[2], tx), Math.max(b[3], ty)]
          : [tx, ty, tx, ty];
      }
    }
    return b;
  }

  /** `session.snapshot` with the fake service's view and revision. */
  snapshot(view: ViewState, revision: number): Snapshot {
    const objects: ObjectSummary[] = this.doc.objects.map((o) => ({
      id: o.id,
      layer: o.layer,
      name: o.name,
      kind: o.kind,
      bounds: this.bounds(o),
      transform: o.transform,
      ...(o.text ? { text: o.text } : {}),
      ...(o.raster
        ? {
            image: {
              tool: o.raster.tool,
              dithering: o.raster.dithering,
              lines_per_mm: o.raster.lines_per_mm,
            },
          }
        : {}),
    }));
    return structuredClone({
      revision,
      document_revision: this.documentRevision,
      dirty: this.undoStack.length > 0,
      selection: this.doc.selection,
      current_layer: this.doc.layers.length ? this.currentLayer() : null,
      view,
      clipboard: this.clipboard.length,
      undo: this.undoStack.map((s) => s.label),
      redo: [...this.redoStack].reverse().map((s) => s.label),
      document: {
        layers: this.doc.layers.map((l) => ({
          ...l,
          objects: this.doc.objects.filter((o) => o.layer === l.id).length,
        })),
        objects,
        bounds: null,
      },
    });
  }

  private layerForColor(color: Color): number {
    const c = color.toUpperCase();
    const found = this.doc.layers.find((l) => l.color.toUpperCase() === c);
    if (found) return found.id;
    const id = Math.max(0, ...this.doc.layers.map((l) => l.id)) + 1;
    this.doc.layers.push(newLayer(id, c));
    return id;
  }

  private select(ids: number[]): void {
    const next = ids.filter((id) => this.doc.objects.some((o) => o.id === id));
    if (JSON.stringify(next) === JSON.stringify(this.doc.selection)) return;
    this.doc.selection = next;
    this.emit('selection');
  }

  private history(from: Step[], to: Step[]): Result<'history.undo'> {
    const step = from.pop();
    if (!step) return { step: null };
    to.push({ label: step.label, doc: JSON.stringify(this.doc) });
    this.doc = JSON.parse(step.doc) as Doc;
    this.emit('document', 'selection', 'history');
    return { step: step.label };
  }

  private readonly table: { [A in ActionName]?: (p: Params<A>) => Result<A> } = {
    'document.select': (p) => {
      const cur = new Set(p.mode === 'replace' || !p.mode ? [] : this.doc.selection);
      for (const id of p.ids) {
        if (p.mode === 'remove' || (p.mode === 'toggle' && cur.has(id))) cur.delete(id);
        else cur.add(id);
      }
      this.select([...cur]);
      return this.doc.selection;
    },
    'document.select_all': () => {
      this.select(this.doc.objects.map((o) => o.id));
      return this.doc.selection.length;
    },
    'document.clear_selection': () => {
      this.select([]);
      return { changed: true };
    },
    'document.add_shape': (p) => {
      if (!(p.width > 0 && p.height > 0))
        throw invalid('document.add_shape', 'width and height must be positive');
      const id = this.edit('add shape', () => {
        const path =
          p.shape === 'rect'
            ? rectPath(p.x, p.y, p.width, p.height)
            : ellipsePath(p.x + p.width / 2, p.y + p.height / 2, p.width / 2, p.height / 2);
        return this.add(p.shape === 'rect' ? 'rectangle' : 'ellipse', [path], p.layer);
      });
      return { id };
    },
    'document.add_text': (p) => {
      const h = p.height_mm ?? 10;
      const id = this.edit('add text', () =>
        this.add('text', textPaths(p.text, p.x, p.y, h), p.layer, { text: p.text, height_mm: h }),
      );
      return { id };
    },
    'document.set_text': (p) => {
      const changed = this.edit('edit text', () => {
        const o = this.obj(p.id, 'document.set_text');
        if (!o.text) throw invalid('document.set_text', `object ${p.id} is not text`);
        const [x, y] = o.paths[0]?.[0] ?? [0, 0];
        o.text = { text: p.text ?? o.text.text, height_mm: p.height_mm ?? o.text.height_mm };
        o.paths = textPaths(o.text.text, x, y, o.text.height_mm);
        return true;
      });
      return { changed };
    },
    'document.transform': (p) => {
      const m = opMatrix(p.op);
      const ids = this.targets(p.ids);
      this.edit('transform', () => {
        for (const id of ids) {
          const o = this.obj(id, 'document.transform');
          o.transform = compose(m, o.transform);
        }
      });
      return { changed: ids.length > 0 };
    },
    'document.align': (p) => {
      this.edit('align', () => this.align(this.targets(p.ids), p.how));
      return { changed: true };
    },
    'document.translate': (p) => {
      const ids = this.targets(p.ids);
      this.edit('move', () => {
        for (const id of ids)
          this.obj(id).transform = compose([1, 0, 0, 1, p.dx, p.dy], this.obj(id).transform);
      });
      return { changed: ids.length > 0 };
    },
    'document.delete': (p) => {
      const ids = this.targets(p.ids);
      this.edit('delete', (d) => {
        d.objects = d.objects.filter((o) => !ids.includes(o.id));
        d.selection = d.selection.filter((id) => !ids.includes(id));
      });
      return { changed: ids.length > 0 };
    },
    'document.copy': (p) => {
      const ids = this.targets(p.ids);
      const objects = this.doc.objects.filter((o) => ids.includes(o.id));
      if (objects.length) {
        this.clipboard = structuredClone(objects);
        this.pastes = 1;
        this.emit('clipboard');
      }
      return { objects: objects.length };
    },
    'document.cut': (p) => {
      const { objects } = this.table['document.copy']!(p);
      if (objects) {
        this.pastes = 0;
        const ids = this.targets(p.ids);
        this.edit('cut', (d) => {
          d.objects = d.objects.filter((o) => !ids.includes(o.id));
          d.selection = d.selection.filter((id) => !ids.includes(id));
        });
      }
      return { objects };
    },
    'document.paste': (p) => {
      if (!this.clipboard.length) return { ids: [] };
      const ids = this.edit('paste', (d) => {
        let [dx, dy] = p.offset_mm ?? [5 * this.pastes, -5 * this.pastes];
        if (!p.offset_mm && p.at_mm) {
          const bs = this.clipboard.map((o) => this.bounds(o)).filter((b) => b !== null);
          const [x0, y0] = [Math.min(...bs.map((b) => b[0])), Math.min(...bs.map((b) => b[1]))];
          const [x1, y1] = [Math.max(...bs.map((b) => b[2])), Math.max(...bs.map((b) => b[3]))];
          [dx, dy] = [p.at_mm[0] - (x0 + x1) / 2, p.at_mm[1] - (y0 + y1) / 2];
        }
        const out: number[] = [];
        for (const c of this.clipboard) {
          const o = structuredClone(c);
          o.id = d.next_id++;
          o.transform = compose([1, 0, 0, 1, dx, dy], o.transform);
          d.objects.push(o);
          out.push(o.id);
        }
        d.selection = out;
        return out;
      });
      this.pastes++;
      return { ids };
    },
    'document.duplicate': (p) => {
      const ids = this.edit('duplicate', (d) => {
        const [dx, dy] = p.offset_mm ?? [5, -5];
        const out: number[] = [];
        for (const id of this.targets(p.ids)) {
          const o = structuredClone(this.obj(id));
          o.id = d.next_id++;
          o.transform = compose([1, 0, 0, 1, dx, dy], o.transform);
          d.objects.push(o);
          out.push(o.id);
        }
        if (out.length) d.selection = out;
        return out;
      });
      return { ids };
    },
    // stand-ins for the real geometry: union and xor merge the outlines, difference
    // keeps the first, intersection is the overlap of the boxes; offsets are boxes
    // simplified: paths are merged, not joined end to end
    'document.combine': (p) => {
      const world = (o: FakeObject) => o.paths.map((path) => path.map(([x, y]) => apply(o.transform, x, y)));
      const chosen = this.doc.objects.filter(
        (o) => this.targets(p.ids).includes(o.id) && o.kind === 'shape' && !o.text,
      );
      const layers = [...new Set(chosen.map((o) => o.layer))];
      const ids = this.edit('combine', (d) => {
        const out: number[] = [];
        for (const layer of layers) {
          const group = chosen.filter((o) => o.layer === layer);
          if (group.length < 2) continue;
          d.objects = d.objects.filter((o) => !group.includes(o));
          const o: FakeObject = {
            id: d.next_id++,
            layer,
            name: group[0]!.name,
            kind: 'shape',
            paths: group.flatMap(world),
            transform: [1, 0, 0, 1, 0, 0],
          };
          d.objects.push(o);
          out.push(o.id);
        }
        if (out.length) d.selection = out;
        return out;
      });
      return { ids };
    },
    'document.split': (p) => {
      const chosen = this.doc.objects.filter(
        (o) => this.targets(p.ids).includes(o.id) && o.kind === 'shape' && !o.text && o.paths.length > 1,
      );
      const ids = this.edit('split', (d) => {
        const out: number[] = [];
        for (const o of chosen) {
          d.objects = d.objects.filter((x) => x !== o);
          for (const path of o.paths) {
            const piece: FakeObject = { ...structuredClone(o), id: d.next_id++, paths: [path] };
            d.objects.push(piece);
            out.push(piece.id);
          }
        }
        if (out.length) d.selection = out;
        return out;
      });
      return { ids };
    },
    'document.boolean': (p) => {
      const chosen = this.doc.objects.filter((o) => this.targets(p.ids).includes(o.id));
      if (chosen.length < 2) throw invalid('document.boolean', 'choose at least two shapes');
      const world = (o: FakeObject) => o.paths.map((path) => path.map(([x, y]) => apply(o.transform, x, y)));
      const first = chosen[0]!;
      let paths: [number, number][][];
      if (p.op === 'difference') paths = world(first);
      else if (p.op === 'intersection') {
        const bs = chosen.map((o) => this.bounds(o)!);
        const [x0, y0] = [Math.max(...bs.map((b) => b[0])), Math.max(...bs.map((b) => b[1]))];
        const [x1, y1] = [Math.min(...bs.map((b) => b[2])), Math.min(...bs.map((b) => b[3]))];
        paths = x1 > x0 && y1 > y0 ? [rectPath(x0, y0, x1 - x0, y1 - y0)] : [];
      } else paths = chosen.flatMap(world);
      const id = this.edit('boolean', (d) => {
        d.objects = d.objects.filter((o) => !chosen.includes(o));
        return this.add(first.name, paths, first.layer);
      });
      return { id };
    },
    'document.offset': (p) => {
      const ids = this.edit('offset', (d) => {
        const out: number[] = [];
        for (const o of d.objects.filter((x) => this.targets(p.ids).includes(x.id) && !x.text)) {
          const b = this.bounds(o)!;
          const k = p.distance_mm;
          out.push(
            this.add(
              'offset',
              [rectPath(b[0] - k, b[1] - k, b[2] - b[0] + 2 * k, b[3] - b[1] + 2 * k)],
              o.layer,
            ),
          );
        }
        if (out.length) d.selection = out;
        return out;
      });
      return { ids };
    },
    'document.array': (p) => {
      if (p.columns < 1 || p.rows < 1) throw invalid('document.array', 'columns and rows must be at least 1');
      const [gx, gy] = p.gap_mm ?? [5, 5];
      const ids = this.edit('array', (d) => {
        const originals = d.objects.filter((o) => this.targets(p.ids).includes(o.id));
        const bs = originals.map((o) => this.bounds(o)!);
        const w = Math.max(...bs.map((b) => b[2])) - Math.min(...bs.map((b) => b[0]));
        const h = Math.max(...bs.map((b) => b[3])) - Math.min(...bs.map((b) => b[1]));
        const out: number[] = [];
        for (let r = 0; r < p.rows; r++)
          for (let c = 0; c < p.columns; c++) {
            if (r === 0 && c === 0) continue;
            for (const o of originals) {
              const copy = structuredClone(o);
              copy.id = d.next_id++;
              copy.transform = compose([1, 0, 0, 1, c * (w + gx), -r * (h + gy)], copy.transform);
              d.objects.push(copy);
              out.push(copy.id);
            }
          }
        d.selection = [...originals.map((o) => o.id), ...out];
        return out;
      });
      return { ids };
    },
    'document.move_to_layer': (p) => {
      const ids = this.targets(p.ids);
      this.edit('move to layer', () => {
        if (!this.doc.layers.some((l) => l.id === p.layer))
          throw invalid('document.move_to_layer', `no layer ${p.layer}`);
        for (const id of ids) this.obj(id).layer = p.layer;
      });
      return { changed: ids.length > 0 };
    },
    'document.layer_for_color': (p) => {
      const created = !this.doc.layers.some((l) => l.color.toUpperCase() === p.color.toUpperCase());
      const layer = created
        ? this.edit('add layer', () => this.layerForColor(p.color))
        : this.layerForColor(p.color);
      if (this.doc.current_layer !== layer) {
        this.doc.current_layer = layer;
        this.emit('selection');
      }
      return { layer, created };
    },
    'document.set_current_layer': (p) => {
      if (!this.doc.layers.some((l) => l.id === p.id))
        throw invalid('document.set_current_layer', `no layer ${p.id}`);
      if (this.doc.current_layer !== p.id) {
        this.doc.current_layer = p.id;
        this.emit('selection');
      }
      return { changed: true };
    },
    'document.add_layer': (p) => {
      const id = this.edit('add layer', (d) => {
        const l = { ...newLayer(Math.max(0, ...d.layers.map((x) => x.id)) + 1, p.color.toUpperCase()) };
        d.layers.push(l);
        return l.id;
      });
      return { id };
    },
    'document.update_layer': (p) => {
      const changed = this.edit('layer settings', (d) => {
        const l = d.layers.find((x) => x.id === p.id);
        if (!l) throw invalid('document.update_layer', `no layer ${p.id}`);
        if (p.passes === 0) throw invalid('document.update_layer', 'passes must be at least 1');
        const before = JSON.stringify(l);
        if (p.name != null) l.name = p.name;
        if (p.mode != null) l.mode = p.mode;
        if (p.speed_mm_min != null) l.speed_mm_min = p.speed_mm_min;
        if (p.power != null) l.power = p.power;
        if (p.passes != null) l.passes = p.passes;
        if (p.visible != null) l.visible = p.visible;
        if (p.fill_pattern != null) l.fill.pattern = p.fill_pattern;
        if (p.lines_per_mm != null) l.fill.lines_per_mm = p.lines_per_mm;
        if (p.laser_mode != null) l.laser_mode = p.laser_mode;
        return JSON.stringify(l) !== before;
      });
      return { changed };
    },
    'document.remove_layer': (p) => {
      this.edit('remove layer', (d) => {
        if (!d.layers.some((l) => l.id === p.id)) throw invalid('document.remove_layer', `no layer ${p.id}`);
        d.layers = d.layers.filter((l) => l.id !== p.id);
        d.objects = d.objects.filter((o) => o.layer !== p.id);
        d.selection = d.selection.filter((id) => d.objects.some((o) => o.id === id));
      });
      return { changed: true };
    },
    'document.move_layer': (p) => {
      const changed = this.edit('move layer', (d) => {
        const i = d.layers.findIndex((x) => x.id === p.id);
        if (i < 0) throw invalid('document.move_layer', `no layer ${p.id}`);
        const to = Math.min(p.to, d.layers.length - 1);
        if (i === to) return false;
        const [l] = d.layers.splice(i, 1);
        d.layers.splice(to, 0, l!);
        return true;
      });
      return { changed };
    },
    'history.undo': () => this.history(this.undoStack, this.redoStack),
    'history.redo': () => this.history(this.redoStack, this.undoStack),
    'document.new': () => {
      this.doc = { layers: [], objects: [], selection: [], current_layer: 0, next_id: this.doc.next_id };
      this.undoStack = [];
      this.redoStack = [];
      this.emit('document', 'selection', 'history');
      return { changed: true };
    },
    'document.import_data': (p) => {
      if (p.format === 'image') {
        // a 40 × 30 mm image on a layer of its own
        const before = this.doc.layers.map((l) => l.id);
        const id = this.edit('import', (d) => {
          const layer = Math.max(0, ...d.layers.map((l) => l.id)) + 1;
          d.layers.push(newLayer(layer, '#000000'));
          const oid = d.next_id++;
          d.objects.push({
            id: oid,
            layer,
            name: p.name ?? '',
            kind: 'image',
            paths: [rectPath(0, 0, 40, 30)],
            transform: IDENTITY,
            raster: defaultRaster(),
          });
          return oid;
        });
        return {
          objects: [id],
          layers: this.doc.layers.map((l) => l.id).filter((l) => !before.includes(l)),
          unsupported: 0,
        };
      }
      if (p.format !== 'svg')
        throw new ActionFailure('document.import_data', {
          kind: 'failed',
          message: 'the fake imports SVG only',
        });
      const text = p.text ?? (p.data ? atob(p.data) : '');
      const shapes = parseSvg(text);
      const before = this.doc.layers.map((l) => l.id);
      const objects = this.edit('import', (d) => {
        const ids: number[] = [];
        for (const s of shapes) {
          const layer = p.colors === 'unify' ? this.currentLayer() : this.layerForColor(s.color);
          const id = d.next_id++;
          d.objects.push({
            id,
            layer,
            name: p.name ?? '',
            kind: 'shape',
            paths: [s.path],
            transform: IDENTITY,
          });
          ids.push(id);
        }
        return ids;
      });
      return {
        objects,
        layers: this.doc.layers.map((l) => l.id).filter((id) => !before.includes(id)),
        unsupported: 0,
      };
    },
    'document.raster_settings': (p) => structuredClone(this.image(p.id, 'document.raster_settings').raster!),
    'document.set_raster': (p) => {
      if (!(p.settings.lines_per_mm > 0))
        throw invalid('document.set_raster', 'lines_per_mm must be positive');
      const changed = this.edit('image settings', () => {
        const o = this.image(p.id, 'document.set_raster');
        if (JSON.stringify(o.raster) === JSON.stringify(p.settings)) return false;
        o.raster = structuredClone(p.settings);
        return true;
      });
      return { changed };
    },
    'document.raster_preview': (p) => {
      const o = this.image(p.id, 'document.raster_preview');
      const s = p.settings ?? o.raster!;
      if (!(s.lines_per_mm > 0)) throw invalid('document.raster_preview', 'lines_per_mm must be positive');
      return {
        png: WHITE_PNG,
        width: Math.round(40 * s.lines_per_mm),
        height: Math.round(30 * s.lines_per_mm),
      };
    },
    'document.material_test': (p) => {
      const range = (r: { from: number; to: number; steps: number }) =>
        Array.from({ length: r.steps }, (_, i) =>
          r.steps === 1 ? r.from : r.from + ((r.to - r.from) * i) / (r.steps - 1),
        );
      if (p.power_pct.steps < 1 || p.speed_mm_min.steps < 1)
        throw invalid('document.material_test', '1 to 20 steps');
      const cell = p.cell_mm ?? 10;
      const maxS = p.max_s ?? 1000;
      const out = { layers: [] as number[], objects: [] as number[] };
      this.edit('material test', (d) => {
        range(p.power_pct).forEach((pct, row) =>
          range(p.speed_mm_min).forEach((speed, col) => {
            const id = Math.max(0, ...d.layers.map((l) => l.id)) + 1;
            const color = `#${(0x203040 + id * 0x101010).toString(16).slice(-6).toUpperCase()}`;
            d.layers.push({
              ...newLayer(id, color),
              name: `${Math.round(pct)}% · ${Math.round(speed)} mm/min`,
              mode: p.mode === 'line' ? 'line' : 'fill',
              power: Math.round((pct / 100) * maxS),
              speed_mm_min: Math.round(speed),
            });
            out.layers.push(id);
            out.objects.push(
              this.add(
                `${Math.round(pct)}% ${Math.round(speed)}`,
                [rectPath(col * (cell + 3), row * (cell + 3), cell, cell)],
                id,
              ),
            );
          }),
        );
        d.selection = [...out.objects];
      });
      return out;
    },
    'migrate.detect': () =>
      this.migration.found
        ? {
            found: true,
            path: this.migration.path,
            items: [
              { kind: 'connection', count: 4 },
              { kind: 'preferences', count: 11 },
              { kind: 'buttons', count: 14 },
              { kind: 'counters', count: 2 },
              { kind: 'materials', count: 1 },
            ],
          }
        : { found: false, path: null, items: [] },
    'migrate.run': (p) => ({
      imported: p.items
        .filter((kind) => kind !== 'materials')
        .map((kind) => ({
          kind,
          count: kind === 'buttons' ? 14 : kind === 'preferences' ? 11 : kind === 'connection' ? 4 : 2,
        })),
      warnings: p.items.includes('materials')
        ? ['UserMaterials.psh was found; user materials are kept once the material library can store them']
        : [],
    }),
  };

  /** The fake's actions, for the fake service's dispatch. */
  get handlers(): Partial<Record<ActionName, Handler>> {
    const out: Partial<Record<ActionName, Handler>> = {};
    for (const [name, h] of Object.entries(this.table)) {
      out[name as ActionName] = (params: unknown) => {
        this.calls.push({ action: name, params });
        return (h as Handler)(params);
      };
    }
    return out;
  }

  private align(ids: number[], how: AlignHow): void {
    const boxes = ids.map((id) => [id, this.bounds(this.obj(id))] as const).filter(([, b]) => b);
    if (boxes.length < 2) return;
    const all = boxes.reduce<[number, number, number, number]>(
      (a, [, b]) => [
        Math.min(a[0], b![0]),
        Math.min(a[1], b![1]),
        Math.max(a[2], b![2]),
        Math.max(a[3], b![3]),
      ],
      [Infinity, Infinity, -Infinity, -Infinity],
    );
    for (const [id, b] of boxes) {
      const [x0, y0, x1, y1] = b!;
      const d: Record<AlignHow, [number, number]> = {
        left: [all[0] - x0, 0],
        right: [all[2] - x1, 0],
        center_h: [(all[0] + all[2]) / 2 - (x0 + x1) / 2, 0],
        top: [0, all[3] - y1],
        bottom: [0, all[1] - y0],
        middle_v: [0, (all[1] + all[3]) / 2 - (y0 + y1) / 2],
      };
      const o = this.obj(id);
      o.transform = compose([1, 0, 0, 1, d[how][0], d[how][1]], o.transform);
    }
  }

  /** The drawable geometry ("FKTP": `power` = layer index, `command` = object index). */
  geometry(): ArrayBuffer {
    const segs: SegmentRecord[] = [];
    this.doc.objects.forEach((o, index) => {
      const layer = this.doc.layers.findIndex((l) => l.id === o.layer);
      if (!this.doc.layers[layer]?.visible) return;
      for (const p of o.paths) {
        for (let i = 1; i < p.length; i++) {
          segs.push({
            from: apply(o.transform, p[i - 1]![0], p[i - 1]![1]),
            to: apply(o.transform, p[i]![0], p[i]![1]),
            power: layer,
            command: index,
          });
        }
      }
    });
    return encodeToolpath(segs, this.doc.objects.length);
  }
}
