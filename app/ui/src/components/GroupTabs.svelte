<script lang="ts">
  // 工具 / 模型 / 项目 / 会话 / 实时: plain text, the chosen one bold on a soft pill that slides to the next choice the
  // moment it is clicked, a fainter pill under the pointer. No frame or dividers. Spread across the row, the first at
  // its left and the last at its right, the same room between each (the user: 左右分散对齐), whatever the language,
  // look or the panel's width; a grouping's note on a line under them.
  import { font, width } from '../lib/measure';
  import { t } from '../lib/i18n.svelte';
  import type { Grouping } from '../lib/types';

  let { options, chosen, note = null, onchoose }: { options: Grouping[]; chosen: string; note?: string | null; onchoose: (key: string) => void } = $props();

  let shown = $state('');
  $effect(() => { shown = chosen; });
  /** The row's width, for the room between the tabs. */
  let room = $state(0);
  const widths = $derived(options.map((o) => width(o.label, font(11.5, 600)) + 20));
  const gap = $derived(options.length > 1 && room > 0
    ? Math.max(2, (room - 16 - widths.reduce((a, b) => a + b, 0)) / (options.length - 1)) : 2);
  const lefts = $derived(widths.map((_, i) => 8 + widths.slice(0, i).reduce((a, b) => a + b + gap, 0)));
  const index = $derived(Math.max(0, options.findIndex((o) => o.key === shown)));

  function choose(key: string) {
    shown = key;
    if (key !== chosen) onchoose(key);
  }

  // A radio group as the keyboard knows one: the arrows (and Home, End) move to the next choice and pick it, the focus
  // with it; only the chosen one is a Tab stop, so Tab leaves the group in one step (GROUP-KEY).
  let buttons: HTMLButtonElement[] = $state([]);
  function keydown(event: KeyboardEvent) {
    if (!options.length) return;
    const at = index;
    const to = event.key === 'ArrowRight' || event.key === 'ArrowDown' ? (at + 1) % options.length
      : event.key === 'ArrowLeft' || event.key === 'ArrowUp' ? (at - 1 + options.length) % options.length
      : event.key === 'Home' ? 0 : event.key === 'End' ? options.length - 1 : -1;
    if (to < 0) return;
    event.preventDefault();
    choose(options[to].key);
    buttons[to]?.focus();
  }
</script>

<div class="tabs" class:noted={!!note} role="radiogroup" tabindex="-1" aria-label={t('卡片分组')} onkeydown={keydown} bind:clientWidth={room}>
  <span class="pill" style:left="{lefts[index]}px" style:width="{widths[index]}px"></span>
  {#each options as option, i (option.key)}
    <button class="tab pressable" class:chosen={option.key === shown} style:left="{lefts[i]}px" style:width="{widths[i]}px"
      bind:this={buttons[i]} tabindex={i === index ? 0 : -1}
      role="radio" aria-checked={option.key === shown} onclick={() => choose(option.key)}>{option.label}</button>
  {/each}
  {#if note}<span class="note">{note}</span>{/if}
</div>

<style>
  .tabs {
    position: relative;
    height: 30px;
  }
  .tabs.noted { height: 44px; }
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
  /* Under the tabs, at the row's right, as the last tab ends. */
  .note {
    position: absolute;
    left: 18px;
    right: 18px;
    top: 29px;
    font-size: 9px;
    color: var(--tertiary);
    text-align: right;
    white-space: nowrap;
    overflow: hidden;
    overflow: clip visible;
    text-overflow: ellipsis;
  }
</style>
