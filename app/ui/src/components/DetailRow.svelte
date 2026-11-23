<script lang="ts">
  // One row of a page: hung on guide lines under its parent, led by its control, light or icon, its figure or switch on
  // the right, a 4pt usage bar with the even-pace mark, its detail, and a chevron that turns when it opens.
  import { barColor, usageColor } from '../lib/colors';
  import { t } from '../lib/i18n.svelte';
  import { actionable, command, configured, destructive, info, keepCommand, leads, pace, toggle, usageShare } from '../lib/detail';
  import { font, width as measure } from '../lib/measure';
  import { panel } from '../lib/ui.svelte';
  import type { Entry } from '../lib/types';
  import Glyph from './Glyph.svelte';
  import StatusLight from './StatusLight.svelte';

  let { entry, depth, aligned, open = false, busy = false, rowWidth = 344, onpress }:
    { entry: Entry; depth: number; aligned: boolean; open?: boolean; busy?: boolean; rowWidth?: number; onpress: () => void } = $props();

  const INSET = 8, INDENT = 16, LEAD = 22;
  const leadX = $derived(INSET + depth * INDENT);
  const left = $derived(leadX + (aligned || leads(entry) ? LEAD : 0));
  /** A row whose action opens a page of the panel (在手机上看) carries the chevron as one with rows inside does. */
  const opensPage = $derived(entry.action?.[0] === 'show-phone');
  const right = $derived(rowWidth - INSET - (entry.children.length || opensPage ? 18 : entry.href !== undefined ? 16 : 0));
  const isToggle = $derived(toggle(entry));
  const plain = $derived(info(entry));
  const titleSize = $derived(plain ? 10.5 : depth === 0 ? 12.5 : 12);
  const titleWeight = $derived(!plain && depth === 0 && entry.children.length ? 500 : 400);
  const value = $derived.by(() => {
    if (isToggle || entry.value === undefined) return undefined;
    const room = (right - left) * 0.62;
    for (const form of [entry.value, ...(entry.fallbacks ?? [])]) if (measure(form, font(11.5, 500)) <= room) return form;
    return entry.fallbacks?.at(-1) ?? entry.value;
  });
  const share = $derived(usageShare(entry));
  const evenPace = $derived(pace(entry));
  const configuredState = $derived(configured(value));
  const canAct = $derived(actionable(entry));
  const tone = $derived(destructive(entry) ? 'var(--red)' : command(entry) ? 'var(--accent)' : canAct ? 'var(--label)' : 'var(--secondary)');
  const titleColour = $derived(plain ? 'var(--secondary)' : destructive(entry) ? 'var(--red)' : 'var(--label)');
  const valueColour = $derived(usageColor(entry.usageLevel) ?? (canAct || entry.usageLevel !== undefined ? 'var(--label)' : 'var(--secondary)'));
  const lineHeight = $derived(Math.ceil(titleSize * 1.2));

  // A switch's knob slides the moment it is clicked; the saved state confirms it after.
  let knob = $state(false);
  $effect(() => { knob = entry.checked === true; });

  function press() {
    if (busy) return;
    if (isToggle) knob = !knob;
    onpress();
  }
</script>

