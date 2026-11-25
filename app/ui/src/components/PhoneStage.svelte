<script lang="ts" module>
  /** The opening plays once a page load (and again when the phone switches to this look): back from the share page,
   *  the page is simply there, where it was left. */
  let played = false;
  let kept = 0;
  export function replayIntro() { played = false; kept = 0; }
</script>

<script lang="ts">
  // 在手机上看, the phone's own look (手机版): the computer's homepage drawn for a phone, under the night of the app's
  // icon. Read-only; the figures are the panel's, counted the same way (Summary's estimate, tokens and requests, the
  // share picture's cache hits), from the same readings App.svelte asks for every 15 seconds.
  //
  // Portrait: one column, top to bottom by importance: the range, the estimate with its three figures, the trend, the
  // tools' shares (星环), the cards (工具 / 模型 / 项目 / 会话), the plans, and at the foot the choice of look. Once the
  // estimate scrolls away it waits in the bar at the top, so the one number that matters stays in sight.
  // Landscape: two columns, the range, the estimate and the trend held still on the left, the rest scrolling on the
  // right, both inside the notch's safe area.
  //
  // Motion, each for a reason: the opening counts the estimate up from 0 (how large it is), draws the trend in from the
  // left (time runs that way), sweeps the shares round (they fill a whole), and raises the cards one after another (in
  // the order they matter). A new reading rolls only the digits that changed and lights the card whose tool grew. The
  // ring round the time fills over the 15 seconds to the next reading. While a range picked is being read, the old
  // figures dim and the star's dots ripple. The sky drifts slowly, and stops while the page is hidden. Reduced motion:
  // everything simply shows.
  import { onMount, untrack } from 'svelte';
  import { byUsage } from '../lib/order';
  import { currency, grouped, summaryPart, tokens } from '../lib/format';
  import { summaryProviders } from '../lib/chart';
  import { t } from '../lib/i18n.svelte';
  import { closeList, listOpen, openList, sessionOpen, toggleSession } from '../lib/expanded.svelte';
  import type { PhoneLook } from '../lib/phoneLook.svelte';
  import { panel } from '../lib/ui.svelte';
  import type { Dashboard, Entry } from '../lib/types';
  import Icon from './Icon.svelte';
  import StageCard from './StageCard.svelte';
  import StageChart from './StageChart.svelte';
  import StageNumber from './StageNumber.svelte';
  import StagePills from './StagePills.svelte';
  import StagePlans from './StagePlans.svelte';
  import StageRing from './StageRing.svelte';
  import StageSession from './StageSession.svelte';

  let { data, rangeOptions, reading = false, grouping = null, regrouping = false, sessions = null, view = 'cards', entries, readAt, problem = null,
    onrange, ongroup, onview, onshare, onlook }: {
    data: Dashboard; rangeOptions: Entry[]; reading?: boolean; grouping?: string | null; regrouping?: boolean;
    sessions?: Entry | null; view?: 'cards' | 'sessions'; entries: Entry[];
    /** When the last reading came in: the ring round the time fills from then to the next. */
    readAt: number;
    /** Why the figures on screen are not fresh. */
    problem?: string | null;
    onrange: (action: string[]) => void; ongroup: (key: string) => void; onview: (view: 'cards' | 'sessions') => void;
    onshare: () => void; onlook: (look: PhoneLook) => void;
  } = $props();

  const intro = !played;
  onMount(() => { played = true; });

  // MARK: The screen

  let screenW = $state(typeof window === 'undefined' ? 390 : window.innerWidth);
  let screenH = $state(typeof window === 'undefined' ? 844 : window.innerHeight);
  /** Turned sideways (or a wide screen): two columns. */
  /** Summary on the homepage (主页模块): the estimate, its figures, the trend and the shares. Without it, as on the panel,
   *  the page is the cards and the plans, one column whichever way up. */
  const summary = $derived(data.showSummary !== false);
  const wide = $derived(summary && screenW >= 600 && screenW > screenH * 1.15);
  let scroller: HTMLDivElement | undefined = $state();
  let scrolled = $state(0);
  let hero: HTMLDivElement | undefined = $state();
  let heroAway = $state(false);
  $effect(() => {
    if (!hero || !scroller || wide) { heroAway = false; return; }
    const observer = new IntersectionObserver(([entry]) => (heroAway = !entry.isIntersecting), { root: scroller, rootMargin: '-56px 0px 0px 0px' });
    observer.observe(hero);
    return () => observer.disconnect();
  });
  onMount(() => {
    if (scroller && kept) scroller.scrollTop = kept;
    // The sky stands still while the page is out of sight.
    const visibility = () => (asleep = document.visibilityState !== 'visible');
    document.addEventListener('visibilitychange', visibility);
    return () => { document.removeEventListener('visibilitychange', visibility); };
  });
  let asleep = $state(false);
  function onscroll() {
    if (!scroller) return;
    scrolled = scroller.scrollTop;
    kept = scrolled;
  }
  // The browser's bars take the sky's colour.
  $effect(() => {
    const colour = panel.dark ? '#0C1A45' : '#EEF1FF';
    let meta = document.querySelector<HTMLMetaElement>('meta[name="theme-color"]');
    if (!meta) { meta = document.createElement('meta'); meta.name = 'theme-color'; document.head.append(meta); }
    meta.content = colour;
    return () => meta?.remove();
  });

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
  /** The estimate as large as the width lets it be. */
  const heroSize = $derived.by(() => {
    // Landscape: beside the three figures, in the left column (43% of the screen, at least 300pt).
    const room = wide ? Math.max(300, screenW * 0.43) - 40 - 150 : Math.min(560, screenW) - 48;
    return Math.round(Math.max(wide ? 30 : 36, Math.min(wide ? 50 : 66, room / (Math.max(4, [...amount].length) * 0.6))));
  });
  const time = $derived(/\d{1,2}:\d{2}/.exec(data.updated ?? '')?.[0] ?? '');
  /** The room the trend has in landscape, measured. */
  let chartRoom = $state(0);

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

