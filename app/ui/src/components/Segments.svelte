<script lang="ts">
  // A few short choices side by side: the chosen one sits on a raised block that slides to the next choice the moment
  // it is clicked.
  import type { Entry } from '../lib/types';

  let { entries, onchoose }: { entries: Entry[]; onchoose: (e: Entry) => void } = $props();

  let chosen = $state(-1);
  $effect(() => { chosen = entries.findIndex((e) => e.checked === true); });
  const each = $derived(100 / entries.length);

  function choose(index: number) {
    if (index === chosen || !entries[index].action) return;
    chosen = index;
    onchoose(entries[index]);
  }
</script>

<div class="segments" role="radiogroup">
  {#if chosen >= 0}
    <span class="block" style:left="calc(2px + (100% - 4px) * {chosen / entries.length})" style:width="calc((100% - 4px) * {1 / entries.length})"></span>
  {/if}
  {#each entries as entry, i (entry.label)}
    <button class="segment" class:chosen={i === chosen} style:width="calc((100% - 4px) * {each / 100})" role="radio" aria-checked={i === chosen}
      onclick={() => choose(i)}><span class="label">{entry.label}</span></button>
  {/each}
</div>

<style>
  .segments {
    position: relative;
    display: flex;
    height: 28px;
    padding: 2px;
    box-sizing: border-box;
    border-radius: 8px;
    background: color-mix(in srgb, var(--label) 6%, transparent);
  }
  .block {
    position: absolute;
    top: 2px;
    bottom: 2px;
    border-radius: 6px;
    background: white;
    box-shadow: 0 0.5px 1.5px rgba(0, 0, 0, 0.12);
    transition: left 0.22s var(--page-curve);
  }
  :global(:root[data-theme='dark']) .block { background: rgba(255, 255, 255, 0.16); box-shadow: none; }
  @media (prefers-color-scheme: dark) { :global(:root:not([data-theme='light'])) .block { background: rgba(255, 255, 255, 0.16); box-shadow: none; } }
  .segment {
    position: relative;
    height: 100%;
    border-radius: 6px;
    font-size: 11.5px;
    color: var(--secondary);
    text-align: center;
  }
  .segment:not(.chosen):hover { background: color-mix(in srgb, var(--label) 5%, transparent); }
  .label {
    display: inline-block;
    transition: transform 0.22s cubic-bezier(0.25, 1, 0.5, 1);
    white-space: nowrap;
  }
  .segment:active .label { transform: scale(0.94); transition-duration: 0.07s; }
  .segment.chosen { color: var(--label); font-weight: 600; }
</style>
