<script lang="ts">
  // A cell of the module grid, flat as magpie's: checkbox, icon, name and a green dot once configured. The checkbox
  // switches the module on the homepage; the name or icon opens its settings, with an arrow under the pointer.
  // A module with settings has the two as two controls, side by side in the one cell: the checkbox its own, so the
  // keyboard (Tab to it, Space) and a screen reader reach it apart from the settings; it had been a part of the cell's
  // button that only a click on its spot could tell apart.
  import { isModule } from '../lib/detail';
  import { t } from '../lib/i18n.svelte';
  import { reads } from '../lib/i18n.svelte';
  import { font, width } from '../lib/measure';
  import { panel } from '../lib/ui.svelte';
  import type { Entry } from '../lib/types';
  import Glyph from './Glyph.svelte';

  let { entry, cellWidth, height, ontoggle, onopen }: { entry: Entry; cellWidth: number; height: number; ontoggle: () => void; onopen: () => void } = $props();

  /** A module set up, or (在手机上看) one that is on: a green dot after the name. */
  const configured = $derived(reads(entry.value, '已配置') || entry.statusLight === 'ok');
  /** Opens a page: its rows, or (在手机上看) the panel's own page. */
  const opens = $derived(entry.children.length > 0 || entry.action?.[0] === 'show-phone');
  const room = $derived(cellWidth - 54 - (opens ? 18 : 6) - (configured ? 12 : 0));
  const nameWidth = $derived(Math.min(room, width(entry.label, font(12.5, 500)) + 1));
  const line = $derived(entry.detail === undefined ? height / 2 : 15);
  // A cell with a note under its name keeps its checkbox and icon in the cell's middle, level with its row's others.
  const middle = $derived(height / 2);
  // Shown at once; the saved list confirms it.
  let checked = $state(false);
  $effect(() => { checked = entry.checked === true; });
  /** A module with settings: its checkbox is a control of its own beside the button that opens them. */
  const split = $derived(isModule(entry) && entry.children.length > 0 && entry.checked !== undefined);

  function toggle() {
    if (entry.checked !== undefined) checked = !checked;
    ontoggle();
  }
  function click() {
    if (entry.children.length) onopen();
    else toggle();
  }
</script>

{#snippet box()}
  {#if checked}<svg viewBox="0 0 13 13" width="13" height="13" aria-hidden="true"><path d="M3 6.5 5.5 9.5 10 3.5" fill="none" style="stroke: var(--on-accent)" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" /></svg>{/if}
{/snippet}

<div class="slot" style:width="{cellWidth}px" style:height="{height}px">
{#if split}
  <!-- The checkbox's own control, first in the order as on screen: the cell's first 26pt, where a click had switched the
       module, the box drawn where it was. -->
  <button class="toggle" role="checkbox" aria-checked={checked} aria-label={t('在主页显示 {name}', { name: entry.label })} onclick={toggle}>
    <span class="box" class:on={checked} style:top="{middle - 6.5}px">{@render box()}</span>
  </button>
{/if}
<button class="cell" aria-label={entry.label} onclick={click}>
  {#if entry.checked !== undefined && !split}
    <span class="box" class:on={checked} style:top="{middle - 6.5}px" role="checkbox" aria-checked={checked} aria-label={t('在主页显示 {name}', { name: entry.label })}>
      {@render box()}
    </span>
  {/if}
  {#if entry.image || entry.symbol}
    <span class="icon" style:top="{middle - 8}px"><Glyph image={entry.image} template={entry.template} symbol={entry.symbol} size={16} tint={panel.dark ? 'rgb(230,230,230)' : 'rgb(46,46,46)'} /></span>
  {/if}
  <span class="name" style:top="{line - 8}px" style:width="{nameWidth}px">{entry.label}</span>
  {#if configured}<span class="dot" style:left="{54 + nameWidth + 6}px" style:top="{line - 3}px"></span>{/if}
  {#if entry.detail}<span class="detail" style:top="{line + 9}px" style:width="{cellWidth - 54 - (opens ? 22 : 8)}px">{entry.detail}</span>{/if}
  {#if opens}
    <svg class="arrow" style:top="{line - 4}px" width="6" height="9" viewBox="0 0 6 9" aria-hidden="true"><path d="M1 .5 5 4.5 1 8.5" fill="none" stroke="var(--secondary)" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round" /></svg>
  {/if}
</button>
</div>

<style>
  .slot {
    position: relative;
    flex: none;
    transition: transform 0.22s cubic-bezier(0.25, 1, 0.5, 1);
  }
  /* Pressed, by its button or its checkbox, the whole cell presses together. */
  .slot:has(:active) { transform: scale(0.96); transition-duration: 0.07s; }
  @media (prefers-reduced-motion: reduce) { .slot:has(:active) { transform: none; } }
  .cell {
    position: absolute;
    inset: 0;
    border-radius: 7px;
  }
  .cell::before {
    content: '';
    position: absolute;
    inset: 1px 0;
    border-radius: 7px;
  }
  /* The whole cell lights under the pointer and presses, its checkbox's spot as well. */
  .slot:hover .cell::before { background: color-mix(in srgb, var(--label) 6%, transparent); }
  .cell:active::before, .slot:has(.toggle:active) .cell::before { background: color-mix(in srgb, var(--label) 12%, transparent); }
  .cell > * { position: absolute; }
  .toggle {
    position: absolute;
    left: 0;
    top: 0;
    bottom: 0;
    width: 26px;
    z-index: 1;
    border-radius: 7px 0 0 7px;
  }
  .toggle .box { position: absolute; }
  .box {
    left: 8px;
    width: 13px;
    height: 13px;
    box-sizing: border-box;
    border-radius: 3.5px;
    border: 1px solid color-mix(in srgb, var(--secondary) 55%, transparent);
  }
  .box.on { background: var(--accent); border-color: var(--accent); }
  .box svg { position: absolute; left: -1px; top: -1px; }
  .icon { left: 29px; }
  .name {
    left: 54px;
    font-size: 12.5px;
    font-weight: 500;
    line-height: 16px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--green);
  }
  .detail {
    left: 54px;
    font-size: 10px;
    color: var(--secondary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    line-height: 13px;
  }
  .arrow { right: 10px; opacity: 0; }
  .slot:hover .arrow { opacity: 1; }
</style>
