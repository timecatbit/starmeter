<script lang="ts">
  // A figure reads as one number: rounded, even-width digits, with the $ and the K/M/B unit smaller and lighter so the
  // digits carry it. Too wide for its slot, it shrinks rather than cuts.
  import { figureParts } from '../lib/format';
  import { figureWidth as width, font, ROUNDED } from '../lib/measure';

  let { value, size, room, align = 'left', weight = 600, color = 'var(--label)' }:
    { value: string; size: number; room: number; align?: 'left' | 'center' | 'right'; weight?: number; color?: string } = $props();

  const parts = $derived(figureParts(value));
  const natural = $derived(width(parts.sign, font(size * 0.62, 500, ROUNDED)) + width(parts.body, font(size, weight, ROUNDED)) + width(parts.unit, font(size * 0.74, weight, ROUNDED)));
  const fitted = $derived(natural > room ? Math.max(9, (size * room) / natural) : size);
</script>

<span class="figure num" style:font-size="{fitted}px" style:text-align={align} style:width="{room}px" style:color={color}
  style:padding-top="{(size - fitted) * 0.7}px" style:--weight={weight}>
  {#if parts.sign}<span class="sign">{parts.sign}</span>{/if}<span class="body">{parts.body}</span>{#if parts.unit}<span class="unit">{parts.unit}</span>{/if}
</span>

<style>
  .figure {
    display: block;
    font-family: var(--rounded);
    white-space: nowrap;
    overflow: hidden;
    overflow: clip visible;
    line-height: 1.2;
  }
  .body { font-weight: var(--weight); }
  .sign {
    font-size: 0.62em;
    font-weight: 500;
    color: var(--secondary);
    vertical-align: 0.48em;
    margin-right: 0.04em;
  }
  .unit {
    font-size: 0.74em;
    font-weight: var(--weight);
  }
</style>
