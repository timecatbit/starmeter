<script lang="ts">
  // The homepage's top: the heading and its buttons, Summary's three figures, the legend and trend (or the live chart
  // while pinned), the heatmap, the grouping tabs and the Harness cards, or 会话's session cards.
  import { byUsage } from '../lib/order';
  import { accent, blend, label as labelColour, totalColor } from '../lib/colors';
  import { t } from '../lib/i18n.svelte';
  import { chartExtra, labelColumn, legendLabels, legendLayout, summaryProviders } from '../lib/chart';
  import { isGrouped, money, tokens } from '../lib/format';
  import { blockHeight, HeatmapLayout, panelDot, rows as heatmapRows } from '../lib/heatmap';
  import type { LiveFeed } from '../lib/live';
  import { font, width } from '../lib/measure';
  import { grabWindow, panel } from '../lib/ui.svelte';
  import type { Dashboard, Entry } from '../lib/types';
  import markSvg from '../../../../assets/starmeter-mark.svg?raw';
  import Dropdown from './Dropdown.svelte';
  import Figure from './Figure.svelte';
  import GroupTabs from './GroupTabs.svelte';
  import Heatmap from './Heatmap.svelte';
  import HintButton from './HintButton.svelte';
  import LiveChart from './LiveChart.svelte';
  import ProviderCard from './ProviderCard.svelte';
  import SessionList from './SessionList.svelte';
  import MoreButton from './MoreButton.svelte';
  import SummaryChart from './SummaryChart.svelte';

  let { data, rangeOptions, reveal, glows, summaryGlow, live, leaving = null, onleft, liveStamp, pinned, pinInterval, onrange,
    onshare, onpin, onsettings, ongroup, ontoggle, onopentool, onsize, onfloor, sessions = null, view = 'cards', onview, busy = null, onopen, onchoose,
    readonly = false, reading = false }: {
    data: Dashboard; rangeOptions: Entry[]; reveal: number; glows: Record<string, number>; summaryGlow: number; live: LiveFeed | null;
    leaving?: number | null; onleft?: () => void;
    liveStamp: string | null; pinned: boolean; pinInterval: number;
    onrange: (action: string[]) => void; onshare: () => void; onpin: () => void;
    onsettings: () => void; ongroup: (key: string) => void; ontoggle: (id: string, expanded: boolean) => void;
    /** On a phone (在手机上看): no pin, settings or share, the range alone in the header. */
    readonly?: boolean;
    /** A range picked and still being read: the menu shows it, the figures are still the last range's. */
    reading?: boolean;
    /** How tall a pinned panel that fits the homepage is: down to the end of the third card or session. */
    onsize?: (height: number) => void;
    /** The least a pinned panel shows of the homepage: down to the end of the heatmap. */
    onfloor?: (height: number) => void;
    onopentool: (id: string) => void;
    /** The 会话 module's row, when it is on the homepage: its sessions are the fourth choice beside the groupings. */
    sessions?: Entry | null; view?: 'cards' | 'sessions'; onview?: (view: 'cards' | 'sessions') => void;
    busy?: string[] | null; onopen?: (e: Entry) => void; onchoose?: (e: Entry) => void;
  } = $props();

  // 344pt between the homepage's margins, more in a pinned panel made wider.
  const left = 12;
  const inner = $derived(panel.width - 16);
  const right = $derived(inner - 12);
  const showSummary = $derived(data.showSummary !== false);
  const extra = $derived(chartExtra(data, right - left));
  const summaryHeight = $derived(showSummary ? 230 + extra : 44);
  const column = $derived(labelColumn(data));
  const dot = $derived(panelDot(panel.width));
  const heatmapLayout = $derived(data.heatmap ? new HeatmapLayout(left + column, summaryHeight + 8, right - left - column, data.heatmap.columns, heatmapRows(data.heatmap), dot) : null);
  const heatmapHeight = $derived(heatmapLayout && data.heatmap ? blockHeight(heatmapLayout, data.heatmap) : 0);
  const harnessHeader = $derived(!data.providers.length || !showSummary || data.heatmap ? 0 : 6);
  const tabOptions = $derived([...(data.providers.length ? data.groupings ?? [] : []), ...(sessions ? [{ key: 'sessions', label: t('会话') }] : [])]);
  // 会话 when chosen, and always when there are no cards to choose instead (its row has left the other rows for here).
  const showingSessions = $derived(!!sessions && (view === 'sessions' || !data.providers.length));
  const groupingHeight = $derived(tabOptions.length < 2 ? 0 : 30);
  let sessionsHeight = $state(0);
  let sessionsBox: HTMLDivElement | undefined = $state();
  // The list's height, taken a frame after it changes: set inside the observer's own callback it would resize what
  // other observers watch in the same frame (the browser then warns of a resize loop).
  $effect(() => {
    const box = sessionsBox;
    if (!box) return;
    let frame = 0;
    const observer = new ResizeObserver(() => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => { sessionsHeight = box.offsetHeight; });
    });
    observer.observe(box);
    sessionsHeight = box.offsetHeight;
    return () => { observer.disconnect(); cancelAnimationFrame(frame); };
  });
  // By tool, the tools that used the most first, as the models and projects are.
  const cardList = $derived(data.cards ?? byUsage(data.providers));
  // Models and projects: the first five, the rest one click away (then the page scrolls to them); by tool every tool,
  // the ones the user picked. Choosing another grouping starts from five again.
  const SHOWN = 5;
  const MORE = 34;
  let allOf = $state<string | null>(null);
  const listed = $derived(!data.cards || allOf === data.grouping ? cardList : cardList.slice(0, SHOWN));
  const unlisted = $derived(cardList.length - listed.length);
  const collapsible = $derived(data.allowCollapse === true && !data.cards);
  const expanded = $derived(new Set(data.expandedProviders ?? []));
  const listHeight = $derived(showingSessions ? sessionsHeight
    : Math.max(cardList.length === 0 && data.providers.length ? 24 : 0, listed.reduce((s, p) => s + cardHeight(p), 0) + (unlisted ? MORE : 0))
      + (data.providers.length ? 4 : 0));
  const total = $derived(summaryHeight + heatmapHeight + harnessHeader + groupingHeight + listHeight);
  // A pinned panel fits down to the end of the third card or session (the user makes it taller at its edges).
  const FIT = 3;
  let sessionsFit = $state(0);
  const fitList = $derived(showingSessions ? sessionsFit
    : cardList.length <= FIT ? listHeight : cardList.slice(0, FIT).reduce((s, p) => s + cardHeight(p), 0) + 4);
  $effect(() => onsize?.(summaryHeight + heatmapHeight + harnessHeader + groupingHeight + fitList));
  $effect(() => onfloor?.(summaryHeight + heatmapHeight));

  const counted = $derived(summaryProviders(data));
  const valid = $derived(counted.filter((p) => p.available));
  const costs = $derived(valid.map((p) => p.cost).filter((c) => c !== null));
  const partial = $derived(valid.length !== counted.length || valid.some((p) => p.unpriced > 0));
  const labels = $derived([partial ? t('已知 API 估算') : t('API 估算'), t('Token 用量'), t('请求')]);
  const values = $derived([money(costs.length ? costs.reduce((a, b) => a + b, 0) : null), valid.length ? tokens(valid.reduce((s, p) => s + p.tokens, 0)) : '—',
    valid.length ? String(valid.reduce((s, p) => s + p.requests, 0)) : '—']);
  const third = $derived((right - left) / 3);
  const legend = $derived(legendLabels(data));
  const legendColours = $derived([totalColor(panel.dark), ...counted.map((p) => accent(p.id, panel.dark))]);
  const legendCells = $derived(legendLayout(legend, right - left).flatMap((row, r) => row.map((cell) => ({ ...cell, row: r }))));
  // 多台电脑合并: the other computers counted in, beside the update time (this one included).
  const note = $derived([data.chartStatus, liveStamp ?? data.updated ?? '', data.devices ? t('含 {n} 台电脑', { n: data.devices + 1 }) : '']
    .filter(Boolean).join(' · '));

  // Header: the star and Starmeter, then share, the pin and the gear leftward from the range picker, 2pt apart. (No
  // refresh: opening reads what is due, pinning reads every few seconds, and each source's own page refreshes it; auto-
  // detect lives in 通用 and the star's menu.)
  // The range picker is drawn bare like the icons beside it, as wide as the range on it: 8pt, the range, 4pt, the
  // chevron, 8pt; its right edge on the content's.
  /** As wide as the range the menu shows: the one picked, while its figures are still being read. */
  const pickerWidth = $derived(8 + width(rangeOptions.find((o) => o.checked)?.label ?? data.rangeLabel, font(11)) + 4 + 7 + 8);
  const pickerX = $derived(inner - 12 - pickerWidth);
  /** The menu bar's star, drawn in the heading's colour. */
  const mark = markSvg.replace(/fill="#000000"/g, 'fill="currentColor"').replace(/width="24" height="24"/, 'width="15" height="15"');
  const firstButton = $derived(pickerX - 6 - 30);
  const leftmost = $derived(firstButton - 2 * 32);
  const chosen = $derived(rangeOptions.find((o) => o.checked)?.action?.[1] ?? data.rangeKey);


  function pickRange(key: string) {
    const option = rangeOptions.find((o) => o.action?.[1] === key);
    if (option?.action && key !== chosen) onrange(option.action);
  }

  const glowColour = $derived(blend(labelColour(panel.dark, 0.85), totalColor(panel.dark), summaryGlow * 0.85));
  function cardHeight(p: Dashboard['providers'][number]) {
    if (collapsible && !expanded.has(p.id)) return 52;
    const clients = p.available && p.clients?.length ? 16 : 0;
    return p.available ? 52 + clients + Math.max(1, p.models.length) * 18 + 8 : 52;
  }
