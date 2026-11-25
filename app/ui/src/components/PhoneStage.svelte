<script lang="ts" module>
  import { PhoneLive } from '../lib/phoneLive.svelte';
  /** The opening plays once a page load (and again when the phone switches to this look): back from the share page,
   *  the page is simply there, where it was left. */
  let played = false;
  let kept = 0;
  export function replayIntro() { played = false; kept = 0; }
  /** The live view's readings and what happened, kept while the share page is on screen (asked for only while this page is). */
  const live = new PhoneLive();
  /** The last turn done said at the top: said once, not again each time 网页版 comes back (from the share page, 电脑版). */
  let doneSaid = 0;
</script>

<script lang="ts">
  // 在手机上看, the phone's own look (网页版): a live monitor of the AI working on the computer, under the night of the
  // app's icon. Someone looks at their phone when they are away from the computer (or it runs a long job): is it working,
  // how hard, what has it spent, when will the turn be done. So the top is now (StageMonitor: working or resting, how
  // fast the models write, today's estimate rolling up, the live curve), then what it is doing (StageActivity: the
  // running sessions with their turn's clock and tools, and what happened), and under them the figures for a range as
  // the homepage has them (the estimate and its three figures, the trend, the shares, the cards, the plans), the choice
  // of look at the foot. Read-only; the figures are the panel's (phoneLive.svelte.ts for the live view).
  //
  // Three layouts. A phone upright: one column. A phone on its side: a monitor by the desk, the live view held still on
  // the left and the rest scrolling on the right, inside the notch's safe area (and on an https link, a switch keeping
  // the screen on). A computer's or a tablet's window (the link opened there): the page scrolls as a page, at most 1320
  // wide; the monitor large on the left with the sessions and what happened beside it, the figures under both in two
  // columns, a pointer reading the curves on hover.
  //
  // Motion, each for a reason: the opening counts the figures up (how large), draws the trend in from the left (time),
  // sweeps the shares round (a whole). Working, the curve flows and each request lands on it; a turn done is said at the
  // top with a tick; waking, a light runs down the page and the headline turns from the estimate to the speed. The sky
  // drifts, and stops while the page is hidden. Reduced motion: everything simply shows.
  import { onMount, untrack } from 'svelte';
  import { byUsage } from '../lib/order';
  import { currency, grouped, summaryPart, tokens } from '../lib/format';
  import { summaryProviders } from '../lib/chart';
  import { t } from '../lib/i18n.svelte';
  import { rateText } from '../lib/live';
  import { closeList, listOpen, openList, sessionOpen, toggleSession } from '../lib/expanded.svelte';
  import type { PhoneLook } from '../lib/phoneLook.svelte';
  import { panel } from '../lib/ui.svelte';
  import { paintEdges } from '../lib/edges';
  import { ScreenLock } from '../lib/screenLock';
  import { style } from '../lib/style.svelte';
  import type { Dashboard, Entry } from '../lib/types';
  import Icon from './Icon.svelte';
  import StageActivity from './StageActivity.svelte';
  import StageCard from './StageCard.svelte';
  import StageChart from './StageChart.svelte';
  import StageMonitor from './StageMonitor.svelte';
  import StageNumber from './StageNumber.svelte';
  import StagePills from './StagePills.svelte';
  import StagePlans from './StagePlans.svelte';
  import StageRing from './StageRing.svelte';
  import StageSession from './StageSession.svelte';

  let { data, today = null, rangeOptions, reading = false, grouping = null, regrouping = false, sessions = null, view = 'cards', entries, problem = null,
    material = 'solid', onrange, ongroup, onview, onshare, onlook }: {
    data: Dashboard;
    /** The computer's background (实色, 磨砂, 玻璃): the panes' material here, over the sky. */
    material?: string;
    /** Today's figures from the homepage, for the monitor until the live view has its own. */
    today?: Dashboard | null;
    rangeOptions: Entry[]; reading?: boolean; grouping?: string | null; regrouping?: boolean;
    sessions?: Entry | null; view?: 'cards' | 'sessions'; entries: Entry[];
    /** Why the figures on screen are not fresh. */
    problem?: string | null;
    onrange: (action: string[]) => void; ongroup: (key: string) => void; onview: (view: 'cards' | 'sessions') => void;
    onshare: () => void; onlook: (look: PhoneLook) => void;
  } = $props();

  const intro = !played;
  onMount(() => {
    played = true;
    live.start();
    return () => live.stop();
  });

  // MARK: The screen

  let screenW = $state(typeof window === 'undefined' ? 390 : window.innerWidth);
  let screenH = $state(typeof window === 'undefined' ? 844 : window.innerHeight);
  /** A computer's or tablet's window; a phone on its side; a phone upright (or a narrow window). */
  const layout = $derived<'desk' | 'side' | 'narrow'>(screenW >= 1000 && screenH >= 600 ? 'desk' : screenW >= 600 && screenW > screenH * 1.15 ? 'side' : 'narrow');
  const wide = $derived(layout === 'side');
  /** Summary on the homepage (主页模块): the estimate, its figures, the trend and the shares; without it, as on the
   *  panel, the figures for a range are the cards and the plans. */
  const summary = $derived(data.showSummary !== false);
  let scroller: HTMLDivElement | undefined = $state();
  let scrolled = $state(0);
  let headline: HTMLDivElement | undefined = $state();
  let headlineAway = $state(false);
  $effect(() => {
    if (!headline || !scroller || wide) { headlineAway = false; return; }
    const observer = new IntersectionObserver(([entry]) => (headlineAway = !entry.isIntersecting), { root: scroller, rootMargin: '-56px 0px 0px 0px' });
    observer.observe(headline);
    return () => observer.disconnect();
  });
  let asleep = $state(false);
  /** The clock the turns' timers and "6 分钟前" read: each second while the page shows. */
  let now = $state(Date.now());
  onMount(() => {
    if (scroller && kept) scroller.scrollTop = kept;
    // The sky stands still while the page is out of sight, and the clock with it.
    const visibility = () => { asleep = document.visibilityState !== 'visible'; now = Date.now(); };
    document.addEventListener('visibilitychange', visibility);
    const ticking = setInterval(() => { if (!asleep) now = Date.now(); }, 1000);
    return () => { document.removeEventListener('visibilitychange', visibility); clearInterval(ticking); };
  });
  function onscroll() {
    if (!scroller) return;
    scrolled = scroller.scrollTop;
    kept = scrolled;
  }
  // The browser's bars take the sky's own edges (its top band and its foot are flat for them), in the computer's theme,
  // look and background.
  let stageBox: HTMLDivElement | undefined = $state();
  const sketch = $derived(style.current === 'sketch');
  const pane = $derived(material === 'frosted' || material === 'glass' ? material : 'solid');
  $effect(() => {
    void panel.dark; void sketch; void pane;
    const box = stageBox;
    if (!box) return;
    const frame = requestAnimationFrame(() => {
      const css = getComputedStyle(box);
      paintEdges(css.getPropertyValue('--edge-top').trim(), css.getPropertyValue('--edge-bottom').trim());
    });
    return () => cancelAnimationFrame(frame);
  });

  // MARK: Live

  const working = $derived(live.state === 'working');
  /** Waking: for a moment after the page turned from resting to working. */
  const waking = $derived(live.woke > 0 && now - live.woke < 1600 && !panel.reduceMotion);
  /** The bar's headline once the monitor's has scrolled away: the speed while working, else today's estimate. */
  const compact = $derived.by(() => {
    void live.version;
    if (working) { const rate = live.rate(now); return `${rate === null ? '—' : rateText(rate)} Token/s`; }
    return live.cost === null ? '' : currency.symbol + grouped(live.cost);
  });
  /** A turn just done, said at the top for a few seconds; a tap lets it go. */
  let doneShown = $state<number | null>(null);
  $effect(() => {
    const done = live.done;
    if (!done || done.at <= doneSaid) return;
    doneSaid = done.at;
    doneShown = done.at;
    const timer = setTimeout(() => { if (doneShown === done.at) doneShown = null; }, 4500);
    return () => clearTimeout(timer);
  });
  /** When the computer was last read (resting, the bar says so: a reading on its way for long is no reading now). */
  const lastRead = $derived.by(() => {
    if (!live.readAt) return '';
    const d = new Date(live.readAt);
    return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
  });

  // Keeping the screen on (a monitor by the desk): only where the browser offers it (an https page, iOS 16.4 on). One
  // lock at most, let go when the switch goes off or this page goes away (to 电脑版, to the share page).
  const lockable = ScreenLock.offered();
  const screenLock = lockable ? ScreenLock.browser() : null;
  let awake = $state(false);
  function holdScreen(on: boolean) {
    awake = on;
    void screenLock?.hold(on).then((held) => { if (!held) awake = false; });
  }
  // The system lets go of it when the page is hidden: taken again when it comes back.
  $effect(() => { if (awake && !asleep) untrack(() => void screenLock?.resume()); });
  onMount(() => () => screenLock?.dispose());

  // MARK: Summary

  const counted = $derived(summaryProviders(data));
  const valid = $derived(counted.filter((p) => p.available));
  const costs = $derived(valid.map((p) => p.cost).filter((c): c is number => c !== null));
  const partial = $derived(valid.length !== counted.length || valid.some((p) => p.unpriced > 0));
  const amount = $derived(costs.length ? currency.symbol + grouped(costs.reduce((a, b) => a + b, 0)) : '—');
  const tokenSum = $derived(valid.reduce((s, p) => s + p.tokens, 0));
  const requestSum = $derived(valid.reduce((s, p) => s + p.requests, 0));
  /** The share picture's cache hits: every tool's cache reads over every tool's cache-eligible input. */
  const cache = $derived.by(() => {
    const cached = valid.filter((p) => p.cache !== null), input = (p: (typeof valid)[number]) => p.cacheInputs ?? p.tokens;
    const weight = cached.reduce((s, p) => s + input(p), 0);
    return weight > 0 ? ((cached.reduce((s, p) => s + (p.cache ?? 0) * input(p), 0) / weight) * 100).toFixed(1) + '%' : '—';
  });
  const figures = $derived([
    { label: t('Token 用量'), value: valid.length ? tokens(tokenSum) : '—' },
    { label: t('请求'), value: valid.length ? requestSum.toLocaleString('en-US') : '—' },
    { label: t('缓存命中'), value: valid.length ? cache : '—' },
  ]);
  /** The monitor's headline and curve, as large as the layout lets them be. */
  const big = $derived(layout === 'desk' ? 80 : layout === 'side' ? 50 : 64);
  const chartHeight = $derived(layout === 'desk' ? 300 : layout === 'side' ? null : 196);

  // MARK: Choices

  const chosenRange = $derived(rangeOptions.find((o) => o.checked)?.action?.[1] ?? data.rangeKey);
  const rangeChoices = $derived(rangeOptions.map((o) => ({ value: o.action?.[1] ?? '', label: o.label })));
  function pickRange(key: string) {
    const option = rangeOptions.find((o) => o.action?.[1] === key);
    if (option?.action) onrange(option.action);
  }
  const wantedGrouping = $derived(grouping ?? data.grouping ?? 'tool');
  const showingSessions = $derived(!!sessions && (view === 'sessions' || !data.providers.length));
  const tabs = $derived([...(data.providers.length ? data.groupings ?? [] : []).map((g) => ({ value: g.key, label: g.label })),
    ...(sessions ? [{ value: 'sessions', label: t('会话') }] : [])]);
  function pickTab(key: string) {
    if (key === 'sessions') { onview('sessions'); return; }
    onview('cards');
    if (key !== wantedGrouping) ongroup(key);
  }

  // MARK: Cards

  /** The cards rise in at the opening, and again whenever another range, grouping or 会话 brings new ones, without the
   *  opening's wait. */
  let changed = $state(false);
  let firstShape: string | null = null;
  $effect(() => {
    const shape = `${data.rangeKey}/${data.grouping ?? 'tool'}/${showingSessions}`;
    if (firstShape === null) firstShape = shape;
    else if (shape !== firstShape) changed = true;
  });
  const entering = $derived(intro || changed);
  const lead = $derived(intro && !changed ? null : '0s');

  const cardList = $derived(data.cards ?? byUsage(data.providers));
  const SHOWN = 5;
  const whole = $derived(!data.cards || listOpen(data.grouping ?? ''));
  const listed = $derived(whole ? cardList : cardList.slice(0, SHOWN));
  const unlisted = $derived(cardList.length - listed.length);
  const folds = $derived(!!data.cards && !unlisted && cardList.length > SHOWN);
  /** Each card's share: a tool's of what Summary counts (as the share picture says it), a model's or a project's of
   *  the list. */
  const parts = $derived.by(() => {
    const out = new Map<string, number>();
    if (data.cards) {
      const sum = data.cards.reduce((s, p) => s + p.tokens, 0);
      for (const p of data.cards) out.set(p.id, sum > 0 ? p.tokens / sum : 0);
    } else {
      const sum = counted.reduce((s, p) => s + p.tokens, 0);
      for (const p of data.providers) out.set(p.id, sum > 0 ? (summaryPart(p)?.tokens ?? 0) / sum : 0);
    }
    return out;
  });
  let opened = $state(new Set<string>());
  const toggleCard = (id: string) => { const next = new Set(opened); if (next.has(id)) next.delete(id); else next.add(id); opened = next; };

  // A card whose figures grew since the last reading of the same range and grouping glows for a moment.
  let before: { key: string; values: Map<string, number> } | null = null;
  let glowing = $state(new Set<string>());
  let glowTimer: ReturnType<typeof setTimeout> | undefined;
  $effect(() => {
    const key = `${data.rangeKey}/${data.grouping ?? 'tool'}`;
    const values = new Map(cardList.map((p) => [p.id, p.tokens + (p.cost ?? 0) * 1e6]));
    untrack(() => {
      if (before?.key === key && !panel.reduceMotion) {
        const grew = [...values].filter(([id, v]) => v > (before!.values.get(id) ?? Infinity)).map(([id]) => id);
        if (grew.length) {
          glowing = new Set(grew);
          clearTimeout(glowTimer);
          glowTimer = setTimeout(() => (glowing = new Set()), 1600);
        }
      }
      before = { key, values };
    });
  });

  // MARK: Sessions

  const sessionRows = $derived(sessions?.children.filter((c) => c.statusLight !== undefined) ?? []);
  const sessionKey = (s: Entry, i: number) => s.key ?? `${s.label}\u001f${i}`;
  const everySession = $derived(listOpen('sessions'));
  const sessionsListed = $derived(everySession ? sessionRows : sessionRows.slice(0, SHOWN));
  const sessionNotes = $derived(sessions?.children.filter((c) => c.statusLight === undefined && !c.separator && c.checked === undefined && !c.action) ?? []);

  // MARK: The star

  /** The menu bar's star, its dots by their distance from the middle: they ripple out from it while a range is read. */
  const DOTS: [number, number, number][] = [[12, 2.67, 1.58], [7.33, 7.33, 0.95], [12, 7.33, 2.15], [16.67, 7.33, 0.95], [2.67, 12, 1.58], [7.33, 12, 2.15],
    [12, 12, 2.15], [16.67, 12, 2.15], [21.33, 12, 1.58], [7.33, 16.67, 0.95], [12, 16.67, 2.15], [16.67, 16.67, 0.95], [12, 21.33, 1.58]];
  /** A few stars in the sky, placed the same on every phone. */
  const STARS = Array.from({ length: 14 }, (_, i) => {
    const r = (n: number) => { const x = Math.sin(i * 12.9898 + n * 78.233) * 43758.5453; return x - Math.floor(x); };
    return { x: r(1) * 100, y: r(2) * 70, size: 1 + r(3) * 1.6, wait: r(4) * 6, length: 3 + r(5) * 4 };
  });
