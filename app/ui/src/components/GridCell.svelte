<script lang="ts">
  // A cell of the module grid, flat as magpie's: checkbox, icon, name and a green dot once configured. The checkbox
  // switches the module on the homepage; the name or icon opens its settings, with an arrow under the pointer.
  import { isModule } from '../lib/detail';
  import { font, width } from '../lib/measure';
  import { panel } from '../lib/ui.svelte';
  import type { Entry } from '../lib/types';
  import Glyph from './Glyph.svelte';

  let { entry, cellWidth, height, ontoggle, onopen }: { entry: Entry; cellWidth: number; height: number; ontoggle: () => void; onopen: () => void } = $props();

  const configured = $derived(entry.value === '已配置');
  const room = $derived(cellWidth - 54 - (entry.children.length ? 18 : 6) - (configured ? 12 : 0));
  const nameWidth = $derived(Math.min(room, width(entry.label, font(12.5, 500)) + 1));
  const line = $derived(entry.detail === undefined ? height / 2 : 15);
  // Shown at once; the saved list confirms it.
  let checked = $state(false);
  $effect(() => { checked = entry.checked === true; });

  function click(event: MouseEvent) {
    const x = event.clientX - (event.currentTarget as HTMLElement).getBoundingClientRect().left;
    if (isModule(entry) && entry.children.length && x >= 26) { onopen(); return; }
    if (!isModule(entry) && entry.children.length) { onopen(); return; }
    if (entry.checked !== undefined) checked = !checked;
    ontoggle();
  }
</script>

<button class="cell pressable" style:width="{cellWidth}px" style:height="{height}px" aria-label={entry.label} onclick={click}>
  {#if entry.checked !== undefined}
    <span class="box" class:on={checked} style:top="{line - 6.5}px" role="checkbox" aria-checked={checked} aria-label="在主页显示 {entry.label}">
      {#if checked}<svg viewBox="0 0 13 13" width="13" height="13"><path d="M3 6.5 5.5 9.5 10 3.5" fill="none" stroke="white" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" /></svg>{/if}
    </span>
  {/if}
  {#if entry.image || entry.symbol}
    <span class="icon" style:top="{line - 8}px"><Glyph image={entry.image} template={entry.template} symbol={entry.symbol} size={16} tint={panel.dark ? 'rgb(230,230,230)' : 'rgb(46,46,46)'} /></span>
  {/if}
  <span class="name" style:top="{line - 8}px" style:width="{nameWidth}px">{entry.label}</span>
  {#if configured}<span class="dot" style:left="{54 + nameWidth + 6}px" style:top="{line - 3}px"></span>{/if}
  {#if entry.detail}<span class="detail" style:top="{line + 9}px" style:width="{cellWidth - 54 - (entry.children.length ? 22 : 8)}px">{entry.detail}</span>{/if}
  {#if entry.children.length}
    <svg class="arrow" style:top="{line - 4}px" width="6" height="9" viewBox="0 0 6 9" aria-hidden="true"><path d="M1 .5 5 4.5 1 8.5" fill="none" stroke="var(--secondary)" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round" /></svg>
  {/if}
</button>

<style>
  .cell {
    --press-scale: 0.96;
    position: relative;
    flex: none;
    border-radius: 7px;
  }
  .cell::before {
    content: '';
    position: absolute;
    inset: 1px 0;
    border-radius: 7px;
  }
  .cell:hover::before { background: color-mix(in srgb, var(--label) 6%, transparent); }
  .cell:active::before { background: color-mix(in srgb, var(--label) 12%, transparent); }
  .cell > * { position: absolute; }
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
  .cell:hover .arrow { opacity: 1; }
</style>
