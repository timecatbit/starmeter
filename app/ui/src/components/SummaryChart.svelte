<script lang="ts">
  // The trend under Summary: each counted tool as a thin line, their total as a soft area with a ringed end, the axis in
  // the column the heatmap's row names share, and the notes (partial data, when it was read) faint above the plot.
  import { alpha, toolColour, summaryColour } from '../lib/colors';
  import { pen } from '../lib/sketchPalette';
  import { handLine, handRing, seeded } from '../lib/sketchStrokes';
  import { style } from '../lib/style.svelte';
  import { chartAxis, chartPositions, labelColumn, seriesOf, smoothCurves, summaryProviders, svgPath } from '../lib/chart';
  import { panel } from '../lib/ui.svelte';
  import type { Dashboard } from '../lib/types';

  let { data, note, reveal = 1, left = 12, right = 332 }: { data: Dashboard; note: string; reveal?: number; left?: number; right?: number } = $props();

  const column = $derived(labelColumn(data));
  const plot = $derived({ x: left + column, y: 127, w: right - left - column, h: 78 });
  const axis = $derived(chartAxis(data));
  const series = $derived(seriesOf(data));
  const providers = $derived(summaryProviders(data));
  /** Nothing read yet (待同步): the grid alone, rather than a flat line on a made-up scale that reads as no usage. */
  const waiting = $derived(!providers.some((p) => p.available));
  const n = $derived(data.dates.length);
  const totals = $derived(Array.from({ length: n }, (_, i) => series.reduce((s, row) => s + row[i], 0)));
  const positions = $derived(chartPositions(data));
  const curves = $derived(n > 1 ? smoothCurves(series, positions, plot, axis.ceiling) : []);
  const total = $derived(summaryColour(panel.dark));
  const bottom = $derived(plot.y + plot.h);
  const area = $derived(curves.length ? svgPath(curves[curves.length - 1]) + `L${plot.x + plot.w},${bottom}L${plot.x},${bottom}Z` : '');
  const end = $derived(n > 1 ? [plot.x + positions[n - 1] * plot.w, bottom - (totals[n - 1] / axis.ceiling) * plot.h] : [0, 0]);
  const middle = $derived.by(() => {
    let best: number | null = null;
    for (let i = 1; i < n - 1; i++) if (positions[i] >= 0.25 && positions[i] <= 0.75 && (best === null || Math.abs(positions[i] - 0.5) < Math.abs(positions[best] - 0.5))) best = i;
    return best;
  });
  const labelIndices = $derived(n > 1 ? [0, ...(middle !== null ? [middle] : []), n - 1] : []);
  const gradient = `trend-${Math.random().toString(36).slice(2, 8)}`;
  /** 手绘: the axes sketched in ink in place of the grid, and a ring by hand round the latest reading, as the picture draws. */
  const sketch = $derived(style.current === 'sketch');
  const axes = $derived(sketch ? handLine(seeded(3), plot.x - 4, bottom, plot.x + plot.w + 4, bottom) + handLine(seeded(4), plot.x, bottom + 4, plot.x, plot.y - 6) : '');
  const ring = $derived(sketch && n > 1 ? handRing(seeded(5), end[0], end[1], 4.5) : '');
</script>