</script>

<svelte:window bind:innerWidth={screenW} bind:innerHeight={screenH} />

<div class="stage {layout} {pane}" class:light={!panel.dark} class:sketch class:wide class:still={panel.reduceMotion} class:asleep class:working class:waking bind:this={stageBox}>
  <div class="sky" aria-hidden="true">
    <!-- The lights fade out at the top and the foot, so the edges stay the flat colour the browser's bars take. -->
    <div class="lights">
      <div class="aurora one"></div>
      <div class="aurora two"></div>
      <div class="dots"></div>
      {#each STARS as star, i (i)}
        <span class="star" style:left="{star.x}%" style:top="{star.y}%" style:width="{star.size}px" style:height="{star.size}px"
          style:animation-delay="{star.wait}s" style:animation-duration="{star.length}s"></span>
      {/each}
    </div>
  </div>
  <!-- What is fixed at the screen's edges, in the edges' colour: Safari 26 tints its bars from it. -->
  <div class="edge top" aria-hidden="true"></div>
  <div class="edge bottom" aria-hidden="true"></div>
  {#if waking}<div class="sweep" aria-hidden="true"></div>{/if}
  {#if live.done && doneShown === live.done.at}
    <div class="turn-done" role="status"><button class="done-body" onclick={() => (doneShown = null)}>
      <svg class="tick" width="30" height="30" viewBox="0 0 30 30" aria-hidden="true"><circle cx="15" cy="15" r="13" /><path d="M9 15.5 13.2 19.6 21.4 11" /></svg>
      <span class="done-text">
        <span class="done-title">{t('这一轮完成')}</span>
        <span class="done-detail">{live.done.title}{#if live.done.took} · {live.done.took}{/if}{#if live.done.cost} · {live.done.cost}{/if}</span>
      </span>
    </button></div>
  {/if}
  <div class="scroll" bind:this={scroller} {onscroll}>
    <div class="frame">
      <header class="bar" class:raised={!wide && scrolled > 6}>
        <span class="brand">
          <svg class="mark" class:reading={reading || live.trouble} viewBox="0 0 24 24" width="22" height="22" aria-hidden="true">
            {#each DOTS as [cx, cy, r], i (i)}
              <circle {cx} {cy} {r} style:--d={Math.hypot(cx - 12, cy - 12) / 4.67} />
            {/each}
          </svg>
          <span class="titles">
            <span class="word" class:away={headlineAway && !!compact}>Starmeter</span>
            <span class="compact num" class:shown={headlineAway && !!compact} aria-hidden={!headlineAway}>{compact}<span class="compact-range">{working ? t('正在工作') : t('今天')}</span></span>
          </span>
        </span>
        {#if lockable}
          <button class="awake" class:on={awake} aria-pressed={awake} aria-label={t('保持亮屏')} title={t('保持亮屏')} onclick={() => holdScreen(!awake)}>
            <svg width="18" height="18" viewBox="0 0 20 20" aria-hidden="true"><circle cx="10" cy="10" r="3.4" /><path d="M10 2.5v2M10 15.5v2M2.5 10h2M15.5 10h2M4.7 4.7l1.4 1.4M13.9 13.9l1.4 1.4M4.7 15.3l1.4-1.4M13.9 6.1l1.4-1.4" /></svg>
          </button>
        {/if}
        <span class="live" class:trouble={live.trouble || !!problem} title={problem ?? ''}>
          {#key live.version}<span class="live-dot" aria-hidden="true"></span>{/key}
          <span class="num">{live.trouble ? t('连不上电脑') : working ? t('实时') : lastRead}</span>
        </span>
        <button class="share" onclick={onshare} aria-label={t('分享用量图')}><Icon name="square.and.arrow.up" size={18} /></button>
      </header>

      <section class="lead" style:--big="{big}px">
        <div class="monitor-box" bind:this={headline}>
          <StageMonitor {live} today={today ?? (data.rangeKey === 'today' ? data : null)} {now} {chartHeight} {intro} />
        </div>
      </section>

      <section class="rest">
        <div class="activity-box"><StageActivity {live} {now} shown={layout === 'desk' ? 8 : 6} /></div>

        <div class="stats">
          <div class="stats-a">
            <div class="stats-head">{t('统计')}</div>
            {#if rangeChoices.length > 1}
              <div class="ranges"><StagePills options={rangeChoices} chosen={chosenRange} label={t('统计范围')} onpick={pickRange} busy={reading} scroll bare /></div>
            {/if}
            {#if summary}
              <div class="hero" class:reading>
                <div class="hero-label">
                  <!-- The range these figures are of: while another range picked is read, still this one. -->
                  <span class="range">{data.rangeLabel}</span>
                  <span class="sep">·</span>
                  <span>{partial ? t('已知 API 估算') : t('API 估算')}</span>
                  {#if data.devices}<span class="devices">{t('含 {n} 台电脑', { n: data.devices + 1 })}</span>{/if}
                </div>
                <div class="amount"><StageNumber value={amount} count={intro} /></div>
                <div class="figures">
                  {#each figures as figure, i (i)}
                    <div class="figure" style:--order={i}>
                      <span class="figure-label">{figure.label}</span>
                      <span class="figure-value"><StageNumber value={figure.value} count={intro} delay={120 + i * 80} /></span>
                    </div>
                  {/each}
                </div>
              </div>
              <div class="trend" class:reading>
                <div class="trend-head"><span>{t('{unit} Token', { unit: data.chartUnit })}</span><span class="span num">{data.chartSpan}</span></div>
                <StageChart {data} height={layout === 'desk' ? 190 : 156} draw={intro} />
              </div>
              {#if counted.some((p) => p.available && p.tokens > 0)}
                <div class="panel shares" class:reading>
                  <div class="panel-head"><span>{t('工具占比')}</span><span class="quiet">{t('按 Token')}</span></div>
                  <StageRing {data} draw={intro} size={128} />
                </div>
              {/if}
            {/if}
          </div>

          <div class="stats-b">
            {#if tabs.length > 1}
              <div class="tabs"><StagePills options={tabs} chosen={showingSessions ? 'sessions' : wantedGrouping} label={t('卡片分组')} onpick={pickTab} small /></div>
            {/if}
            {#if !showingSessions && !regrouping && data.groupNote}<p class="note">{data.groupNote}</p>{/if}

            {#if showingSessions && sessions}
              <div class="list" style:--lead={lead}>
                {#each sessionsListed as s, i (sessionKey(s, i))}
                  <StageSession {s} order={i} enter={entering} open={sessionOpen(sessionKey(s, i))} ontoggle={() => toggleSession(sessionKey(s, i))} />
                {/each}
                {#if sessionRows.length > SHOWN}
                  <button class="more" onclick={() => (everySession ? closeList : openList)('sessions')}>
                    {everySession ? t('收起') : t('查看更多 {n} 个会话', { n: sessionRows.length - sessionsListed.length })}</button>
                {/if}
                {#each sessionNotes as note, i (i)}<p class="note">{note.label}</p>{/each}
              </div>
            {:else}
              <div class="list" class:held={regrouping} inert={regrouping} aria-busy={regrouping} style:--lead={lead}>
                {#if !cardList.length && data.providers.length}<p class="note center">{t('此时段暂无调用')}</p>{/if}
                {#each listed as p, i (p.id)}
                  <StageCard {p} part={parts.get(p.id) ?? 0} order={i} enter={entering} open={opened.has(p.id)} glow={glowing.has(p.id)} ontoggle={() => toggleCard(p.id)} />
                {/each}
                {#if unlisted || folds}
                  <button class="more" onclick={() => (folds ? closeList : openList)(data.grouping ?? '')}>
                    {folds ? t('收起') : t(data.grouping === 'project' ? '查看更多 {n} 个项目' : '查看更多 {n} 个模型', { n: unlisted })}</button>
                {/if}
              </div>
            {/if}

            {#if entries.some((e) => !e.separator && e.label)}
              <div class="plans-head">{t('订阅与服务')}</div>
              <StagePlans {entries} enter={intro} />
            {/if}
          </div>
        </div>

        <footer class="foot">
          <StagePills options={[{ value: 'stage', label: t('网页版') }, { value: 'panel', label: t('电脑版') }]} chosen="stage" label={t('手机上的样式')} onpick={(v) => onlook(v as PhoneLook)} small />
          <p class="read-only">{t('只读：实时数字每 2 秒从电脑上的 Starmeter 读一次，休息时每 5 秒')}</p>
        </footer>
      </section>
    </div>
  </div>
</div>

<style>
  .stage {
    --ink: rgba(240, 244, 255, 0.96);
    --ink-2: rgba(214, 222, 255, 0.68);
    --ink-3: rgba(214, 222, 255, 0.44);
    --glass: rgba(255, 255, 255, 0.055);
    --glass-strong: rgba(255, 255, 255, 0.1);
    --glass-edge: rgba(255, 255, 255, 0.11);
    --line: rgba(214, 222, 255, 0.18);
    --line-faint: rgba(214, 222, 255, 0.09);
    --lift: 0 10px 30px -18px rgba(0, 0, 0, 0.7);
    --pill: rgba(255, 255, 255, 0.16);
    --pill-glow: rgba(140, 170, 255, 0.25);
    --shine: rgba(255, 255, 255, 0.35);
    --bubble: rgba(18, 30, 72, 0.94);
    --star: rgb(165, 180, 252);
    --on-star: rgb(12, 22, 60);
    --warn: rgb(255, 178, 92);
    --good: rgb(74, 222, 128);
    --sky-top: #1F3A7A;
    --sky-mid: #0E1D4C;
    --sky-low: #050A24;
    --dot: rgba(255, 255, 255, 0.07);
    --halo: rgba(196, 216, 255, 0.32);
    /* The edges' colours: the sky's top and its foot, flat, for the browser's bars to take. */
    --edge-top: var(--sky-top);
    --edge-bottom: var(--sky-low);
    /* One scale for the page, as the panel's: figures in the rounded face (Figure.svelte), at 600; words in the panel's
       own face (system, PingFang SC or TC by language; by hand in 手绘). */
    --text-face: inherit;
    --fig-l: 20px;
    --fig-m: 17px;
    --type-title: 15px;
    --type-head: 13px;
    --type-caption: 12px;
    --type-small: 11px;
    position: absolute;
    inset: 0;
    overflow: hidden;
    color: var(--ink);
    background: var(--edge-bottom);
    -webkit-tap-highlight-color: transparent;
  }
  .stage.light {
    --ink: rgba(14, 24, 66, 0.94);
    --ink-2: rgba(14, 24, 66, 0.62);
    --ink-3: rgba(14, 24, 66, 0.42);
    --glass: rgba(255, 255, 255, 0.66);
    --glass-strong: rgba(14, 29, 76, 0.07);
    --glass-edge: rgba(14, 29, 76, 0.08);
    --line: rgba(14, 29, 76, 0.16);
    --line-faint: rgba(14, 29, 76, 0.08);
    --lift: 0 12px 30px -20px rgba(40, 60, 140, 0.45);
    --pill: rgba(255, 255, 255, 0.96);
    --pill-glow: rgba(80, 100, 200, 0.18);
    --shine: rgba(120, 140, 255, 0.35);
    --bubble: rgba(255, 255, 255, 0.97);
    --star: rgb(88, 79, 227);
    --on-star: #fff;
    --warn: rgb(214, 104, 0);
    --good: rgb(22, 163, 74);
    --sky-top: #DCE3FF;
    --sky-mid: #EEF0FF;
    --sky-low: #F7F6FF;
    --dot: rgba(14, 29, 76, 0.07);
    --halo: rgba(120, 140, 255, 0.22);
  }

  /* MARK: The sky: brighter, and its light quicker, while the AI works; still while it rests. */
  .sky {
    position: absolute;
    inset: 0;
    overflow: hidden;
    /* Flat at the top (the status bar's band) and at the foot (the toolbar's), the night between. */
    background: linear-gradient(180deg, var(--edge-top) 0, var(--sky-top) 72px, var(--sky-mid) 48%, var(--sky-low) calc(100% - 90px), var(--edge-bottom) 100%);
    pointer-events: none;
  }
  .lights {
    position: absolute;
    inset: 0;
    mask-image: linear-gradient(180deg, transparent 0, transparent 40px, #000 150px, #000 calc(100% - 160px), transparent calc(100% - 60px));
    -webkit-mask-image: linear-gradient(180deg, transparent 0, transparent 40px, #000 150px, #000 calc(100% - 160px), transparent calc(100% - 60px));
  }
  .edge { position: absolute; left: 0; right: 0; z-index: 6; pointer-events: none; }
  .edge.top { top: 0; height: max(1px, env(safe-area-inset-top)); background: var(--edge-top); }
  .edge.bottom { bottom: 0; height: max(1px, env(safe-area-inset-bottom)); background: var(--edge-bottom); }
  .aurora {
    position: absolute;
    width: 120vmax;
    height: 120vmax;
    border-radius: 50%;
    will-change: transform;
    opacity: 0.7;
    transition: opacity 1.2s;
    animation: drift 38s ease-in-out infinite alternate;
  }
  .working .aurora { opacity: 1; }
  .aurora.one { left: -40vmax; top: -62vmax; background: radial-gradient(closest-side, rgba(120, 150, 255, 0.34), transparent); }
  .aurora.two { right: -55vmax; top: -20vmax; background: radial-gradient(closest-side, rgba(110, 230, 190, 0.14), transparent); animation-duration: 47s; animation-direction: alternate-reverse; }
  .light .aurora.one { background: radial-gradient(closest-side, rgba(150, 170, 255, 0.45), transparent); }
  .light .aurora.two { background: radial-gradient(closest-side, rgba(120, 220, 190, 0.3), transparent); }
  .stage:not(.working) .aurora { animation-play-state: paused; }
  @keyframes drift {
    from { transform: translate3d(0, 0, 0) rotate(0deg); }
    to { transform: translate3d(8vmax, 6vmax, 0) rotate(25deg); }
  }
  .dots {
    position: absolute;
    inset: 0;
    background-image: radial-gradient(circle, var(--dot) 1px, transparent 1.4px);
    background-size: 22px 22px;
    mask-image: radial-gradient(ellipse 90% 60% at 50% 15%, #000, transparent);
    -webkit-mask-image: radial-gradient(ellipse 90% 60% at 50% 15%, #000, transparent);
  }
  .star {
    position: absolute;
    border-radius: 50%;
    background: #fff;
    box-shadow: 0 0 6px rgba(200, 220, 255, 0.9);
    opacity: 0.2;
    animation: twinkle 4s ease-in-out infinite;
  }
  .light .star { display: none; }
  @keyframes twinkle { 50% { opacity: 0.85; } }
  .asleep .aurora, .asleep .star { animation-play-state: paused; }
  .still .aurora, .still .star { animation: none; }
  /* Waking: a light runs down the page. */
  .sweep {
    position: absolute;
    left: 0;
    right: 0;
    top: 0;
    height: 60%;
    z-index: 30;
    pointer-events: none;
    background: radial-gradient(ellipse 70% 60% at 50% 50%, rgba(140, 255, 200, 0.28), transparent 70%);
    animation: sweep 1.5s cubic-bezier(0.3, 0.6, 0.2, 1) forwards;
  }
  .light .sweep { background: radial-gradient(ellipse 70% 60% at 50% 50%, rgba(40, 200, 130, 0.22), transparent 70%); }
  @keyframes sweep { from { transform: translateY(-60%); opacity: 0; } 25% { opacity: 1; } to { transform: translateY(170%); opacity: 0; } }

  /* MARK: A turn done */
  .turn-done {
    position: absolute;
    left: 50%;
    top: calc(12px + env(safe-area-inset-top));
    z-index: 40;
    transform: translateX(-50%);
    width: min(440px, calc(100% - 28px));
    border-radius: 20px;
    color: var(--ink);
    background: color-mix(in srgb, var(--sky-mid) 80%, transparent);
    backdrop-filter: blur(20px) saturate(1.6);
    -webkit-backdrop-filter: blur(20px) saturate(1.6);
    box-shadow: 0 14px 40px -10px rgba(0, 0, 0, 0.55), inset 0 0 0 1px color-mix(in srgb, var(--good) 45%, transparent);
    animation: drop-in 0.55s cubic-bezier(0.22, 1.3, 0.36, 1);
  }
  .done-body { width: 100%; display: flex; align-items: center; gap: 12px; padding: 12px 16px; text-align: left; }
  @keyframes drop-in { from { opacity: 0; transform: translate(-50%, -24px) scale(0.96); } }
  .tick circle { fill: var(--good); }
  .tick path { fill: none; stroke: #06281A; stroke-width: 2.6; stroke-linecap: round; stroke-linejoin: round; stroke-dasharray: 20; stroke-dashoffset: 20; animation: draw-tick 0.5s ease-out 0.25s forwards; }
  .still .tick path { animation: none; stroke-dashoffset: 0; }
  @keyframes draw-tick { to { stroke-dashoffset: 0; } }
  .done-text { min-width: 0; display: flex; flex-direction: column; gap: 2px; }
  .done-title { font-size: var(--type-title); font-weight: 600; }
  .done-detail { font-size: 12px; color: var(--ink-2); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }

  /* MARK: Layout */
  .scroll {
    position: absolute;
    inset: 0;
    overflow-x: hidden;
    overflow-y: auto;
    -webkit-overflow-scrolling: touch;
    overscroll-behavior-y: contain;
  }
  .frame {
    max-width: 600px;
    margin: 0 auto;
    /* The foot clears Safari's floating toolbar, which the page now runs on under. */
    padding: 0 max(18px, env(safe-area-inset-right)) calc(76px + env(safe-area-inset-bottom)) max(18px, env(safe-area-inset-left));
    display: flex;
    flex-direction: column;
  }
  .lead, .rest { display: flex; flex-direction: column; gap: 16px; min-width: 0; }
  .rest { margin-top: 18px; }
  .stats, .stats-a, .stats-b { display: flex; flex-direction: column; gap: 16px; min-width: 0; }
  .stats { margin-top: 10px; }
  .stats-head { margin: 8px 2px -4px; font-size: var(--type-head); font-weight: 600; color: var(--ink-2); }

  /* A phone on its side: the monitor held still on the left, the rest scrolling on the right. */
  .side .scroll { overflow: hidden; }
  .side .frame {
    max-width: none;
    height: 100%;
    box-sizing: border-box;
    padding: env(safe-area-inset-top) max(20px, env(safe-area-inset-right)) 0 max(20px, env(safe-area-inset-left));
    display: grid;
    grid-template-columns: minmax(320px, 56%) minmax(0, 1fr);
    grid-template-rows: auto minmax(0, 1fr);
    column-gap: 22px;
  }
  .side .bar { grid-column: 1; grid-row: 1; }
  .side .lead { grid-column: 1; grid-row: 2; padding-bottom: max(12px, env(safe-area-inset-bottom)); min-height: 0; display: grid; grid-template-rows: minmax(0, 1fr); }
  .side .monitor-box { grid-row: 1; grid-column: 1; min-height: 0; height: 100%; }
  .side .rest {
    grid-column: 2;
    grid-row: 1 / span 2;
    margin: 0;
    overflow-y: auto;
    -webkit-overflow-scrolling: touch;
    padding: 14px 2px calc(24px + env(safe-area-inset-bottom));
    scrollbar-width: none;
  }
  .side .rest::-webkit-scrollbar { display: none; }

  /* A computer's or a tablet's window: a page, the monitor large beside what is happening, the figures under both. */
  .desk .frame {
    max-width: 1320px;
    box-sizing: border-box;
    padding: 0 40px 40px;
    display: grid;
    grid-template-columns: minmax(0, 1.6fr) minmax(340px, 1fr);
    column-gap: 28px;
    align-items: start;
  }
  .desk .bar { grid-column: 1 / -1; margin: 0 -40px; padding: 0 40px; height: 64px; }
  .desk .lead { grid-column: 1; padding: 22px 26px 18px; border-radius: 28px; background: var(--glass); box-shadow: inset 0 0 0 0.5px var(--glass-edge), var(--lift); margin-top: 6px; }
  .desk .rest { display: contents; }
  .desk .activity-box { grid-column: 2; margin-top: 6px; }
  .desk .stats { grid-column: 1 / -1; display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); column-gap: 28px; align-items: start; margin-top: 30px; }
  .desk .foot { grid-column: 1 / -1; margin-top: 30px; }

  /* MARK: The bar */
  .bar {
    position: sticky;
    top: 0;
    z-index: 5;
    display: flex;
    align-items: center;
    gap: 10px;
    height: 54px;
    margin: 0 -18px;
    padding: env(safe-area-inset-top) 18px 0;
    transition: background-color 0.3s, box-shadow 0.3s;
  }
  /* Scrolled, the bar is the status bar's own colour carried down: one band at the top of the screen. */
  .bar.raised {
    background: color-mix(in srgb, var(--edge-top) 88%, transparent);
    backdrop-filter: blur(18px) saturate(1.5);
    -webkit-backdrop-filter: blur(18px) saturate(1.5);
    box-shadow: 0 0.5px 0 var(--glass-edge);
  }
  .side .bar { position: relative; margin: 0; padding: 0; height: 50px; }
  .brand { flex: 1; min-width: 0; display: flex; align-items: center; gap: 9px; }
  .mark { flex: none; fill: var(--ink); filter: drop-shadow(0 0 6px var(--halo)); }
  .mark circle { transform-box: fill-box; transform-origin: center; }
  .mark.reading circle { animation: ripple 1.25s ease-in-out infinite; animation-delay: calc(var(--d) * 0.13s); }
  @keyframes ripple {
    0%, 100% { opacity: 1; transform: scale(1); }
    40% { opacity: 0.25; transform: scale(0.55); }
  }
  .titles { position: relative; flex: 1; min-width: 0; height: 22px; overflow: hidden; }
  .word, .compact {
    position: absolute;
    left: 0;
    top: 0;
    line-height: 22px;
    white-space: nowrap;
    transition: transform 0.4s cubic-bezier(0.22, 1, 0.36, 1), opacity 0.3s;
  }
  .word { font-size: 17px; font-weight: 600; letter-spacing: -0.01em; }
  .word.away { transform: translateY(-100%); opacity: 0; }
  .compact { font-family: var(--rounded); font-size: var(--fig-m); font-weight: 600; transform: translateY(100%); opacity: 0; }
  .compact.shown { transform: none; opacity: 1; }
  .compact-range { margin-left: 8px; font-family: var(--text-face); font-size: var(--type-caption); font-weight: 500; color: var(--ink-2); }
  .live { flex: none; display: flex; align-items: center; gap: 6px; font-size: 12.5px; color: var(--ink-2); }
  /* Blinks at each reading: the page is in touch with the computer. */
  .live-dot { width: 7px; height: 7px; border-radius: 50%; background: var(--good); animation: blink 0.9s ease-out; }
  @keyframes blink { from { box-shadow: 0 0 0 0 color-mix(in srgb, var(--good) 80%, transparent); transform: scale(1.5); } to { box-shadow: 0 0 0 6px transparent; transform: scale(1); } }
  .trouble .live-dot { background: var(--warn); animation: none; }
  .stage:not(.working) .live-dot { background: var(--ink-3); }
  .awake, .share {
    flex: none;
    width: 36px;
    height: 36px;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--ink);
    transition: background-color 0.2s, transform 0.2s, color 0.2s;
  }
  .share { margin-right: -6px; }
  .awake svg { fill: none; stroke: currentColor; stroke-width: 1.6; stroke-linecap: round; }
  .awake svg circle { fill: none; }
  .awake.on { color: #FFD66B; background: rgba(255, 214, 107, 0.16); }
  .awake.on svg circle { fill: currentColor; }
  .awake:active, .share:active { background: var(--glass-strong); transform: scale(0.92); }
  @media (hover: hover) {
    .awake:hover, .share:hover { background: var(--glass-strong); }
  }

  /* MARK: The figures for a range */
  .ranges {
    margin: 0 -18px;
    mask-image: linear-gradient(90deg, transparent, #000 18px, #000 calc(100% - 28px), transparent);
    -webkit-mask-image: linear-gradient(90deg, transparent, #000 18px, #000 calc(100% - 28px), transparent);
  }
  .ranges :global(.pills) { max-width: none; width: 100%; padding: 3px 18px; }
  .side .ranges, .desk .ranges { margin: 0; mask-image: none; -webkit-mask-image: none; }
  .side .ranges :global(.pills), .desk .ranges :global(.pills) { padding: 3px 0; }
  .hero { position: relative; padding: 4px 0 2px; transition: opacity 0.35s; }
  .reading { opacity: 0.45; }
  .hero-label { display: flex; align-items: center; flex-wrap: wrap; gap: 6px; font-size: 13px; color: var(--ink-2); }
  .range { color: var(--ink); font-weight: 600; }
  .sep { opacity: 0.5; }
  .devices { padding: 1px 8px; border-radius: 999px; font-size: 11px; background: var(--glass-strong); }
  .amount {
    margin: 2px 0 0 -1px;
    font-family: var(--rounded);
    font-size: 34px;
    font-weight: 600;
    letter-spacing: -0.02em;
    color: var(--ink);
    line-height: 1.1;
  }
  .figures { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 10px; margin-top: 12px; }
  .figure {
    padding: 10px 12px 11px;
    border-radius: 16px;
    background: var(--glass);
    box-shadow: inset 0 0 0 0.5px var(--glass-edge);
    min-width: 0;
  }
  .figure-label { display: block; font-size: 11px; color: var(--ink-2); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .figure-value { display: block; margin-top: 3px; font-family: var(--rounded); font-size: var(--fig-l); font-weight: 600; color: var(--ink); }

  /* MARK: Panels */
  .trend, .panel {
    padding: 14px 14px 12px;
    border-radius: 22px;
    background: var(--glass);
    box-shadow: inset 0 0 0 0.5px var(--glass-edge), var(--lift);
    transition: opacity 0.35s;
  }
  .trend-head, .panel-head { display: flex; align-items: baseline; justify-content: space-between; gap: 10px; margin-bottom: 8px; font-size: var(--type-head); font-weight: 600; }
  .span, .quiet { font-size: 11px; font-weight: 500; color: var(--ink-3); }
  .tabs { display: flex; justify-content: center; margin-top: 4px; }
  .desk .tabs { justify-content: flex-start; margin-top: 30px; }
  .note { margin: -6px 0 0; font-size: 11.5px; color: var(--ink-3); text-align: center; }
  .note.center { margin: 8px 0; }
  .list { display: flex; flex-direction: column; gap: 10px; transition: opacity 0.3s; }
  .list.held { opacity: 0.35; }
  .more {
    align-self: center;
    height: 34px;
    padding: 0 18px;
    border-radius: 999px;
    font-size: 13px;
    font-weight: 600;
    color: var(--star);
    background: var(--glass);
    box-shadow: inset 0 0 0 0.5px var(--glass-edge);
  }
  .plans-head { margin: 10px 2px -4px; font-size: var(--type-head); font-weight: 600; color: var(--ink-2); }
  .foot { margin-top: 18px; display: flex; flex-direction: column; align-items: center; gap: 10px; }
  .read-only { margin: 0; font-size: 11.5px; line-height: 1.5; color: var(--ink-3); text-align: center; max-width: 34em; }
  @media (hover: hover) {
    .more:hover { background: var(--glass-strong); }
    .stage :global(.card .head:hover), .stage :global(.session .head:hover) { opacity: 0.86; }
  }

  /* MARK: The computer's background as the panes' material. 实色 (the default): panes of their own faint tint, as above.
     磨砂: frosted panes, the sky blurred through them. 玻璃: clear glass, a bright rim and a light along its top edge. */
  .stage.frosted { --glass: rgba(255, 255, 255, 0.09); --glass-edge: rgba(255, 255, 255, 0.14); --pane-blur: blur(22px) saturate(1.5); }
  .stage.frosted.light { --glass: rgba(255, 255, 255, 0.5); --glass-edge: rgba(255, 255, 255, 0.7); }
  .stage.glass {
    --glass: rgba(255, 255, 255, 0.03);
    --glass-edge: rgba(255, 255, 255, 0.24);
    --pane-blur: blur(8px) saturate(1.8);
    --lift: inset 0 1px 0 rgba(255, 255, 255, 0.2), 0 18px 40px -22px rgba(0, 0, 0, 0.75);
  }
  .stage.glass.light {
    --glass: rgba(255, 255, 255, 0.26);
    --glass-edge: rgba(255, 255, 255, 0.8);
    --lift: inset 0 1px 0 rgba(255, 255, 255, 0.95), 0 14px 32px -20px rgba(40, 60, 140, 0.35);
  }
  .stage:is(.frosted, .glass) :is(.trend, .panel, .figure, .turn-done),
  .stage:is(.frosted, .glass) :global(:is(.card, .session, .plan, .tile, .feed)) {
    backdrop-filter: var(--pane-blur);
    -webkit-backdrop-filter: var(--pane-blur);
  }

  /* MARK: 手绘, as the panel draws it (sketch.css): coloured markers on dotted ivory paper by day, coloured chalk on a
     blackboard by night, in the hand's letters (the page inherits them); panes outlined by hand, never filled; the
     lines (curves, rings, bars) wobble a little, the words never do; the lit choice ringed in coral. */
  .stage.sketch {
    --sky-top: var(--paper);
    --sky-mid: var(--paper);
    --sky-low: var(--paper);
    --ink: var(--label);
    --ink-2: var(--secondary);
    --ink-3: var(--tertiary);
    --glass: transparent;
    --glass-strong: var(--hover);
    --glass-edge: var(--separator);
    --line: var(--separator);
    --line-faint: var(--track);
    --lift: 0 0 0 0 transparent;
    --pill: transparent;
    --pill-glow: transparent;
    --shine: rgba(240, 86, 90, 0.22);
    --bubble: var(--paper);
    --star: var(--accent);
    --on-star: var(--on-accent);
    --good: #37b24d;
    --warn: #e8590c;
    --halo: transparent;
  }
  .stage.sketch:not(.light) { --good: #8ce99a; --warn: #ffa94d; }
  .stage.sketch .sky {
    background-color: var(--paper);
    background-image: var(--grain), var(--glow), var(--shade), radial-gradient(circle, var(--dots) 0.75px, transparent 1.1px);
    background-size: 180px 180px, 100% 100%, 100% 100%, 15px 15px;
  }
  .stage.sketch .lights, .stage.sketch .sweep { display: none; }
  .stage.sketch :is(.trend, .panel, .figure, .turn-done),
  .stage.sketch :global(:is(.card, .session, .plan, .tile, .feed)) {
    box-shadow: inset 0 0 0 1.3px var(--separator);
    border-radius: 18px 7px 17px 8px / 8px 17px 7px 18px;
  }
  .stage.sketch .turn-done { background: var(--paper); }
  .stage.sketch .bar.raised { background: color-mix(in srgb, var(--paper) 92%, transparent); box-shadow: inset 0 -1px var(--separator); }
  .stage.sketch :global(:is(.chart svg, .live-chart canvas, .ring svg, .dial svg, .bar .fill, .model-fill, .window-fill, .context span)) { filter: url(#hand); }
  .stage.sketch :global(.pills .pill) {
    background: transparent;
    box-shadow: inset 0 0 0 1.4px var(--coral);
    border-radius: 255px 15px 225px 15px / 15px 225px 15px 255px;
    filter: url(#hand);
  }
  /* The figures in the picture's pens: the headline blue, today green, the minute violet; each card's amount its tool's. */
  .stage.sketch :global(.monitor .big) { color: var(--accent); filter: none; }
  .stage.sketch :global(.tick-value[data-figure='today']) { color: #37b24d; }
  .stage.sketch:not(.light) :global(.tick-value[data-figure='today']) { color: #8ce99a; }
  .stage.sketch :global(.card .amount) { color: var(--tint); }
  .stage.sketch .amount :global(.drawn) {
    text-decoration: underline var(--highlight);
    text-decoration-thickness: 0.42em;
    text-underline-offset: -0.34em;
    text-decoration-skip-ink: none;
  }
</style>
