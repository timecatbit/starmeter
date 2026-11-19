<script lang="ts">
  // The squares under Summary, in the room after the shared label column. Pointing at one reads it out on the line
  // under them, where the streak (or why there's nothing yet) sits otherwise; only this block redraws as the pointer moves.
  import { heatmapShades, label } from '../lib/colors';
  import { detail, HeatmapLayout, levels as shadeLevels, note, past as pastCount, rows as rowCount } from '../lib/heatmap';
  import { font, width } from '../lib/measure';
  import { panel } from '../lib/ui.svelte';
  import type { UsageHeatmap } from '../lib/types';

  let { heatmap, column, top, left = 12, right = 332 }: { heatmap: UsageHeatmap; column: number; top: number; left?: number; right?: number } = $props();

  const layout = $derived(new HeatmapLayout(left + column, top + 8, right - left - column, heatmap.columns, rowCount(heatmap)));
  const levels = $derived(shadeLevels(heatmap));
  const past = $derived(pastCount(heatmap));
  const shades = $derived(heatmapShades(panel.dark));
  const count = $derived(Math.min(heatmap.tokens.length, layout.rowCount * layout.columns, levels.length));
  const squares = $derived(Array.from({ length: count }, (_, i) => layout.square(i)));
  const line = $derived(layout.y + layout.height + 2);
  const hintWidth = $derived(width(heatmap.hint, font(7.5)));
  let pointed = $state<number | null>(null);
  const caption = $derived(pointed !== null ? detail(heatmap, pointed) : note(heatmap));
  let box: SVGSVGElement | undefined = $state();

  function move(event: PointerEvent) {
    if (!box) return;
    const frame = box.getBoundingClientRect();
    const index = layout.index(event.clientX - frame.left + layout.x - 2, event.clientY - frame.top + layout.y - 2);
    pointed = index !== null && index < past ? index : null;
  }
</script>

<div class="heatmap" style:top="{top}px" style:height="{8 + layout.height + 15}px">
  {#each heatmap.rowLabels ?? [] as name, row (row)}
    {#if row < layout.rowCount && name}
      <span class="row-name" style:left="{left}px" style:top="{8 + row * layout.rowPitch + HeatmapLayout.cell / 2 - 6}px" style:width="{column - 4}px">{name}</span>
    {/if}
  {/each}
  <svg bind:this={box} class="squares" style:left="{layout.x - 2}px" style:top="{8 - 2}px" width={layout.width + 4} height={layout.height + 4}
    onpointermove={move} onpointerleave={() => (pointed = null)} role="img" aria-label="用量热力图">
    <g transform="translate({2 - layout.x}, {2 - layout.y})">
      {#each squares as square, i (i)}
        {#if i >= past}
          <rect x={square.x + 0.4} y={square.y + 0.4} width={square.size - 0.8} height={square.size - 0.8} rx="1.6" fill="none" stroke={shades[0]} stroke-width="0.8" />
        {:else}
          <rect x={square.x} y={square.y} width={square.size} height={square.size} rx="1.6" fill={shades[levels[i]]} />
        {/if}
      {/each}
      {#if pointed !== null && squares[pointed]}
        {@const square = squares[pointed]}
        <rect x={square.x - 1} y={square.y - 1} width={square.size + 2} height={square.size + 2} rx="2.2" fill="none" stroke={label(panel.dark, 0.6)} stroke-width="1.2" />
      {/if}
    </g>
  </svg>
  <span class="hint" style:right="{344 - right}px" style:top="{line - top + 1}px">{heatmap.hint}</span>
  {#if caption}
    <span class="caption" style:left="{layout.x}px" style:top="{line - top}px" style:width="{right - layout.x - hintWidth - 10}px">{caption}</span>
  {/if}
</div>

<style>
  .heatmap {
    position: absolute;
    left: 0;
    width: 344px;
  }
  .row-name, .hint, .caption {
    position: absolute;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    line-height: 1.2;
  }
  .row-name {
    font-size: 8px;
    color: var(--secondary);
  }
  .squares {
    position: absolute;
    overflow: visible;
  }
  .hint {
    font-size: 7.5px;
    color: var(--tertiary);
  }
  .caption {
    font-size: 9px;
    color: var(--secondary);
  }
</style>
