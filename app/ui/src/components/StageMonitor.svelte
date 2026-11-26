<script lang="ts">
  // The top of 网页版: is the AI working, how hard, and what has today cost. Working, the line says so with a beating
  // light and how many sessions are running; the headline is how fast the models are writing (Token/s, as the pinned
  // panel reads it), today's estimate under it rolling up as each request lands with what it added rising off it; the
  // live curve below. Resting, the line says since when; the headline is today's estimate, and the live curve stays,
  // lying flat: today's trend is the statistics' just below (用户：有点信息重复), and the card keeps its size, the curve
  // rising in its place when work comes (no chart taking another's place). Waking, the headline turns to the speed.
  import { currency, grouped, tokens } from '../lib/format';
  import { t } from '../lib/i18n.svelte';
  import { rateText } from '../lib/live';
  import { estimate, type PhoneLive } from '../lib/phoneLive.svelte';
  import { panel } from '../lib/ui.svelte';
  import type { Dashboard } from '../lib/types';
  import StageLiveChart from './StageLiveChart.svelte';
  import StageNumber from './StageNumber.svelte';

  let { live, today, now, chartHeight, least = 64, intro = true }: {
    live: PhoneLive;
    /** Today's figures when the live view has none yet (the homepage's, from the snapshot). */
    today: Dashboard | null;
    /** The clock, ticking each second. */
    now: number;
    /** The curve's height; null: as tall as the room left under the words (a phone on its side). */
    chartHeight: number | null;
    /** Filling, the least the curve is given (more on a computer's screen). */
    least?: number;
    intro?: boolean;
  } = $props();

  const working = $derived(live.state === 'working');
  let room = $state(0);
  /** The live chart's legend, over its curve: the curve gets its least under it (on its side, two rows of legend left the
   *  curve 10 points and its end's dot was cut). */
  let legend = $state(0);
  /** Filling, the least the curve takes: the live chart's legend and its plot of at least 40 points between its 18
   *  over and 20 under (its scale's words, its time line), working or resting alike. */
  const leastNow = $derived(legend + Math.max(least, 78));
  const curveHeight = $derived(chartHeight ?? Math.max(leastNow, room));

  const running = $derived(live.sessions.filter((s) => s.running).length);
  const data = $derived(live.dashboard ?? today);
  /** The figures' range: today on the app; on SwiftBar's panel, the range the computer shows, said as such. */
  const ofToday = $derived(!data || data.rangeKey === 'today');
  const rangeWord = $derived(ofToday ? t('今天') : data!.rangeLabel);
  /** The speed, and the last minute's tokens: — until the readings cover enough time (a page just opened with no
   *  last minutes from the computer), not 0 while the sessions run. */
  const rate = $derived.by(() => { void live.version; return live.rate(now); });
  const ready = $derived.by(() => { void live.version; return live.ready(); });
  /** Tokens added over the last minute, from the samples. */
  const minute = $derived.by(() => {
    void live.version;
    const samples = live.feed.samples, last = samples.at(-1);
    if (!last) return 0;
    const from = [...samples].reverse().find((s) => s.at <= last.at - 60_000) ?? samples[0];
    return Math.max(0, last.total - from.total);
  });
  /** Today's estimate: the live view's once it has read the computer, the homepage's figures for today until then. */
  const cost = $derived(live.dashboard ? live.cost : today ? estimate(today) : null);
  // Written again at each reading: the computer's currency comes with it (currency is no state of its own).
  const amount = $derived.by(() => { void live.version; return cost === null ? '—' : currency.symbol + grouped(cost); });
  const since = $derived(live.lastActive === null ? null : Math.max(0, Math.round((now - live.lastActive) / 60_000)));
  // Before the first answer: as the computer said when it gave the page (or the page last saw it), else 正在读取….
  const runningNow = $derived(live.answered ? running : live.runningBefore);
  const stateLine = $derived(!live.known ? t('正在读取…') : working
    ? (runningNow ? t('正在工作 · {n} 个会话运行中', { n: runningNow }) : t('正在工作'))
    : since === null ? t('休息中') : since < 1 ? t('休息中 · 刚刚还在工作') : t('休息中 · 上次活动 {n} 分钟前', { n: since }));
  /** What the last reading added to today's estimate, rising off it for a moment. */
  const added = $derived(live.costAdded && now - live.costAdded.at < 2500 ? live.costAdded : null);
