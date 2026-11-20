<script lang="ts">
  // The panel: which page is on screen (the homepage, a row's page, the module grid), actions and their optimistic
  // display, the window's size, the pinned live mode and the rolling figures.
  import { onMount, tick, untrack } from 'svelte';
  import { fade } from 'svelte/transition';
  import { backend, inTauri, onPhone, shell } from './lib/backend';
  import { automatic, entryKey, isModule, keepsPanelOpen, opening, passive, sessionsEntry, without } from './lib/detail';
  import { currency } from './lib/format';
  import { PanelInteraction } from './lib/interaction';
  import { LiveFeed } from './lib/live';
  import { font, width as measure } from './lib/measure';
  import { pageIn, pageOut } from './lib/motion';
  import { answer as answerQuestion, ask, show as showQuestion } from './lib/ask.svelte';
  import { chooseLanguage, localize, source, t } from './lib/i18n.svelte';
  import { phoneStatus } from './lib/phone.svelte';
  import { chooseShortcut, restoreShortcut } from './lib/shortcut.svelte';
  import { panel } from './lib/ui.svelte';
  import type { Dashboard as DashboardData, Entry, ShellInfo, Snapshot } from './lib/types';
  import Dashboard from './components/Dashboard.svelte';
  import DetailPage from './components/DetailPage.svelte';
  import Guide, { type GuideState } from './components/Guide.svelte';
  import PhonePage from './components/PhonePage.svelte';
  import HomePage from './components/HomePage.svelte';
  import InlineAsk from './components/InlineAsk.svelte';
  import ModulesPage from './components/ModulesPage.svelte';
  import SharePage from './components/SharePage.svelte';
  import ToolPage from './components/ToolPage.svelte';

  const interaction = new PanelInteraction();
  let version = $state(0);
  // As the panel shows it: in the language chosen, with the 语言 row in 通用.
  const snapshot: Snapshot | null = $derived.by(() => { void version; const shown = interaction.displayed(); return shown && localize(shown); });
  let info = $state<ShellInfo>({ platform: 'browser', mode: 'browser', version: '', maxHeight: 900, modules: false });

  // MARK: Where the panel is

  let managerOnly = $state(false);
  let settingsFromHome = $state(false);
  let pagePath = $state<string[]>([]);
  let pageOpen = $state(new Set<string>());
  /** The next change's direction: 1 into a page, -1 back, 0 none (a refresh, a reopen). */
  let dir = $state(0);
  let homeScroll = $state(0), gridScroll = $state(0), pageScroll = $state(0), savedHome = 0;
  let leading = $state(new Set<string>());
  const sourceChecks = new Map<string, number>();

  const pageEntry = $derived.by(() => {
    if (!snapshot || !pagePath.length) return null;
    let list = (managerOnly ? snapshot.moduleOptions : snapshot.entries) ?? [], found: Entry | null = null;
    for (const key of pagePath) {
      const entry = list.find((e) => entryKey(e) === key && e.children.length);
      if (!entry) return null;
      found = entry; list = entry.children;
    }
    return found;
  });
  // A page whose row has gone (a module turned off, a key removed) gives way to its list.
  $effect(() => { if (pagePath.length && snapshot && !pageEntry) { pagePath = []; pageOpen = new Set(); } });
  /** The share page: the picture previewed before it is copied or saved. */
  let sharing = $state(false);
  /** 在手机上看: its page, over whatever opened it. */
  let phoneOpen = $state(false);
  /** The range last picked, ticked in the menus at once: the core keeps the old one ticked, with its figures, until the
   *  new one has been read, and a pick that seemed not to take was picked again (or away and back, and lost). */
  let pickedRange = $state<string | null>(null);
  const rangeOptions = $derived.by(() => {
    const options = snapshot?.rangeOptions ?? [];
    return pickedRange ? options.map((o) => ({ ...o, checked: o.action?.[1] === pickedRange })) : options;
  });
  $effect(() => { if (pickedRange && snapshot?.dashboard?.rangeKey === pickedRange) pickedRange = null; });
  /** On a phone: why the figures on screen are not fresh, until a reading comes through. */
  let phoneProblem = $state<string | null>(null);
  /** A tool's page, opened from its card: its usage and its settings. */
  let toolId = $state<string | null>(null);
  const tool = $derived(toolId ? snapshot?.dashboard?.providers.find((p) => p.id === toolId) ?? null : null);
  const toolSettings = $derived(toolId ? (snapshot?.moduleOptions ?? []).find((e) => e.action?.[0] === 'set-module' && e.action[1] === toolId) ?? null : null);
  const viewKey = $derived(phoneOpen ? 'phone' : sharing ? 'share' : toolId ? 'tool:' + toolId : pageEntry ? (managerOnly ? 'grid:' : 'home:') + pagePath.join('/') : managerOnly ? 'grid' : 'home');

  // MARK: The window

  let pinned = $state(false);
  // 会话 sits beside 工具 / 模型 / 项目 when its module is on the homepage, instead of among the other rows. Pinning
  // turns to it (a pinned panel is for watching work as it happens) and unpinning turns back.
  const sessions = $derived(snapshot ? sessionsEntry(snapshot.entries) : null);
  const VIEW = 'starmeter.homeView';
  let view = $state<'cards' | 'sessions'>(typeof localStorage !== 'undefined' && localStorage.getItem(VIEW) === 'sessions' ? 'sessions' : 'cards');
  let viewBeforePin: 'cards' | 'sessions' | null = null;
  function chooseView(next: 'cards' | 'sessions') {
    view = next;
    viewBeforePin = null;
    if (typeof localStorage !== 'undefined') localStorage.setItem(VIEW, next);
  }
  /** The size the user gave the pinned panel at its edges: the homepage and the pages take it (the module grid keeps
   *  its own) until unpinned. */
  let free = $state<{ width: number; height: number } | null>(null);
  let wanted = $state({ home: 400, page: 300, grid: 400, share: 600, tool: 500, phone: 420 });
  const grid = $derived(managerOnly && !pageEntry && !sharing && !toolId && !phoneOpen);
  /** On a phone (在手机上看): the page fills the screen's width, between 320 and 480. */
  let screenWidth = $state(typeof window === 'undefined' ? 360 : window.innerWidth);
  const phoneWidth = $derived(Math.min(480, Math.max(320, screenWidth)));
  const panelWidth = $derived(onPhone ? phoneWidth : grid ? 588 : free?.width ?? 360);
  // Pinned, the homepage keeps the size the panel was given (fitted, or the user's); every other page fits its own
  // content (a share page its picture, a tool's or a row's page its rows), unless the user sizes it at its edges while
  // on it, which holds until they leave it.
  const onPage = $derived(!grid && !!(phoneOpen || sharing || toolId || pageEntry));
  let pageHeight = $state<number | null>(null);
  $effect(() => { void viewKey; pageHeight = null; });
  // Pinned, the homepage is never shorter than its top down to the heatmap, whatever size it was given.
  let floorHeight = $state(0);
  const homeFloor = $derived(Math.max(220, Math.round(8 + floorHeight + 8)));
  $effect(() => { if (pinned) void shell.setMinHeight(onPage || grid ? 220 : homeFloor); });
  const panelHeight = $derived(free && !grid && !onPage ? Math.max(free.height, homeFloor)
    : free && onPage && pageHeight !== null ? pageHeight
    : Math.min(phoneOpen ? wanted.phone : sharing ? wanted.share : toolId ? wanted.tool : pageEntry ? wanted.page : managerOnly ? wanted.grid : wanted.home, info.maxHeight));
  $effect(() => { panel.width = onPhone ? phoneWidth : free?.width ?? 360; });
  let lastSize = '';
  /** The latest size asked of the shell, for an opening to wait on. */
  let sizing: Promise<void> = Promise.resolve();
  $effect(() => {
    const resizable = pinned && !grid;
    const size = `${panelWidth}x${Math.round(panelHeight)}${resizable ? ' resizable' : ''}`;
    if (size === lastSize) return;
    lastSize = size;
    sizing = shell.resize(panelWidth, Math.round(panelHeight), !panel.reduceMotion, resizable).catch(() => {});
  });

  // MARK: Theme and background

  // 实色 (the default), 磨砂 or 玻璃; the shell says whether a material shows (not on Linux, not with Reduce
  // Transparency), and otherwise the page paints the solid colour.
  let glass = $state(false);
  $effect(() => {
    const theme = snapshot?.theme ?? 'system', material = snapshot?.appearance ?? 'solid';
    panel.theme = theme;
    if (theme === 'system') delete document.documentElement.dataset.theme; else document.documentElement.dataset.theme = theme;
    void shell.setLook(theme, material).then((shown) => (glass = shown));
  });

  // MARK: The figures on screen: they roll from the last reading to the next, and a new range draws its trend in

  let shown = $state<DashboardData | null>(null);
  let reveal = $state(1);
  let glows = $state<Record<string, number>>({});
  let summaryGlow = $state(0);
  let motion = 0, revealing = 0;

  function mix(a: DashboardData, b: DashboardData, t: number): DashboardData {
    const lerp = (x: number, y: number) => x + (y - x) * t;
    const blendList = (list: DashboardData['providers'], old: DashboardData['providers']) => list.map((p) => {
      const o = old.find((q) => q.id === p.id);
      if (!o) return p;
      const series = (x: number[], y: number[]) => (x.length === y.length ? y.map((v, i) => lerp(x[i], v)) : y);
      return { ...p, tokens: lerp(o.tokens, p.tokens), output: lerp(o.output, p.output), requests: Math.round(lerp(o.requests, p.requests)),
        cost: o.cost !== null && p.cost !== null ? lerp(o.cost, p.cost) : p.cost, credits: o.credits !== null && p.credits !== null ? lerp(o.credits, p.credits) : p.credits,
        history: series(o.history, p.history),
        counted: o.counted && p.counted ? { ...p.counted, tokens: lerp(o.counted.tokens, p.counted.tokens), requests: Math.round(lerp(o.counted.requests, p.counted.requests)),
          history: series(o.counted.history, p.counted.history), cost: o.counted.cost !== null && p.counted.cost !== null ? lerp(o.counted.cost, p.counted.cost) : p.counted.cost } : p.counted,
        models: p.models.map((m) => { const n = o.models.find((x) => x.name === m.name); return n ? { ...m, tokens: lerp(n.tokens, m.tokens), share: lerp(n.share, m.share) } : m; }) };
    });
    return { ...b, providers: blendList(b.providers, a.providers), cards: b.cards ? blendList(b.cards, a.cards ?? []) : b.cards };
  }
  const readings = (d: DashboardData) => [...d.providers, ...(d.cards ?? [])].flatMap((p) => [p.tokens, p.cost ?? 0, p.requests, p.output, ...p.history]).join(',');
  const summaryTotal = (d: DashboardData) => d.providers.reduce((s, p) => s + (p.summaryIncluded === false ? 0 : (p.counted?.tokens ?? p.tokens)), 0);

  function present(next: DashboardData | null | undefined) {
    if (!next) { shown = null; return; }
    const old = shown;
    cancelAnimationFrame(motion);
    // The new range's trend draws in on a frame of its own: a reading arriving meanwhile (which restarts the roll) must
    // not stop it, or the curve stays clipped away with only its axes showing.
    if (old && old.rangeKey !== next.rangeKey && !panel.reduceMotion) {
      cancelAnimationFrame(revealing);
      const start = performance.now();
      const step = (now: number) => { const t = Math.min(1, (now - start) / 600); reveal = 1 - Math.pow(1 - t, 3); if (t < 1) revealing = requestAnimationFrame(step); };
      reveal = 0;
      revealing = requestAnimationFrame(step);
    }
    if (!old || old.rangeKey !== next.rangeKey || old.grouping !== next.grouping || panel.reduceMotion) { shown = next; glows = {}; summaryGlow = 0; return; }
    const grew: Record<string, number> = {};
    for (const p of next.cards ?? next.providers) {
      const before = (old.cards ?? old.providers).find((q) => q.id === p.id);
      if (before && (p.tokens > before.tokens || (p.cost ?? 0) > (before.cost ?? 0) + 0.0001 || (p.credits ?? 0) > (before.credits ?? 0))) grew[p.id] = 1;
    }
    const summaryGrew = summaryTotal(next) > summaryTotal(old);
    const rolling = readings(old) !== readings(next);
    if (!rolling && !Object.keys(grew).length && !summaryGrew) { shown = next; return; }
    const start = performance.now();
    const step = (now: number) => {
      const elapsed = now - start, t = Math.min(1, elapsed / 700), fade = Math.max(0, 1 - elapsed / 1600) ** 2;
      shown = rolling && t < 1 ? mix(old, next, 1 - Math.pow(1 - t, 3)) : next;
      glows = Object.fromEntries(Object.keys(grew).map((id) => [id, fade]));
      summaryGlow = summaryGrew ? fade : 0;
      if (elapsed < 1600) motion = requestAnimationFrame(step); else { shown = next; glows = {}; summaryGlow = 0; }
    };
    motion = requestAnimationFrame(step);
  }
  $effect(() => {
    const next = snapshot?.dashboard;
    currency.unit = next?.currency ?? null;
    untrack(() => present(next ?? null));
  });

  // MARK: Pinned

  let feed = $state<LiveFeed | null>(null);
  /** Unpinned at this moment: the live chart zooms back out to the trend, then lets go of the feed. */
  let leaving = $state<number | null>(null);
  let liveStamp = $state<string | null>(null);
  let liveTimer: ReturnType<typeof setInterval> | undefined;
  const pinInterval = $derived(Math.max(1, snapshot?.pinInterval ?? 2));

  function setPinned(value: boolean, animate = true) {
    if (value === pinned) {
      if (!value && !animate) { feed = null; leaving = null; }
      return;
    }
    pinned = value;
    panel.pinned = value;
    // Unpinned, the panel goes back under the star at its pages' own size, and to the cards it showed. Pinned from the
    // homepage, it turns to 会话 and fits the dashboard: down to just above the subscriptions, whatever is chosen.
    fitting = false;
    if (!value) {
      free = null;
      if (viewBeforePin !== null) view = viewBeforePin;
      viewBeforePin = null;
    } else if (!free && !managerOnly && !pageEntry && !sharing && !toolId) {
      if (sessions && view !== 'sessions') { viewBeforePin = view; view = 'sessions'; }
      homeScroll = 0;
      fitting = true;
    }
    void shell.setPinned(value);
    clearInterval(liveTimer);
    if (value) {
      const live = new LiveFeed();
      if (snapshot?.dashboard) live.add(snapshot.dashboard);
      leaving = null;
      feed = live;
      schedule();
    } else {
      liveStamp = null;
      // Zooms back out on screen; a closed panel just lets go.
      if (animate && feed && !panel.reduceMotion) leaving = Date.now();
      else { feed = null; leaving = null; }
    }
  }
  /** Pinned from the homepage, the panel follows the dashboard's height (the tab chosen, a session or card opened, the
   *  rest shown) down to just above the subscriptions, within the screen, until the user sizes it at its edges. */
  let fitting = $state(false);
  let dashboardHeight = $state(0);
  $effect(() => {
    if (!pinned || !fitting || grid || pageEntry || sharing || toolId || dashboardHeight <= 0) return;
    const height = Math.min(info.maxHeight, Math.max(homeFloor, Math.round(8 + dashboardHeight + 10)));
    const now = untrack(() => free);
    if (now?.height !== height) free = { width: now?.width ?? 360, height };
  });
  /** The grabber and the corner show while the pointer moves over a pinned panel and fade once it rests (the corner
   *  still resizes when faded); they stay while the corner is held. */
  let chromeShown = $state(true);
  let chromeTimer: ReturnType<typeof setTimeout> | undefined;
  /** Shows the grabber and the corner; they fade after `rest` ms, or (null) stay until told otherwise. */
  function showChrome(rest: number | null = 2500) {
    chromeShown = true;
    clearTimeout(chromeTimer);
    if (rest !== null) chromeTimer = setTimeout(() => { if (!cornerFrom) chromeShown = false; }, rest);
  }
  $effect(() => { if (pinned) untrack(() => showChrome(3500)); });

  /** A question waits a frame for its row to show it before the panel's own sheet does. */
  let sheetReady = $state(false);
  $effect(() => {
    void ask.current;
    sheetReady = false;
    const frame = requestAnimationFrame(() => (sheetReady = true));
    return () => cancelAnimationFrame(frame);
  });

  // The corner handle: the size follows the pointer, the top left stays (the window's own edges resize it too).
  let cornerFrom: { x: number; y: number; width: number; height: number } | null = null;
  let cornerFrame = 0;
  function cornerDown(event: PointerEvent) {
    if (event.button !== 0) return;
    event.preventDefault();
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    panel.arranged = true;
    if (!onPage) fitting = false;
    panel.hint = null;
    cornerFrom = { x: event.clientX, y: event.clientY, width: panelWidth, height: Math.round(panelHeight) };
  }
  function cornerMove(event: PointerEvent) {
    if (!cornerFrom) return;
    const width = Math.round(Math.min(480, Math.max(360, cornerFrom.width + event.clientX - cornerFrom.x)));
    const height = Math.round(Math.max(onPage ? 220 : homeFloor, cornerFrom.height + event.clientY - cornerFrom.y));
    cancelAnimationFrame(cornerFrame);
    cornerFrame = requestAnimationFrame(() => void shell.sizeTo(width, height));
  }
  function cornerUp() { cornerFrom = null; showChrome(); }

  function schedule() {
    clearInterval(liveTimer);
    liveTimer = setInterval(() => {
      if (!pinned || panel.menuOpen) return;
      perform(['refresh-live']);
    }, pinInterval * 1000);
  }
  $effect(() => { void pinInterval; if (pinned) schedule(); });

  // MARK: Data

  let deferred = false;

  /** A button held down keeps its look: a data refresh waits until the press ends. */
  let pressing = false;

  /** Counts the range and grouping picks: a reading asked before the last pick (a phone's slow reading of the range it
   *  had before) is left unused when it answers, so it never takes the page back. Readings between picks all count,
   *  however they overlap. */
  let picks = 0;
  async function reload() {
    // A redraw while a native menu is open would close it; it waits until the menu does, or the press ends.
    if (panel.menuOpen || pressing) { deferred = true; return; }
    const mine = picks;
    let answer: Snapshot;
    try {
      answer = await backend.snapshot();
    } catch (error) {
      if (mine !== picks) return;
      // On a phone a computer out of reach is said on the page, not only in the console.
      if (onPhone) { phoneProblem = error instanceof Error ? error.message : String(error); return; }
      console.error('snapshot failed', error);
      return;
    }
    if (mine !== picks) return;
    interaction.source = answer;
    phoneProblem = null;
    if (pinned && interaction.source.dashboard) {
      feed?.add(interaction.source.dashboard);
      const now = new Date();
      liveStamp = t('实时') + ' ' + [now.getHours(), now.getMinutes(), now.getSeconds()].map((n) => String(n).padStart(2, '0')).join(':');
    }
    version++;
  }
  // A native menu (the range, the share style) swallows the button coming up, so its closing ends the press too;
  // otherwise the new range would wait for the next click to show.
  $effect(() => {
    if (panel.menuOpen) return;
    pressing = false;
    if (deferred) { deferred = false; void reload(); }
  });

  // 新手指引: once, the first time the panel opens on its homepage (the web view keeps that it was seen); again from
  // 通用 → 新手指引. In a browser (the fixtures, the screenshots) only when the address asks: ?guide shows it
  // every time, ?firstrun as the app does.
  const address = typeof location === 'undefined' ? new URLSearchParams() : new URLSearchParams(location.search);
  const firstRuns = inTauri || address.has('firstrun');
  let guiding = $state(false);
  let guideWanted = $state(!inTauri && address.has('guide'));
  /** What the tour has to show: no tool on the homepage yet, tools still being read, or usage. */
  const guideState = $derived.by((): GuideState => {
    const providers = snapshot?.dashboard?.providers ?? [];
    if (!providers.length) return 'empty';
    return providers.every((p) => !p.available) ? 'loading' : 'ready';
  });
  /** The modules of the plan and API rows (the 订阅用量 and API groups of 主页模块), by their Chinese names. */
  const planKeys = $derived.by(() => {
    const keys = new Set<string>();
    let inside = false;
    for (const e of snapshot?.moduleOptions ?? []) {
      if (e.section) { const name = source(e).label; inside = name === '订阅用量' || name === 'API'; continue; }
      if (inside && e.action?.[0] === 'set-module' && e.action[1]) keys.add(e.action[1]);
    }
    return keys;
  });
  let windowBox: HTMLDivElement | undefined = $state();
  // Ready once the homepage is there; a yes or no, so the readings that keep arriving on a first run (each a new
  // snapshot) don't put the tour off again and again.
  const guideReady = $derived(guideWanted && !guiding && !!snapshot?.dashboard && !managerOnly && !onPage);
  $effect(() => {
    if (!guideReady) return;
    // A moment for the homepage to draw (and the page change to finish) before its parts are lit.
    // Shown is seen: a panel closed halfway, or the other panel (the app's or SwiftBar's), doesn't bring it back.
    const timer = setTimeout(() => { guideWanted = false; guiding = true; void shell.markGuideShown(); }, panel.reduceMotion ? 0 : 420);
    return () => clearTimeout(timer);
  });
  function endGuide() {
    guiding = false;
    void shell.markGuideShown();
  }

  function perform(action: string[]) {
    if (action[0] === 'set-range' || action[0] === 'set-group') picks++;
    // 在手机上看 from the module grid: its page slides in over the grid.
    if (action[0] === 'show-phone') { panel.hint = null; dir = 1; phoneOpen = true; return; }
    // 新手指引 from 通用: back to the homepage, then the tour.
    if (action[0] === 'show-guide') { startOver(false); dir = -1; guideWanted = true; return; }
    // 选择工具… opens the module grid in place.
    if (action[0] === 'panel-modules') { openSettings(); return; }
    // The language is the panel's own choice: no engine action, the page redraws in it at once.
    if (action[0] === 'set-language') { chooseLanguage(action[1]); return; }
    // So is the global shortcut: registered by the shell, kept only when the system let us have it.
    if (action[0] === 'set-shortcut') {
      void chooseShortcut(action[1]).then((ok) => {
        if (!ok) showQuestion({ kind: 'notice', id: 0, title: t('快捷键已被占用'), detail: t('这个组合键已被系统或其他程序占用，换一个试试。') }, null);
        // Redrawn either way: a refused choice slides back to the one kept.
        version++;
      });
      return;
    }
    if (action[0] === 'set-range') pickedRange = action[1] === snapshot?.dashboard?.rangeKey ? null : action[1];
    switch (interaction.submit(action)) {
      case 'start': void dispatch(action); break;
      case 'queued': version++; break;
    }
  }

  async function dispatch(action: string[]) {
    const keep = keepsPanelOpen(action, info.mode === 'swiftbar');
    // A dialog or another app takes over: the panel steps aside first.
    if (!keep) await shell.close(false);
    else if (!passive(action)) version++;
    let failed = false;
    try {
      const receipt = await backend.action(action);
      failed = receipt.status === 'failed';
      interaction.finish(receipt.preferences);
      if (failed && !automatic(action)) console.warn('action failed', action, receipt.message);
    } catch (error) {
      interaction.finish();
      console.error('action failed', action, error);
    }
    await reload();
    const next = interaction.next();
    if (next) void dispatch(next);
  }

  // MARK: Pages

  /** A question answered under its row goes with its page: leaving the page (back, Esc, a swipe, another page) is the
   *  same as 取消. One the homepage or the grid asked, at the panel's foot, stays. */
  function leavePage() {
    if (ask.current && ask.placed > 0) answerQuestion(null);
  }

  function push(entry: Entry) {
    leavePage();
    const check = entry.onOpenAction;
    if (check?.length === 2 && Date.now() - (sourceChecks.get(check[1]) ?? 0) >= 5000) {
      sourceChecks.set(check[1], Date.now());
      setTimeout(() => perform(check), 0);
    }
    if (!managerOnly) savedHome = homeScroll;
    panel.hint = null;
    dir = 1;
    pagePath = [entryKey(entry)];
    pageOpen = opening(entry);
    pageScroll = 0;
  }

  function back() {
    leavePage();
    panel.hint = null;
    if (phoneOpen) {
      dir = -1;
      phoneOpen = false;
      return;
    }
    if (sharing || toolId) {
      dir = -1;
      sharing = false;
      toolId = null;
      homeScroll = savedHome;
      return;
    }
    if (!pagePath.length) {
      if (settingsFromHome && managerOnly) finishModules();
      else void shell.close(true);
      return;
    }
    dir = -1;
    pagePath = [];
    pageOpen = new Set();
    if (!managerOnly) homeScroll = savedHome;
  }

  function openSettings() {
    panel.hint = null;
    void phoneStatus();
    savedHome = homeScroll;
    leading = new Set((snapshot?.moduleOptions ?? []).filter((e) => isModule(e) && e.checked).map(entryKey));
    dir = 1;
    managerOnly = true;
    settingsFromHome = true;
    pagePath = [];
    gridScroll = 0;
  }

  function finishModules() {
    if (!settingsFromHome) { void shell.close(true); return; }
    panel.hint = null;
    dir = -1;
    settingsFromHome = false;
    managerOnly = false;
    pagePath = [];
    homeScroll = savedHome;
  }

  /** What a row without a page does: open the app or the link, or run its action. */
  function open(entry: Entry) {
    if (entry.href) { void shell.openLink(entry.href); return; }
    if (entry.action) perform(entry.action);
  }

  function activate(entry: Entry) {
    if (entry.children.length) { push(entry); return; }
    open(entry);
  }

  function choose(entry: Entry) {
    if (entry.action) perform(entry.action);
  }

  /** A tool's card: the panel slides to its page. */
  function openTool(id: string) {
    panel.hint = null;
    savedHome = homeScroll;
    dir = 1;
    toolScroll = 0;
    toolId = id;
  }
  let toolScroll = $state(0);
  // A tool that has left the homepage (its module turned off) gives way to it.
  $effect(() => { if (toolId && snapshot?.dashboard && !tool) toolId = null; });

  /** The share button: the panel slides to the share page, as to a row's page. */
  function openShare() {
    if (!snapshot?.dashboard) return;
    panel.hint = null;
    savedHome = homeScroll;
    dir = 1;
    sharing = true;
  }

  // MARK: Opening and closing

  /** The page the panel opens on, without sliding: the homepage, or the module grid. */
  function startOver(modules: boolean) {
    panel.hint = null;
    phoneOpen = false;
    sharing = false;
    toolId = null;
    managerOnly = modules;
    settingsFromHome = false;
    pagePath = [];
    pageOpen = new Set();
    dir = 0;
    homeScroll = 0;
    gridScroll = 0;
    if (modules) leading = new Set((snapshot?.moduleOptions ?? []).filter((e) => isModule(e) && e.checked).map(entryKey));
  }

  function opened(modules: boolean, maxHeight?: number) {
    if (maxHeight) info = { ...info, maxHeight };
    startOver(modules);
    void reload();
    if (modules) void phoneStatus();
    if (!modules) perform(['refresh-view']);
    if (!modules && firstRuns) void shell.guideShown().then((shown) => {
      if (shown) return;
      // Seen before this panel kept it in the shared folder (its own storage said so): noted there, not shown again.
      let before = false;
      try { before = localStorage.getItem('starmeter.guide') === '1'; } catch { /* not known */ }
      if (before) void shell.markGuideShown();
      else guideWanted = true;
    });
    void revealWhenDrawn();
  }

  const nextFrame = () => new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
  /** Opening on another page than the closed panel drew (the module grid), the shell keeps the window clear until the
   *  window has the page's size and the page has drawn into it. */
  async function revealWhenDrawn() {
    if (!inTauri) return;
    await tick();
    await nextFrame();
    for (let i = 0; i < 30; i++) {
      await sizing;
      if (Math.abs(window.innerWidth - panelWidth) <= 1 && Math.abs(window.innerHeight - Math.round(panelHeight)) <= 1) break;
      await nextFrame();
    }
    await nextFrame();
    await nextFrame();
    void shell.shown();
  }

  /** Closed: back to the homepage at once, so the next opening shows it rather than the page left open, with nothing
   *  left focused. */
  function closed() {
    phoneOpen = false;
    // A tour left halfway was still seen: it starts again only from 通用 → 新手指引.
    guiding = false;
    guideWanted = false;
    setPinned(false, false);
    ask.current = null;
    free = null;
    startOver(false);
    (document.activeElement as HTMLElement | null)?.blur?.();
  }

  function keydown(event: KeyboardEvent) {
    if (event.key === 'Escape') { event.preventDefault(); back(); }
  }

  // Right-click does nothing inside the panel (no web page menu): the gear opens the settings, and a hidden way in
  // with no way back out only surprised.
  function contextmenu(event: MouseEvent) {
    event.preventDefault();
  }

  onMount(() => {
    if (!inTauri) document.body.classList.add('preview');
    const down = () => { pressing = true; document.documentElement.classList.remove('keyboard'); };
    // Focus rings belong to moving through the panel with Tab.
    const tabbing = (event: KeyboardEvent) => { if (event.key === 'Tab') document.documentElement.classList.add('keyboard'); };
    window.addEventListener('keydown', tabbing, true);
    const up = () => { pressing = false; if (deferred && !panel.menuOpen) { deferred = false; void reload(); } };
    window.addEventListener('pointerdown', down, true);
    window.addEventListener('pointerup', up, true);
    window.addEventListener('pointercancel', up, true);
    const stops = [backend.onSnapshot(() => void reload()), shell.onOpened(opened), shell.onClosed(closed),
      shell.onResized((width, height) => {
        if (!pinned) return;
        // On a page the height is that page's; the width is the panel's everywhere.
        if (onPage) { pageHeight = height; free = { width, height: free?.height ?? height }; }
        else { fitting = false; free = { width, height }; }
      }),
      shell.onAsk((question) => showQuestion(question, interaction.active ?? null)),
      // Pinned and not in front, the page hears no pointer: the shell tells when it comes over the panel or leaves.
      // The shell says only in and out (not every move): in, they stay until the pointer leaves.
      shell.onHover((inside) => { if (!pinned) return; if (inside) showChrome(null); else if (!cornerFrom) showChrome(700); })];
    // The shell starts with no global shortcut; the one chosen is handed over again.
    void restoreShortcut();
    void shell.info().then((value) => {
      info = value;
      opened(value.modules);
    });
    return () => {
      stops.forEach((stop) => stop());
      clearInterval(liveTimer);
      window.removeEventListener('pointerdown', down, true);
      window.removeEventListener('pointerup', up, true);
      window.removeEventListener('pointercancel', up, true);
      window.removeEventListener('keydown', tabbing, true);
    };
  });

  const hint = $derived(panel.hint ? (() => {
    const text = panel.hint!.text, width = measure(text, font(11)) + 24;
    return { text, left: Math.min(Math.max(8, panel.hint!.x + panel.hint!.width / 2 - width / 2), panelWidth - width - 8), top: panel.hint!.y + panel.hint!.height + 4 };
  })() : null);