</script>

<div class="dashboard" style:width="{inner}px" style:height="{total}px">
  <div class="summary" style:height="{summaryHeight}px">
    <!-- Pinned, the header's free space moves the window, as a title bar would. -->
    <div class="grip" class:movable={panel.pinned} onmousedown={grabWindow} aria-hidden="true"></div>
    <h1 class="heading" style:width="{(readonly ? firstButton : leftmost) - 6 - 12}px"><span class="brand" aria-hidden="true">{@html mark}</span>Starmeter</h1>
    <div class="buttons">
      <!-- The pin first, then settings and share; on a Mac the symbols are the system's own. A phone shares, and
           neither pins nor changes settings. -->
      {#if !readonly}
      <span class="slot" style:left="{leftmost}px">
        <HintButton icon={pinned ? 'pin.fill' : 'pin'} system hint={pinned ? t('取消钉住') : t('钉住，每 {n} 秒刷新', { n: pinInterval })} label={pinned ? t('取消钉住面板') : t('钉住面板')}
          onclick={onpin} on={pinned} />
      </span>
      <span class="slot" style:left="{firstButton - 32}px"><HintButton icon="gearshape" system hint={t('主页模块与设置')} label={t('主页模块与设置')} onclick={onsettings} /></span>
      {/if}
      <span class="slot" style:left="{firstButton}px">
        <HintButton icon="square.and.arrow.up" system hint={t('分享用量图')} label={t('分享用量图')} onclick={onshare} />
      </span>
      <span class="picker" class:reading style:left="{pickerX}px" style:width="{pickerWidth}px">
        <Dropdown label={t('统计范围')} value={chosen} options={rangeOptions.map((o) => ({ value: o.action?.[1] ?? '', label: o.label }))} onpick={pickRange} />
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
        <!-- Centred on the name's middle (Chinese and capitals, 8.5pt type in a line from 90: 95.15), not its top. -->
        <span class="swatch" style:left="{left + cell.x}px" style:top="{95.15 - (i === 0 ? 1.5 : 1) + cell.row * 17}px" style:height="{i === 0 ? 3 : 2}px" style:background={legendColours[i]}></span>
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
    <Heatmap heatmap={data.heatmap} {column} top={summaryHeight} {right} current={data.rangeKey !== 'yesterday'} max={dot} />
  {/if}
  <div class="cards" style:top="{summaryHeight + heatmapHeight + harnessHeader}px">
    {#if groupingHeight}
      <GroupTabs options={tabOptions} chosen={showingSessions ? 'sessions' : data.grouping ?? 'tool'} note={showingSessions ? null : data.groupNote}
        onchoose={(key) => { if (key === 'sessions') onview?.('sessions'); else { onview?.('cards'); if (key !== (data.grouping ?? 'tool')) ongroup(key); } }} />
    {/if}
    {#if showingSessions && sessions}
      <div class="sessions" bind:this={sessionsBox}>
        <SessionList entry={sessions} {busy} onopen={(e) => onopen?.(e)} onchoose={(e) => onchoose?.(e)} onfit={(h) => (sessionsFit = h)} />
      </div>
    {:else}
      {#if !cardList.length && data.providers.length}
        <div class="no-calls">{t('此时段暂无调用')}</div>
      {/if}
      {#each listed as p (p.id)}
        <ProviderCard {p} {collapsible} expanded={expanded.has(p.id)} glow={glows[p.id] ?? 0} ontoggle={() => ontoggle(p.id, !expanded.has(p.id))}
          onopen={isGrouped(p) ? undefined : () => onopentool(p.id)} />
      {/each}
      {#if unlisted}
        <MoreButton label={t(data.grouping === 'project' ? '查看更多 {n} 个项目' : '查看更多 {n} 个模型', { n: unlisted })}
          onclick={() => (allOf = data.grouping ?? null)} />
      {/if}
    {/if}
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
  .grip.movable { cursor: grab; }
  .grip.movable:active { cursor: grabbing; }
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
    display: flex;
    align-items: center;
    gap: 5px;
  }
  .brand {
    flex: none;
    display: flex;
    width: 15px;
    height: 15px;
  }
  .buttons { position: absolute; left: 0; top: 0; width: 100%; height: 40px; pointer-events: none; }
  .buttons > * { pointer-events: auto; }
  .slot { top: 6px; }
  .picker {
    top: 6px;
    height: 28px;
    display: flex;
    justify-content: flex-end;
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
  /* A range picked and still being read: its name a little faint until its figures arrive. */
  .picker.reading :global(.current) { opacity: 0.55; transition: opacity 0.2s; }
</style>
