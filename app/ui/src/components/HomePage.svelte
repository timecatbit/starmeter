<script lang="ts">
  // The homepage: the dashboard, a divider, then the subscription, API and other rows, groups split by lines.
  import { entryKey } from '../lib/detail';
  import type { Snippet } from 'svelte';
  import type { Entry } from '../lib/types';
  import HomeRow from './HomeRow.svelte';
  import Scroller from './Scroller.svelte';

  let { entries, dashboard, scrollTop = $bindable(0), onactivate, onheight }: {
    entries: Entry[]; dashboard: Snippet | null; scrollTop?: number; onactivate: (e: Entry) => void; onheight: (height: number) => void;
  } = $props();

  const hasIcon = (e: Entry) => e.image !== undefined || e.symbol !== undefined || e.checked !== undefined || e.statusLight !== undefined;
  const column = $derived(entries.some(hasIcon) ? 36 : 12);
</script>

<div class="home">
  <Scroller bind:scrollTop onheight={(h) => onheight(h + 16)}>
    {#if dashboard}
      {@render dashboard()}
    {:else}
      <div class="placeholder">AI 用量面板</div>
    {/if}
    <div class="rows">
      <div class="divider lead"></div>
      {#each entries as entry, i (entryKey(entry) + i)}
        {#if entry.separator}
          <div class="divider"></div>
        {:else}
          <HomeRow {entry} {column} {onactivate} />
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
</style>