<svg class="chart" width={right + 12} height="230" aria-hidden="true">
  <defs>
    <linearGradient id="{gradient}-fill" x1="0" y1={plot.y} x2="0" y2={bottom} gradientUnits="userSpaceOnUse">
      <stop offset="0" stop-color={alpha(total, panel.dark ? 0.16 : 0.11)} />
      <stop offset="1" stop-color={alpha(total, 0)} />
    </linearGradient>
    <!-- 手绘: the trend hatched in the teal pen under the coral line, as the 手绘 picture draws it. -->
    <pattern id="{gradient}-hatch" width="4.2" height="4.2" patternUnits="userSpaceOnUse" patternTransform="rotate(-37)">
      <line x1="0" y1="0" x2="0" y2="4.2" stroke={pen('teal', panel.dark)} stroke-width="1" stroke-opacity="0.75" />
    </pattern>
    <clipPath id="{gradient}-reveal">
      <rect x={plot.x - 6} y={plot.y - 10} width={(plot.w + 12) * reveal} height={plot.h + 20} />
    </clipPath>
  </defs>
  {#if sketch}
    <path d={axes} fill="none" stroke="var(--label)" stroke-opacity="0.8" stroke-width="1.2" stroke-linecap="round" />
  {:else}
    {#each [0, 0.5, 1] as f (f)}
      <line x1={plot.x} x2={plot.x + plot.w} y1={bottom - f * plot.h} y2={bottom - f * plot.h}
        stroke={panel.dark ? `rgba(255,255,255,${0.1 * (f === 0 ? 0.3 : 0.14)})` : `rgba(0,0,0,${0.1 * (f === 0 ? 0.3 : 0.14)})`} stroke-width="0.5" />
    {/each}
  {/if}
  {#if waiting}
    <!-- The grid is all there is to draw. -->
  {:else if n === 1}
    {#each series as values, i (i)}
      <circle cx={plot.x + plot.w / 2} cy={bottom - (values[0] / axis.ceiling) * plot.h} r="2.5" fill={toolColour(providers[i].id, panel.dark)} />
    {/each}
    <circle cx={plot.x + plot.w / 2} cy={bottom - (totals[0] / axis.ceiling) * plot.h} r="3" fill={total} />
  {:else if n > 1}
    <g clip-path="url(#{gradient}-reveal)">
      <path d={area} fill="url(#{gradient}-{sketch ? 'hatch' : 'fill'})" />
      {#each curves as curve, i (i)}
        {@const isTotal = i === curves.length - 1}
        <path d={svgPath(curve)} fill="none" stroke={isTotal ? total : alpha(toolColour(providers[i].id, panel.dark), 0.8)} stroke-width={isTotal ? (sketch ? 2.4 : 1.8) : 1.2}
          stroke-linecap="round" stroke-linejoin="round" />
        {#if isTotal && sketch}
          <!-- The hand's second pass over the total, a little off the first. -->
          <path d={svgPath(curve)} transform="translate(0.7 -0.6)" fill="none" stroke={total} stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" />
        {/if}
      {/each}
      {#if sketch}
        <path d={ring} fill="none" stroke={total} stroke-width="1.7" stroke-linecap="round" />
      {:else}
        <circle cx={end[0]} cy={end[1]} r="5" fill={alpha(total, 0.18)} />
        <circle cx={end[0]} cy={end[1]} r="2.5" fill={total} />
      {/if}
    </g>
  {/if}
</svg>
{#each waiting ? [] : axis.labels as label, i (i)}
  <span class="axis num" style:left="{left}px" style:top="{bottom - [0, 0.5, 1][i] * plot.h - 5}px" style:width="{column - 4}px">{label}</span>
{/each}
{#if note}
  <span class="note" style:left="{plot.x}px" style:top="107px" style:width="{plot.w}px">{note}</span>
{/if}
{#if n === 1}
  <span class="date" style:left="{plot.x + plot.w / 2 - 30}px" style:width="60px" style:text-align="center">{data.dates[0]}</span>
{/if}
{#each labelIndices as i (i)}
  {@const x = plot.x + positions[i] * plot.w}
  <span class="date" style:left="{i === 0 ? x : i === n - 1 ? right - 48 : x - 25}px" style:width="{i === n - 1 ? 48 : 50}px"
    style:text-align={i === 0 ? 'left' : i === n - 1 ? 'right' : 'center'}>{data.dates[i]}</span>
{/each}

<style>
  .chart {
    position: absolute;
    left: 0;
    top: 0;
    overflow: visible;
    pointer-events: none;
  }
  .axis, .note, .date {
    position: absolute;
    white-space: nowrap;
    overflow: hidden;
    overflow: clip visible;
    line-height: 1.2;
  }
  .axis {
    font-size: 8px;
    color: var(--secondary);
  }
  .note {
    font-size: 7.5px;
    color: var(--tertiary);
    text-align: right;
  }
  .date {
    top: 212px;
    font-size: 8.5px;
    color: var(--secondary);
  }
</style>
