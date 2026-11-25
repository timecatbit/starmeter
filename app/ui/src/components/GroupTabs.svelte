<script lang="ts">
  // 工具 / 模型 / 项目: plain text, the chosen one bold on a soft pill that slides to the next choice the moment it is
  // clicked, a fainter pill under the pointer. No frame or dividers.
  import { font, width } from '../lib/measure';
  import { t } from '../lib/i18n.svelte';
  import type { Grouping } from '../lib/types';

  let { options, chosen, note = null, onchoose }: { options: Grouping[]; chosen: string; note?: string | null; onchoose: (key: string) => void } = $props();

  let shown = $state('');
  $effect(() => { shown = chosen; });
  const widths = $derived(options.map((o) => width(o.label, font(11.5, 600)) + 20));
  const lefts = $derived(widths.map((_, i) => 8 + widths.slice(0, i).reduce((a, b) => a + b + 2, 0)));
  const index = $derived(Math.max(0, options.findIndex((o) => o.key === shown)));
  /** The note keeps to the room after the last tab, cut short with … rather than lying over it. */
  const noteLeft = $derived(options.length ? lefts[options.length - 1] + widths[options.length - 1] + 8 : 8);

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

<div class="tabs" role="radiogroup" tabindex="-1" aria-label={t('卡片分组')} onkeydown={keydown}>
  <span class="pill" style:left="{lefts[index]}px" style:width="{widths[index]}px"></span>
  {#each options as option, i (option.key)}
    <button class="tab pressable" class:chosen={option.key === shown} style:left="{lefts[i]}px" style:width="{widths[i]}px"
      bind:this={buttons[i]} tabindex={i === index ? 0 : -1}
      role="radio" aria-checked={option.key === shown} onclick={() => choose(option.key)}>{option.label}</button>
  {/each}
  {#if note}<span class="note" style:left="{noteLeft}px">{note}</span>{/if}
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
    text-align: right;
    white-space: nowrap;
    overflow: hidden;
    overflow: clip visible;
    text-overflow: ellipsis;
  }
</style>
