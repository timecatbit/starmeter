<script lang="ts">
  // The module list as magpie lays out its provider page: a wide, flat three-column grid under plain section titles,
  // every module in view at once. Ticked modules lead each section; the order is fixed when the list opens, so ticking
  // one never moves cells under the pointer.
  import { entryKey, isModule } from '../lib/detail';
  import type { Entry } from '../lib/types';
  import GridCell from './GridCell.svelte';
  import HintButton from './HintButton.svelte';
  import Scroller from './Scroller.svelte';

  let { entries, leading, fromHome, scrollTop = $bindable(0), onback, ondone, onaction, onopen, onheight }: {
    entries: Entry[]; leading: Set<string>; fromHome: boolean; scrollTop?: number;
    onback: () => void; ondone: () => void; onaction: (action: string[]) => void; onopen: (e: Entry) => void; onheight: (height: number) => void;
  } = $props();

  const INNER = 572, COLUMNS = 3, SPACING = 4;
  const cellWidth = Math.floor((INNER - SPACING * (COLUMNS - 1)) / COLUMNS);
  const sections = $derived.by(() => {
    const out: { title: string | null; cells: Entry[] }[] = [];
    let current: { title: string | null; cells: Entry[] } = { title: null, cells: [] };
    const close = () => {
      if (current.cells.length) {
        const first = (e: Entry) => isModule(e) && leading.has(entryKey(e));
        current.cells = [...current.cells.filter(first), ...current.cells.filter((e) => !first(e))];
        out.push(current);
      }
      current = { title: null, cells: [] };
    };
    for (const e of entries) {
      if (e.separator) close();
      else if (e.section) { close(); current.title = e.label; }
      else current.cells.push(e);
    }
    close();
    return out;
  });
  const rows = (cells: Entry[]) => Array.from({ length: Math.ceil(cells.length / COLUMNS) }, (_, i) => cells.slice(i * COLUMNS, i * COLUMNS + COLUMNS));
  const enabled = $derived(entries.filter((e) => isModule(e) && e.checked === true).length);
  const lead = $derived(fromHome ? 36 : 12);
</script>

<div class="modules">
  <Scroller bind:scrollTop onswipeback={fromHome ? onback : undefined} onheight={(h) => onheight(h + 16)}>
    <div class="content">
      {#if fromHome}<span class="back"><HintButton icon="chevron.left" size={17} hint="返回" label="返回" onclick={onback} /></span>{/if}
      <h2 class="heading" style:left="{lead}px">主页模块</h2>
      <span class="count" style:left="{lead}px">已启用 {enabled} 项</span>
      {#if !fromHome}<button class="done pressable" onclick={ondone}>完成</button>{/if}
      <div class="grid">
        {#each sections as section, s (s)}
          {#if section.title}<div class="section">{section.title}</div>{/if}
          {#each rows(section.cells) as line, r (r)}
            {@const height = line.some((e) => e.detail !== undefined) ? 44 : 32}
            <div class="line" style:height="{height}px">
              {#each line as entry (entryKey(entry))}
                <GridCell {entry} {cellWidth} {height} ontoggle={() => entry.action && onaction(entry.action)} onopen={() => onopen(entry)} />
              {/each}
            </div>
          {/each}
          <div class="gap"></div>
        {/each}
      </div>
    </div>
  </Scroller>
</div>

<style>
  .modules {
    position: absolute;
    inset: 8px;
    display: flex;
  }
  .content {
    position: relative;
    width: 572px;
    padding-top: 49px;
  }
  .back { position: absolute; left: 3px; top: 7.5px; }
  .heading {
    position: absolute;
    top: 7px;
    margin: 0;
    font-size: 14px;
    font-weight: 600;
    line-height: 20px;
  }
  .count {
    position: absolute;
    top: 29px;
    font-size: 10px;
    color: var(--secondary);
    line-height: 15px;
  }
  .done {
    position: absolute;
    right: 12px;
    top: 10px;
    height: 20px;
    padding: 0 12px;
    border-radius: 6px;
    background: var(--control);
    box-shadow: 0 0 0 0.5px var(--control-rim), 0 0.5px 1.5px rgba(0, 0, 0, 0.08);
    font-size: 11px;
  }
  .section {
    height: 34px;
    box-sizing: border-box;
    padding: 12px 8px 0;
    font-size: 12.5px;
    font-weight: 600;
    line-height: 17px;
  }
  .line {
    display: flex;
    gap: 4px;
  }
  .gap { height: 6px; }
</style>
