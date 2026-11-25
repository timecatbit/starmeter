<script lang="ts">
  // 网页版's trend: the total over the range as a glowing line on a soft area, drawn in from the left as time runs
  // when a range first shows (so the eye follows it), its latest point breathing while the range is still running. A
  // finger on the chart (or a pointer) reads it: a line at the nearest slot and a bubble with its time, the total and
  // the tools that made most of it. Vertical swipes still scroll the page.
  import { alpha, summaryColour, toolColour } from '../lib/colors';
  import { style } from '../lib/style.svelte';
  import { chartAxis, chartPositions, seriesOf, smoothCurves, summaryProviders, svgPath } from '../lib/chart';
  import { legendName, tokens } from '../lib/format';
  import { t } from '../lib/i18n.svelte';
  import { panel } from '../lib/ui.svelte';
  import type { Dashboard } from '../lib/types';

  let { data, height, draw = true }: { data: Dashboard; height: number; draw?: boolean } = $props();

  let box: HTMLDivElement | undefined = $state();
  let width = $state(0);
  $effect(() => {
    if (!box) return;
    const observer = new ResizeObserver(() => (width = box?.clientWidth ?? 0));
    observer.observe(box);
    width = box.clientWidth;
    return () => observer.disconnect();
  });

  const providers = $derived(summaryProviders(data));
  const waiting = $derived(!providers.some((p) => p.available));
  const series = $derived(seriesOf(data));
  const n = $derived(data.dates.length);
  const totals = $derived(Array.from({ length: n }, (_, i) => series.reduce((s, row) => s + row[i], 0)));
  const axis = $derived(chartAxis(data));
  const positions = $derived(chartPositions(data));
  const top = 18, foot = 20;
  const plot = $derived({ x: 2, y: top, w: Math.max(10, width - 4), h: Math.max(20, height - top - foot) });
  const bottom = $derived(plot.y + plot.h);
  const curves = $derived(n > 1 && width > 0 ? smoothCurves(series, positions, plot, axis.ceiling) : []);
  const line = $derived(curves.length ? svgPath(curves[curves.length - 1]) : '');
  const area = $derived(line ? line + `L${plot.x + plot.w},${bottom}L${plot.x},${bottom}Z` : '');
  const x = (i: number) => plot.x + positions[i] * plot.w;
  const y = (v: number) => bottom - (v / axis.ceiling) * plot.h;
  const empty = $derived(waiting || totals.every((v) => v === 0));
  /** The range still running (today, this week…): its last point is now, and breathes. Yesterday's is over. */
  const running = $derived(data.rangeKey !== 'yesterday');
  // The total as the panel draws it: the night's soft teal, or by hand the coral pen (no glow then).
  const colour = $derived(summaryColour(panel.dark));
  const sketch = $derived(style.current === 'sketch');
  const glow = $derived(panel.dark ? '#C4D8FF' : '#5B6BD6');
  const middle = $derived.by(() => {
    let best: number | null = null;
    for (let i = 1; i < n - 1; i++) if (positions[i] >= 0.3 && positions[i] <= 0.7 && (best === null || Math.abs(positions[i] - 0.5) < Math.abs(positions[best] - 0.5))) best = i;
    return best;
  });
  const marks = $derived(n > 1 ? [0, ...(middle !== null ? [middle] : []), n - 1] : n === 1 ? [0] : []);
  const id = `stage-${Math.random().toString(36).slice(2, 8)}`;
  /** Drawn in when it first shows if asked (not when the page comes back from the share page), and whenever the range
   *  changes after that. */
  let firstRange: string | null = null;
  let moved = $state(false);
  $effect(() => {
    const key = data.rangeKey;
    if (firstRange === null) firstRange = key;
    else if (key !== firstRange) moved = true;
  });
  const drawing = $derived(!panel.reduceMotion && (draw || moved));

  // MARK: Reading it with a finger

  let at = $state<number | null>(null);
  let pressed = false;
  function nearest(clientX: number) {
    if (!box || n === 0) return null;
    const left = box.getBoundingClientRect().left, px = clientX - left;
    let best = 0;
    for (let i = 1; i < n; i++) if (Math.abs(x(i) - px) < Math.abs(x(best) - px)) best = i;
    return best;
  }
  function down(event: PointerEvent) {
    if (empty) return;
    pressed = true;
    at = nearest(event.clientX);
  }
  function move(event: PointerEvent) {
    // A mouse reads on hover; a finger once it is down.
    if (empty || (!pressed && event.pointerType !== 'mouse')) return;
    at = nearest(event.clientX);
  }
  function up() { pressed = false; at = null; }
  const picked = $derived.by(() => {
    if (at === null || at >= n) return null;
    const i = at;
    const parts = providers.map((p, k) => ({ id: p.id, name: legendName(p), value: series[k]?.[i] ?? 0 }))
      .filter((p) => p.value > 0).sort((a, b) => b.value - a.value).slice(0, 3);
    return { i, x: x(i), y: y(totals[i]), label: data.dates[i], total: totals[i], parts };
  });
  const bubbleLeft = $derived(picked ? Math.min(Math.max(0, picked.x - 70), Math.max(0, width - 140)) : 0);
