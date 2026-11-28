<script lang="ts" module>
  import { PhoneLive } from '../lib/phoneLive.svelte';
  /** The opening plays once a page load: back from the share page, the page is simply there, where it was left. */
  let played = false;
  let kept = 0;
  /** The live view's readings and what happened, kept while the share page is on screen (asked for only while this page is). */
  const live = new PhoneLive();
  /** The last turn done said at the top: said once, not again each time 网页版 comes back (from the share page). */
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
  import { usageShare } from '../lib/detail';
  import { t } from '../lib/i18n.svelte';
  import { rateText } from '../lib/live';
  import { closeList, listOpen, openList, sessionOpen, toggleSession } from '../lib/expanded.svelte';
  import { panel } from '../lib/ui.svelte';
  import { paintEdges } from '../lib/edges';
  import { ScreenLock } from '../lib/screenLock';
  import { style } from '../lib/style.svelte';
  import type { Dashboard, Entry } from '../lib/types';
  import Icon from './Icon.svelte';
  import StageActivity from './StageActivity.svelte';
  import StageCard from './StageCard.svelte';
  import StageDrift from './StageDrift.svelte';
  import StageChart from './StageChart.svelte';
  import StageMonitor from './StageMonitor.svelte';
  import StageNumber from './StageNumber.svelte';
  import StagePills from './StagePills.svelte';
  import StagePlans from './StagePlans.svelte';
  import StageRing from './StageRing.svelte';
  import StageSession from './StageSession.svelte';

  let { data, today = null, rangeOptions, reading = false, grouping = null, regrouping = false, sessions = null, view = 'cards', entries, problem = null,
    material = 'solid', onrange, ongroup, onview, onshare }: {
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
    onshare: () => void;
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
  const desk = $derived(layout === 'desk');
  /** Summary on the homepage (主页模块): the estimate, its figures, the trend and the shares; without it, as on the
   *  panel, the figures for a range are the cards and the plans. */
  const summary = $derived(data.showSummary !== false);
  let scroller: HTMLDivElement | undefined = $state();
  let raised = $state(false);
  let headline: HTMLDivElement | undefined = $state();
  let headlineAway = $state(false);
  $effect(() => {
    if (!headline || !scroller || wide) { headlineAway = false; return; }
    const observer = new IntersectionObserver(([entry]) => (headlineAway = !entry.isIntersecting), { root: scroller, rootMargin: '-56px 0px 0px 0px' });
    observer.observe(headline);
    return () => observer.disconnect();
  });
  /** The height the page is seen at: on its side, Safari's tab and address bars over the top of the screen leave less
   *  than the window's height (100dvh where the browser can't say), and the monitor held still must fit in it. */
  let visible = $state<number | null>(null);
  onMount(() => {
    const view = window.visualViewport;
    if (!view) return;
    const measure = () => { visible = view.height; };
    measure();
    view.addEventListener('resize', measure);
    return () => view.removeEventListener('resize', measure);
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
    kept = scroller.scrollTop;
    const next = kept > 6;
    if (next !== raised) raised = next;
  }
  // The browser's bars take the sky's own edges (its top band and its foot are flat for them), in the computer's theme,
  // look and background.
  let stageBox: HTMLDivElement | undefined = $state();
  /** Over the page: where a tap's stars and the light on a curve are drawn (StageDrift), over the panes. */
  let tapLayer: HTMLCanvasElement | undefined = $state();
  const sketch = $derived(style.current === 'sketch');
  const driftLook = $derived(sketch ? (panel.dark ? 'sketch-dark' : 'sketch-light') : panel.dark ? 'dark' : 'light');
  const pane = $derived(material === 'frosted' || material === 'glass' ? material : 'solid');
  $effect(() => {
    void panel.dark; void sketch; void pane;
    const box = stageBox;
    if (!box) return;
    const frame = requestAnimationFrame(() => {
      const css = getComputedStyle(box);
      paintEdges(css.getPropertyValue('--edge-top').trim(), css.getPropertyValue('--edge-bottom').trim(), css.getPropertyValue('--sky-mid').trim() || undefined);
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
  /** The estimate's range in the bar: today, or on SwiftBar's panel the range the computer shows (STAGE-10). */
  const compactRange = $derived(live.dashboard && live.dashboard.rangeKey !== 'today' ? live.dashboard.rangeLabel : t('今天'));
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

  // Keeping the screen on (a monitor by the desk): where the browser offers it, an https page (the public link; iOS 16.4
  // on). One lock at most, let go when the switch goes off or this page goes away (to the share page). On an http page
  // (the same network, Tailscale) the browser has no such thing: the sun is there all the same, fainter, and says so
  // when tapped, so the feature is known of (a looping silent video, the old way round it, is no longer to be relied
  // on in iOS's Safari and costs a phone's battery; not tried).
  const lockable = ScreenLock.offered();
  const screenLock = lockable ? ScreenLock.browser() : null;
  let awake = $state(false);
  /** What the switch just did, said under the bar for a moment (a phone has no tooltip to tell what a sun means). */
  let notice = $state<{ text: string; at: number; long: boolean } | null>(null);
  function say(text: string, long = false) {
    const at = Date.now();
    notice = { text, at, long };
    setTimeout(() => { if (notice?.at === at) notice = null; }, long ? 5200 : 3200);
  }
  /** Why the sun can't keep the screen on here. */
  const unoffered = () => t(typeof isSecureContext !== 'undefined' && !isSecureContext
    ? '保持亮屏要用 https 链接：用电脑上的 Cloudflare 临时链接或 https 的自有域名打开就能用' : '这个浏览器不支持保持亮屏（iPhone 需要 iOS 16.4 以上）');
  const refused = () => t(screenLock?.refusal === 'NotAllowedError'
    ? '没能保持亮屏：浏览器没有允许（低电量模式下会这样）' : '没能保持亮屏：这个浏览器不支持');
  /** Each turn of the switch; only the last says what it did (STAGE-19: switched off before the lock came, the page
   *  said the screen would stay on). */
  let switched = 0;
  function holdScreen(on: boolean) {
    if (!screenLock) { say(unoffered(), true); return; }
    awake = on;
    const turn = ++switched;
    void screenLock?.hold(on).then((held) => {
      if (turn !== switched) return;
      if (!on) say(t('已恢复自动锁屏'));
      else if (held) say(t('屏幕会保持常亮，离开这一页就恢复自动锁屏'));
      else { awake = false; say(refused()); }
    });
  }
  // The system lets go of it when the page is hidden: taken again when it comes back.
  // Refused on coming back (low power mode), the switch goes off rather than stay lit holding nothing (STAGE-10).
  $effect(() => { if (awake && !asleep) untrack(() => void screenLock?.resume().then((held) => { if (!held) { awake = false; say(refused()); } })); });
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
  /** On a computer's screen and on its side the curve fills the monitor (as tall as what is beside it); upright, its own. */
  const chartHeight = $derived(layout === 'narrow' ? 196 : null);

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

  // MARK: A computer's screen: two columns of figures, level at both ends
  // Under the monitor, in its two columns: the figures for a range and their trend head the left, the cards the
  // right. The tools' shares, the plans and the other rows' tiles go to whichever column leaves the two closest in
  // height, of every way to share them out, however many tools and plans there are (用户：就算选择的工具数量不同也
  // 自适应排版对齐); what is left between them is taken up by the shorter column's blocks growing (the trend's chart
  // first), so the two end on one line, and go on doing so as a card opens. Placed when the screen or what there is to
  // place changes, not as a card opens or a grouping is picked (nothing moves under the pointer); hidden till placed.
  type Side = 'left' | 'right';
  const BLOCKS = ['shares', 'plans', 'tiles'] as const;
  type Block = (typeof BLOCKS)[number];
  /** Where each goes when it makes no difference. */
  const HOME: Record<Block, Side> = { shares: 'left', plans: 'right', tiles: 'right' };
  let place = $state<Record<Block, Side>>({ ...HOME });
  let placed = $state(false);
  let statsBox: HTMLDivElement | undefined = $state();
  /** The columns' heads and the blocks, as they stand on the page, to be measured. */
  const boxes: Partial<Record<Block | 'a' | 'b', HTMLElement | null>> = {};
  /** The room the trend's chart has on a computer's screen: it fills it. */
  let trendRoom = $state(0);
  const shownRows = $derived(entries.filter((e) => !e.separator && !e.section && e.label));
  const hasPlans = $derived(shownRows.some((e) => usageShare(e) !== null));
  const hasTiles = $derived(shownRows.some((e) => usageShare(e) === null));
  /** The plans and the tiles in different columns: each its own heading. */
  const apart = $derived(hasPlans && hasTiles && place.plans !== place.tiles);
  const shape = $derived(`${layout}|${screenW}|${shownRows.map((e) => e.label).join('·')}|${data.providers.length}|${summary}|${showingSessions}`);
  /** The way to share the blocks out that leaves the columns closest; between ways as close, the one with most at home. */
  function shared(a: number, b: number, sizes: number[], gap: number): Record<Block, Side> {
    let best: { cost: number; sides: Side[] } | null = null;
    for (let way = 0; way < 1 << BLOCKS.length; way++) {
      const sides = BLOCKS.map((key, i): Side => (!sizes[i] ? HOME[key] : way & (1 << i) ? 'right' : 'left'));
      let left = a, right = b, away = 0;
      sides.forEach((side, i) => {
        if (!sizes[i]) return;
        if (side === 'left') left += gap + sizes[i]; else right += gap + sizes[i];
        if (side !== HOME[BLOCKS[i]]) away++;
      });
      const cost = Math.abs(left - right) + away * 6;
      if (!best || cost < best.cost) best = { cost, sides };
    }
    return Object.fromEntries(BLOCKS.map((key, i) => [key, best!.sides[i]])) as Record<Block, Side>;
  }
  // The capsules' edges across the two columns (用户：我是说胶囊没对齐……主要影响美观的就是一个个胶囊): the three figures
  // stand beside the first card, as tall as it and on its line; the trend starts on the line of what follows that card
  // and ends on a card's foot; the cards' tabs stand beside the range's estimate, over the cards. Measured from the
  // page (a card is as tall as what it says) with nothing grown, and written as the layout's own variables: nothing
  // is drawn again for it.
  //   With something under the trend in its column (the shares, plans or tiles: they even the columns' ends) and
  //   cards enough beside it: the trend ends on a card's foot (`snap`), of those that leave its chart its least the
  //   one that leaves the columns closest in height; the cards past that foot may grow (`grow`).
  //   Alone in its column, or with too few cards beside it, it grows to even the columns as before, the cards after
  //   the first growing if theirs is the shorter.
  const VARIABLES = ['--words-top', '--words-height', '--row', '--trend-shift', '--trend-height'];
  function align(): boolean {
    const box = statsBox;
    if (!box) return false;
    let changed = false;
    const put = (name: string, value: number | null) => {
      const text = value === null ? '' : `${Math.round(value * 2) / 2}px`;
      if (box.style.getPropertyValue(name) === text) return;
      if (text) box.style.setProperty(name, text); else box.style.removeProperty(name);
      changed = true;
    };
    const mark = (name: string, value: string | null) => {
      if ((box.dataset[name] ?? null) === value) return;
      if (value === null) delete box.dataset[name]; else box.dataset[name] = value;
      changed = true;
    };
    const trendAs = (shift: number | null, height: number | null, grow: string | null) => {
      put('--trend-shift', shift);
      put('--trend-height', height);
      mark('snap', height === null ? null : '');
      mark('grow', grow);
    };
    if (!desk) {
      for (const name of VARIABLES) put(name, null);
      trendAs(null, null, null);
      return changed;
    }
    // As each is of itself: the columns not stretched, no card grown (read at once, nothing drawn in between).
    const measuring = box.classList.contains('measuring');
    box.classList.add('measuring');
    // Where each is laid out, from the figures' top (the layout's own places: a card still rising in, scaled and
    // moved by its entrance, is measured where it will stand).
    const top = 0;
    const metric = (element: HTMLElement) => {
      let from = 0;
      for (let node: HTMLElement | null = element; node && node !== box; node = node.offsetParent as HTMLElement | null) from += node.offsetTop;
      return { top: from, bottom: from + element.offsetHeight, height: element.offsetHeight };
    };
    const rect = (selector: string) => { const element = box.querySelector<HTMLElement>(selector); return element ? metric(element) : null; };
    const words = rect('.stats-a .hero-words');
    const rows = [...box.querySelectorAll<HTMLElement>('.stats-b .list > :is(.card, .session)')];
    const opened = rows.some((row) => row.classList.contains('open'));
    const boxes = rows.map(metric);
    const first = boxes[0] ?? null;
    const figures = rect('.stats-a .figures'), trend = rect('.stats-a .trend'), chart = rect('.stats-a .trend-chart'), list = rect('.stats-b .list');
    const after = first ? [...box.querySelectorAll<HTMLElement>('.stats-b :is(.card, .session, .panel, .plan, .tile)')]
      .map(metric).filter((r) => r.top >= first.bottom - 0.5) : [];
    const gap = parseFloat(getComputedStyle(box.querySelector('.stats-a .hero') ?? box).rowGap) || 16;
    const under = !!box.querySelector('.stats-a > :is(.shares, .part.fills)');
    // The columns as tall as what is in them.
    const left = box.querySelector<HTMLElement>('.stats-a')?.offsetHeight ?? 0, right = box.querySelector<HTMLElement>('.stats-b')?.offsetHeight ?? 0;
    if (!measuring) box.classList.remove('measuring');

    // The tabs beside the range's words.
    put('--words-top', words ? words.top - top : null);
    put('--words-height', words ? words.height : null);
    // With a card opened, the cards' closed heights aren't there to read: the rest as it was.
    if (opened) return changed;
    put('--row', first ? first.height : null);
    if (!first || !figures || !trend || !chart || !list) {
      trendAs(null, null, null);
      return changed;
    }
    // The trend's top: on the line of what follows the first card (its own place is under the figures, the gap on).
    const next = after.length ? Math.min(...after.map((r) => r.top)) : null;
    const own = figures.top + Math.max(figures.height, first.height) + gap;
    const shift = next === null ? null : next - own;
    const from = next ?? own;
    const least = trend.height - chart.height + 190;
    // The cards' feet it could end on, and how far apart each would leave the columns.
    const feet = under ? boxes.slice(1).map((r) => r.bottom).filter((bottom) => bottom - from >= least - 0.5) : [];
    const apart = (foot: number) => Math.abs(right - (left - trend.height + (foot - from)));
    const foot = feet.length ? feet.reduce((best, each) => (apart(each) < apart(best) ? each : best)) : null;
    const beside = foot === null ? 1 : boxes.filter((r) => r.bottom <= foot + 0.5).length;
    trendAs(shift, foot === null ? null : foot - from, rows.length > beside ? String(Math.min(beside, 8)) : null);
    return changed;
  }
  /** Measured again over the next frames, until three in a row change nothing (what one measure moves, the next sees). */
  let settling = 0;
  function settle(passes = 0, calm = 0) {
    cancelAnimationFrame(settling);
    settling = requestAnimationFrame(() => {
      const still = align() ? 0 : calm + 1;
      if (still < 3 && passes < 40) settle(passes + 1, still);
    });
  }
  $effect(() => {
    void shape; void data; void placed; void place; void showingSessions;
    const box = statsBox;
    if (!box) return;
    align();
    settle();
    // Again whenever what is measured changes size (a card's note coming, the window, a list longer).
    const observer = new ResizeObserver(() => settle());
    for (const element of box.querySelectorAll('.stats-a, .stats-b, .hero-words, .list, .list > *')) observer.observe(element);
    return () => { observer.disconnect(); cancelAnimationFrame(settling); };
  });
  $effect(() => {
    void shape;
    if (!desk) { place = { ...HOME }; placed = true; return; }
    const frame = requestAnimationFrame(() => untrack(() => {
      const box = statsBox;
      if (!box) return;
      align();
      // Measured as they are of themselves, not grown to fill their column: for this one look the columns aren't
      // stretched (read at once, nothing drawn in between).
      box.classList.add('measuring');
      const tall = (element: HTMLElement | null | undefined) => element?.offsetHeight ?? 0;
      const a = tall(boxes.a), b = tall(boxes.b), sizes = BLOCKS.map((key) => tall(boxes[key]));
      const gap = parseFloat(getComputedStyle(box.firstElementChild ?? box).rowGap) || 16;
      box.classList.remove('measuring');
      place = shared(a, b, sizes, gap);
      placed = true;
    }));
    return () => cancelAnimationFrame(frame);
  });

  // MARK: Cards

  /** The cards rise in at the opening, with the page's first frame, and again whenever another range, grouping or 会话
   *  brings new ones. */
  let changed = $state(false);
  let firstShape: string | null = null;
  $effect(() => {
    const shape = `${data.rangeKey}/${data.grouping ?? 'tool'}/${showingSessions}`;
    if (firstShape === null) firstShape = shape;
    else if (shape !== firstShape) changed = true;
  });
  const entering = $derived(intro || changed);

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

  // For today's range, prefer the live tick's fresh 会话 entry (updated every 2-2.5s) over the 15s snapshot poll.
  const activeSessions = $derived(sessions && data.rangeKey === 'today' && live.sessionsEntry ? live.sessionsEntry : sessions);
  const sessionRows = $derived(activeSessions?.children.filter((c) => c.statusLight !== undefined) ?? []);
  const sessionKey = (s: Entry, i: number) => s.key ?? `${s.label}\u001f${i}`;
  const everySession = $derived(listOpen('sessions'));
  const sessionsListed = $derived(everySession ? sessionRows : sessionRows.slice(0, SHOWN));
  const sessionNotes = $derived(activeSessions?.children.filter((c) => c.statusLight === undefined && !c.separator && c.checked === undefined && !c.action) ?? []);

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

<div class="stage {layout} {pane}" class:light={!panel.dark} class:sketch class:wide class:still={panel.reduceMotion} class:asleep class:working class:waking bind:this={stageBox}
  style:--visible-h={visible ? `${visible}px` : null}>
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
    <!-- Things floating in the sky, more and faster as the models write (StageDrift.svelte). -->
    <StageDrift {live} look={driftLook} still={panel.reduceMotion} {asleep} burst={live.done && doneShown === live.done.at ? live.done.at : 0} overlay={tapLayer} />
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
      <header class="bar" class:raised={!wide && raised}>
        <span class="brand">
          <svg class="mark" class:reading={reading || live.trouble} viewBox="0 0 24 24" width="22" height="22" aria-hidden="true">
            {#each DOTS as [cx, cy, r], i (i)}
              <circle {cx} {cy} {r} style:--d={Math.hypot(cx - 12, cy - 12) / 4.67} />
            {/each}
          </svg>
          <span class="titles">
            <span class="word" class:away={headlineAway && !!compact}>Starmeter</span>
            <span class="compact num" class:shown={headlineAway && !!compact} aria-hidden={!headlineAway}>{compact}<span class="compact-range">{working ? t('正在工作') : compactRange}</span></span>
          </span>
        </span>
        <button class="awake" class:on={awake} class:unoffered={!lockable} aria-pressed={awake} aria-label={t('保持亮屏')}
          title={!lockable ? unoffered() : awake ? t('屏幕保持常亮，点按恢复自动锁屏') : t('保持亮屏：屏幕不自动锁定')}
          onclick={() => holdScreen(!awake)}>
          <svg width="18" height="18" viewBox="0 0 20 20" aria-hidden="true"><circle cx="10" cy="10" r="3.4" /><path d="M10 2.5v2M10 15.5v2M2.5 10h2M15.5 10h2M4.7 4.7l1.4 1.4M13.9 13.9l1.4 1.4M4.7 15.3l1.4-1.4M13.9 6.1l1.4-1.4" /></svg>
        </button>
        {#if notice}{#key notice.at}<p class="notice" class:long={notice.long} role="status">{notice.text}</p>{/key}{/if}
        <span class="live" class:trouble={live.trouble || !!problem} title={problem ?? ''}>
          {#key live.version}<span class="live-dot" aria-hidden="true"></span>{/key}
          <span class="num">{live.trouble ? t('连不上电脑') : working ? t('实时') : lastRead}</span>
        </span>
        <button class="share" onclick={onshare} aria-label={t('分享用量图')}><Icon name="square.and.arrow.up" size={18} /></button>
      </header>

      <section class="lead" style:--big="{big}px">
        <div class="monitor-box" bind:this={headline}>
          <StageMonitor {live} today={today ?? (data.rangeKey === 'today' ? data : null)} {now} {chartHeight} least={desk ? 280 : 64} {intro} />
        </div>
      </section>

      <section class="rest">
        <div class="activity-box"><StageActivity {live} {now} carded={desk} /></div>

        <div class="stats" bind:this={statsBox}>
          <div class="stats-a" class:placing={!placed}>
            <div class="part figures-part" bind:this={boxes.a}>
            <div class="stats-head">{t('统计')}</div>
            {#if rangeChoices.length > 1}
              <div class="ranges"><StagePills options={rangeChoices} chosen={chosenRange} label={t('统计范围')} onpick={pickRange} busy={reading} scroll bare /></div>
            {/if}
            {#if summary}
              <div class="hero" class:reading>
                <div class="hero-words">
                <div class="hero-label">
                  <!-- The range these figures are of: while another range picked is read, still this one. -->
                  <span class="range">{data.rangeLabel}</span>
                  <span class="sep">·</span>
                  <span>{partial ? t('已知 API 估算') : t('API 估算')}</span>
                  {#if data.devices}<span class="devices">{t('含 {n} 台电脑', { n: data.devices + 1 })}</span>{/if}
                </div>
                <div class="amount" data-tap="hop"><StageNumber value={amount} count={intro} /></div>
                </div>
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
                <div class="trend-chart" bind:clientHeight={trendRoom}>
                  <StageChart {data} height={desk ? Math.max(190, trendRoom) : 156} draw={intro} />
                </div>
              </div>
            {/if}
            </div>
            {@render blocks('left')}
          </div>

          <div class="stats-b" class:placing={!placed}>
            <div class="part cards" bind:this={boxes.b}>
            {#if tabs.length > 1}
              <div class="tabs"><StagePills options={tabs} chosen={showingSessions ? 'sessions' : wantedGrouping} label={t('卡片分组')} onpick={pickTab} small /></div>
            {:else if desk}
              <!-- A computer's screen with no tabs to show: their row all the same, the cards starting where the figures do. -->
              <div class="tabs" aria-hidden="true"></div>
            {/if}
            {#if !showingSessions && !regrouping && data.groupNote}<p class="note">{data.groupNote}</p>{/if}

            {#if showingSessions && sessions}
              <div class="list">
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
              <div class="list" class:held={regrouping} inert={regrouping} aria-busy={regrouping}>
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

            </div>
            {@render blocks('right')}
            {#if !desk && entries.some((e) => !e.separator && e.label)}
              <div class="plans-head">{t('订阅与服务')}</div>
              <StagePlans {entries} enter={intro} />
            {/if}
          </div>
        </div>
        {#snippet blocks(side: Side)}
          <!-- A column's blocks, in one order in either: the shares, the plans, the tiles. -->
          {#if summary && place.shares === side && counted.some((p) => p.available && p.tokens > 0)}
            <div class="panel shares" class:reading bind:this={boxes.shares}>
              <div class="panel-head"><span>{t('工具占比')}</span><span class="quiet">{t('按 Token')}</span></div>
              <div class="shares-ring"><StageRing {data} draw={intro} size={128} /></div>
            </div>
          {/if}
          {#if desk && hasPlans && place.plans === side}
            <div class="part fills" bind:this={boxes.plans}>
              <div class="plans-head">{t(apart ? '订阅' : '订阅与服务')}</div>
              <StagePlans {entries} enter={intro} only="plans" fill />
            </div>
          {/if}
          {#if desk && hasTiles && place.tiles === side}
            <div class="part fills" bind:this={boxes.tiles}>
              {#if apart || !hasPlans}<div class="plans-head">{t(apart ? '服务' : '订阅与服务')}</div>{/if}
              <StagePlans {entries} enter={intro} only="tiles" fill />
            </div>
          {/if}
        {/snippet}

        <footer class="foot">
          <p class="read-only">{live.interval >= 3
            ? t('只读：实时数字每 {n} 秒从电脑上的 Starmeter 读一次', { n: live.interval })
            : t('只读：实时数字每 {n} 秒从电脑上的 Starmeter 读一次，休息时每 2.5 秒', { n: live.interval })}</p>
        </footer>
      </section>
    </div>
  </div>
  <!-- A tap's stars and ring, over everything and in the way of nothing (StageDrift draws them). -->
  <canvas class="taps" bind:this={tapLayer} aria-hidden="true"></canvas>
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
    --sky-top: var(--night-top);
    --sky-mid: var(--night-mid);
    --sky-low: var(--night-low);
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
  .taps { position: absolute; inset: 0; width: 100%; height: 100%; z-index: 35; pointer-events: none; }
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
  .asleep .aurora, .asleep .star, .asleep :global(.tip), .asleep :global(.orbit), .asleep :global(.beat::after) { animation-play-state: paused; }
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
  /* As tall as what is seen of the page, not the window's height under Safari's bars. */
  .side .scroll { overflow: hidden; bottom: auto; height: min(100%, var(--visible-h, 100dvh)); }
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
  /* The curve gives way first; past its least, the monitor scrolls rather than lose its foot. */
  .side .lead {
    grid-column: 1;
    grid-row: 2;
    padding-bottom: max(12px, env(safe-area-inset-bottom));
    min-height: 0;
    overflow-y: auto;
    scrollbar-width: none;
    display: grid;
    grid-template-rows: minmax(0, 1fr);
  }
  .side .lead::-webkit-scrollbar { display: none; }
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
    /* One pair of columns down the whole page: the figures under the monitor keep its columns, one gutter from the top
       to the foot (用户：这都没对齐). */
    --columns: minmax(0, 1.6fr) minmax(340px, 1fr);
    max-width: 1320px;
    box-sizing: border-box;
    padding: 0 40px 40px;
    display: grid;
    grid-template-columns: var(--columns);
    column-gap: 28px;
    align-items: start;
  }
  .desk .bar { grid-column: 1 / -1; margin: 0 -40px; padding: 0 40px; height: 64px; }
  /* The monitor as tall as what is beside it, its curve filling it; what happened reaching down to the same line. Never
     taller than what is seen under the bar, though: with many sessions and long titles beside it the curve would grow
     past the screen's foot, its 1 分钟前 · 现在 out of sight. */
  .desk .lead {
    grid-column: 1;
    align-self: stretch;
    max-height: calc(var(--visible-h, 100dvh) - 64px - 30px);
    display: flex;
    flex-direction: column;
    margin-top: 6px;
  }
  .desk .monitor-box { flex: 1; display: flex; flex-direction: column; }
  .desk .monitor-box > :global(.monitor) { flex: 1; }
  .desk .rest { display: contents; }
  .desk .activity-box { grid-column: 2; align-self: stretch; display: flex; flex-direction: column; margin-top: 6px; }
  .desk .activity-box > :global(.activity) { flex: 1; }
  .desk .activity-box :global(.feed) { flex: 1; }
  /* The two columns as tall as each other: the shorter one's blocks grow into what is left (the trend's chart most, then
     the plans and the tiles, the shares' ring, the cards), so both end on one line. */
  .desk .stats { position: relative; grid-column: 1 / -1; display: grid; grid-template-columns: var(--columns); column-gap: 28px; align-items: stretch; margin-top: 22px; }
  /* Measured (the script, for a moment): each column as tall as what is in it. */
  .desk .stats:global(.measuring) { align-items: start; }
  .desk .stats:global(.measuring) .trend { height: auto; }
  .part { display: flex; flex-direction: column; gap: 16px; min-width: 0; }
  .placing { visibility: hidden; }
  .desk .part.figures-part { flex: 6 1 auto; }
  /* The trend's top on the line of what follows the first card beside it (the script's measure). */
  .desk .trend { flex: 1 1 auto; display: flex; flex-direction: column; margin-top: var(--trend-shift, 0px); }
  /* Ending on a card's foot: as tall as the script found, the column's other blocks evening the ends. */
  .desk .stats:global([data-snap]) .part.figures-part { flex: 0 0 auto; }
  .desk .stats:global([data-snap]) .trend { flex: 0 0 auto; height: var(--trend-height); box-sizing: border-box; }
  .desk .trend-chart { position: relative; flex: 1 1 auto; min-height: 190px; }
  .desk .trend-chart > :global(.chart) { position: absolute; left: 0; right: 0; top: 0; }
  .desk .shares { flex: 1 1 auto; display: flex; flex-direction: column; }
  .desk .shares-ring { margin-block: auto; }
  .desk .part.fills { flex: 2 1 auto; }
  /* The cards as tall as what they say, so their edges are the ones the left column lines up with; only those past
     the trend's foot may grow, to even the columns' ends (data-grow: how many stay as they are). */
  .desk .stats:global([data-grow]) .part.cards, .desk .stats:global([data-grow]) .part.cards .list { flex: 1 1 auto; }
  .desk .stats:global([data-grow="1"]) .list > :global(:is(.card, .session):nth-child(n + 2)),
  .desk .stats:global([data-grow="2"]) .list > :global(:is(.card, .session):nth-child(n + 3)),
  .desk .stats:global([data-grow="3"]) .list > :global(:is(.card, .session):nth-child(n + 4)),
  .desk .stats:global([data-grow="4"]) .list > :global(:is(.card, .session):nth-child(n + 5)),
  .desk .stats:global([data-grow="5"]) .list > :global(:is(.card, .session):nth-child(n + 6)),
  .desk .stats:global([data-grow="6"]) .list > :global(:is(.card, .session):nth-child(n + 7)),
  .desk .stats:global([data-grow="7"]) .list > :global(:is(.card, .session):nth-child(n + 8)),
  .desk .stats:global([data-grow="8"]) .list > :global(:is(.card, .session):nth-child(n + 9)) { flex: 1 1 auto; }
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
    /* Solid: the status bar's own colour (Safari takes it from the samplers at the top edge, index.html) carried on
       down, one band; see-through, what scrolled under it showed and the two read as different colours. */
    background: var(--edge-top);
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
  /* The figure whole; the word after it (正在工作, the range) gives way with an ellipsis on a narrow bar. */
  .compact { display: flex; align-items: baseline; max-width: 100%; font-family: var(--rounded); font-size: var(--fig-m); font-weight: 600; transform: translateY(100%); opacity: 0; }
  .compact.shown { transform: none; opacity: 1; }
  .compact-range { min-width: 0; overflow: hidden; text-overflow: ellipsis; line-height: 1.3; margin-left: 8px; font-family: var(--text-face); font-size: var(--type-caption); font-weight: 500; color: var(--ink-2); }
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
  :global(.light) .awake.on, .sketch .awake.on { color: #E8590C; background: rgba(232, 89, 12, 0.12); }
  /* Not to be had on this page (http): there, fainter, and saying why when tapped. */
  .awake.unoffered { opacity: 0.5; }
  /* What 保持亮屏 just did, under the bar for a moment. */
  .notice {
    position: absolute;
    z-index: 7;
    right: 0;
    top: calc(100% + 4px);
    max-width: min(92vw, 380px);
    margin: 0;
    padding: 7px 14px;
    border-radius: 999px;
    font-size: var(--type-caption);
    line-height: 1.35;
    text-align: center;
    color: var(--ink);
    background: var(--bubble);
    box-shadow: inset 0 0 0 0.5px var(--glass-edge), var(--lift);
    animation: notice 3.2s ease-out both;
  }
  :global(.light) .notice { background: rgba(255, 255, 255, 0.94); }
  @keyframes notice { 0% { opacity: 0; transform: translateY(-6px); } 8%, 85% { opacity: 1; transform: none; } 100% { opacity: 0; } }
  .notice.long { border-radius: 16px; animation-duration: 5.2s; }
  .still .notice { animation: none; }
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
  /* Tapped (StageDrift): a little hop, as the monitor's figure does. */
  .amount:global(.tapped) { transform-origin: 0 100%; animation: hop 0.42s cubic-bezier(0.3, 1.5, 0.5, 1); }
  @keyframes hop { 40% { transform: translateY(-4px) scale(1.035); } }
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
  /* A computer's screen: 统计's heading in a row of its own, the ranges under it and the cards' tabs beside them in rows
     as tall as each other, so what follows starts on one line in both columns. */
  .desk .stats-head { display: flex; align-items: center; height: 24px; margin: 0 2px; }
  .desk .ranges, .desk .tabs { display: flex; align-items: center; }
  .desk .ranges { height: 38px; }
  .desk .ranges > :global(*) { flex: 1; min-width: 0; }
  /* The cards' tabs beside the range's words (its estimate), wherever those stand (with a row of ranges over them or
     without, SwiftBar's panel giving its own range alone), so the cards under them start on the figures' line: the
     three figures stand beside the first card, as tall as it (用户：我是说胶囊没对齐). The script measures both. */
  .desk .tabs { justify-content: flex-start; margin-top: var(--words-top, 94px); height: var(--words-height, 60px); }
  .desk .hero { display: flex; flex-direction: column; gap: 16px; padding: 0; }
  .desk .figures { margin-top: 0; }
  .desk .figure { box-sizing: border-box; min-height: var(--row, 0px); display: flex; flex-direction: column; justify-content: center; }
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
  .stage.frosted { --glass: rgba(255, 255, 255, 0.09); --glass-edge: rgba(255, 255, 255, 0.14); --pane-blur: blur(14px) saturate(1.4); }
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
     blackboard by night, in the hand's letters (the page inherits them); panes outlined by hand, of the paper itself; the
     lines (curves, rings, bars) wobble a little, the words never do; the lit choice ringed in coral. */
  .stage.sketch {
    --sky-top: var(--paper);
    --sky-mid: var(--paper);
    --sky-low: var(--paper);
    --ink: var(--label);
    --ink-2: var(--secondary);
    --ink-3: var(--tertiary);
    /* The paper again under each pane, nearly opaque: what floats behind stays out from under the words. */
    --glass: color-mix(in srgb, var(--paper) 80%, transparent);
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
  /* 手绘 with the computer's 磨砂 or 玻璃 (sketch.css has its papers): the panes laid on the paper as sheets of their own.
     磨砂: patches of heavy cold-pressed paper, lighter than the page, its tooth and clouds, what floats behind blurred
     away. 玻璃: tracing paper, what floats behind seen softly through it, fine fibres, a sheen across it, its top edge
     catching the light and the sheet's shadow under it. */
  .stage.sketch.frosted { --glass: var(--frost-pane); --pane-blur: blur(10px) saturate(1.15); }
  .stage.sketch.glass { --glass: var(--trace-pane); --pane-blur: blur(1.5px); }
  .stage.sketch.frosted :is(.trend, .panel, .figure, .turn-done),
  .stage.sketch.frosted :global(:is(.card, .session, .plan, .tile, .feed)) {
    background-image: var(--tooth), var(--mottle);
    background-size: 160px 160px, 320px 320px;
  }
  .stage.sketch.glass :is(.trend, .panel, .figure, .turn-done),
  .stage.sketch.glass :global(:is(.card, .session, .plan, .tile, .feed)) {
    background-image: var(--sheen), var(--fibres);
    background-size: 100% 100%, 360px 360px;
    box-shadow: inset 0 0 0 1.3px var(--separator), inset 0 1px 0 var(--sheen-edge), var(--sheet-shadow);
  }
  .stage.sketch .turn-done { background: var(--paper); }
  .stage.sketch .bar.raised { background: var(--paper); box-shadow: inset 0 -1px var(--separator); }
  /* The wobble filter on what stands still only (worked out again whenever what it covers is drawn): the live chart draws
     its lines twice by hand itself, and the trend's end stops breathing. */
  .stage.sketch :global(:is(.chart svg, .ring svg, .dial svg, .bar .fill, .model-fill, .window-fill, .context span)) { filter: url(#hand); }
  .stage.sketch :global(.chart .pulse) { animation: none; }
  .stage.sketch :global(.pills .pill) {
    background: transparent;
    box-shadow: inset 0 0 0 1.4px var(--coral);
    border-radius: 255px 15px 225px 15px / 15px 225px 15px 255px;
    filter: url(#hand);
  }
  /* The figures in the picture's pens: the headline blue, today green, the minute violet; each card's amount its tool's. */
  .stage.sketch :global(.monitor .big) { color: var(--accent); filter: none; }
  /* No speed yet: a quiet dash in the figures' face, never a pen's colour (it read as a bar). */
  .stage.sketch :global(.monitor .big.unknown) { color: var(--ink-3); }
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