<div class="stage" class:light={!panel.dark} class:wide class:still={panel.reduceMotion} class:asleep>
  <div class="sky" aria-hidden="true">
    <div class="aurora one"></div>
    <div class="aurora two"></div>
    <div class="dots"></div>
    {#each STARS as star, i (i)}
      <span class="star" style:left="{star.x}%" style:top="{star.y}%" style:width="{star.size}px" style:height="{star.size}px"
        style:animation-delay="{star.wait}s" style:animation-duration="{star.length}s"></span>
    {/each}
  </div>
  <div class="scroll" bind:this={scroller} {onscroll}>
    <div class="frame">
      <header class="bar" class:raised={!wide && scrolled > 6}>
        <span class="brand">
          <svg class="mark" class:reading viewBox="0 0 24 24" width="22" height="22" aria-hidden="true">
            {#each DOTS as [cx, cy, r], i (i)}
              <circle {cx} {cy} {r} style:--d={Math.hypot(cx - 12, cy - 12) / 4.67} />
            {/each}
          </svg>
          <span class="titles">
            <span class="word" class:away={heroAway}>Starmeter</span>
            <span class="compact num" class:shown={heroAway} aria-hidden={!heroAway}>{amount}<span class="compact-range">{data.rangeLabel}</span></span>
          </span>
        </span>
        <span class="live" class:trouble={!!problem} title={problem ?? data.updated ?? ''}>
          <svg width="16" height="16" viewBox="0 0 16 16" aria-hidden="true">
            <circle cx="8" cy="8" r="6.5" class="live-track" />
            {#key readAt}<circle cx="8" cy="8" r="6.5" class="live-ring" transform="rotate(-90 8 8)" />{/key}
            <circle cx="8" cy="8" r="2.6" class="live-dot" />
          </svg>
          <span class="num">{time}</span>
        </span>
        <button class="share" onclick={onshare} aria-label={t('分享用量图')}><Icon name="square.and.arrow.up" size={18} /></button>
      </header>

      <section class="lead">
        {#if rangeChoices.length > 1}
          <div class="ranges"><StagePills options={rangeChoices} chosen={chosenRange} label={t('统计范围')} onpick={pickRange} busy={reading} scroll bare /></div>
        {/if}
        {#if summary}
        <div class="hero" class:reading bind:this={hero}>
          <div class="halo" aria-hidden="true"></div>
          <div class="hero-label">
            <!-- The range these figures are of: while another range picked is read, still this one. -->
            <span class="range">{data.rangeLabel}</span>
            <span class="sep">·</span>
            <span>{partial ? t('已知 API 估算') : t('API 估算')}</span>
            {#if data.devices}<span class="devices">{t('含 {n} 台电脑', { n: data.devices + 1 })}</span>{/if}
          </div>
          <div class="amount" style:font-size="{heroSize}px"><StageNumber value={amount} count={intro} /></div>
          <div class="figures" class:column={wide}>
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
          <div class="trend-body" bind:clientHeight={chartRoom}>
            <StageChart {data} height={wide ? Math.max(70, chartRoom) : 156} draw={intro} />
          </div>
        </div>
        {/if}
      </section>

      <section class="rest">
        {#if summary && counted.some((p) => p.available && p.tokens > 0)}
          <div class="panel shares" class:reading>
            <div class="panel-head"><span>{t('工具占比')}</span><span class="quiet">{t('按 Token')}</span></div>
            <StageRing {data} draw={intro} size={128} />
          </div>
        {/if}

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

        <footer class="foot">
          <StagePills options={[{ value: 'stage', label: t('手机版') }, { value: 'panel', label: t('电脑版') }]} chosen="stage" label={t('手机上的样式')} onpick={(v) => onlook(v as PhoneLook)} small />
          <p class="read-only">{t('只读：数字来自电脑上的 Starmeter，每 15 秒更新一次')}</p>
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
    position: absolute;
    inset: 0;
    overflow: hidden;
    color: var(--ink);
    background: var(--sky-low);
    font-family: -apple-system, BlinkMacSystemFont, 'SF Pro Text', 'PingFang SC', system-ui, sans-serif;
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
    --sky-top: #DDE4FF;
    --sky-mid: #EEF0FF;
    --sky-low: #FAF9FF;
    --dot: rgba(14, 29, 76, 0.07);
    --halo: rgba(120, 140, 255, 0.22);
  }

  /* MARK: The sky */
  .sky {
    position: absolute;
    inset: 0;
    overflow: hidden;
    background: linear-gradient(165deg, var(--sky-top) 0%, var(--sky-mid) 42%, var(--sky-low) 100%);
    pointer-events: none;
  }
  .aurora {
    position: absolute;
    width: 120vmax;
    height: 120vmax;
    border-radius: 50%;
    will-change: transform;
    animation: drift 38s ease-in-out infinite alternate;
  }
  .aurora.one { left: -40vmax; top: -62vmax; background: radial-gradient(closest-side, rgba(120, 150, 255, 0.34), transparent); }
  .aurora.two { right: -55vmax; top: -20vmax; background: radial-gradient(closest-side, rgba(196, 120, 255, 0.16), transparent); animation-duration: 47s; animation-direction: alternate-reverse; }
  .light .aurora.one { background: radial-gradient(closest-side, rgba(150, 170, 255, 0.45), transparent); }
  .light .aurora.two { background: radial-gradient(closest-side, rgba(255, 180, 220, 0.3), transparent); }
  @keyframes drift {
    from { transform: translate3d(0, 0, 0) rotate(0deg); }
    to { transform: translate3d(8vmax, 6vmax, 0) rotate(25deg); }
  }
  /* The icon's dot grid, faint, brightest near the top. */
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
  .still .aurora, .still .star, .asleep .aurora, .asleep .star { animation-play-state: paused; }
  .still .aurora, .still .star { animation: none; }

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
    padding: 0 max(18px, env(safe-area-inset-right)) calc(28px + env(safe-area-inset-bottom)) max(18px, env(safe-area-inset-left));
    display: flex;
    flex-direction: column;
  }
  .lead, .rest { display: flex; flex-direction: column; gap: 16px; min-width: 0; }
  .rest { margin-top: 18px; }
  .wide .scroll { overflow: hidden; }
  .wide .frame {
    max-width: none;
    height: 100%;
    box-sizing: border-box;
    padding: env(safe-area-inset-top) max(20px, env(safe-area-inset-right)) 0 max(20px, env(safe-area-inset-left));
    display: grid;
    grid-template-columns: minmax(300px, 43%) minmax(0, 1fr);
    grid-template-rows: auto minmax(0, 1fr);
    column-gap: 22px;
  }
  .wide .bar { grid-column: 1; grid-row: 1; }
  .wide .lead { grid-column: 1; grid-row: 2; gap: 10px; padding-bottom: max(14px, env(safe-area-inset-bottom)); min-height: 0; }
  .wide .rest {
    grid-column: 2;
    grid-row: 1 / span 2;
    margin: 0;
    overflow-y: auto;
    -webkit-overflow-scrolling: touch;
    padding: 14px 2px calc(24px + env(safe-area-inset-bottom));
    scrollbar-width: none;
  }
  .wide .rest::-webkit-scrollbar { display: none; }

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
  .bar.raised {
    background: color-mix(in srgb, var(--sky-mid) 86%, transparent);
    backdrop-filter: blur(18px) saturate(1.5);
    -webkit-backdrop-filter: blur(18px) saturate(1.5);
    box-shadow: 0 0.5px 0 var(--glass-edge);
  }
  .wide .bar { position: relative; margin: 0; padding: 0; height: 50px; }
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
  .word { font-size: 17px; font-weight: 700; letter-spacing: -0.01em; }
  .word.away { transform: translateY(-100%); opacity: 0; }
  .compact { font-family: var(--rounded); font-size: 17px; font-weight: 700; transform: translateY(100%); opacity: 0; }
  .compact.shown { transform: none; opacity: 1; }
  .compact-range { margin-left: 8px; font-family: -apple-system, system-ui, sans-serif; font-size: 12px; font-weight: 500; color: var(--ink-2); }
  .live { flex: none; display: flex; align-items: center; gap: 6px; font-size: 12.5px; color: var(--ink-2); }
  .live svg { display: block; }
  .live-track { fill: none; stroke: var(--line-faint); stroke-width: 1.6; }
  .live-ring {
    fill: none;
    stroke: var(--good);
    stroke-width: 1.6;
    stroke-linecap: round;
    stroke-dasharray: 40.84;
    stroke-dashoffset: 40.84;
    animation: countdown 15s linear forwards;
  }
  @keyframes countdown { to { stroke-dashoffset: 0; } }
  .still .live-ring { animation: none; stroke-dashoffset: 0; opacity: 0.5; }
  .live-dot { fill: var(--good); }
  .trouble .live-dot { fill: var(--warn); }
  .trouble .live-ring { display: none; }
  .share {
    flex: none;
    width: 36px;
    height: 36px;
    margin-right: -6px;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--ink);
    transition: background-color 0.2s, transform 0.2s;
  }
  .share:active { background: var(--glass-strong); transform: scale(0.92); }

  /* MARK: The estimate */
  /* The ranges run to the screen's edges and fade there, so a range past the edge reads as one to scroll to. */
  .ranges {
    margin: 4px -18px 0;
    mask-image: linear-gradient(90deg, transparent, #000 18px, #000 calc(100% - 28px), transparent);
    -webkit-mask-image: linear-gradient(90deg, transparent, #000 18px, #000 calc(100% - 28px), transparent);
  }
  .ranges :global(.pills) { max-width: none; width: 100%; padding: 3px 18px; }
  .wide .ranges { margin: 0 -20px 0 -20px; }
  .wide .ranges :global(.pills) { padding: 3px 20px; }
  .hero { position: relative; padding: 10px 0 2px; transition: opacity 0.35s; }
  /* Landscape: the estimate and its three figures side by side, so the trend under them has the height. */
  .wide .hero {
    padding: 2px 0 0;
    display: grid;
    grid-template-columns: auto minmax(130px, 1fr);
    grid-template-rows: auto auto;
    column-gap: 16px;
    align-items: end;
  }
  .wide .hero-label { grid-column: 1 / -1; }
  .reading { opacity: 0.45; }
  .halo {
    position: absolute;
    left: -10%;
    top: -30%;
    width: 80%;
    height: 140%;
    background: radial-gradient(closest-side, var(--halo), transparent);
    pointer-events: none;
    z-index: -1;
  }
  .hero-label { display: flex; align-items: center; flex-wrap: wrap; gap: 6px; font-size: 13px; color: var(--ink-2); }
  .range { color: var(--ink); font-weight: 600; }
  .sep { opacity: 0.5; }
  .devices { padding: 1px 8px; border-radius: 999px; font-size: 11px; background: var(--glass-strong); }
  /* The estimate in starlight: near-white with the icon's glow round it (by day, the icon's navy). */
  .amount {
    margin: -2px 0 -4px -2px;
    font-family: var(--rounded);
    font-weight: 700;
    letter-spacing: -0.02em;
    color: #F5F8FF;
    line-height: 1.1;
    /* The glow round the digits as drawn (each wheel clips its own text, so a text shadow would show as boxes). */
    filter: drop-shadow(0 0 16px rgba(196, 216, 255, 0.5));
  }
  .light .amount { color: #0E1D4C; filter: drop-shadow(0 0 14px rgba(120, 140, 255, 0.28)); }
  .figures { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 10px; margin-top: 14px; }
  .figures.column { grid-template-columns: none; gap: 2px; margin: 0 0 6px; padding: 6px 12px; border-radius: 14px; background: var(--glass); box-shadow: inset 0 0 0 0.5px var(--glass-edge); }
  .figure {
    padding: 10px 12px 11px;
    border-radius: 16px;
    background: var(--glass);
    box-shadow: inset 0 0 0 0.5px var(--glass-edge);
    min-width: 0;
  }
  .column .figure { display: flex; align-items: baseline; justify-content: space-between; gap: 8px; padding: 0; background: none; box-shadow: none; border-radius: 0; }
  .figure-label { display: block; font-size: 11px; color: var(--ink-2); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .figure-value { display: block; margin-top: 3px; font-family: var(--rounded); font-size: 21px; font-weight: 700; color: var(--ink); }
  .column .figure-value { margin: 0; font-size: 15px; }

  /* MARK: Panels */
  .trend, .panel {
    padding: 14px 14px 12px;
    border-radius: 22px;
    background: var(--glass);
    box-shadow: inset 0 0 0 0.5px var(--glass-edge), var(--lift);
    transition: opacity 0.35s;
  }
  .wide .trend { flex: 1; min-height: 0; display: flex; flex-direction: column; padding: 10px 12px 8px; }
  .trend-head, .panel-head { display: flex; align-items: baseline; justify-content: space-between; gap: 10px; margin-bottom: 8px; font-size: 13px; font-weight: 600; }
  .span, .quiet { font-size: 11px; font-weight: 500; color: var(--ink-3); }
  .trend-body { position: relative; }
  .wide .trend-body { flex: 1; min-height: 0; overflow: visible; }
  .tabs { display: flex; justify-content: center; margin-top: 4px; }
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
  .plans-head { margin: 10px 2px -4px; font-size: 13px; font-weight: 600; color: var(--ink-2); }
  .foot { margin-top: 18px; display: flex; flex-direction: column; align-items: center; gap: 10px; }
  .read-only { margin: 0; font-size: 11.5px; line-height: 1.5; color: var(--ink-3); text-align: center; max-width: 34em; }
</style>
