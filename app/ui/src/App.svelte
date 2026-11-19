<script lang="ts">
  // The panel: which page is on screen (the homepage, a row's page, the module grid), actions and their optimistic
  // display, the window's size, the pinned live mode and the rolling figures.
  import { onMount, tick, untrack } from 'svelte';
  import { fade } from 'svelte/transition';
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

  let pinned = $state(false);
  /** The size the user gave the pinned panel at its edges: the homepage and the pages take it (the module grid keeps
   *  its own) until unpinned. */
  let free = $state<{ width: number; height: number } | null>(null);
  let wanted = $state({ home: 400, page: 300, grid: 400 });
  const grid = $derived(managerOnly && !pageEntry);
  const panelWidth = $derived(grid ? 588 : free?.width ?? 360);
  const panelHeight = $derived(free && !grid ? free.height : Math.min(pageEntry ? wanted.page : managerOnly ? wanted.grid : wanted.home, info.maxHeight));
  $effect(() => { panel.width = free?.width ?? 360; });
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
    panel.pinned = value;
    // Unpinned, the panel goes back under the star at its pages' own size.
    if (!value) free = null;
    void shell.setPinned(value);
    clearInterval(liveTimer);
    clearTimeout(pinHintTimer);
    pinHint = value && !panel.arranged;
    if (pinHint) pinHintTimer = setTimeout(() => (pinHint = false), 3200);
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
  /** Pinning says, until the user has done it once, that the top moves the panel and the corner resizes it. */
  let pinHint = $state(false);
  let pinHintTimer: ReturnType<typeof setTimeout> | undefined;
  $effect(() => { if (panel.arranged) pinHint = false; });

  // The corner handle: the size follows the pointer, the top left stays (the window's own edges resize it too).
  let cornerFrom: { x: number; y: number; width: number; height: number } | null = null;
  let cornerFrame = 0;
  function cornerDown(event: PointerEvent) {
    if (event.button !== 0) return;
    event.preventDefault();
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    panel.arranged = true;
    panel.hint = null;
    cornerFrom = { x: event.clientX, y: event.clientY, width: panelWidth, height: Math.round(panelHeight) };
  }
  function cornerMove(event: PointerEvent) {
    if (!cornerFrom) return;
    const width = Math.round(Math.min(720, Math.max(360, cornerFrom.width + event.clientX - cornerFrom.x)));
    const height = Math.round(Math.max(280, cornerFrom.height + event.clientY - cornerFrom.y));
    cancelAnimationFrame(cornerFrame);
    cornerFrame = requestAnimationFrame(() => void shell.sizeTo(width, height));
  }
  function cornerUp() { cornerFrom = null; }

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

  /** The page the panel opens on, without sliding: the homepage, or the module grid. */
  function startOver(modules: boolean) {
    panel.hint = null;
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
    if (!modules) perform(['refresh-view']);
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
    setPinned(false, false);
    free = null;
    startOver(false);
    (document.activeElement as HTMLElement | null)?.blur?.();
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
    const down = () => { pressing = true; document.documentElement.classList.remove('keyboard'); };
    // Focus rings belong to moving through the panel with Tab.
    const tabbing = (event: KeyboardEvent) => { if (event.key === 'Tab') document.documentElement.classList.add('keyboard'); };
    window.addEventListener('keydown', tabbing, true);
    const up = () => { pressing = false; if (deferred && !panel.menuOpen) { deferred = false; void reload(); } };
    window.addEventListener('pointerdown', down, true);
    window.addEventListener('pointerup', up, true);
    window.addEventListener('pointercancel', up, true);
    const stops = [backend.onSnapshot(() => void reload()), shell.onOpened(opened), shell.onClosed(closed),
      shell.onResized((width, height) => { if (pinned) free = { width, height }; })];
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
  {#if pinned && !grid}
    <!-- Pinned: a grabber says the top moves the panel; a thick corner, as iOS draws one, resizes it. -->
    <span class="grabber" transition:fade={{ duration: panel.reduceMotion ? 0 : 200 }} aria-hidden="true"></span>
    <span class="corner" role="separator" aria-label="拖动调整大小" transition:fade={{ duration: panel.reduceMotion ? 0 : 200 }}
      onpointerdown={cornerDown} onpointermove={cornerMove} onpointerup={cornerUp} onpointercancel={cornerUp}>
      <!-- Its bend shares the window corner's centre (13pt in from both edges), 5pt inside the edge. -->
      <svg width="30" height="30" viewBox="0 0 30 30" aria-hidden="true"><path d="M25 8V17A8 8 0 0 1 17 25H8" /></svg>
    </span>
  {/if}
  {#if pinHint}
    <span class="pin-hint" role="status" transition:fade={{ duration: panel.reduceMotion ? 0 : 200 }}>拖动顶部移动面板 · 拖右下角调整大小</span>
  {/if}
</div>

{#snippet dashboardView()}
  {#if shown && snapshot}
    <Dashboard data={shown} rangeOptions={snapshot.rangeOptions ?? []} {reveal} {glows} {summaryGlow} live={pinned || leaving !== null ? feed : null} {leaving} onleft={() => { leaving = null; feed = null; }} {liveStamp} {pinned}
      {pinInterval} {liveSince} onrange={perform} onshare={share} ondetect={() => perform(['detect-modules'])}
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
  .pin-hint {
    position: absolute;
    left: 50%;
    top: 44px;
    z-index: 10;
    transform: translateX(-50%);
    padding: 4px 12px;
    border-radius: 6px;
    background: var(--solid);
    box-shadow: 0 0 0 0.5px var(--separator), 0 2px 8px rgba(0, 0, 0, 0.12);
    font-size: 11px;
    white-space: nowrap;
    pointer-events: none;
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
