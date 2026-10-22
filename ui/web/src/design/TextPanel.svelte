<script lang="ts">
  // The text part of the inspector: the text, its size, font (with a sample),
  // style, alignment, spacing, effects and, on a path, where it sits. Each edit is
  // one `document.set_text` (or `text_off_path`, `text_to_paths`): one undo step.
  import type { FontFamily, ObjectSummary, TextAlign, TextSpec } from '../gen/api';
  import { app } from '../context';
  import { t } from '../i18n/index.svelte';
  import Icon from '../lib/Icon.svelte';
  import type { Params } from '../service';
  import type { DocumentProjection } from '../state/document.svelte';
  import NumField from './NumField.svelte';

  let { doc, object }: { doc: DocumentProjection; object: ObjectSummary } = $props();
  const { run, settings } = app();

  /** Fonts and samples, read once per panel. */
  let fonts: FontFamily[] = $state([]);
  let sample: { d: string; view_box: number[]; single_line: boolean } | null = $state(null);

  const spec = $derived(object.text as TextSpec);
  const font = $derived(spec.font ?? 'LaserGRBL Hershey');
  const family = $derived(fonts.find((f) => f.name === font));
  /** Effects that need closed outlines are off for single-line fonts. */
  const singleLine = $derived(family?.single_line ?? true);
  const inch = $derived(settings.ui.units === 'in');
  const lenUnit = $derived(inch ? 'in' : 'mm');
  const lenScale = $derived(inch ? 25.4 : 1);

  $effect(() => {
    void run(async () => {
      fonts = await doc.service.dispatch('fonts.list', {});
    });
  });
  $effect(() => {
    const name = font;
    void run(async () => {
      sample = await doc.service.dispatch('fonts.preview', { family: name });
    });
  });

  type Change = Omit<Params<'document.set_text'>, 'id'>;
  const set = (change: Change) =>
    run(() => doc.service.dispatch('document.set_text', { id: object.id, ...change }));

  const styles: ['bold' | 'italic' | 'underline' | 'strikethrough', string][] = [
    ['bold', 'B'],
    ['italic', 'I'],
    ['underline', 'U'],
    ['strikethrough', 'S'],
  ];
  const aligns: [TextAlign, 'alignLeft' | 'alignCenterH' | 'alignRight' | 'menu'][] = [
    ['left', 'alignLeft'],
    ['center', 'alignCenterH'],
    ['right', 'alignRight'],
    ['justify', 'menu'],
  ];
</script>

<div class="field">
  <label for="obj-text">{t('obj-text')}</label>
  <textarea
    id="obj-text"
    class="textarea"
    rows="2"
    value={spec.text}
    data-testid="text-content"
    onchange={(e) => set({ text: e.currentTarget.value || ' ' })}></textarea>
</div>
<NumField
  label={t('obj-size')}
  title={t('text-size-hint')}
  value={spec.height_mm}
  unit={lenUnit}
  scale={lenScale}
  min={0.5}
  testid="text-size"
  oncommit={(v) => set({ height_mm: v })}
/>