</script>

<svelte:window onkeydown={keydown} onresize={() => (screenWidth = window.innerWidth)} />

<div class="window" bind:this={windowBox} class:solid={!glass} class:browser={!inTauri && !onPhone} class:phone={onPhone} oncontextmenu={contextmenu} role="application" aria-label={t('AI 用量面板')}
  onpointermove={() => { if (pinned) showChrome(); }} onpointerleave={() => { if (pinned && !cornerFrom) showChrome(700); }}
  style:width={inTauri ? '100vw' : `${panelWidth}px`} style:height={inTauri ? '100vh' : onPhone ? '100dvh' : `${panelHeight}px`}>
  {#key viewKey}
    <div class="view" in:pageIn={{ dir }} out:pageOut={{ dir }}>
      {#if phoneOpen}
        <PhonePage onback={back} onheight={(h) => (wanted.phone = h)} />
      {:else if sharing && snapshot?.dashboard}
        <SharePage dashboard={snapshot.dashboard} project={snapshot.project ?? { name: 'Starmeter', url: 'https://github.com/timecatbit/starmeter' }}
          {rangeOptions} onrange={perform} onback={back} onheight={(h) => (wanted.share = h)} />
      {:else if toolId && tool && snapshot?.dashboard}
        <ToolPage p={tool} dashboard={snapshot.dashboard} settings={toolSettings} busy={interaction.active} bind:scrollTop={toolScroll} onback={back}
          onopen={open} onchoose={choose} onheight={(h) => (wanted.tool = h)} />
      {:else if snapshot && pageEntry}
        <DetailPage page={pageEntry} open={pageOpen} busy={interaction.active} bind:scrollTop={pageScroll} onback={back}
          onopen={open} onchoose={choose} ontoggled={(next) => (pageOpen = next)} onheight={(h) => (wanted.page = h)} />
      {:else if snapshot && managerOnly}
        <ModulesPage entries={snapshot.moduleOptions ?? []} {leading} fromHome={settingsFromHome} bind:scrollTop={gridScroll}
          onback={finishModules} ondone={finishModules} onaction={perform} onopen={push} onheight={(h) => (wanted.grid = h)} />
      {:else if snapshot}
        <HomePage entries={shown ? without(snapshot.entries, sessions) : snapshot.entries} dashboard={shown ? dashboardView : null} bind:scrollTop={homeScroll} onactivate={activate}
          onheight={(h) => (wanted.home = h)} />
      {/if}
    </div>
  {/key}
  {#if onPhone && phoneProblem}
    <p class="phone-problem" role="status">{t(phoneProblem)}</p>
  {/if}
  {#if hint}
    <span class="hint-bubble" role="tooltip" style:left="{hint.left}px" style:top="{hint.top}px">{hint.text}</span>
  {/if}
  {#if pinned && !grid}
    <!-- Pinned: a grabber says the top moves the panel; a thick corner, as iOS draws one, resizes it. -->
    <span class="grabber" class:faded={!chromeShown} transition:fade={{ duration: panel.reduceMotion ? 0 : 200 }} aria-hidden="true"></span>
    <span class="corner" class:faded={!chromeShown} role="separator" aria-label={t('拖动调整大小')} transition:fade={{ duration: panel.reduceMotion ? 0 : 200 }}
      onpointerdown={cornerDown} onpointermove={cornerMove} onpointerup={cornerUp} onpointercancel={cornerUp}>
      <!-- Its bend shares the window corner's centre (13pt in from both edges), 5pt inside the edge. -->
      <svg width="30" height="30" viewBox="0 0 30 30" aria-hidden="true"><path d="M25 8V17A8 8 0 0 1 17 25H8" /></svg>
    </span>
  {/if}
  {#if guiding && windowBox}
    <Guide container={windowBox} stage={guideState} rowKeys={planKeys} onfinish={endGuide}
      onmodules={() => { endGuide(); openSettings(); }} />
  {/if}
  {#if ask.current && sheetReady && ask.placed === 0}
    <!-- A question no row on screen asked (from the homepage or the grid): the panel shows it at its foot. -->
    <div class="ask-sheet"><InlineAsk question={ask.current.question} sheet /></div>
  {/if}
</div>

{#snippet dashboardView()}
  {#if shown && snapshot}
    <Dashboard data={shown} {rangeOptions} reading={pickedRange !== null} {reveal} {glows} {summaryGlow} live={pinned || leaving !== null ? feed : null} {leaving} onleft={() => { leaving = null; feed = null; }} {liveStamp} {pinned}
      {pinInterval} onrange={perform} onshare={openShare}
      onpin={() => { panel.hint = null; setPinned(!pinned); if (pinned) perform(['refresh-live']); }} onsettings={openSettings}
      onopentool={openTool} ongroup={(key) => perform(['set-group', key])} {sessions} {view} onview={chooseView} busy={interaction.active}
      onsize={(h) => (dashboardHeight = h)} onfloor={(h) => (floorHeight = h)} ontoggle={(id, expanded) => perform(['set-expanded', id, expanded ? 'true' : 'false'])}
      onopen={open} onchoose={choose} readonly={onPhone} />
  {/if}
{/snippet}

<style>
  .window {
    position: relative;
    overflow: hidden;
    border-radius: 13px;
    box-sizing: border-box;
  }
  .window::after {
    content: '';
    position: absolute;
    inset: 0;
    border-radius: 13px;
    box-shadow: inset 0 0 0 0.5px color-mix(in srgb, var(--separator) 35%, transparent);
    pointer-events: none;
  }
  .window.solid { background: var(--solid); }
  /* On a phone the page is the screen: no edge, no shadow, the width centred on a wide one. */
  .window.phone { margin: 0 auto; border-radius: 0; background: var(--solid); }
  .window.phone::after { display: none; }
  .window.browser {
    margin: 24px;
    box-shadow: 0 18px 50px rgba(0, 0, 0, 0.22), 0 0 0 0.5px rgba(0, 0, 0, 0.12);
    transition: height 0.3s var(--page-curve), width 0.3s var(--page-curve);
  }
  .window.browser:not(.solid) {
    background: color-mix(in srgb, var(--solid) 82%, transparent);
    backdrop-filter: blur(30px) saturate(1.6);
  }
  .view {
    position: absolute;
    inset: 0;
  }
  .ask-sheet {
    position: absolute;
    left: 8px;
    right: 8px;
    bottom: 8px;
    z-index: 12;
  }
  .grabber, .corner svg { transition: opacity 0.35s; }
  .grabber.faded, .corner.faded svg { opacity: 0; }
  .grabber {
    position: absolute;
    left: 50%;
    top: 4px;
    width: 34px;
    height: 4px;
    margin-left: -17px;
    border-radius: 2px;
    background: var(--knob);
    pointer-events: none;
  }
  .corner {
    position: absolute;
    right: 0;
    bottom: 0;
    width: 30px;
    height: 30px;
    z-index: 5;
    cursor: nwse-resize;
    touch-action: none;
  }
  .corner path {
    fill: none;
    stroke: var(--knob);
    stroke-width: 4;
    stroke-linecap: round;
    transition: stroke 0.15s;
  }
  .corner:hover path, .corner:active path { stroke: var(--secondary); }
  .hint-bubble {
    position: absolute;
    z-index: 10;
    padding: 4px 12px;
    border-radius: 6px;
    background: var(--solid);
    box-shadow: 0 0 0 0.5px var(--separator), 0 2px 8px rgba(0, 0, 0, 0.12);
    font-size: 11px;
    white-space: nowrap;
    pointer-events: none;
  }
  .phone-problem {
    position: fixed;
    left: 12px;
    right: 12px;
    bottom: calc(12px + env(safe-area-inset-bottom));
    margin: 0;
    padding: 9px 12px;
    border-radius: 10px;
    font-size: 12px;
    line-height: 1.45;
    color: var(--label);
    background: color-mix(in srgb, var(--orange) 22%, var(--solid));
    box-shadow: 0 2px 10px rgba(0, 0, 0, 0.12);
  }
</style>
