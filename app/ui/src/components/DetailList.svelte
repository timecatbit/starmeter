<script lang="ts">
  // A page's rows. Opening one moves the rows below to their new places and fades the new ones in under it; a run of
  // short choices becomes one segmented control; separators become thin rules between groups.
  import { flip } from 'svelte/animate';
  import { entryKey, iconed, leads, toggle } from '../lib/detail';
  import { font, width } from '../lib/measure';
  import { pageCurve, rowIn, rowOut } from '../lib/motion';
  import { panel } from '../lib/ui.svelte';
  import type { Entry } from '../lib/types';
  import DetailRow from './DetailRow.svelte';
  import Segments from './Segments.svelte';

  let { entries, open, busy, rowWidth = 344, onopen, onchoose, ontoggled }: {
    entries: Entry[]; open: Set<string>; busy: string[] | null; rowWidth?: number;
    onopen: (e: Entry) => void; onchoose: (e: Entry) => void; ontoggled: (open: Set<string>) => void;
  } = $props();

  type Item = { id: string } & ({ kind: 'row'; entry: Entry; depth: number; aligned: boolean } | { kind: 'section'; entry: Entry; depth: number }
    | { kind: 'rule'; depth: number } | { kind: 'segments'; entries: Entry[]; depth: number });

  const left = (depth: number) => 8 + depth * 16;
  const fits = (list: Entry[], room: number) => list.length <= 4 && list.every((e) => width(e.label, font(11.5, 600)) + 14 <= (room - 4) / list.length);

  const items = $derived.by(() => {
    const out: Item[] = [];
    const walk = (raw: Entry[], depth: number, parent: string) => {
      const list = raw.map(iconed), aligned = list.some(leads), seen = new Map<string, number>();
      const runs = new Map<number, number>();
      for (let start = 0; start < list.length;) {
        let end = start;
        while (end < list.length && list[end].radio && !list[end].children.length) end++;
        if (end - start >= 2 && fits(list.slice(start, end), rowWidth - left(depth) - 8)) runs.set(start, end);
        start = Math.max(end, start + 1);
      }
      for (let index = 0; index < list.length; index++) {
        const entry = list[index], end = runs.get(index);
        if (end !== undefined) { out.push({ id: `${parent}/§${index}`, kind: 'segments', entries: list.slice(index, end), depth }); index = end - 1; continue; }
        if (entry.separator) { if (index > 0 && index < list.length - 1) out.push({ id: `${parent}/—${index}`, kind: 'rule', depth }); continue; }
        const base = parent + '/' + entryKey(entry), count = seen.get(base) ?? 0;
        seen.set(base, count + 1);
        const id = count === 0 ? base : `${base}#${count}`;
        if (entry.section) { out.push({ id, kind: 'section', entry, depth }); continue; }
        out.push({ id, kind: 'row', entry, depth, aligned });
        if (entry.children.length && open.has(id)) walk(entry.children, depth + 1, id);
      }
    };
    walk(entries, 0, '');
    return out;
  });

  const isBusy = (e: Entry) => !!busy && !!e.action && !toggle(e) && busy.length === e.action.length && busy.every((x, i) => x === e.action![i]);

  function press(id: string, entry: Entry) {
    if (toggle(entry)) { onchoose(entry); return; }
    if (!entry.children.length) { onopen(entry); return; }
    const next = new Set(open);
    if (next.has(id)) { for (const key of next) if (key === id || key.startsWith(id + '/')) next.delete(key); }
    else next.add(id);
    ontoggled(next);
  }
</script>

<div class="list">
  {#each items as item (item.id)}
    <div class="item" animate:flip={{ duration: panel.reduceMotion ? 0 : 260, easing: pageCurve }} in:rowIn out:rowOut>
      {#if item.kind === 'row'}
        <DetailRow entry={item.entry} depth={item.depth} aligned={item.aligned} open={open.has(item.id)} busy={isBusy(item.entry)} {rowWidth}
          onpress={() => press(item.id, item.entry)} />
      {:else if item.kind === 'section'}
        <div class="section" style:padding-left="{left(item.depth)}px">{item.entry.label}</div>
      {:else if item.kind === 'rule'}
        <div class="rule" style:margin-left="{left(item.depth)}px"></div>
      {:else}
        <div class="segment-row" style:padding-left="{left(item.depth)}px">
          {#each Array.from({ length: item.depth }, (_, i) => i) as level (level)}
            <span class="guide" style:left="{left(level) + 6}px"></span>
          {/each}
          <Segments entries={item.entries} onchoose={onchoose} />
        </div>
      {/if}
    </div>
  {/each}
</div>

<style>
  .list {
    position: relative;
    width: 100%;
  }
  .item {
    width: 100%;
  }
  .section {
    height: 28px;
    box-sizing: border-box;
    padding-top: 11px;
    font-size: 10px;
    font-weight: 600;
    color: var(--secondary);
    line-height: 15px;
  }
  .rule {
    height: 1px;
    margin: 6px 8px 6px 0;
    background: var(--separator);
  }
  .segment-row {
    position: relative;
    padding: 6px 8px 6px 0;
  }
  .guide {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 1px;
    background: var(--guide);
  }
</style>
