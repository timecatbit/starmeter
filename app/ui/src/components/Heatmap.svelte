<script lang="ts">
  // A punch card under Summary, after the shared label column, in the logo's halftone: a dot for each hour or day,
  // spread edge to edge, its size and depth of the total line's colour four steps by rank; an empty hour is a grey
  // speck, one yet to come a fainter one. The dot of now (this hour, or today) wears a ring. Several days by the hour get
  // an hour axis. Pointing at a dot reads it out on the line under them, where the streak (or why there's nothing yet)
  // sits otherwise; only this block redraws as the pointer moves.
  import { heatmapFuture, heatmapShades, label, summaryColour } from '../lib/colors';
  import { t } from '../lib/i18n.svelte';
  import { detail, dotRadius, HeatmapLayout, levels as shadeLevels, note, past as pastCount, rows as rowCount } from '../lib/heatmap';
  import { font, width } from '../lib/measure';
  import { panel } from '../lib/ui.svelte';
  import type { UsageHeatmap } from '../lib/types';

  let { heatmap, column, top, left = 12, right = 332, current = true, max = 10 }:
    { heatmap: UsageHeatmap; column: number; top: number; left?: number; right?: number; current?: boolean; max?: number } = $props();

  const layout = $derived(new HeatmapLayout(left + column, top + 8, right - left - column, heatmap.columns, rowCount(heatmap), max));
  const levels = $derived(shadeLevels(heatmap));
  const past = $derived(pastCount(heatmap));
  const shades = $derived(heatmapShades(panel.dark));
  const future = $derived(heatmapFuture(panel.dark));
  const count = $derived(Math.min(heatmap.tokens.length, layout.rowCount * layout.columns, levels.length));
  const squares = $derived(Array.from({ length: count }, (_, i) => layout.square(i)));
  const axis = $derived(layout.hourAxis(heatmap));
  const line = $derived(layout.y + layout.height + (axis ? 11 : 0) + 2);
  const hintWidth = $derived(width(heatmap.hint, font(7.5)));
  /** The square of now: the last one gone by, in a range that reaches the present. */
  const now = $derived(current && past > 0 && past <= count ? past - 1 : null);
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

<div class="heatmap" style:top="{top}px" style:height="{line - top + 13}px">
  {#each heatmap.rowLabels ?? [] as name, row (row)}
    {#if row < layout.rowCount && name}
      <span class="row-name" style:left="{left}px" style:top="{8 + row * layout.rowPitch + layout.size / 2 - 5.5}px" style:width="{column - 4}px">{name}</span>
    {/if}
  {/each}
  <svg bind:this={box} class="squares" style:left="{layout.x - 2}px" style:top="{8 - 2}px" width={layout.width + 4} height={layout.height + 4}
    onpointermove={move} onpointerleave={() => (pointed = null)} role="img" aria-label={t('用量热力图')}>
    <g transform="translate({2 - layout.x}, {2 - layout.y})">
      {#each squares as square, i (i)}
        <circle class="cell" cx={square.x + square.size / 2} cy={square.y + square.size / 2} r={dotRadius(square.size, levels[i], i >= past)}
          fill={i >= past ? future : shades[levels[i]]} />
      {/each}
      {#if now !== null && squares[now]}
        {@const square = squares[now]}
        <circle class="now" cx={square.x + square.size / 2} cy={square.y + square.size / 2} r={square.size / 2 + 1.6}
          fill="none" stroke={summaryColour(panel.dark)} stroke-width="1" />
      {/if}
      {#if pointed !== null && squares[pointed]}
        {@const square = squares[pointed]}
        <circle class="pointed" cx={square.x + square.size / 2} cy={square.y + square.size / 2} r={square.size / 2 + 1.6}
          fill="none" stroke={label(panel.dark, 0.7)} stroke-width="1.2" />
      {/if}
    </g>
  </svg>
  {#if axis}
    {#each [0, 6, 12, 18, 24] as hour (hour)}
      <span class="tick" class:end={hour === 24} style:left="{hour === 24 ? layout.x + layout.width : layout.x + hour * layout.columnPitch}px"
        style:top="{8 + layout.height + 2}px">{hour}</span>
    {/each}
  {/if}
  <!-- Its right edge on the squares' (the block spans the panel's inner width, wider in a pinned panel made wider). -->
  <span class="hint" style:right="{panel.width - 16 - right}px" style:top="{line - top + 1}px">{heatmap.hint}</span>
  {#if caption}
    <span class="caption" style:left="{layout.x}px" style:top="{line - top}px" style:width="{right - layout.x - hintWidth - 10}px">{caption}</span>
  {/if}
</div>

<style>
  .heatmap {
    position: absolute;
    left: 0;
    width: 100%;
  }
  .row-name, .hint, .caption, .tick {
    position: absolute;
    white-space: nowrap;
    overflow: hidden;
    overflow: clip visible;
    text-overflow: ellipsis;
    line-height: 1.2;
  }
  .row-name {
    font-size: 8.5px;
    color: var(--secondary);
  }
  .squares {
    position: absolute;
    overflow: visible;
  }
  .cell { transition: fill 0.3s ease-out, r 0.3s ease-out; }
  .tick {
    font-size: 7.5px;
    color: var(--tertiary);
    font-variant-numeric: tabular-nums;
  }
  .tick.end { transform: translateX(-100%); }
  .hint {
    font-size: 7.5px;
    color: var(--tertiary);
  }
  .caption {
    font-size: 9.5px;
    color: var(--secondary);
  }
</style>
