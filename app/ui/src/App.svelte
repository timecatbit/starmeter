<script lang="ts">
  // The panel: which page is on screen (the homepage, a row's page, the module grid), actions and their optimistic
  // display, the window's size, the pinned live mode and the rolling figures.
  import { onMount, untrack } from 'svelte';
  import { backend, inTauri, shell } from './lib/backend';
  import { automatic, entryKey, isModule, keepsPanelOpen, opening, passive } from './lib/detail';
  import { currency } from './lib/format';
  import { PanelInteraction } from './lib/interaction';
  import { LiveFeed } from './lib/live';
  import { font, width as measure } from './lib/measure';
  import { pageIn, pageOut } from './lib/motion';
  import { openShareMenu, renderPicture, type ShareStyle } from './lib/share';
  import { panel, showToast } from './lib/ui.svelte';
  import type { Dashboard as DashboardData, Entry, ShellInfo, Snapshot } from './lib/types';
  import Dashboard from './components/Dashboard.svelte';
  import DetailPage from './components/DetailPage.svelte';
  import HomePage from './components/HomePage.svelte';
  import ModulesPage from './components/ModulesPage.svelte';

  const interaction = new PanelInteraction();
  let version = $state(0);
  const snapshot: Snapshot | null = $derived.by(() => { void version; return interaction.displayed(); });
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
  const viewKey = $derived(pageEntry ? (managerOnly ? 'grid:' : 'home:') + pagePath.join('/') : managerOnly ? 'grid' : 'home');

  // MARK: The window

  let wanted = $state({ home: 400, page: 300, grid: 400 });
  const panelWidth = $derived(managerOnly && !pageEntry ? 588 : 360);
  const panelHeight = $derived(Math.min(pageEntry ? wanted.page : managerOnly ? wanted.grid : wanted.home, info.maxHeight));
  let lastSize = '';
  $effect(() => {
    const size = `${panelWidth}x${Math.round(panelHeight)}`;
    if (size === lastSize) return;
    lastSize = size;
    void shell.resize(panelWidth, Math.round(panelHeight), !panel.reduceMotion);
  });

  // MARK: Theme and background

  // The shell says whether the system material shows (not on Linux, not with Reduce Transparency).
  let glass = $state(true);
  $effect(() => {
    const theme = snapshot?.theme ?? 'system', wanted = (snapshot?.appearance ?? 'glass') !== 'solid';
    panel.theme = theme;
    if (theme === 'system') delete document.documentElement.dataset.theme; else document.documentElement.dataset.theme = theme;
    void shell.setLook(theme, wanted).then((shown) => (glass = shown));
  });

  // MARK: The figures on screen: they roll from the last reading to the next, and a new range draws its trend in

  let shown = $state<DashboardData | null>(null);
  let reveal = $state(1);
  let glows = $state<Record<string, number>>({});
  let summaryGlow = $state(0);
  let motion = 0;

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
    if (old && old.rangeKey !== next.rangeKey && !panel.reduceMotion) {
      const start = performance.now();
      const step = (now: number) => { const t = Math.min(1, (now - start) / 600); reveal = 1 - Math.pow(1 - t, 3); if (t < 1) motion = requestAnimationFrame(step); };
      reveal = 0;
      motion = requestAnimationFrame(step);
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

  let pinned = $state(false);
  let feed = $state<LiveFeed | null>(null);
  /** Unpinned at this moment: the live chart zooms back out to the trend, then lets go of the feed. */
  let leaving = $state<number | null>(null);
  let liveSince = $state(Date.now());
  let liveStamp = $state<string | null>(null);
  let liveTimer: ReturnType<typeof setInterval> | undefined;
  const pinInterval = $derived(Math.max(1, snapshot?.pinInterval ?? 2));

  function setPinned(value: boolean, animate = true) {
    if (value === pinned) {
      if (!value && !animate) { feed = null; leaving = null; }
      return;
    }
    pinned = value;
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
  function schedule() {
    clearInterval(liveTimer);
    liveSince = Date.now();
    liveTimer = setInterval(() => {
      if (!pinned || panel.menuOpen) return;
      liveSince = Date.now();
      perform(['refresh-live']);
    }, pinInterval * 1000);
  }
  $effect(() => { void pinInterval; if (pinned) schedule(); });

  // MARK: Data

  let deferred = false;
  let spin = $state<{ from: number; until: number | null } | null>(null);

  /** A button held down keeps its look: a data refresh waits until the press ends. */
  let pressing = false;

  async function reload() {
    // A redraw while a native menu is open would close it; it waits until the menu does, or the press ends.
    if (panel.menuOpen || pressing) { deferred = true; return; }
    try {
      interaction.source = await backend.snapshot();
    } catch (error) {
      console.error('snapshot failed', error);
      return;
    }
    if (spin && spin.until === null && Date.now() > spin.from) {
      const turns = Math.max(1, Math.ceil((Date.now() - spin.from) / 900));
      spin = { from: spin.from, until: spin.from + turns * 900 };
    }
    if (pinned && interaction.source.dashboard) {
      feed?.add(interaction.source.dashboard);
      const now = new Date();
      liveStamp = '实时 ' + [now.getHours(), now.getMinutes(), now.getSeconds()].map((n) => String(n).padStart(2, '0')).join(':');
    }
    version++;
  }
  $effect(() => { if (!panel.menuOpen && deferred) { deferred = false; void reload(); } });

  function perform(action: string[]) {
    // 选择 Harness… opens the module grid in place.
    if (action[0] === 'panel-modules') { openSettings(); return; }
    switch (interaction.submit(action)) {
      case 'start': void dispatch(action); break;
      case 'queued': version++; break;
    }
  }

  async function dispatch(action: string[]) {
    const keep = keepsPanelOpen(action);
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

  function push(entry: Entry) {
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
    panel.hint = null;
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

  function refresh() {
    if (!panel.reduceMotion) {
      const from = Date.now();
      spin = { from, until: null };
      setTimeout(() => { if (spin?.from === from && spin.until === null) spin = { from, until: from + Math.max(1, Math.ceil((Date.now() - from) / 900)) * 900 }; }, 8000);
    }
    perform(['refresh']);
  }

  async function share(button: HTMLElement) {
    panel.hint = null;
    const dashboard = snapshot?.dashboard;
    if (!dashboard) return;
    const picked = await openShareMenu(button);
    if (!picked) return;
    const png = await renderPicture(picked.style as ShareStyle, dashboard, snapshot?.project ?? { name: 'Starmeter', url: 'https://github.com/timecatbit/starmeter' }, panel.dark);
    if (!png) { showToast('复制失败', false); return; }
    if (picked.op === 'copy') {
      const ok = await shell.copyPicture(png).catch(() => false);
      showToast(ok ? '已复制' : '复制失败', ok);
    } else {
      const box = button.getBoundingClientRect();
      await shell.sharePicture(png, `Starmeter ${dashboard.date}.png`, { x: box.left, y: box.top, width: box.width, height: box.height });
    }
  }

  // MARK: Opening and closing

  function opened(modules: boolean, maxHeight?: number) {
    if (maxHeight) info = { ...info, maxHeight };
    managerOnly = modules;
    settingsFromHome = false;
    pagePath = [];
    pageOpen = new Set();
    dir = 0;
    homeScroll = 0;
    gridScroll = 0;
    if (modules) leading = new Set((snapshot?.moduleOptions ?? []).filter((e) => isModule(e) && e.checked).map(entryKey));
    void reload();
    if (!modules) perform(['refresh-view']);
  }

  function keydown(event: KeyboardEvent) {
    if (event.key === 'Escape') { event.preventDefault(); back(); }
  }

  // Right-click inside the panel opens the module list, as the 5.x panel did.
  function contextmenu(event: MouseEvent) {
    event.preventDefault();
    if (managerOnly) return;
    leading = new Set((snapshot?.moduleOptions ?? []).filter((e) => isModule(e) && e.checked).map(entryKey));
    dir = 0;
    managerOnly = true;
    settingsFromHome = false;
    pagePath = [];
    gridScroll = 0;
  }

  onMount(() => {
    if (!inTauri) document.body.classList.add('preview');
    const down = () => (pressing = true);
    const up = () => { pressing = false; if (deferred && !panel.menuOpen) { deferred = false; void reload(); } };
    window.addEventListener('pointerdown', down, true);
    window.addEventListener('pointerup', up, true);
    window.addEventListener('pointercancel', up, true);
    const stops = [backend.onSnapshot(() => void reload()), shell.onOpened(opened), shell.onClosed(() => setPinned(false, false))];
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
    };
  });

  const hint = $derived(panel.hint ? (() => {
    const text = panel.hint!.text, width = measure(text, font(11)) + 24;
    return { text, left: Math.min(Math.max(8, panel.hint!.x + panel.hint!.width / 2 - width / 2), panelWidth - width - 8), top: panel.hint!.y + panel.hint!.height + 4 };
  })() : null);
</script>

<svelte:window onkeydown={keydown} />

<div class="window" class:solid={!glass} class:browser={!inTauri} oncontextmenu={contextmenu} role="application" aria-label="AI 用量面板"
  style:width={inTauri ? '100vw' : `${panelWidth}px`} style:height={inTauri ? '100vh' : `${panelHeight}px`}>
  {#key viewKey}
    <div class="view" in:pageIn={{ dir }} out:pageOut={{ dir }}>
      {#if snapshot && pageEntry}
        <DetailPage page={pageEntry} open={pageOpen} busy={interaction.active} bind:scrollTop={pageScroll} onback={back}
          onopen={open} onchoose={choose} ontoggled={(next) => (pageOpen = next)} onheight={(h) => (wanted.page = h)} />
      {:else if snapshot && managerOnly}
        <ModulesPage entries={snapshot.moduleOptions ?? []} {leading} fromHome={settingsFromHome} bind:scrollTop={gridScroll}
          onback={finishModules} ondone={finishModules} onaction={perform} onopen={push} onheight={(h) => (wanted.grid = h)} />
      {:else if snapshot}
        <HomePage entries={snapshot.entries} dashboard={shown ? dashboardView : null} bind:scrollTop={homeScroll} onactivate={activate}
          onheight={(h) => (wanted.home = h)} />
      {/if}
    </div>
  {/key}
  {#if hint}
    <span class="hint-bubble" role="tooltip" style:left="{hint.left}px" style:top="{hint.top}px">{hint.text}</span>
  {/if}
</div>

{#snippet dashboardView()}
  {#if shown && snapshot}
    <Dashboard data={shown} rangeOptions={snapshot.rangeOptions ?? []} {reveal} {glows} {summaryGlow} live={pinned || leaving !== null ? feed : null} {leaving} onleft={() => { leaving = null; feed = null; }} {liveStamp} {pinned}
      {pinInterval} {liveSince} {spin} onrange={perform} onrefresh={refresh} onshare={share} ondetect={() => perform(['detect-modules'])}
      onpin={() => { panel.hint = null; setPinned(!pinned); if (pinned) perform(['refresh-live']); }} onsettings={openSettings}
      ongroup={(key) => perform(['set-group', key])} ontoggle={(id, expanded) => perform(['set-expanded', id, expanded ? 'true' : 'false'])} />
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
</style>
