<script lang="ts">
  // The top of 手机版: is the AI working, how hard, and what has today cost. Working, the line says so with a beating
  // light and how many sessions are running; the headline is how fast the models are writing (Token/s, as the pinned
  // panel reads it), today's estimate under it rolling up as each request lands with what it added rising off it; the
  // live curve below. Resting, the line says since when; the headline is today's estimate, the curve today's trend, and
  // the page is still. Waking, the headline turns from the estimate to the speed and the trend gives way to the curve.
  import { currency, grouped, tokens } from '../lib/format';
  import { t } from '../lib/i18n.svelte';
  import { rateText } from '../lib/live';
  import type { PhoneLive } from '../lib/phoneLive.svelte';
  import { panel } from '../lib/ui.svelte';
  import type { Dashboard } from '../lib/types';
  import StageChart from './StageChart.svelte';
  import StageLiveChart from './StageLiveChart.svelte';
  import StageNumber from './StageNumber.svelte';

  let { live, today, now, chartHeight, intro = true }: {
    live: PhoneLive;
    /** Today's figures when the live view has none yet (the homepage's, from the snapshot). */
    today: Dashboard | null;
    /** The clock, ticking each second. */
    now: number;
    /** The curve's height; null: as tall as the room left under the words (a phone on its side). */
    chartHeight: number | null;
    intro?: boolean;
  } = $props();

  let room = $state(0);
  const curveHeight = $derived(chartHeight ?? Math.max(80, room));

  const working = $derived(live.state === 'working');
  const running = $derived(live.sessions.filter((s) => s.running).length);
  const data = $derived(live.dashboard ?? today);
  /** The figures' range: today on the app; on SwiftBar's panel, the range the computer shows, said as such. */
  const ofToday = $derived(!data || data.rangeKey === 'today');
  const rangeWord = $derived(ofToday ? t('今天') : data!.rangeLabel);
  const rate = $derived.by(() => { void live.version; return live.rate(now) ?? 0; });
  /** Tokens added over the last minute, from the samples. */
  const minute = $derived.by(() => {
    void live.version;
    const samples = live.feed.samples, last = samples.at(-1);
    if (!last) return 0;
    const from = [...samples].reverse().find((s) => s.at <= last.at - 60_000) ?? samples[0];
    return Math.max(0, last.total - from.total);
  });
  const amount = $derived(live.cost === null ? '—' : currency.symbol + grouped(live.cost));
  const since = $derived(live.lastActive === null ? null : Math.max(0, Math.round((now - live.lastActive) / 60_000)));
  const stateLine = $derived(working
    ? (running ? t('正在工作 · {n} 个会话运行中', { n: running }) : t('正在工作'))
    : since === null ? t('休息中') : since < 1 ? t('休息中 · 刚刚还在工作') : t('休息中 · 上次活动 {n} 分钟前', { n: since }));
  /** What the last reading added to today's estimate, rising off it for a moment. */
  const added = $derived(live.costAdded && now - live.costAdded.at < 2500 ? live.costAdded : null);
</script>

