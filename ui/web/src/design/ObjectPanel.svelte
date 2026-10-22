<script lang="ts">
  // The inspector of the selection (design/design-system.md "Objeto"): position by
  // the centre, size, rotation, text (TextPanel; a text and a shape: put the text
  // on the shape's path), layer, align, duplicate, delete. Each edit is
  // one `document.*` action; "keep proportions" is a local preference of the panel.
  import type { AlignHow, BooleanOp, TransformOp } from '../gen/api';
  import { app } from '../context';
  import { t } from '../i18n/index.svelte';
  import Icon from '../lib/Icon.svelte';
  import type { DocumentProjection } from '../state/document.svelte';
  import { PALETTE_COLORS, assignColor, layerCode } from './layers';
  import NumField from './NumField.svelte';
  import TextPanel from './TextPanel.svelte';
  import { rotationOf, selectionBox } from './transform';

  let { doc }: { doc: DocumentProjection } = $props();
  const { run, settings } = app();
  let lock = $state(true);
  /** Offset and array parameters: local, kept while the panel is open. */
  let offsetMm = $state(1);
  let columns = $state(2);
  let rows = $state(2);
  let gapMm = $state(5);

  const sel = $derived(doc.selected);
  const ids = $derived(sel.map((o) => o.id));
  const box = $derived(selectionBox(sel.map((o) => o.bounds ?? null)));
  const cx = $derived(box ? box.x + box.width / 2 : 0);
  const cy = $derived(box ? box.y + box.height / 2 : 0);
  const rot = $derived(sel.length === 1 ? rotationOf(sel[0]!.transform) : 0);
  const text = $derived(sel.length === 1 && sel[0]!.kind === 'text' ? sel[0]! : null);
  /** A text and a shape selected: the text can go on the shape's path. */
  const onPath = $derived.by(() => {
    if (sel.length !== 2) return null;
    const words = sel.find((o) => o.kind === 'text');
    const shape = sel.find((o) => o.kind === 'shape');
    return words && shape ? { text: words.id, path: shape.id } : null;
  });
  const inch = $derived(settings.ui.units === 'in');
  const lenUnit = $derived(inch ? 'in' : 'mm');
  const lenScale = $derived(inch ? 25.4 : 1);
  const usedColors = $derived(
    new Set(sel.map((o) => doc.layers.find((l) => l.id === o.layer)?.color.toUpperCase() ?? '')),
  );

  const transform = (op: TransformOp) => run(() => doc.service.dispatch('document.transform', { ids, op }));
  function size(axis: 'w' | 'h', v: number) {
    if (!box || v <= 0) return;
    const cur = axis === 'w' ? box.width : box.height;
    if (cur <= 0) return;
    const k = v / cur;
    const scale: [number, number] = lock ? [k, k] : axis === 'w' ? [k, 1] : [1, k];
    return transform({ scale, pivot: [cx, cy] });
  }
  const align = (how: AlignHow) => run(() => doc.service.dispatch('document.align', { ids, how }));
  const combine = (op: BooleanOp) => run(() => doc.service.dispatch('document.boolean', { ids, op }));
  const combines: [BooleanOp, 'weld' | 'subtract' | 'intersect'][] = [
    ['union', 'weld'],
    ['difference', 'subtract'],
    ['intersection', 'intersect'],
  ];
  const aligns: [
    AlignHow,
    'alignLeft' | 'alignCenterH' | 'alignRight' | 'alignTop' | 'alignMiddleV' | 'alignBottom',
  ][] = [
    ['left', 'alignLeft'],
    ['center_h', 'alignCenterH'],
    ['right', 'alignRight'],
    ['top', 'alignTop'],
    ['middle_v', 'alignMiddleV'],
    ['bottom', 'alignBottom'],
  ];
  const kindLabel = (k: string, name: string) =>
    k === 'text'
      ? t('obj-type-text')
      : k === 'image'
        ? t('obj-type-image')
        : name === 'rectangle'
          ? t('obj-type-rect')
          : name === 'ellipse'
            ? t('obj-type-ellipse')
            : t('obj-type-path');
</script>

<p class="panel-sub" data-testid="obj-sub">
  {sel.length === 1
    ? kindLabel(sel[0]!.kind, sel[0]!.name)
    : sel.length > 1
      ? t('obj-count', { n: sel.length })
      : ''}
