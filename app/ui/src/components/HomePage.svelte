<script lang="ts">
  // The homepage: the dashboard, a divider, then the subscription, API and other rows, groups split by lines. A row
  // held for a moment lifts and can be dragged to another place in its group (the others make way); ⌥↑ ⌥↓ move the
  // row in focus. The tool cards keep their order by usage.
  import { tick, type Snippet } from 'svelte';
  import { onPhone } from '../lib/backend';
  import { entryKey } from '../lib/detail';
  import { t } from '../lib/i18n.svelte';
  import type { Entry } from '../lib/types';
  import { panel } from '../lib/ui.svelte';
  import HomeRow from './HomeRow.svelte';
  import Scroller from './Scroller.svelte';

  let { entries, dashboard, scrollTop = $bindable(0), onactivate, onmove, onheight }: {
    entries: Entry[]; dashboard: Snippet | null; scrollTop?: number; onactivate: (e: Entry) => void;
    onmove: (action: string[]) => void; onheight: (height: number) => void;
  } = $props();

  const hasIcon = (e: Entry) => e.image !== undefined || e.symbol !== undefined || e.checked !== undefined || e.statusLight !== undefined;
  const column = $derived(entries.some(hasIcon) ? 36 : 12);

  /** The modules of each group, in order: the rows that can be dragged, and where. */
  const groups = $derived.by(() => {
    const out: string[][] = [[]];
    for (const e of entries) {
      if (e.separator) out.push([]);
      else if (e.module) out.at(-1)!.push(e.module);
    }
    return out.filter((group) => group.length);
  });
  const groupOf = (key: string) => groups.find((group) => group.includes(key)) ?? [];

  /** Held this long without moving, a row lifts. */
  const HOLD = 350;
  /** Moved this far before then, it was a scroll or a click, not a hold. */
  const SLOP = 6;
  let slots: Record<string, HTMLDivElement> = $state({});
  let holding: { key: string; x: number; y: number; timer: ReturnType<typeof setTimeout> } | null = null;
  /** The row being dragged: where the pointer took it, how far it has gone, and the place it would drop into. */
  let lifted = $state<{ key: string; pointer: number; startY: number; dy: number; tops: number[]; heights: number[]; from: number; to: number } | null>(null);
  /** The click a drag's release makes is not a click on the row. */
  let swallow = false;

  function down(event: PointerEvent, entry: Entry) {
    if (!entry.module || onPhone || event.button !== 0 || lifted) return;
    const key = entry.module;
    holding = { key, x: event.clientX, y: event.clientY, timer: setTimeout(() => lift(key, event.pointerId), HOLD) };
  }
  function lift(key: string, pointer: number) {
    if (!holding || holding.key !== key) return;
    const group = groupOf(key);
    const boxes = group.map((k) => slots[k]?.getBoundingClientRect());
    if (boxes.some((box) => !box)) { holding = null; return; }
    const from = group.indexOf(key);
    lifted = { key, pointer, startY: holding.y, dy: 0, tops: boxes.map((b) => b!.top), heights: boxes.map((b) => b!.height), from, to: from };
    holding = null;
    try { slots[key].setPointerCapture(pointer); } catch { /* the pointer went away meanwhile */ }
  }
  function move(event: PointerEvent) {
    if (holding && Math.hypot(event.clientX - holding.x, event.clientY - holding.y) > SLOP) cancelHold();
    if (!lifted || event.pointerId !== lifted.pointer) return;
    const { tops, heights, from } = lifted;
    const dy = event.clientY - lifted.startY;
    // Past a neighbour's middle, the dragged row takes its place.
    const centre = tops[from] + heights[from] / 2 + dy;
    let to = from;
    while (to < tops.length - 1 && centre > tops[to + 1] + heights[to + 1] / 2) to++;
    while (to > 0 && centre < tops[to - 1] + heights[to - 1] / 2) to--;
    lifted = { ...lifted, dy, to };
  }
  function up(event: PointerEvent) {
    cancelHold();
    if (!lifted || event.pointerId !== lifted.pointer) return;
    const { key, from, to } = lifted;
    const group = groupOf(key);
    lifted = null;
    swallow = true;
    setTimeout(() => (swallow = false), 0);
    if (to === from) return;
    const order = group.filter((k) => k !== key);
    order.splice(to, 0, key);
    onmove(to === order.length - 1 ? ['move-module', key, 'after', order[to - 1]] : ['move-module', key, 'before', order[to + 1]]);
  }
  function cancelHold() {
    if (holding) clearTimeout(holding.timer);
    holding = null;
  }
  function cancelDrag() {
    cancelHold();
    lifted = null;
  }
  /** How far a row stands from its place while another is dragged: the dragged one with the pointer, the ones it
   *  passed by its height, the other way. */
  function offset(key: string): number {
    if (!lifted) return 0;
    if (key === lifted.key) return lifted.dy;
    const group = groupOf(lifted.key);
    const index = group.indexOf(key);
    if (index < 0) return 0;
    const { from, to, heights } = lifted;
    if (from < to && index > from && index <= to) return -heights[from];
    if (to < from && index >= to && index < from) return heights[from];
    return 0;
  }

  async function keydown(event: KeyboardEvent, entry: Entry) {
    if (!entry.module || onPhone || !event.altKey || (event.key !== 'ArrowUp' && event.key !== 'ArrowDown')) return;
    event.preventDefault();
    const group = groupOf(entry.module);
    const index = group.indexOf(entry.module);
    const neighbour = group[index + (event.key === 'ArrowUp' ? -1 : 1)];
    if (!neighbour) return;
    onmove(['move-module', entry.module, event.key === 'ArrowUp' ? 'before' : 'after', neighbour]);
    // The row keeps the focus in its new place.
    await tick();
    slots[entry.module]?.querySelector('button')?.focus();
  }