</script>

<div class="monitor" class:working class:trouble={live.trouble} class:filling={chartHeight === null} style:--least="{leastNow}px">
  <div class="state" role="status">
    <span class="beat" aria-hidden="true"></span>
    <span class="state-text">{stateLine}</span>
  </div>
  <div class="headline">
    {#key working}
      <div class="figure-block" class:enter={!panel.reduceMotion && live.woke > 0}>
        {#if working}
          <span class="big num" class:unknown={rate === null} data-figure="rate" data-tap="hop"><StageNumber value={rate === null ? '—' : rateText(rate)} count={intro} /></span>
          <span class="unit">{t('输出 Token/s')}{#if rate === null}<span class="reading">· {t('正在读取…')}</span>{/if}</span>
        {:else}
          <span class="big num" data-figure="today" data-tap="hop"><StageNumber value={amount} count={intro} /></span>
          <span class="unit">{ofToday ? t('今天 · API 估算') : t('{range} · API 估算', { range: rangeWord })}</span>
        {/if}
      </div>
    {/key}
  </div>
  <div class="ticker">
    {#if working}
      <span class="tick"><span class="tick-label">{rangeWord}</span> <span class="tick-value num" data-figure="today"><StageNumber value={amount} /></span>
        {#if added}{#key added.at}<span class="added num" data-passing>+{currency.symbol}{grouped(added.amount)}</span>{/key}{/if}</span>
      <span class="tick"><span class="tick-label">{t('近 1 分钟')}</span> <span class="tick-value num"><StageNumber value={ready ? '+' + tokens(minute) : '—'} /></span></span>
    {:else if data}
      <span class="tick"><span class="tick-label">{rangeWord}</span> <span class="tick-value num"><StageNumber value={tokens(data.providers.reduce((s, p) => s + (p.available && p.summaryIncluded !== false ? (p.counted?.tokens ?? p.tokens) : 0), 0))} /></span><span class="tick-label">{t('Token')}</span></span>
    {/if}
  </div>
  <div class="curve" class:resting={!working} bind:clientHeight={room}>
    <StageLiveChart {live} height={curveHeight} bind:legendHeight={legend} />
  </div>
</div>

<style>
  .monitor { position: relative; display: flex; flex-direction: column; min-height: 0; }
  /* (Not `fill`: 手绘 draws every `.fill` through its wobble filter, meant for the bars, and this holds the moving figures.) */
  .monitor.filling { min-height: 100%; }
  /* Filling, the curve's box is the room itself, and what is drawn in it never pushes it taller. */
  .filling .curve { overflow: hidden; }
  .filling .curve > :global(*) { position: absolute; left: 0; right: 0; top: 0; }
  .state { display: flex; align-items: center; gap: 9px; font-size: 14px; font-weight: 600; color: var(--ink); min-height: 24px; }
  .beat { position: relative; flex: none; pointer-events: none; width: 10px; height: 10px; border-radius: 50%; background: var(--ink-3); }
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
  /* No speed to say yet (a page just opened): a quiet dash, not a figure. */
  .big.unknown, :global(.light) .working .big.unknown { color: var(--ink-3); filter: none; }
  /* Tapped (StageDrift): a little hop. */
  .big:global(.tapped) { transform-origin: 0 100%; animation: hop 0.42s cubic-bezier(0.3, 1.5, 0.5, 1); }
  @keyframes hop { 40% { transform: translateY(-5px) scale(1.04); } }
  /* A dash, not a bar: half the headline's size on the headline's line (nothing moves when the speed comes). */
  .big.unknown :global(.number) { font-size: 0.5em; }
  .reading { margin-left: 0.4em; color: var(--ink-3); }
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
  .filling .curve { min-height: var(--least, 64px); }
  .curve { transition: opacity 0.6s ease-out; }
  .curve.resting { opacity: 0.78; }
</style>
