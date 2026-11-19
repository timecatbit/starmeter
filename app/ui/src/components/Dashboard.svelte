<script lang="ts">
  // The homepage's top: the heading and its buttons, Summary's three figures, the legend and trend (or the live chart
  // while pinned), the heatmap, the grouping tabs and the Harness cards.
  import { shell } from '../lib/backend';
  import { accent, blend, label as labelColour, totalColor } from '../lib/colors';
  import { chartExtra, labelColumn, legendLabels, legendLayout, summaryProviders } from '../lib/chart';
  import { heading, money, tokens } from '../lib/format';
  import { HeatmapLayout, rows as heatmapRows } from '../lib/heatmap';
  import type { LiveFeed } from '../lib/live';
  import { font, width } from '../lib/measure';
  import { panel } from '../lib/ui.svelte';
  import type { Dashboard, Entry } from '../lib/types';
  import Figure from './Figure.svelte';
  import GroupTabs from './GroupTabs.svelte';
  import Heatmap from './Heatmap.svelte';
  import HintButton from './HintButton.svelte';
  import LiveChart from './LiveChart.svelte';
  import ProviderCard from './ProviderCard.svelte';
  import SummaryChart from './SummaryChart.svelte';

  let { data, rangeOptions, reveal, glows, summaryGlow, live, leaving = null, onleft, liveStamp, pinned, pinInterval, liveSince, spin, onrange,
    onrefresh, onshare, ondetect, onpin, onsettings, ongroup, ontoggle }: {
    data: Dashboard; rangeOptions: Entry[]; reveal: number; glows: Record<string, number>; summaryGlow: number; live: LiveFeed | null;
    leaving?: number | null; onleft?: () => void;
    liveStamp: string | null; pinned: boolean; pinInterval: number; liveSince: number; spin: { from: number; until: number | null } | null;
    onrange: (action: string[]) => void; onrefresh: () => void; onshare: (button: HTMLElement) => void; ondetect: () => void; onpin: () => void;
    onsettings: () => void; ongroup: (key: string) => void; ontoggle: (id: string, expanded: boolean) => void;
  } = $props();

  // 344pt between the homepage's margins, more in a pinned panel made wider.
  const left = 12;
  const inner = $derived(panel.width - 16);
  const right = $derived(inner - 12);
  const showSummary = $derived(data.showSummary !== false);
  const extra = $derived(chartExtra(data, right - left));
  const summaryHeight = $derived(showSummary ? 230 + extra : 44);
  const column = $derived(labelColumn(data));
  const heatmapLayout = $derived(data.heatmap ? new HeatmapLayout(left + column, summaryHeight + 8, right - left - column, data.heatmap.columns, heatmapRows(data.heatmap)) : null);
  const heatmapHeight = $derived(heatmapLayout ? 8 + heatmapLayout.height + 15 : 0);
  const harnessHeader = $derived(!data.providers.length || !showSummary || data.heatmap ? 0 : 6);
  const groupingHeight = $derived(!data.providers.length || (data.groupings?.length ?? 0) < 2 ? 0 : 30);
  const cardList = $derived(data.cards ?? data.providers);
  const collapsible = $derived(data.allowCollapse === true && !data.cards);
  const expanded = $derived(new Set(data.expandedProviders ?? []));

  const counted = $derived(summaryProviders(data));
  const valid = $derived(counted.filter((p) => p.available));
  const costs = $derived(valid.map((p) => p.cost).filter((c) => c !== null));
  const partial = $derived(valid.length !== counted.length || valid.some((p) => p.unpriced > 0));
  const labels = $derived([partial ? '已知 API 估算' : 'API 估算', 'Token 用量', '请求']);
  const values = $derived([money(costs.length ? costs.reduce((a, b) => a + b, 0) : null), valid.length ? tokens(valid.reduce((s, p) => s + p.tokens, 0)) : '—',
    valid.length ? String(valid.reduce((s, p) => s + p.requests, 0)) : '—']);
  const third = $derived((right - left) / 3);
  const legend = $derived(legendLabels(data));
  const legendColours = $derived([totalColor(panel.dark), ...counted.map((p) => accent(p.id, panel.dark))]);
  const legendCells = $derived(legendLayout(legend, right - left).flatMap((row, r) => row.map((cell) => ({ ...cell, row: r }))));
  const note = $derived([data.chartStatus, liveStamp ?? data.updated ?? ''].filter(Boolean).join(' · '));

  // Header: refresh, share, auto-detect, the pin and the gear leftward from the range picker, 2pt apart; a long heading
  // keeps its room by leaving auto-detect out.
  // The range picker is drawn bare like the icons beside it, as wide as the range on it: 8pt, the range, 4pt, the
  // chevron, 8pt; its right edge on the content's.
  const pickerWidth = $derived(8 + width(data.rangeLabel, font(11)) + 4 + 7 + 8);
  const pickerX = $derived(inner - 12 - pickerWidth);
  const headingText = $derived(heading(data.showSummary));
  const headingRight = $derived(12 + width(headingText, font(14, 600)) + 6);
  const firstButton = $derived(pickerX - 6 - 27);
  const fitsAll = $derived(firstButton - 4 * 29 >= headingRight);
  const buttonCount = $derived(fitsAll ? 5 : 4);
  const leftmost = $derived(firstButton - (buttonCount - 1) * 29);
  const toastWidth = $derived(panel.toast ? width(panel.toast.text, font(11, 500)) + 4 : 0);
  const toastX = $derived(leftmost - 12 - toastWidth);
  const chosen = $derived(rangeOptions.find((o) => o.checked)?.action?.[1] ?? data.rangeKey);

  let shareButton: HTMLSpanElement | undefined = $state();
  let select: HTMLSelectElement | undefined = $state();

  function pickRange(event: Event) {
    const key = (event.target as HTMLSelectElement).value;
    panel.menuOpen = false;
    const option = rangeOptions.find((o) => o.action?.[1] === key);
    if (option?.action && key !== data.rangeKey) onrange(option.action);
    // The snapshot alone marks the range; until it arrives the old range stays chosen.
    if (select) select.value = chosen;
  }
  const glowColour = $derived(blend(labelColour(panel.dark, 0.85), totalColor(panel.dark), summaryGlow * 0.85));
  /** Pinned, the header's free space moves the window, as a title bar would. */
  function grab(event: PointerEvent) {
    if (event.button !== 0 || !panel.pinned) return;
    event.preventDefault();
    void shell.drag();
  }
  function cardHeight(p: Dashboard['providers'][number]) {
    if (collapsible && !expanded.has(p.id)) return 52;
    const clients = p.available && p.clients?.length ? 16 : 0;
    return p.available ? 52 + clients + Math.max(1, p.models.length) * 18 + 8 : 52;
  }