<div class="monitor" class:working class:trouble={live.trouble} class:fill={chartHeight === null}>
  <div class="state" role="status">
    <span class="beat" aria-hidden="true"></span>
    <span class="state-text">{stateLine}</span>
  </div>
  <div class="headline">
    {#key working}
      <div class="figure-block" class:enter={!panel.reduceMotion && live.woke > 0}>
        {#if working}
          <span class="big num" data-figure="rate"><StageNumber value={rateText(rate)} count={intro} /></span>
          <span class="unit">{t('输出 Token/s')}</span>
        {:else}
          <span class="big num" data-figure="today"><StageNumber value={amount} count={intro} /></span>
          <span class="unit">{ofToday ? t('今天 · API 估算') : t('{range} · API 估算', { range: rangeWord })}</span>
        {/if}
      </div>
    {/key}
  </div>
  <div class="ticker">
    {#if working}
      <span class="tick"><span class="tick-label">{rangeWord}</span> <span class="tick-value num" data-figure="today"><StageNumber value={amount} /></span>
        {#if added}{#key added.at}<span class="added num">+{currency.symbol}{grouped(added.amount)}</span>{/key}{/if}</span>
      <span class="tick"><span class="tick-label">{t('近 1 分钟')}</span> <span class="tick-value num"><StageNumber value={'+' + tokens(minute)} /></span></span>
    {:else if data}
      <span class="tick"><span class="tick-label">{rangeWord}</span> <span class="tick-value num"><StageNumber value={tokens(data.providers.reduce((s, p) => s + (p.available && p.summaryIncluded !== false ? (p.counted?.tokens ?? p.tokens) : 0), 0))} /></span><span class="tick-label">Token</span></span>
    {/if}
  </div>
  <div class="curve" bind:clientHeight={room}>
    {#if working}
      <StageLiveChart {live} height={curveHeight} />
    {:else if data}
      <div class="resting-chart"><StageChart {data} height={curveHeight} draw={intro} /></div>
    {/if}
  </div>
</div>

<style>
  .monitor { position: relative; display: flex; flex-direction: column; min-height: 0; }
  .monitor.fill { height: 100%; }
  /* Filling, the curve's box is the room itself, and what is drawn in it never pushes it taller. */
  .fill .curve { overflow: hidden; }
  .fill .curve > :global(*) { position: absolute; left: 0; right: 0; top: 0; }
  .state { display: flex; align-items: center; gap: 9px; font-size: 14px; font-weight: 600; color: var(--ink); min-height: 24px; }
  .beat { position: relative; flex: none; width: 10px; height: 10px; border-radius: 50%; background: var(--ink-3); }
  .working .beat { background: var(--good); }
  /* The working light beats: a ring goes out from it, again and again. */
  .working .beat::after {
    content: '';
    position: absolute;
    inset: -1px;
    border-radius: 50%;
    border: 2px solid var(--good);
    animation: beat 1.4s cubic-bezier(0.2, 0.6, 0.3, 1) infinite;
  }
  @keyframes beat { from { transform: scale(1); opacity: 0.9; } to { transform: scale(2.8); opacity: 0; } }
  .trouble .beat { background: var(--warn); }
  .trouble .beat::after { display: none; }
  .state-text { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .monitor:not(.working) .state-text { color: var(--ink-2); }
  .headline { display: grid; margin-top: 6px; }
  .figure-block { grid-area: 1 / 1; display: flex; flex-direction: column; align-items: flex-start; }
  .figure-block.enter { animation: turn-in 0.7s cubic-bezier(0.22, 1, 0.36, 1); }
  @keyframes turn-in { from { opacity: 0; transform: translateY(14px) scale(0.96); filter: blur(4px); } }
  .big {
    font-family: var(--rounded);
    font-size: var(--big, 64px);
    font-weight: 600;
    letter-spacing: -0.02em;
    line-height: 1.1;
    color: #F5F8FF;
    filter: drop-shadow(0 0 16px rgba(196, 216, 255, 0.5));
    margin-left: -2px;
  }
  :global(.light) .big { color: #0E1D4C; filter: drop-shadow(0 0 14px rgba(120, 140, 255, 0.28)); }
  .working .big { color: #F2FFF8; filter: drop-shadow(0 0 18px rgba(110, 255, 190, 0.38)); }
  :global(.light) .working .big { color: #0B3B2E; filter: drop-shadow(0 0 14px rgba(40, 180, 120, 0.25)); }
  .unit { font-size: 12px; color: var(--ink-2); margin-top: 2px; }
  .ticker { display: flex; flex-wrap: wrap; gap: 6px 18px; margin: 6px 0 10px; font-size: 13px; color: var(--ink-2); min-height: 20px; }
  .tick { position: relative; display: inline-flex; align-items: baseline; gap: 6px; }
  .tick-value { font-family: var(--rounded); font-size: var(--fig-m, 17px); font-weight: 600; color: var(--ink); }
  .added {
    position: absolute;
    left: 100%;
    bottom: 55%;
    margin-left: 6px;
    white-space: nowrap;
    font-family: var(--rounded);
    font-size: 12px;
    font-weight: 600;
    color: var(--good);
    animation: rise 2.4s ease-out forwards;
    pointer-events: none;
  }
  @keyframes rise { from { opacity: 0; transform: translateY(8px); } 15% { opacity: 1; } to { opacity: 0; transform: translateY(-18px); } }
  .curve { position: relative; flex: 1; min-height: 0; }
  .resting-chart { opacity: 0.78; }
</style>
