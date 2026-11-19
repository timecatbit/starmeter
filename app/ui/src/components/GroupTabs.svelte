<script lang="ts">
  // 工具 / 模型 / 项目: plain text, the chosen one bold on a soft pill that slides to the next choice the moment it is
  // clicked, a fainter pill under the pointer. No frame or dividers.
  import { font, width } from '../lib/measure';
  import type { Grouping } from '../lib/types';

  let { options, chosen, note = null, onchoose }: { options: Grouping[]; chosen: string; note?: string | null; onchoose: (key: string) => void } = $props();

  let shown = $state('');
  $effect(() => { shown = chosen; });
  const widths = $derived(options.map((o) => width(o.label, font(11.5, 600)) + 20));
  const lefts = $derived(widths.map((_, i) => 8 + widths.slice(0, i).reduce((a, b) => a + b + 2, 0)));
  const index = $derived(Math.max(0, options.findIndex((o) => o.key === shown)));

  function choose(key: string) {
    shown = key;
    if (key !== chosen) onchoose(key);
  }
</script>

<div class="tabs" role="radiogroup" aria-label="卡片分组">
  <span class="pill" style:left="{lefts[index]}px" style:width="{widths[index]}px"></span>
  {#each options as option, i (option.key)}
    <button class="tab pressable" class:chosen={option.key === shown} style:left="{lefts[i]}px" style:width="{widths[i]}px"
      role="radio" aria-checked={option.key === shown} onclick={() => choose(option.key)}>{option.label}</button>
  {/each}
  {#if note}<span class="note">{note}</span>{/if}
</div>

<style>
  .tabs {
    position: relative;
    height: 30px;
  }
  .pill, .tab {
    position: absolute;
    top: 4px;
    height: 22px;
    border-radius: 11px;
  }
  .pill {
    background: var(--hover);
    background: color-mix(in srgb, var(--label) 9.5%, transparent);
    transition: left 0.26s var(--page-curve), width 0.26s var(--page-curve);
  }
  .tab {
    --press-scale: 0.92;
    font-size: 11.5px;
    color: var(--secondary);
    text-align: center;
    line-height: 22px;
  }
  .tab:not(.chosen):hover { background: color-mix(in srgb, var(--label) 5%, transparent); }
  .tab:active { background: color-mix(in srgb, var(--label) 14%, transparent); }
  .tab.chosen {
    font-weight: 600;
    color: var(--label);
  }
  .note {
    position: absolute;
    right: 12px;
    top: 9px;
    font-size: 9px;
    color: var(--tertiary);
  }
</style>