</script>

<svelte:window onkeydown={(event) => { if (lifted && event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); cancelDrag(); } }} onblur={cancelDrag} />

<div class="home">
  <Scroller bind:scrollTop onheight={(h) => onheight(h + 16)}>
    {#if dashboard}
      {@render dashboard()}
    {:else}
      <div class="placeholder">{t('AI 用量面板')}</div>
    {/if}
    <div class="rows" class:dragging={lifted !== null}>
      <div class="divider lead"></div>
      {#each entries as entry, i (entry.module ?? entryKey(entry) + ':' + i)}
        {#if entry.separator}
          <div class="divider"></div>
        {:else if entry.module}
          {@const key = entry.module}
          <div class="slot" class:lifted={lifted?.key === key} data-module={key} bind:this={slots[key]} style:transform="translateY({offset(key)}px)"
            role="presentation" onpointerdown={(event) => down(event, entry)} onpointermove={move} onpointerup={up} onpointercancel={cancelDrag}
            onclickcapture={(event) => { if (swallow) { event.stopPropagation(); event.preventDefault(); } }} onkeydown={(event) => keydown(event, entry)}>
            <HomeRow {entry} {column} rowWidth={panel.width - 16} {onactivate} />
          </div>
        {:else}
          <HomeRow {entry} {column} rowWidth={panel.width - 16} {onactivate} />
        {/if}
      {/each}
    </div>
  </Scroller>
</div>

<style>
  .home {
    position: absolute;
    inset: 8px;
    display: flex;
  }
  .placeholder {
    height: 36px;
    padding: 10px 12px 0;
    box-sizing: border-box;
  }
  /* Section divider on the homepage: a little heavier than a hairline so groups read apart. */
  .divider {
    height: 12px;
    position: relative;
  }
  .divider::after {
    content: '';
    position: absolute;
    left: 8px;
    right: 8px;
    top: 5px;
    height: 2px;
    border-radius: 1px;
    background: var(--divider);
  }
  .divider.lead { height: 8px; }
  .divider.lead::after { top: 2px; }
  .slot {
    position: relative;
    border-radius: 7px;
  }
  /* While a row is dragged the others glide out of its way; it follows the pointer, lifted over them. */
  .dragging .slot { transition: transform 150ms ease; }
  .dragging .slot.lifted {
    z-index: 2;
    transition: none;
    background: var(--solid);
    box-shadow: 0 4px 14px rgba(0, 0, 0, 0.16), 0 0 0 0.5px var(--separator);
    cursor: grabbing;
  }
  .dragging :global(.row) { pointer-events: none; }
</style>