<button class="detail-row pressable" class:plain disabled={!canAct} aria-expanded={entry.children.length ? open : undefined}
  aria-checked={isToggle ? knob : undefined} role={isToggle ? 'switch' : undefined} onclick={press}
  style:padding-top="{plain ? 5 : 8}px" style:padding-bottom="{plain ? 5 : 8}px" style:min-height="{canAct ? 32 : 22}px"
  style:padding-left="{left}px" style:padding-right="{rowWidth - right}px">
  {#each Array.from({ length: depth }, (_, i) => i) as level (level)}
    <span class="guide" style:left="{INSET + level * INDENT + 6}px"></span>
  {/each}
  <span class="lead" style:left="{leadX}px" style:top="{(plain ? 5 : 8) + lineHeight / 2}px">
    {#if entry.checked !== undefined && !isToggle}
      <span class="box" class:radio={entry.radio} class:on={entry.checked}>
        {#if entry.checked && !entry.radio}<svg viewBox="0 0 13 13" width="13" height="13"><path d="M3 6.5 5.5 9.5 10 3.5" fill="none" style="stroke: var(--on-accent)" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" /></svg>{/if}
        {#if entry.checked && entry.radio}<span class="pip"></span>{/if}
      </span>
    {:else if entry.statusLight}
      <span class="light"><StatusLight state={entry.statusLight} /></span>
    {:else if entry.image || entry.symbol}
      <span class="glyph"><Glyph image={entry.image} template={entry.template} symbol={entry.symbol} size={15} tint={tone} /></span>
    {/if}
  </span>
  <span class="body">
    <span class="title-line">
      <span class="title" style:font-size="{titleSize}px" style:font-weight={titleWeight} style:color={titleColour} style:opacity={busy ? 0.55 : 1}>{entry.label}</span>
      {#if isToggle}
        <span class="switch" class:on={knob}><span class="knob"></span></span>
      {:else if busy}
        <span class="spinner" aria-label={t('正在进行')}></span>
      {:else if value !== undefined}
        {#if entry.attention}
          <span class="value pill num">{value}</span>
        {:else if configuredState !== null}
          <span class="value configured num"><StatusLight state={configuredState ? 'ok' : 'unknown'} />{value}</span>
        {:else}
          <span class="value num" style:color={valueColour}>{value}</span>
        {/if}
      {/if}
    </span>
    {#if share !== null}
      <span class="bar">
        <span class="bar-fill" style:width="max(4px, {share * 100}%)" style:background={barColor(entry.usageLevel, panel.dark)}
          style:--pen={barColor(entry.usageLevel, panel.dark)}></span>
        {#if evenPace !== null}
          <span class="pace" style:left="clamp(0px, calc({evenPace * 100}% - 1px), calc(100% - 2px))"></span>
        {/if}
      </span>
    {/if}
    {#if entry.detail}<span class="detail">{keepCommand(entry.detail)}</span>{/if}
  </span>
  {#if entry.children.length || opensPage}
    <svg class="chevron" class:open style:top="{(plain ? 5 : 8) + lineHeight / 2 - 6}px" width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
      <path d="M4.5 2.5 8 6 4.5 9.5" fill="none" stroke="var(--tertiary)" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" />
    </svg>
  {:else if entry.href}
    <svg class="out" style:top="{(plain ? 5 : 8) + lineHeight / 2 - 5}px" width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
      <path d="M2.5 7.5 7.5 2.5M3.6 2.5h3.9v3.9" fill="none" stroke="var(--tertiary)" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" />
    </svg>
  {/if}
</button>

<style>
  .detail-row {
    position: relative;
    display: flex;
    width: 100%;
    box-sizing: border-box;
    border-radius: 8px;
  }
  .detail-row::before {
    content: '';
    position: absolute;
    inset: 1px 2px;
    border-radius: 8px;
    transition: background-color 0.12s;
  }
  .detail-row:not(:disabled):hover::before { background: var(--hover); }
  .detail-row:not(:disabled):active::before { background: var(--press); }
  .guide {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 1px;
    background: var(--guide);
  }
  .lead {
    position: absolute;
    width: 15px;
    height: 0;
  }
  .box {
    position: absolute;
    left: 1px;
    top: -6.5px;
    width: 13px;
    height: 13px;
    box-sizing: border-box;
    border-radius: 3.5px;
    border: 1px solid color-mix(in srgb, var(--secondary) 55%, transparent);
  }
  .box.radio { border-radius: 6.5px; }
  .box.on { background: var(--accent); border-color: var(--accent); }
  .box svg { position: absolute; left: -1px; top: -1px; }
  .pip {
    position: absolute;
    left: 3.5px;
    top: 3.5px;
    width: 4px;
    height: 4px;
    border-radius: 50%;
    background: white;
  }
  .light { position: absolute; left: 3.5px; top: -3.5px; display: flex; }
  .glyph { position: absolute; left: 0; top: -7.5px; }
  .body {
    position: relative;
    display: flex;
    flex-direction: column;
    flex: 1;
    min-width: 0;
  }
  .title-line {
    display: flex;
    align-items: flex-start;
    gap: 10px;
  }
  .title {
    flex: 1;
    min-width: 40px;
    line-height: 1.2;
    overflow-wrap: anywhere;
    transition: opacity 0.15s;
  }
  .value {
    flex: none;
    font-size: 11.5px;
    font-weight: 500;
    line-height: 1.2;
    white-space: nowrap;
    margin-top: 0.5px;
  }
  .value.pill {
    padding: 1px 7px;
    border-radius: 9px;
    background: color-mix(in srgb, var(--accent) 12%, transparent);
    color: var(--accent);
  }
  .value.configured {
    display: flex;
    align-items: center;
    gap: 5px;
    color: var(--secondary);
  }
  .switch {
    position: relative;
    flex: none;
    width: 30px;
    height: 18px;
    margin-top: -1px;
    border-radius: 9px;
    background: color-mix(in srgb, var(--label) 19%, transparent);
    transition: background-color 0.18s var(--page-curve);
  }
  .switch.on { background: var(--accent); }
  .knob {
    position: absolute;
    left: 2px;
    top: 2px;
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: white;
    box-shadow: 0 0.5px 2px rgba(0, 0, 0, 0.25);
    transition: transform 0.18s cubic-bezier(0.33, 1, 0.68, 1);
  }
  .switch.on .knob { transform: translateX(12px); }
  .spinner {
    flex: none;
    width: 12px;
    height: 12px;
    margin: 1px 2px 0 0;
    border-radius: 50%;
    border: 1.6px solid color-mix(in srgb, var(--secondary) 35%, transparent);
    border-top-color: var(--secondary);
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin { to { transform: rotate(360deg); } }
  .bar {
    position: relative;
    height: 4px;
    margin-top: 6px;
    border-radius: 2px;
    background: var(--track);
  }
  .bar-fill {
    position: absolute;
    left: 0;
    top: 0;
    bottom: 0;
    border-radius: 2px;
  }
  .pace {
    position: absolute;
    top: -3px;
    width: 2px;
    height: 10px;
    border-radius: 1px;
    background: color-mix(in srgb, var(--label) 70%, transparent);
  }
  .detail {
    margin-top: 3px;
    font-size: 10.5px;
    line-height: 1.25;
    color: var(--secondary);
    overflow-wrap: anywhere;
    white-space: pre-line;
  }
  .chevron, .out {
    position: absolute;
    right: 8px;
    transition: transform 0.22s var(--page-curve);
  }
  .chevron.open { transform: rotate(90deg); }
</style>
