<script lang="ts">
  // A figure on the phone's own page (网页版) whose digits roll like a meter's wheels: a new reading turns only the
  // digits that changed, so the eye sees which place moved and which way. The first time it shows (`count`), every
  // digit rolls up from 0, which says how large the number is. The currency sign and the K/M/B unit sit smaller beside
  // the digits, as on the panel. Reduced motion: the figure simply changes.
  import { onMount } from 'svelte';
  import { figureParts } from '../lib/format';
  import { panel } from '../lib/ui.svelte';

  let { value, count = false, delay = 0 }: { value: string; count?: boolean; delay?: number } = $props();

  const parts = $derived(figureParts(value));
  const chars = $derived([...parts.body]);
  let ready = $state(false);
  onMount(() => {
    if (!count || panel.reduceMotion) { ready = true; return; }
    // Drawn at zero first, then turned: two frames so the wheels have somewhere to roll from.
    let second = 0;
    const first = requestAnimationFrame(() => { second = requestAnimationFrame(() => (ready = true)); });
    return () => { cancelAnimationFrame(first); cancelAnimationFrame(second); };
  });
  /** A wheel's turn: the places to the left start a little later, so the number settles from its last digit up. */
  const wait = (fromRight: number) => (panel.reduceMotion ? 0 : delay + fromRight * 45);
</script>

<span class="number num" data-value={value}>
  <span class="said">{value}</span>
  <span class="drawn" aria-hidden="true">
    {#if parts.sign}<span class="sign">{parts.sign}</span>{/if}{#each chars as ch, i (chars.length - i)}{#if /\d/.test(ch)}<span
          class="wheel"><span class="digits" style:transform="translateY({-(ready ? Number(ch) : 0) * 10}%)"
            style:transition-delay="{wait(chars.length - i)}ms">{#each [0, 1, 2, 3, 4, 5, 6, 7, 8, 9] as d (d)}<span>{d}</span>{/each}</span></span>{:else}<span
          class="mark">{ch}</span>{/if}{/each}{#if parts.unit}<span class="unit">{parts.unit}</span>{/if}
  </span>
</span>

<style>
  .number {
    display: inline-flex;
    position: relative;
    white-space: nowrap;
    line-height: 1;
  }
  .said {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
  }
  .drawn { display: inline-flex; align-items: flex-end; }
  .wheel {
    display: inline-block;
    height: 1.1em;
    overflow: hidden;
    /* The wheel's edges fade, so a turning digit comes up out of the dark rather than from behind a cut. */
    mask-image: linear-gradient(transparent, #000 12%, #000 88%, transparent);
    -webkit-mask-image: linear-gradient(transparent, #000 12%, #000 88%, transparent);
  }
  .digits {
    display: flex;
    flex-direction: column;
    transition: transform 0.95s cubic-bezier(0.22, 1, 0.36, 1);
  }
  .digits > span, .mark { height: 1.1em; line-height: 1.1em; display: block; text-align: center; }
  .mark { display: inline-block; }
  /* As the panel's figures (Figure.svelte): the sign smaller, lighter and raised, the unit smaller, both beside the
     digits in the same rounded face (by hand in 手绘). */
  .sign {
    font-size: 0.62em;
    font-weight: 500;
    line-height: 1;
    color: var(--ink-2, var(--secondary));
    align-self: flex-start;
    margin: 0.1em 0.04em 0 0;
  }
  .unit {
    font-size: 0.74em;
    line-height: 1;
    margin: 0 0 0.07em 0.02em;
  }
</style>