</script>

<div class="chart" bind:this={box} style:height="{height}px" role="img" aria-label={t('用量趋势图')} data-tap="beam"
  onpointerdown={down} onpointermove={move} onpointerup={up} onpointercancel={up} onpointerleave={up}>
  {#if width > 0}
    <svg {width} {height} aria-hidden="true">
      <defs>
        <linearGradient id="{id}-fill" x1="0" y1={plot.y} x2="0" y2={bottom} gradientUnits="userSpaceOnUse">
          <stop offset="0" stop-color={alpha(colour, panel.dark ? 0.42 : 0.3)} />
          <stop offset="0.6" stop-color={alpha(colour, panel.dark ? 0.1 : 0.08)} />
          <stop offset="1" stop-color={alpha(colour, 0)} />
        </linearGradient>
        <filter id="{id}-blur" x="-10%" y="-40%" width="120%" height="180%"><feGaussianBlur stdDeviation="3.5" /></filter>
        <linearGradient id="{id}-line" x1="0" y1="0" x2="1" y2="0">
          <stop offset="0" stop-color={alpha(colour, 0.55)} />
          <stop offset="1" stop-color={colour} />
        </linearGradient>
      </defs>
      {#each [0.5, 1] as f (f)}
        <line class="grid" x1={plot.x} x2={plot.x + plot.w} y1={bottom - f * plot.h} y2={bottom - f * plot.h} />
      {/each}
      <line class="base" x1={plot.x} x2={plot.x + plot.w} y1={bottom} y2={bottom} />
      {#if !empty && line}
        {#key data.rangeKey}
          <g class:drawn={drawing}>
            <path class="area" d={area} fill="url(#{id}-fill)" />
            {#if !sketch}<path class="glow" d={line} pathLength="1" stroke={glow} filter="url(#{id}-blur)" />{/if}
            <path class="line" d={line} pathLength="1" stroke="url(#{id}-line)" />
            <g class="end" style:transform="translate({x(n - 1)}px, {y(totals[n - 1])}px)">
              {#if running}<circle class="pulse" r="9" fill={alpha(colour, 0.35)} />{/if}
              <circle r="4" fill={colour} stroke={panel.dark ? '#0B1638' : '#fff'} stroke-width="2" />
            </g>
          </g>
        {/key}
      {:else if !empty && n === 1}
        <circle cx={plot.x + plot.w / 2} cy={y(totals[0])} r="4" fill={colour} />
      {/if}
      {#if picked}
        <line class="cursor" x1={picked.x} x2={picked.x} y1={plot.y - 6} y2={bottom} />
        <circle cx={picked.x} cy={picked.y} r="5" fill={colour} stroke={panel.dark ? '#0B1638' : '#fff'} stroke-width="2" />
      {/if}
    </svg>
    <span class="ceiling num">{axis.labels[2]}</span>
    {#each marks as i (i)}
      <span class="tick num" style:left="{x(i)}px" class:first={i === 0} class:last={i === n - 1 && n > 1}>{data.dates[i]}</span>
    {/each}
    {#if empty}<span class="quiet">{waiting ? t('正在读取…') : data.chartStatus || t('此时段暂无调用')}</span>{/if}
    {#if picked}
      <div class="bubble" style:left="{bubbleLeft}px">
        <div class="when num">{picked.label}</div>
        <div class="sum num">{t('{n} Token', { n: tokens(picked.total) })}</div>
        {#each picked.parts as part (part.id)}
          <div class="part"><span class="dot" style:background={toolColour(part.id, panel.dark)}></span><span class="who">{part.name}</span><span class="num">{tokens(part.value)}</span></div>
        {/each}
      </div>
    {/if}
  {/if}
</div>

<style>
  .chart {
    position: relative;
    touch-action: pan-y;
    user-select: none;
    -webkit-user-select: none;
  }
  svg { display: block; overflow: visible; }
  .grid { stroke: var(--line-faint); stroke-width: 1; stroke-dasharray: 2 4; }
  .base { stroke: var(--line); stroke-width: 1; }
  .line { fill: none; stroke-width: 2.4; stroke-linecap: round; stroke-linejoin: round; }
  .glow { fill: none; stroke-width: 7; stroke-linecap: round; opacity: 0.3; }
  .end { transition: transform 0.6s cubic-bezier(0.22, 1, 0.36, 1); }
  .pulse { transform-box: fill-box; transform-origin: center; animation: breathe 2.4s ease-in-out infinite; }
  @keyframes breathe {
    0%, 100% { transform: scale(0.55); opacity: 0.9; }
    50% { transform: scale(1.25); opacity: 0.15; }
  }
  /* Drawn in from the left, as time runs; the area rises in under it, then the latest point lights. */
  .drawn .line, .drawn .glow { stroke-dasharray: 1; stroke-dashoffset: 1; animation: trace 1.1s cubic-bezier(0.45, 0, 0.2, 1) 0.15s forwards; }
  .drawn .area { opacity: 0; animation: rise 0.9s ease-out 0.45s forwards; }
  .drawn .end { opacity: 0; animation: rise 0.4s ease-out 1.15s forwards; }
  @keyframes trace { to { stroke-dashoffset: 0; } }
  @keyframes rise { to { opacity: 1; } }
  .cursor { stroke: var(--ink-2); stroke-width: 1; stroke-dasharray: 3 3; }
  .ceiling {
    position: absolute;
    left: 2px;
    top: 0;
    font-size: 10px;
    color: var(--ink-3);
  }
  .tick {
    position: absolute;
    bottom: 0;
    transform: translateX(-50%);
    font-size: 10px;
    color: var(--ink-3);
    white-space: nowrap;
  }
  .tick.first { transform: none; }
  .tick.last { transform: translateX(-100%); }
  .quiet {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 12px;
    color: var(--ink-3);
  }
  .bubble {
    position: absolute;
    top: -4px;
    width: 140px;
    box-sizing: border-box;
    padding: 7px 10px 8px;
    border-radius: 12px;
    background: var(--bubble);
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.28), inset 0 0 0 0.5px var(--glass-edge);
    pointer-events: none;
    font-size: 11px;
    color: var(--ink);
  }
  .when { color: var(--ink-3); font-size: 10px; }
  .sum { font-family: var(--rounded); font-size: 14px; font-weight: 600; margin: 1px 0 3px; }
  .part { display: flex; align-items: center; gap: 6px; height: 16px; color: var(--ink-2); }
  .part .who { flex: 1; min-width: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .dot { width: 7px; height: 7px; border-radius: 50%; flex: none; }
</style>