</script>

<div class="dashboard" style:width="{inner}px" style:height="{summaryHeight + heatmapHeight + harnessHeader + groupingHeight + Math.max(cardList.length === 0 && data.providers.length ? 24 : 0, cardList.reduce((s, p) => s + cardHeight(p), 0)) + (data.providers.length ? 4 : 0)}px">
  <div class="summary" style:height="{summaryHeight}px">
    <div class="grip" class:movable={panel.pinned} onpointerdown={grab} aria-hidden="true"></div>
    <h1 class="heading" style:width="{inner - 233 - 18}px">{headingText}</h1>
    <div class="buttons">
      <span class="slot" style:left="{firstButton}px">
        <HintButton icon="arrow.clockwise" hint="刷新数据" label="刷新数据" onclick={onrefresh} spinFrom={spin?.from ?? null} spinUntil={spin?.until ?? null} />
      </span>
      <span class="slot" bind:this={shareButton} style:left="{firstButton - 29}px">
        <HintButton icon={panel.toast ? (panel.toast.ok ? 'checkmark' : 'exclamationmark') : 'square.and.arrow.up'} hint="分享用量图" label="分享用量图"
          tint={panel.toast ? (panel.toast.ok ? 'var(--green)' : 'var(--orange)') : 'var(--label)'} onclick={() => shareButton && onshare(shareButton)} />
      </span>
      {#if fitsAll}
        <span class="slot" style:left="{firstButton - 58}px"><HintButton icon="sparkle.magnifyingglass" hint="自动检测" label="自动检测" onclick={ondetect} /></span>
      {/if}
      <span class="slot" style:left="{firstButton - (fitsAll ? 87 : 58)}px">
        <HintButton icon={pinned ? 'pin.fill' : 'pin'} hint={pinned ? '取消钉住' : `钉住，每 ${pinInterval} 秒刷新`} label={pinned ? '取消钉住面板' : '钉住面板'}
          onclick={onpin} ring={pinned ? { interval: pinInterval, since: liveSince } : null} />
      </span>
      <span class="slot" style:left="{leftmost}px"><HintButton icon="gearshape" hint="主页模块与设置" label="主页模块与设置" onclick={onsettings} /></span>
      {#if panel.toast && toastX >= headingRight}
        <span class="toast" class:ok={panel.toast.ok} style:left="{toastX}px" style:width="{toastWidth}px" role="status">{panel.toast.text}</span>
      {/if}
      <span class="picker" class:open={panel.menuOpen} style:left="{pickerX}px" style:width="{pickerWidth}px">
        <span class="picker-label">{data.rangeLabel}</span>
        <svg class="picker-arrows" width="7" height="5" viewBox="0 0 7 5" aria-hidden="true"><path d="M1 1.1 3.5 3.6 6 1.1" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg>
        <select bind:this={select} aria-label="统计范围 · {data.rangeLabel}" value={chosen} onchange={pickRange}
          onpointerdown={() => (panel.menuOpen = true)} onblur={() => (panel.menuOpen = false)}>
          {#each rangeOptions as option (option.label)}
            <option value={option.action?.[1]}>{option.label}</option>
          {/each}
        </select>
      </span>
    </div>
    {#if showSummary}
      {#each [0, 1, 2] as i (i)}
        {@const x = left + third * i + 2 * i}
        {@const align = (['left', 'center', 'right'] as const)[i]}
        <span class="figure-label" style:left="{x}px" style:width="{third - 4}px" style:text-align={align}>{labels[i]}</span>
        <span class="figure" style:left="{x}px"><Figure value={values[i]} size={22} room={third - 4} {align} color={glowColour} /></span>
      {/each}
      {#each legendCells as cell, i (i)}
        <span class="swatch" style:left="{left + cell.x}px" style:top="{96 + cell.row * 17}px" style:height="{i === 0 ? 3 : 2}px" style:background={legendColours[i]}></span>
        <span class="legend" style:left="{left + cell.x + 16}px" style:top="{90 + cell.row * 17}px" style:width="{cell.w - 16}px">{legend[i]}</span>
      {/each}
      <div class="trend" style:transform="translateY({extra}px)">
        {#if live}
          <LiveChart feed={live} {column} {counted} {data} {note} {leaving} {onleft} width={right - left} />
        {:else}
          <SummaryChart {data} {note} {reveal} {right} />
        {/if}
      </div>
    {/if}
  </div>
  {#if data.heatmap}
    <Heatmap heatmap={data.heatmap} {column} top={summaryHeight} {right} />
  {/if}
  <div class="cards" style:top="{summaryHeight + heatmapHeight + harnessHeader}px">
    {#if groupingHeight && data.groupings}
      <GroupTabs options={data.groupings} chosen={data.grouping ?? 'tool'} note={data.groupNote} onchoose={ongroup} />
    {/if}
    {#if !cardList.length && data.providers.length}
      <div class="no-calls">此时段暂无调用</div>
    {/if}
    {#each cardList as p (p.id)}
      <ProviderCard {p} {collapsible} expanded={expanded.has(p.id)} glow={glows[p.id] ?? 0} ontoggle={() => ontoggle(p.id, !expanded.has(p.id))} />
    {/each}
  </div>
</div>

<style>
  .dashboard {
    position: relative;
  }
  .summary {
    position: relative;
  }
  .summary > span, .buttons > span {
    position: absolute;
    white-space: nowrap;
    line-height: 1.2;
  }
  /* Under the heading and the buttons: the header's free space. */
  .grip {
    position: absolute;
    left: 0;
    top: 0;
    width: 100%;
    height: 40px;
  }
  .heading {
    position: absolute;
    left: 12px;
    top: 10px;
    pointer-events: none;
    margin: 0;
    font-size: 14px;
    font-weight: 600;
    line-height: 1.2;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .buttons { position: absolute; left: 0; top: 0; width: 100%; height: 40px; pointer-events: none; }
  .buttons > * { pointer-events: auto; }
  .slot { top: 7.5px; }
  .toast {
    top: 12px;
    font-size: 11px;
    font-weight: 500;
    color: var(--orange);
  }
  .toast.ok { color: var(--green); }
  .picker {
    top: 7.5px;
    height: 25px;
    display: flex;
    align-items: center;
    gap: 4px;
    box-sizing: border-box;
    padding: 0 8px;
    border-radius: 7px;
    font-size: 11px;
    color: var(--label);
  }
  .picker:hover { background: rgba(0, 0, 0, 0.06); }
  .picker.open, .picker:active { background: rgba(0, 0, 0, 0.11); }
  :global(:root[data-theme='dark']) .picker:hover { background: rgba(255, 255, 255, 0.09); }
  :global(:root[data-theme='dark']) .picker.open, :global(:root[data-theme='dark']) .picker:active { background: rgba(255, 255, 255, 0.16); }
  @media (prefers-color-scheme: dark) {
    :global(:root:not([data-theme='light'])) .picker:hover { background: rgba(255, 255, 255, 0.09); }
    :global(:root:not([data-theme='light'])) .picker.open, :global(:root:not([data-theme='light'])) .picker:active { background: rgba(255, 255, 255, 0.16); }
  }
  .picker-label { flex: 1; white-space: nowrap; }
  .picker-arrows { flex: none; color: var(--secondary); }
  .picker select {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    opacity: 0;
    font-size: 11px;
    cursor: default;
  }
  .figure-label {
    top: 40px;
    font-size: 10px;
    color: var(--secondary);
    overflow: hidden;
  }
  .figure { top: 55px; }
  .swatch {
    width: 11px;
    border-radius: 1.5px;
  }
  .legend {
    font-size: 8.5px;
    color: var(--secondary);
    overflow: hidden;
    text-overflow: ellipsis;
  }
  /* The chart's layer spans the header too; clicks go through to the buttons. */
  .trend {
    position: absolute;
    left: 0;
    top: 0;
    width: 100%;
    height: 230px;
    pointer-events: none;
  }
  .cards {
    position: absolute;
    left: 0;
    width: 100%;
  }
  .no-calls {
    padding: 4px 12px 0;
    height: 20px;
    font-size: 10.5px;
    color: var(--secondary);
  }
</style>
