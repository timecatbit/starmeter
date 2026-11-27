<script lang="ts">
  // The homepage's top: the heading and its buttons, Summary's three figures, the legend and trend (or the live chart
  // while pinned), the heatmap, the grouping tabs and the Harness cards, or 会话's session cards.
  import { byUsage } from '../lib/order';
  import { blend, label as labelColour, toolColour, summaryColour } from '../lib/colors';
  import { pen } from '../lib/sketchPalette';
  import { style } from '../lib/style.svelte';
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
  import { onMount } from 'svelte';
  import PanelLive from './PanelLive.svelte';
  import ProviderCard from './ProviderCard.svelte';
  import SessionList from './SessionList.svelte';
  import MoreButton from './MoreButton.svelte';
  import { closeList, listOpen, openList } from '../lib/expanded.svelte';
  import SummaryChart from './SummaryChart.svelte';

  let { data, rangeOptions, reveal, glows, summaryGlow, live, leaving = null, onleft, liveStamp, pinned, pinInterval, onrange,
    onshare, onphone, phoneOn = false, onpin, onsettings, ongroup, ontoggle, onopentool, onsize, onfloor, sessions = null, view = 'cards', onview, busy = null, onopen, onchoose,
    readonly = false, reading = false, grouping = null, regrouping = false, onprefetch, updateMark = null, updateSaid = '', onupdates }: {
    data: Dashboard; rangeOptions: Entry[]; reveal: number; glows: Record<string, number>; summaryGlow: number; live: LiveFeed | null;
    leaving?: number | null; onleft?: () => void;
    liveStamp: string | null; pinned: boolean; pinInterval: number;
    onrange: (action: string[]) => void; onshare: () => void; onpin: () => void;
    /** 在手机上看, beside 分享用量图; `phoneOn`: a way of it on, a green dot on its icon. */
    onphone?: () => void; phoneOn?: boolean;
    onsettings: () => void; ongroup: (key: string) => void; ontoggle: (id: string, expanded: boolean) => void;
    /** On a phone (在手机上看): no pin, settings or 在手机上看, the share and the range alone in the header. */
    readonly?: boolean;
    /** A range picked and still being read: the menu shows it, the figures are still the last range's. */
    /** A range picked and not read through yet: what is on screen is the old reading, dimmed, until the new one is in. */
    reading?: boolean;
    /** The grouping last picked, while its cards are on their way: its tab is the lit one, and the one a click is
     *  measured against (a click back to the grouping on screen is a pick too). */
    grouping?: string | null;
    /** The cards on screen are another grouping's than the one picked: held, dimmed and out of reach, till it comes. */
    regrouping?: boolean;
    onprefetch?: () => void;
    /** A newer version waiting: a pill beside Starmeter until looked at, then a dot (updateHints.svelte.ts). */
    updateMark?: { count: number; own: string | null; fresh: boolean } | null;
    /** What the mark says on hover and to assistive technology. */
    updateSaid?: string;
    onupdates?: () => void;
    /** How tall a pinned panel that fits the homepage is: down to the end of the third card or session. */
    onsize?: (height: number) => void;
    /** The least a pinned panel shows of the homepage: down to the end of the heatmap. */
    onfloor?: (height: number) => void;
    onopentool: (id: string) => void;
    /** The 会话 module's row, when it is on the homepage: its sessions are the fourth choice beside the groupings. */
    sessions?: Entry | null; view?: 'cards' | 'sessions' | 'live'; onview?: (view: 'cards' | 'sessions' | 'live') => void;
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
  /** The grouping the tabs show and a click is measured against: the one last picked while it is on its way. */
  const wantedGrouping = $derived(grouping ?? data.grouping ?? 'tool');
  // 实时 last: what the AI is doing now (PanelLive), where pinning turns.
  const tabOptions = $derived([...(data.providers.length ? data.groupings ?? [] : []), ...(sessions ? [{ key: 'sessions', label: t('会话') }] : []),
    { key: 'live', label: t('实时') }]);
  // 会话 when chosen, and always when there are no cards to choose instead (its row has left the other rows for here).
  const showingLive = $derived(view === 'live');
  const showingSessions = $derived(!showingLive && !!sessions && (view === 'sessions' || !data.providers.length));
  /** A grouping's note (项目: 按客户端自己的记录), on a line under the tabs. */
  const groupNote = $derived(showingLive || showingSessions || regrouping ? null : data.groupNote);
  const groupingHeight = $derived(tabOptions.length < 2 ? 0 : groupNote ? 44 : 30);
  let sessionsHeight = $state(0);
  // 实时's height as it is drawn (its sessions and what happened come and go), taken a frame after it changes; one
  // coming in while a pointer is down waits till the click is done, so the rows under it don't move under the press.
  let liveHeight = $state(0);
  let pressed = false, heldHeight: number | null = null;
  function liveHeightIs(height: number) { if (pressed) heldHeight = height; else liveHeight = height; }
  onMount(() => {
    const down = () => (pressed = true);
    const up = () => {
      pressed = false;
      setTimeout(() => { if (!pressed && heldHeight !== null) { liveHeight = heldHeight; heldHeight = null; } }, 0);
    };
    window.addEventListener('pointerdown', down, true);
    window.addEventListener('pointerup', up, true);
    window.addEventListener('pointercancel', up, true);
    return () => { window.removeEventListener('pointerdown', down, true); window.removeEventListener('pointerup', up, true); window.removeEventListener('pointercancel', up, true); };
  });
  let liveBox: HTMLDivElement | undefined = $state();
  /** Pinned, 实时 fits down to the end of its third happening (用户：pin 住的时候整个画面高度，显示三个就好); the list
   *  itself is whole (12), the rest a scroll away. All of it while it has fewer. */
  let liveFit = $state(0);
  $effect(() => {
    const box = liveBox;
    if (!box) return;
    let frame = 0;
    const measure = () => {
      liveHeightIs(box.offsetHeight);
      const third = box.querySelectorAll<HTMLElement>('.feed .event')[2];
      // Where it is laid out, not where it is drawn: a new happening slides the others down for a moment (flip), and
      // the box's size doesn't change when that ends, so a drawn position read mid-slide would stay. The pinned window
      // is 10 taller than this under it and shows all but its last 8 (App.svelte), so 2 less: what is seen ends just at
      // the third's foot.
      let foot = third ? third.offsetHeight : 0;
      for (let e: HTMLElement | null = third ?? null; e && e !== box; e = e.offsetParent as HTMLElement | null) foot += e.offsetTop;
      liveFit = third ? foot - 2 : box.offsetHeight;
    };
    const observer = new ResizeObserver(() => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(measure);
    });
    observer.observe(box);
    measure();
    return () => { observer.disconnect(); cancelAnimationFrame(frame); };
  });
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
  // the ones the user picked. Once shown whole, a grouping stays whole (expanded.svelte.ts): coming back to it, from a
  // page or another tab or a reopened panel, doesn't fold it up again.
  const SHOWN = 5;
  const MORE = 34;
  const listed = $derived(!data.cards || listOpen(data.grouping ?? '') ? cardList : cardList.slice(0, SHOWN));
  const unlisted = $derived(cardList.length - listed.length);
  /** The whole list shown by 查看更多: 收起 at its end folds it back. */
  const folds = $derived(!!data.cards && !unlisted && cardList.length > SHOWN);
  const collapsible = $derived(data.allowCollapse === true && !data.cards);
  const expanded = $derived(new Set(data.expandedProviders ?? []));
  const listHeight = $derived(showingLive ? liveHeight : showingSessions ? sessionsHeight
    : Math.max(cardList.length === 0 && data.providers.length ? 24 : 0, listed.reduce((s, p) => s + cardHeight(p), 0) + (unlisted || folds ? MORE : 0))
      + (data.providers.length ? 4 : 0));
  const total = $derived(summaryHeight + heatmapHeight + harnessHeader + groupingHeight + listHeight);
  // A pinned panel fits down to the end of the third card or session (the user makes it taller at its edges).
  const FIT = 3;
  let sessionsFit = $state(0);
  // 实时 fits down to its third happening: its sessions and the latest three of what happened.
  const fitList = $derived(showingLive ? Math.min(liveHeight, liveFit) : showingSessions ? sessionsFit
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
  /** 手绘's three figures in the 手绘 picture's pens: blue, green, violet. */
  const figurePens = $derived(style.current === 'sketch' ? (['blue', 'green', 'violet'] as const).map((name) => pen(name, panel.dark)) : null);
  const legendColours = $derived([summaryColour(panel.dark), ...counted.map((p) => toolColour(p.id, panel.dark))]);
  const legendCells = $derived(legendLayout(legend, right - left).flatMap((row, r) => row.map((cell) => ({ ...cell, row: r }))));
  // 多台电脑合并: the other computers counted in, beside the update time (this one included).
  const note = $derived(reading ? t('正在读取…') : [data.chartStatus, liveStamp ?? data.updated ?? '', data.devices ? t('含 {n} 台电脑', { n: data.devices + 1 }) : '']
    .filter(Boolean).join(' · '));

  // Header: the star and Starmeter, then share, the pin and the gear leftward from the range picker, 2pt apart. (No
  // refresh: opening reads what is due, pinning reads every few seconds, and each source's own page refreshes it; auto-
  // detect lives in 通用 and the star's menu.)
  // The range picker is drawn bare like the icons beside it, as wide as the range on it: 8pt, the range, 4pt, the
  // chevron, 8pt; its right edge on the content's.
  /** As wide as the range the menu shows: the one picked, while its figures are still being read. */
  /** The range as the closed menu shows it: in English the rolling ranges without their "Last" (24 hours, 7 days, 30
   *  days), so the header keeps room for Starmeter beside four buttons; the list says them in full. */
  const brief = (label: string) => label.replace(/^Last (?=\d)/, '');
  const pickerWidth = $derived(8 + width(brief(rangeOptions.find((o) => o.checked)?.label ?? data.rangeLabel), font(11)) + 4 + 7 + 8);
  const pickerX = $derived(inner - 12 - pickerWidth);
  /** The menu bar's star, drawn in the heading's colour. */
  const mark = markSvg.replace(/fill="#000000"/g, 'fill="currentColor"').replace(/width="24" height="24"/, 'width="15" height="15"');
  /** The four buttons 28pt apart, each as wide (the pin, settings, 在手机上看, share), the share 4pt before the range. */
  const SLOT = 28;
  const firstButton = $derived(pickerX - 4 - SLOT);
  const leftmost = $derived(firstButton - 3 * SLOT);
  const chosen = $derived(rangeOptions.find((o) => o.checked)?.action?.[1] ?? data.rangeKey);


  let liveRunning = $state<string[]>([]);
  $effect(() => { if (!showingLive) liveRunning = []; });
  /** Tools with at least one running session: their live lines stay continuous to 现在 even while resting at 0. */
  const runningTools = $derived.by(() => {
    const ids = new Set<string>(liveRunning);
    for (const c of sessions?.children ?? []) {
      if (c.statusLight === 'new' && c.tool) ids.add(c.tool);
    }
    return [...ids];
  });

  function pickRange(key: string) {
    const option = rangeOptions.find((o) => o.action?.[1] === key);
    if (option?.action && key !== chosen) onrange(option.action);
  }

  const glowColour = $derived(blend(labelColour(panel.dark, 0.85), summaryColour(panel.dark), summaryGlow * 0.85));
  function cardHeight(p: Dashboard['providers'][number]) {
    if (collapsible && !expanded.has(p.id)) return 52;
    const clients = p.available && p.clients?.length ? 16 : 0;
    return p.available ? 52 + clients + Math.max(1, p.models.length) * 18 + 8 : 52;
  }
</script>

<div class="dashboard" class:reading style:width="{inner}px" style:height="{total}px">
  <div class="summary" style:height="{summaryHeight}px">
    <!-- Pinned, the header's free space moves the window, as a title bar would. -->
    <div class="grip" class:movable={panel.pinned} onmousedown={grabWindow} aria-hidden="true"></div>
    <h1 class="heading" style:width="{(readonly ? firstButton : leftmost) - 6 - 12}px"><span class="brand" aria-hidden="true">{@html mark}</span>Starmeter{#if updateMark}<button
        class="update-mark pressable" class:fresh={updateMark.fresh} onclick={onupdates}
        aria-label={updateSaid} title={updateSaid}>{#if updateMark.fresh}<svg
          class="ring" viewBox="0 0 16 16" width="14" height="14" aria-hidden="true"><circle cx="8" cy="8" r="6.6" fill="none" stroke="currentColor" stroke-width="1.4" /><path
          d="M8 4.6v6.2M5.3 8.3 8 11 10.7 8.3" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg>{:else}<span
          class="dot"></span>{/if}</button>{/if}</h1>
    <div class="buttons">
      <!-- The pin first, then settings, 在手机上看 and share; on a Mac the symbols are the system's own. A phone shares,
           and neither pins, changes settings nor shows itself a code. -->
      {#if !readonly}
      <span class="slot" style:left="{leftmost}px">
        <HintButton icon={pinned ? 'pin.fill' : 'pin'} system width={SLOT} hint={pinned ? t('取消钉住') : t('钉住，每 {n} 秒刷新', { n: pinInterval })} label={pinned ? t('取消钉住面板') : t('钉住面板')}
          onclick={onpin} on={pinned} />
      </span>
      <span class="slot" style:left="{firstButton - 2 * SLOT}px"><HintButton icon="gearshape" system width={SLOT} hint={t('主页模块与设置')} label={t('主页模块与设置')} onclick={onsettings} /></span>
      <span class="slot" style:left="{firstButton - SLOT}px">
        <HintButton icon="iphone" system width={SLOT} hint={t('在手机上看')} label={t('在手机上看')} onclick={() => onphone?.()} />
        <!-- On (同一个网络 or the public link): a green dot at its corner, as a module set up has after its name. -->
        {#if phoneOn}<span class="phone-on" aria-hidden="true"></span>{/if}
      </span>
      {/if}
      <span class="slot" style:left="{firstButton}px">
        <HintButton icon="square.and.arrow.up" system width={SLOT} hint={t('分享用量图')} label={t('分享用量图')} onclick={onshare} />
      </span>
      <span class="picker" class:reading style:left="{pickerX}px" style:width="{pickerWidth}px">
        <Dropdown label={t('统计范围')} value={chosen} options={rangeOptions.map((o) => ({ value: o.action?.[1] ?? '', label: o.label, short: brief(o.label) }))} onpick={pickRange} onopen={onprefetch} />
      </span>
    </div>
    {#if showSummary}
      {#each [0, 1, 2] as i (i)}
        {@const x = left + third * i + 2 * i}
        {@const align = (['left', 'center', 'right'] as const)[i]}
        <span class="figure-label" style:left="{x}px" style:width="{third - 4}px" style:text-align={align}>{labels[i]}</span>
        <!-- 手绘: each figure in its own pen, the estimate over a highlighter swash. -->
        <span class="figure" class:first={i === 0} style:left="{x}px" style:--pen={figurePens?.[i]}><Figure value={values[i]} size={22} room={third - 4} {align}
          color={figurePens?.[i] ?? glowColour} /></span>
      {/each}
      {#each legendCells as cell, i (i)}
        <!-- Centred on the name's middle (Chinese and capitals, 8.5pt type in a line from 90: 95.15), not its top. -->
        <span class="swatch" style:left="{left + cell.x}px" style:top="{95.15 - (i === 0 ? 1.5 : 1) + cell.row * 17}px" style:height="{i === 0 ? 3 : 2}px" style:background={legendColours[i]}></span>
        <span class="legend" style:left="{left + cell.x + 16}px" style:top="{90 + cell.row * 17}px" style:width="{cell.w - 16}px">{legend[i]}</span>
      {/each}
      <div class="trend" style:transform="translateY({extra}px)">
        {#if live}
          <LiveChart feed={live} {column} {counted} {data} {note} {leaving} {onleft} {runningTools} width={right - left} />
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
      <GroupTabs options={tabOptions} chosen={showingLive ? 'live' : showingSessions ? 'sessions' : wantedGrouping} note={groupNote}
        onchoose={(key) => { if (key === 'live' || key === 'sessions') onview?.(key); else { onview?.('cards'); if (key !== wantedGrouping) ongroup(key); } }} />
    {/if}
    {#if showingLive}
      <div class="live-room" bind:this={liveBox}><PanelLive onrunning={(ids) => (liveRunning = ids)} /></div>
    {:else if showingSessions && sessions}
      <div class="sessions" bind:this={sessionsBox}>
        <SessionList entry={sessions} {busy} onopen={(e) => onopen?.(e)} onchoose={(e) => onchoose?.(e)} onfit={(h) => (sessionsFit = h)} />
      </div>
    {:else}
      <div class="list" class:held={regrouping} inert={regrouping} aria-busy={regrouping}>
        {#if !cardList.length && data.providers.length}
          <div class="no-calls">{t('此时段暂无调用')}</div>
        {/if}
        {#each listed as p (p.id)}
          <ProviderCard {p} {collapsible} expanded={expanded.has(p.id)} glow={glows[p.id] ?? 0} ontoggle={() => ontoggle(p.id, !expanded.has(p.id))}
            onopen={isGrouped(p) ? undefined : () => onopentool(p.id)} />
        {/each}
        {#if unlisted || folds}
          <MoreButton open={folds} label={folds ? t('收起') : t(data.grouping === 'project' ? '查看更多 {n} 个项目' : '查看更多 {n} 个模型', { n: unlisted })}
            onclick={() => (folds ? closeList : openList)(data.grouping ?? '')} />
        {/if}
      </div>
    {/if}
  </div>
</div>

<style>
  /* Where 实时's rows are measured from (offsetTop: the pinned fit, laid out, not as drawn mid-slide). */
  .live-room { position: relative; }
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
    overflow: clip visible;
    text-overflow: ellipsis;
    display: flex;
    align-items: center;
    gap: 5px;
  }
  /* The update mark: a ring with a down arrow in it, no words (用户：画个圈弄个下箭头在里面就好了，不用字), until looked
     at, then a dot; what is new is said on hover and to assistive technology. Nothing moves or blinks. */
  .update-mark {
    --press-scale: 0.92;
    flex: none;
    pointer-events: auto;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 2px;
    height: 16px;
    min-width: 16px;
    padding: 0;
    margin-left: 1px;
    border: none;
    border-radius: 8px;
    background: transparent;
    color: var(--accent);
    font: inherit;
    font-size: 10px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    cursor: pointer;
  }
  .update-mark .ring { display: block; }
  .update-mark:hover { background: color-mix(in srgb, var(--accent) 22%, transparent); }
  .update-mark .dot { width: 6px; height: 6px; border-radius: 3px; background: var(--accent); }
  .brand {
    flex: none;
    display: flex;
    width: 15px;
    height: 15px;
  }
  .buttons { position: absolute; left: 0; top: 0; width: 100%; height: 40px; pointer-events: none; }
  .buttons > * { pointer-events: auto; }
  .slot { top: 6px; }
  .phone-on {
    position: absolute; right: 6px; top: 5px; width: 6px; height: 6px; border-radius: 50%; background: var(--green);
    box-shadow: 0 0 0 1.5px var(--solid); pointer-events: none;
  }
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
    overflow: clip visible;
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
    overflow: clip visible;
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
  /* A range picked and not read through yet: the old figures stay where they are (nothing jumps), dimmed, and 正在读取…
     says why; the new reading takes their place whole, never a source at a time (用户：切换时间段时有机会看到旧的信息). */
  /* Another grouping picked: the cards on screen (the last grouping's) stay where they are, dimmed and out of reach (no
     detail opened, nothing folded), until the picked one's take their place (GROUP-FLASH). */
  .list { transition: opacity 0.18s ease-out; }
  .list.held { opacity: 0.32; pointer-events: none; }
  .figure, .trend, .cards, .dashboard > :global(.heatmap) { transition: opacity 0.18s ease-out; }
  .dashboard.reading .figure, .dashboard.reading .trend, .dashboard.reading .cards, .dashboard.reading > :global(.heatmap) {
    opacity: 0.32;
  }
</style>