</p>
<div class="inspector" data-testid="inspector">
  {#if !box}
    <p class="empty">{t('obj-none')}</p>
  {:else}
    <div class="fgrid">
      <NumField
        label="X"
        title={t('obj-center')}
        value={cx}
        unit={lenUnit}
        scale={lenScale}
        testid="obj-x"
        oncommit={(v) => transform({ translate: [v - cx, 0] })}
      />
      <NumField
        label="Y"
        title={t('obj-center')}
        value={cy}
        unit={lenUnit}
        scale={lenScale}
        testid="obj-y"
        oncommit={(v) => transform({ translate: [0, v - cy] })}
      />
      <NumField
        label={t('obj-w')}
        value={box.width}
        unit={lenUnit}
        scale={lenScale}
        min={0.1}
        testid="obj-w"
        oncommit={(v) => size('w', v)}
      />
      <NumField
        label={t('obj-h')}
        value={box.height}
        unit={lenUnit}
        scale={lenScale}
        min={0.1}
        testid="obj-h"
        oncommit={(v) => size('h', v)}
      />
      <NumField
        label={t('obj-rot')}
        value={rot}
        unit="°"
        decimals={1}
        testid="obj-rot"
        oncommit={(v) => transform({ rotate_deg: v - rot, pivot: [cx, cy] })}
      />
    </div>
    <label class="check"
      ><input type="checkbox" bind:checked={lock} data-testid="obj-lock" /><Icon name="lock" /><span
        >{t('obj-lock')}</span
      ></label
    >

    {#if sel.length === 1 && sel[0]!.kind === 'image'}
      <button
        class="btn"
        data-testid="obj-image-open"
        onclick={() => run(() => doc.service.dispatch('view.open_dialog', { name: 'raster' }))}
        ><Icon name="image" /><span>{t('obj-image-open')}</span></button
      >
    {/if}

    {#if text?.text}
      <TextPanel {doc} object={text} />
    {/if}
    {#if onPath}
      <button
        class="btn"
        title={t('text-on-path-hint')}
        data-testid="text-on-path"
        onclick={() =>
          run(() => doc.service.dispatch('document.text_on_path', { id: onPath.text, path: onPath.path }))}
        ><Icon name="text" /><span>{t('text-put-on-path')}</span></button
      >
    {/if}

    <div class="field">
      <span class="mini">{t('obj-layer')}</span>
      <div class="btn-row" role="group" aria-label={t('obj-layer')}>
        {#each PALETTE_COLORS as color (color)}
          <button
            class="icon-btn swatch"
            style="--c:{color}"
            class:used={usedColors.has(color)}
            title={layerCode(color)}
            aria-label={layerCode(color)}
            aria-pressed={usedColors.has(color)}
            onclick={() => run(() => assignColor(doc, color))}>{layerCode(color).slice(1)}</button
          >
        {/each}
      </div>
    </div>

    <div class="field">
      <span class="mini">{t('obj-align')}</span>
      <div class="btn-row" role="group" aria-label={t('obj-align')}>
        {#each aligns as [how, icon] (how)}
          <button
            class="icon-btn"
            title={t(`obj-${icon}`)}
            aria-label={t(`obj-${icon}`)}
            disabled={sel.length < 2}
            onclick={() => align(how)}><Icon name={icon} /></button
          >
        {/each}
      </div>
    </div>

    <div class="field">
      <span class="mini">{t('obj-combine')}</span>
      <div class="btn-row" role="group" aria-label={t('obj-combine')}>
        {#each combines as [op, icon] (op)}
          <button
            class="icon-btn"
            title={t(`tool-${icon}`)}
            aria-label={t(`tool-${icon}`)}
            data-testid="obj-{op}"
            disabled={sel.length < 2}
            onclick={() => combine(op)}><Icon name={icon} /></button
          >
        {/each}
      </div>
    </div>

    <div class="field" data-testid="obj-offset">
      <span class="mini">{t('obj-offset')}</span>
      <div class="btn-row">
        <NumField
          label={t('obj-offset-distance')}
          value={offsetMm}
          unit={lenUnit}
          scale={lenScale}
          testid="obj-offset-distance"
          oncommit={(v) => (offsetMm = v)}
        />
        <button
          class="btn"
          data-testid="obj-offset-go"
          disabled={offsetMm === 0}
          onclick={() => run(() => doc.service.dispatch('document.offset', { ids, distance_mm: offsetMm }))}
          ><Icon name="offset" /><span>{t('obj-offset-go')}</span></button
        >
      </div>
    </div>

    <div class="field" data-testid="obj-array">
      <span class="mini">{t('obj-array')}</span>
      <div class="fgrid">
        <NumField
          label={t('obj-array-columns')}
          value={columns}
          decimals={0}
          min={1}
          testid="obj-array-columns"
          oncommit={(v) => (columns = Math.max(1, Math.round(v)))}
        />
        <NumField
          label={t('obj-array-rows')}
          value={rows}
          decimals={0}
          min={1}
          testid="obj-array-rows"
          oncommit={(v) => (rows = Math.max(1, Math.round(v)))}
        />
        <NumField
          label={t('obj-array-gap')}
          value={gapMm}
          unit={lenUnit}
          scale={lenScale}
          testid="obj-array-gap"
          oncommit={(v) => (gapMm = v)}
        />
        <button
          class="btn"
          data-testid="obj-array-go"
          onclick={() =>
            run(() => doc.service.dispatch('document.array', { ids, columns, rows, gap_mm: [gapMm, gapMm] }))}
          ><Icon name="array" /><span>{t('obj-array-go')}</span></button
        >
      </div>
    </div>

    <div class="btn-row">
      <button
        class="btn"
        title={t('obj-duplicate')}
        data-testid="obj-duplicate"
        onclick={() => run(() => doc.service.dispatch('document.duplicate', { ids }))}
        ><Icon name="copy" /><span>{t('obj-duplicate').replace(/\s*\(.*\)$/, '')}</span></button
      >
      <button
        class="btn"
        title={t('obj-delete')}
        data-testid="obj-delete"
        onclick={() => run(() => doc.service.dispatch('document.delete', { ids }))}
        ><Icon name="trash" /><span>{t('obj-delete').replace(/\s*\(.*\)$/, '')}</span></button
      >
    </div>
  {/if}
</div>

<style>
  .swatch {
    background: var(--c);
    border: 2px solid transparent;
    color: #000c;
    font: 700 10px var(--fk-font-family-mono);
  }
  .swatch.used {
    border-color: var(--fk-color-text);
  }
</style>