<div class="field">
  <label for="text-font">{t('text-font')}</label>
  <select
    id="text-font"
    data-testid="text-font"
    value={font}
    onchange={(e) => set({ font: e.currentTarget.value })}
  >
    {#if !fonts.some((f) => f.name === font)}
      <option value={font}>{t('text-font-missing', { name: font })}</option>
    {/if}
    <optgroup label={t('text-fonts-single')}>
      {#each fonts.filter((f) => f.single_line) as f (f.name)}
        <option value={f.name}>{f.name}</option>
      {/each}
    </optgroup>
    {#if fonts.some((f) => !f.single_line)}
      <optgroup label={t('text-fonts-outline')}>
        {#each fonts.filter((f) => !f.single_line) as f (f.name)}
          <option value={f.name}>{f.name}</option>
        {/each}
      </optgroup>
    {/if}
  </select>
  {#if sample}
    <svg
      class="sample"
      class:stroke={sample.single_line}
      viewBox={sample.view_box.join(' ')}
      preserveAspectRatio="xMinYMid meet"
      role="img"
      aria-label={t('text-sample', { name: font })}
      data-testid="text-sample"><path d={sample.d} /></svg
    >
  {/if}
</div>

<div class="field">
  <span class="mini">{t('text-style')}</span>
  <div class="btn-row" role="group" aria-label={t('text-style')}>
    {#each styles as [key, letter] (key)}
      <button
        class="icon-btn toggle letter {key}"
        title={t(`text-${key}`)}
        aria-label={t(`text-${key}`)}
        aria-pressed={spec[key] === true}
        data-testid="text-{key}"
        onclick={() => set({ [key]: !spec[key] })}>{letter}</button
      >
    {/each}
    <span class="gap"></span>
    {#each aligns as [how, icon] (how)}
      <button
        class="icon-btn toggle"
        title={t(`text-align-${how}`)}
        aria-label={t(`text-align-${how}`)}
        aria-pressed={(spec.align ?? 'left') === how}
        data-testid="text-align-{how}"
        onclick={() => set({ align: how })}><Icon name={icon} /></button
      >
    {/each}
  </div>
</div>

<div class="fgrid">
  <NumField
    label={t('text-line-spacing')}
    value={spec.line_spacing ?? 1}
    unit="×"
    min={0.1}
    testid="text-line-spacing"
    oncommit={(v) => set({ line_spacing: v })}
  />
  <NumField
    label={t('text-letter-spacing')}
    value={spec.letter_spacing_mm ?? 0}
    unit={lenUnit}
    scale={lenScale}
    testid="text-letter-spacing"
    oncommit={(v) => set({ letter_spacing_mm: v })}
  />
  <NumField
    label={t('text-width')}
    value={spec.width_scale ?? 1}
    unit="%"
    scale={0.01}
    decimals={0}
    min={0.05}
    testid="text-width"
    oncommit={(v) => set({ width_scale: v })}
  />
</div>

<div class="field" data-testid="text-effects">
  <span class="mini">{t('text-effects')}</span>
  <label class="check" title={singleLine ? t('text-outline-only') : t('text-weld-hint')}
    ><input
      type="checkbox"
      checked={spec.weld === true}
      disabled={singleLine}
      data-testid="text-weld"
      onchange={(e) => set({ weld: e.currentTarget.checked })}
    /><span>{t('text-weld')}</span></label
  >
  <div class="fgrid">
    <NumField
      label={t('text-outline')}
      title={t('text-zero-off')}
      value={spec.outline_mm ?? 0}
      unit={lenUnit}
      scale={lenScale}
      testid="text-outline"
      oncommit={(v) => set({ outline_mm: v })}
    />
    <NumField
      label={t('text-stencil')}
      title={singleLine ? t('text-outline-only') : t('text-zero-off')}
      value={spec.stencil_mm ?? 0}
      unit={lenUnit}
      scale={lenScale}
      min={0}
      testid="text-stencil"
      oncommit={(v) => !singleLine && set({ stencil_mm: v })}
    />
    {#if !spec.path}
      <NumField
        label={t('text-arc')}
        title={t('text-arc-hint')}
        value={spec.arc_radius_mm ?? 0}
        unit={lenUnit}
        scale={lenScale}
        testid="text-arc"
        oncommit={(v) => set({ arc_radius_mm: v })}
      />
    {/if}
  </div>
</div>

{#if spec.path}
  <div class="field" data-testid="text-path">
    <span class="mini">{spec.path.object != null ? t('text-on-path') : t('text-on-path-copy')}</span>
    <div class="fgrid">
      <NumField
        label={t('text-path-offset')}
        value={spec.path.offset_mm ?? 0}
        unit={lenUnit}
        scale={lenScale}
        testid="text-path-offset"
        oncommit={(v) => set({ path_offset_mm: v })}
      />
      <NumField
        label={t('text-path-distance')}
        value={spec.path.distance_mm ?? 0}
        unit={lenUnit}
        scale={lenScale}
        testid="text-path-distance"
        oncommit={(v) => set({ path_distance_mm: v })}
      />
    </div>
    <label class="check"
      ><input
        type="checkbox"
        checked={spec.path.flip === true}
        data-testid="text-path-flip"
        onchange={(e) => set({ path_flip: e.currentTarget.checked })}
      /><span>{t('text-path-flip')}</span></label
    >
    <button
      class="btn"
      data-testid="text-off-path"
      onclick={() => run(() => doc.service.dispatch('document.text_off_path', { id: object.id }))}
      ><span>{t('text-off-path')}</span></button
    >
  </div>
{/if}

<button
  class="btn"
  title={t('text-to-paths-hint')}
  data-testid="text-to-paths"
  onclick={() => run(() => doc.service.dispatch('document.text_to_paths', { ids: [object.id] }))}
  ><Icon name="pen" /><span>{t('text-to-paths')}</span></button
>

<style>
  .letter {
    font: 700 var(--fk-font-size-sm) var(--fk-font-family-sans, inherit);
  }
  .letter.italic {
    font-style: italic;
  }
  .letter.underline {
    text-decoration: underline;
  }
  .letter.strikethrough {
    text-decoration: line-through;
  }
  .gap {
    flex: 0 0 var(--fk-space-2);
  }
  .sample {
    width: 100%;
    height: 32px;
    fill: var(--fk-color-text);
    stroke: none;
  }
  .sample.stroke {
    fill: none;
    stroke: var(--fk-color-text);
    stroke-width: 0.6;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
</style>
