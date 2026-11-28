<script lang="ts" module>
  /** The page opened last: a page closed with a step on its way reads where things stand once that step is done, unless
   *  the page has been opened again since (it reads them itself). */
  let opened = 0;
</script>

<script lang="ts">
  // 在手机上看: a QR code a phone scans to open the homepage in its browser, read-only. Two ways, each on and off by
  // itself (用户：同一网络和任何网络为什么是一切开始和结束？互不影响啊): 同一个网络, which the app serves itself, its
  // network address and its Tailscale and WireGuard ones side by side, until its 停止; and 任何网络, a public link through Cloudflare
  // (a download of cloudflared the first time), an hour at most, until its own 停止, or instead the user's own domain
  // (自有域名: their own reverse proxy or tunnel forwards it to this computer's port, on until its 停止). All start off, and
  // only their own
  // 开始 starts them (用户：默认，同一网络，任何网络都是停止状态，用户点击开始才开始), and one left on is on again when the
  // app starts again, an update too, 同一个网络 at the same address (the shell keeps it): looking at a page starts nothing,
  // and a page that is off says what its 开始 will do (任何网络's, that the figures go through Cloudflare) before it is
  // pressed. A 停止 stays on its page, saying so in the code's place.
  import { onDestroy, onMount, untrack } from 'svelte';
  import { language, t } from '../lib/i18n.svelte';
  import { style } from '../lib/style.svelte';
  import { shell, type DomainCheck, type DomainSetup, type PhoneView } from '../lib/backend';
  import { chooseNear, chooseVia, phone, secondsLeft, setPhone, type Near, type Via } from '../lib/phone.svelte';
  import { composing, grabWindow, panel } from '../lib/ui.svelte';
  import HintButton from './HintButton.svelte';
  import Icon from './Icon.svelte';
  import QrCode from './QrCode.svelte';
  import Scroller from './Scroller.svelte';

  let { onback, onheight }: { onback: () => void; onheight: (height: number) => void } = $props();

  /** 同一个网络 being started. */
  let starting = $state(false);
  /** The VPN page (任何网络 › Tailscale 等 VPN) being started; its start that failed: the reason, in the code's place
   *  with 重试. */
  let vpnStarting = $state(false);
  let vpnFailed = $state<string | null>(null);
  /** 同一个网络's start that failed (its port taken for a moment): the reason, in the code's place with 重试. */
  let localFailed = $state<string | null>(null);
  /** The public link's last failure, said on 任何网络 only. */
  let publicFailed = $state<string | null>(null);
  /** 任何网络's link not there: it couldn't be made, or it dropped. Its page says so in the code's place, with 重试 (or
   *  重新生成) and 改用同一个网络; it doesn't go to the other page on its own (用户：在所有网络失败，应该停留在所有网络的页面). */
  let lost = $state<null | 'failed' | 'dropped'>(null);
  /** The public link being made. */
  let busy = $state(false);
  /** Whether cloudflared has to be downloaded first (its 开始 says so, 下载并开始); unknown until the host has said. */
  let download = $state(false);
  /** Which link was copied, or the clipboard refused (the full link is then shown, selected, to copy by hand). */
  let copied = $state<{ key: string; state: 'ok' | 'failed' } | null>(null);
  let manual: HTMLInputElement | undefined = $state();
  let copiedTimer: ReturnType<typeof setTimeout> | undefined;
  /** 自有域名 as set (its address, the server's port, this computer's network address), and its page's fields as the
   *  user left them: the address, and the port, shown in the address to forward to and changed only when asked (it is
   *  the server's: 同一个网络's codes use it too). */
  let setup = $state<DomainSetup | null>(null);
  let domainInput = $state('');
  let portInput = $state('');
  /** The fields typed into since they were filled: the setup read again (every ten seconds, for this computer's addresses
   *  on another network) leaves them as typed. */
  let domainEdited = false, portEdited = false;
  let portOpen = $state(false);
  /** The port's field to take the focus once it is there: as 换端口 opens it, or a start fails on the port. */
  let focusPort = $state(false);
  let portField: HTMLInputElement | undefined = $state();
  /** The kind's 开始s on their way, both of their steps: the domain's check waits for them (a proxy on a NAS gets in once
   *  the same network is on). A count, so a second 开始 pressed while the first is on its way keeps it waiting (R12-5). */
  let nearStarting = $state(0);
  /** 同一网络's port that couldn't be had (not one to give, or another app's): said under its field, 开始 still there. */
  let portSaid = $state<string | null>(null);
  /** 自有域名 being started; its start that failed (the address, or the port taken): the reason, under its fields. */
  let domainStarting = $state(false);
  let domainFailed = $state<string | null>(null);
  /** Whether its address opens this server from here, asked once it is on; 'failed' when the asking itself failed. */
  let domainCheck = $state<'checking' | 'failed' | DomainCheck | null>(null);
  const checked = $derived(typeof domainCheck === 'object' ? domainCheck : null);

  const view = $derived(phone.view);
  const reach = $derived(phone.reach);
  // The phone opens in this panel's language, and its look (手绘).
  const looked = (base: string | null | undefined) => (base ? `${base}&l=${language.current}${style.current === 'sketch' ? '&s=sketch' : ''}` : null);
  const localOn = $derived(!localFailed && !!(view?.localOpen ?? view?.local));
  const lan = $derived(localOn ? looked(view?.local) : null);
  /** The VPN page's: on with its own key, a code for each VPN this computer is on (用户：同一网络|反代，任何网络，就这两个
   *  分类。tailscale,等vpn, cloudflare放到任意网络里), side by side when both. */
  const vpnOn = $derived(!vpnFailed && !!view?.vpnOpen);
  const tailnet = $derived(vpnOn ? looked(view?.tailnet) : null);
  const wireguard = $derived(vpnOn ? looked(view?.wireguard) : null);
  const vpnCodes = $derived([
    tailnet && { key: 'tailnet', url: tailnet, title: 'Tailscale', label: '连着 Tailscale 的手机扫这个二维码', detail: '手机开着 Tailscale' },
    wireguard && { key: 'wireguard', url: wireguard, title: 'WireGuard', label: '连着 WireGuard 的手机扫这个二维码', detail: '手机开着 WireGuard' },
  ].filter((code) => !!code));
  /** What one VPN's code alone says. */
  const vpnNote = (key: string) => key === 'tailnet'
    ? '手机连上同一个 Tailscale 后扫码，不经过 Cloudflare。只读，点“停止”前一直开着。'
    : '手机连上同一个 WireGuard 后扫码，不经过 Cloudflare。只读，点“停止”前一直开着。';
  const outside = $derived(looked(view?.public));
  /** 自有域名's link while it is on, its address, and where its proxy forwards to (this computer's port). */
  const own = $derived(looked(view?.domain));
  /** 反代 on, a domain given or not; with none, what to put after one's own domain or IP: /#k=… and the page's language
   *  (用户：域名也可以不写，但是开始之后，告诉用户如何拼域名/IP+后面的key). */
  const proxyOn = $derived(!!view?.domainKey || !!own);
  const proxyTail = $derived(view?.domainKey && !own ? looked(`/#k=${view.domainKey}`) : null);
  const ownOrigin = $derived(view?.domain ? view.domain.replace(/\/?#.*$/, '') : null);
  const ownHost = $derived(ownOrigin?.replace(/^https?:\/\//, '') ?? '');
  const portTyped = $derived(/^\d+$/.test(portInput.trim()) ? Number(portInput.trim()) : null);
  const forwardPort = $derived(proxyOn && !portOpen ? view?.port : portTyped ?? setup?.port);
  /** Where the user's proxy hands requests on to: this computer's network address (用户：那就用子网的ip吧), good for a
   *  proxy on this computer and one on a NAS or the router alike, the same network being on with the proxy; this
   *  computer itself (127.0.0.1) without a network. Not 0.0.0.0: that is where the server listens, not an address to
   *  send to. */
  const forwardTo = $derived(forwardPort != null ? `http://${setup?.lan ?? '127.0.0.1'}:${forwardPort}` : null);
  /** What the check found, said with what to look at. */
  const checkText = $derived.by(() => {
    if (domainCheck === 'failed') return t('没能检查 {host} 能不能打开', { host: ownHost });
    if (!checked) return '';
    const target = forwardTo?.replace(/^http:\/\//, '') ?? '';
    if (checked.opens) return t('已确认能通过 {host} 打开', { host: ownHost });
    if (checked.status && checked.status >= 300 && checked.status < 400 && ownOrigin?.startsWith('http:')) {
      // Sent on to https: as https at its usual port, the http one left off.
      return t('{host} 把请求转到了别处（HTTP {n}）：检查反代的设置；总是跳到 https 时，把域名改成 https://{name}', { host: ownHost, n: checked.status, name: ownHost.replace(/:\d+$/, '') });
    }
    if (checked.status) return t('{host} 有回应（HTTP {n}），但不是 Starmeter：检查反代是否转发到 {target}', { host: ownHost, n: checked.status, target });
    return t('这台电脑没能打开 {host}（{reason}）：手机也打不开时，检查域名解析和反代是否转发到 {target}', { host: ownHost, reason: t(checked.reason ?? ''), target });
  });
  const opening = $derived(busy || !!view?.publicOpening);
  const minutes = $derived(view ? Math.max(1, Math.round(secondsLeft() / 60)) : 0);
  const choices = ['local', 'public'] as const;
  const vias: Via[] = ['cloudflare', 'vpn'];
  /** Which of 任何网络's pages are on, a green dot after each. */
  const viaOn = $derived({ cloudflare: !!outside || opening, vpn: vpnOn });
  /** 同一网络 / 反代's codes while on: the network's, and with 反代 the domain's beside it (用户：同一个网络和反代放在一起。
   *  可以只开同一网络，或者同一网络和反代一起开两种模式). */
  const nearCodes = $derived([
    lan && { key: 'local', url: lan, title: '局域网', label: '手机扫这个二维码', detail: '手机连同一个 Wi-Fi' },
    own && { key: 'domain', url: own, title: '反代', label: '手机在哪都能扫这个二维码', detail: '手机在哪都能扫' },
  ].filter((code) => !!code));
  /** 自有域名 started on another port than the server's: the same network's codes and Cloudflare's link go with it. */
  const movesServer = $derived(!!view && !!view.port && portTyped !== null && portTyped !== view.port
    && (localOn || vpnOn || !!outside || opening || proxyOn));
  /** Which ways are on, a green dot after each one's name. */
  const on = $derived({ local: localOn || proxyOn, public: !!outside || opening || vpnOn });

  /** Each way's steps belong to its own round, so what one answers late can't undo a later step of the same way: a
   *  start after a 停止, a link closed while it was made. Every step also moves `epoch`, so the status asked every ten
   *  seconds, if a step came between its asking and its answer, is dropped instead of putting back what has gone. */
  let localRound = 0, vpnRound = 0, publicRound = 0, domainRound = 0, epoch = 0;
  /** The steps on their way to the host: a page closed meanwhile drops their answers, then reads where things stand
   *  once they are done, so the homepage's dot is right (R12-3, R12-4). */
  const inflight = new Set<Promise<unknown>>();
  function sent<T>(step: Promise<T>): Promise<T> {
    inflight.add(step);
    const done = () => { inflight.delete(step); };
    step.then(done, done);
    return step;
  }
  let page = 0;
  let destroyed = false;
  /** A 自有域名 start moving the server on its way: Cloudflare's link waits for it before saying it failed (the move
   *  closes it, and the host answers that the phone view closed). */
  let moving: Promise<void> | null = null;

  /** What is left on once a way goes, shown at once: nothing on, nothing at all (the dot on the homepage's button goes
   *  too). The host's own answer follows. */
  function without(gone: Partial<PhoneView>) {
    const left = view ? { ...view, ...gone } : null;
    setPhone(left && (left.localOpen || left.vpnOpen || left.public || left.publicOpening || left.domain || left.domainKey) ? left : null);
  }

  /** 同一个网络's 开始 (重新开始, 重试). A start that fails stays in the code's place with its reason and 重试, instead of
   *  leaving a page with neither a code nor a way on. */
  async function startLocal() {
    // The port as its field was left (用户：同一网络也支持指定端口号吧): untouched, as it is; emptied, the app picks
    // again; a number, that port, held from then on.
    const asked = portAsked();
    portSaid = null;
    if (asked === false) { portSaid = '端口填 1024 到 65535 之间的数'; openPort(); return; }
    const mine = ++localRound;
    epoch++;
    localFailed = null;
    starting = true;
    const moved = holdPublic();
    try {
      const started = await serial(() => sent(shell.phoneStart(asked)));
      moved(true);
      if (mine === localRound) {
        setPhone(started);
        // Its port as kept now: the row says it, the field put away.
        if (asked !== null) { portEdited = false; portOpen = false; readSetup(); }
      }
    } catch (error) {
      moved(false);
      if (mine === localRound) {
        // A port another app holds: said under its field, open to pick another, 开始 still there.
        if (/^端口 \d+ 被别的程序占用$/.test(String(error))) { portSaid = String(error); openPort(); }
        else localFailed = String(error);
      }
    }
    if (mine === localRound) starting = false;
  }
  /** The port's field as it was left: untouched, the port as it is (null); emptied, the app's own pick again (0); a
   *  number, that port. `false`: not a port one can give. */
  function portAsked(): number | null | false {
    if (!portEdited) return null;
    if (portInput.trim() === '') return 0;
    return portTyped !== null && portTyped >= 1024 && portTyped <= 65535 ? portTyped : false;
  }
  /** A start that moves the server to another port closes Cloudflare's link (and one still being made). Its round is
   *  held meanwhile, so the host's answer to it (手机查看已经关闭) isn't said as a failure; ended as its 停止 would once
   *  the move is done, handed back as it was when the start fails before anything moved. What it returns is called
   *  with how the start went. */
  function holdPublic(): (went: boolean) => void {
    if (!(movesServer && (outside || opening))) return () => {};
    const before = publicRound;
    let release = () => {};
    moving = new Promise((done) => (release = done));
    return (went) => {
      if (went && publicRound === before) {
        publicRound++;
        busy = false;
        lost = null;
        publicFailed = null;
        phone.publicStopped = true;
      }
      // Nothing moved: Cloudflare's link as the host has it now (an answer held meanwhile is in its status).
      if (!went) void pollStatus();
      release();
      moving = null;
    };
  }

  /** The VPN page's 开始 (重试): on with its own key; a start that fails stays in the code's place with its reason. */
  async function startVpn() {
    const mine = ++vpnRound;
    epoch++;
    vpnFailed = null;
    vpnStarting = true;
    try {
      const started = await sent(shell.phoneVpnStart());
      if (mine === vpnRound) { phone.vpnStopped = false; setPhone(started); }
    } catch (error) {
      if (mine === vpnRound) vpnFailed = String(error);
    }
    if (mine === vpnRound) vpnStarting = false;
  }
  /** 换链接: a page's links keep their key from one 开始 to the next (用户：每次的内网链接都不同？那个key可不可以固定啊？
   *  用户可以手动刷新，刷新之前就不要改变); this makes new ones, the codes and links before opening nothing from then on,
   *  so it asks once more first. `renewing`: the page asked about; `renewed`: the page whose links are new, said for a
   *  moment. */
  let renewing = $state<'near' | 'vpn' | null>(null);
  let renewed = $state<'near' | 'vpn' | null>(null);
  let renewedTimer: ReturnType<typeof setTimeout> | undefined;
  async function newLink(of: 'near' | 'vpn') {
    renewing = null;
    epoch++;
    const local = localRound, domain = domainRound, vpn = vpnRound;
    let next: PhoneView | null;
    try { next = await serial(() => sent(shell.phoneNewLink(of))); } catch { return; }
    // A step of the same page since (its 停止): that one's answer stands.
    if (destroyed || (of === 'near' ? local !== localRound || domain !== domainRound : vpn !== vpnRound)) return;
    setPhone(next ?? null);
    renewed = of;
    clearTimeout(renewedTimer);
    renewedTimer = setTimeout(() => (renewed = null), 4000);
  }
  // Asked about on one page, the question doesn't wait on another, nor outlive the way going off.
  $effect(() => { void phone.reach; void phone.via; void localOn; void vpnOn; untrack(() => { renewing = null; }); });

  /** The VPN page's 停止: its codes stop opening; the other ways as they are. */
  async function stopVpn() {
    const mine = ++vpnRound;
    epoch++;
    vpnStarting = false;
    vpnFailed = null;
    phone.vpnStopped = true;
    without({ vpnOpen: false, tailnet: null, wireguard: null });
    const left = await sent(shell.phoneVpnStop()).catch(() => phone.view);
    if (mine === vpnRound) setPhone(left ?? null);
  }

  let tick: ReturnType<typeof setInterval> | undefined;
  let fast: ReturnType<typeof setInterval> | undefined;
  async function pollStatus() {
    const mine = epoch;
    const had = !!phone.view?.public;
    const status = await shell.phoneStatus().catch(() => phone.view);
    if (mine !== epoch) return;
    setPhone(status);
    if (had && !phone.view?.public && !opening) { publicFailed = null; lost = 'dropped'; }
    readSetup();
  }
  /** 自有域名 as set and this computer's addresses, read again as the page opens and every ten seconds after (on another
   *  network, or a VPN joined, the addresses to forward to change); an answer after a 开始 or 停止 of it is dropped. */
  function readSetup() {
    const asked = domainRound;
    void shell.phoneDomain().then((next) => { if (next && asked === domainRound && !destroyed) settled(next); }).catch(() => {});
  }
  /** 自有域名 as the shell has it: its fields filled from it, and its page chosen when a domain is set and nothing else has
   *  been chosen. */
  function settled(next: DomainSetup) {
    setup = next;
    if (!domainEdited) domainInput = next.domain ? next.domain.replace(/^https:\/\//, '') : domainInput;
    if (!portEdited) portInput = String(next.port);
    if (next.domain && !phone.nearChosen) phone.near = 'domain';
  }
  onMount(() => {
    // Nothing starts by opening the page: each way as it was left, off until its 开始. What is set for 自有域名, unless
    // a 开始 or 停止 of it came first (its answer is newer).
    page = ++opened;
    void pollStatus();
    if (phone.reach === 'public') void checkTunnel();
    // Asked after while the page is open: a public link whose cloudflared has ended is taken down, and said so.
    tick = setInterval(() => void pollStatus(), 10_000);
  });
  // While the host is opening a public link in the background (startup resume or after re-entering this page, R10-8),
  // poll sooner so the QR code appears as soon as it is ready.
  $effect(() => {
    if (!view?.publicOpening || busy) return;
    const timer = setInterval(() => void pollStatus(), 1500);
    fast = timer;
    return () => { clearInterval(timer); if (fast === timer) fast = undefined; };
  });
  onDestroy(() => {
    destroyed = true;
    localRound++; vpnRound++; publicRound++; domainRound++; epoch++;
    clearInterval(tick); clearInterval(fast); clearTimeout(copiedTimer); clearTimeout(renewedTimer);
    // Steps still on their way: their answers dropped with the page, where things stand read once they are done.
    if (inflight.size) {
      void Promise.allSettled([...inflight]).then(() => shell.phoneStatus()).then((status) => { if (page === opened) setPhone(status); }).catch(() => {});
    }
  });

  /** Whether cloudflared is there, for 任何网络's 开始 to say 下载并开始 when it isn't; an answer that comes back after a
   *  later look is dropped. */
  let checking = 0;
  async function checkTunnel() {
    const mine = ++checking;
    const ready = await shell.tunnelReady().catch(() => false);
    if (mine === checking) download = !ready;
  }
  /** 自有域名's 开始 and 停止 reach the host in the order they were pressed: each waits for the one before, so a quick
   *  停止 then 开始 can't land the other way round and leave a code shown for a way that is off. */
  let domainQueue: Promise<unknown> = Promise.resolve();
  function serial<T>(step: () => Promise<T>): Promise<T> {
    const next = domainQueue.then(step, step);
    domainQueue = next.catch(() => {});
    return next;
  }
  /** 自有域名's 开始 (the button, or Return in a field): its address and port kept once it is on (the port held from
   *  then on), and its link on. One at a time: Return while one is on its way does nothing. */
  async function startDomain() {
    if (domainStarting) return;
    domainFailed = null;
    // The port's field emptied: the port as it is, the app's own pick again from then on.
    const typed = portInput.trim();
    const port = typed === '' ? setup?.port ?? null : portTyped;
    if (port === null || port < 1024 || port > 65535) { domainFailed = '端口填 1024 到 65535 之间的数'; openPort(); return; }
    const chosen = portEdited ? typed !== '' : null;
    const mine = ++domainRound;
    epoch++;
    const moved = holdPublic();
    domainStarting = true;
    try {
      const started = await serial(() => sent(shell.phoneDomainStart(domainInput, port, chosen)));
      moved(true);
      if (mine !== domainRound) return;
      phone.domainStopped = false;
      setPhone(started);
      // As kept now: the fields show what was saved.
      domainEdited = false;
      portEdited = false;
      const next = await shell.phoneDomain().catch(() => null);
      if (next && mine === domainRound) settled(next);
    } catch (error) {
      moved(false);
      if (mine !== domainRound) return;
      domainFailed = String(error);
      // A port another app holds: the port's field open, to pick another.
      if (/^端口 \d+ 被别的程序占用$/.test(domainFailed)) openPort();
    } finally {
      if (mine === domainRound) domainStarting = false;
    }
  }
  /** The kind's 开始: with 同时开启反代 the domain first (it binds the port typed under 换端口, held from then on, so a
   *  port taken can be changed there), then the same network on the same server; a domain that fails starts nothing
   *  else, its reason under its form. Without, the same network alone. */
  async function startNear() {
    nearStarting++;
    // 重试 after the same network failed: its reason goes as this start shows its spinner, and a new one says why (R12-6).
    localFailed = null;
    try {
      if (phone.near === 'domain' && !proxyOn) {
        await startDomain();
        if (!proxyOn || destroyed) return;
      }
      if (!localOn && !destroyed) await startLocal();
    } finally {
      nearStarting--;
    }
  }
  /** The kind's 停止: whatever of it is on, or on its way (stopped after its start, in the order pressed). */
  async function stopNear() {
    // In one step (R12-3): the domain's answer alone would say the same network still on, and its code would come back
    // for a moment.
    const local = ++localRound, domain = ++domainRound;
    epoch++;
    const wasDomain = proxyOn || domainStarting;
    const wasLocal = localOn || !!localFailed || starting;
    starting = false;
    domainStarting = false;
    localFailed = null;
    domainFailed = null;
    portSaid = null;
    if (wasLocal) phone.localStopped = true;
    if (wasDomain) phone.domainStopped = true;
    without({ localOpen: false, local: null, domain: null, domainKey: null });
    const left = await serial(() => sent(shell.phoneStopNear())).catch(() => phone.view);
    if (local === localRound && domain === domainRound) setPhone(left ?? null);
  }
  /** 同时开启反代 turns 反代 on and off itself, on this page (用户：同时开启反代不要再进入一个界面。就在同一界面管理): with
   *  the network on, on starts it at once (a domain is not needed: without one the page says how its link goes
   *  together), its domain and port then set under the codes; off takes its link down (one on its way too). With
   *  everything off, it says 开始 starts both. */
  function pickNear(next: Near) {
    chooseNear(next);
    if (next === 'local' && (proxyOn || domainStarting)) void stopDomain();
    else if (next === 'domain' && localOn && !proxyOn && !domainStarting) void startDomain();
  }
  /** Return (or leaving a field changed) in 反代's fields: everything off, 开始; on, its domain and port as typed, at
   *  once (the same key, so a link already given keeps opening; an emptied domain is no domain). */
  function applyNear() {
    if (!localOn && !proxyOn) { void startNear(); return; }
    if (phone.near === 'domain' && (domainEdited || portEdited || !proxyOn)) void startDomain();
  }
  /** 自有域名's 停止: its links stop opening; its address and port stay for the next 开始. */
  async function stopDomain() {
    const mine = ++domainRound;
    epoch++;
    domainStarting = false;
    domainFailed = null;
    phone.domainStopped = true;
    without({ domain: null, domainKey: null });
    const left = await serial(() => sent(shell.phoneDomainStop())).catch(() => phone.view);
    if (mine === domainRound) setPhone(left ?? null);
  }
  // 换端口, or a start that failed on the port: the field takes the focus once, as it is asked for (the form coming back
  // later, after a 停止 or another page, leaves the focus where it is).
  $effect(() => { if (focusPort && portField) { focusPort = false; portField.focus(); portField.select(); } });
  function openPort() { portOpen = true; focusPort = true; }
  /** Whether its address opens this server from here: asked once each link of it is on, and on 再检查. */
  let checkRound = 0;
  async function checkDomain() {
    const mine = ++checkRound;
    domainCheck = 'checking';
    const found = await shell.phoneDomainCheck().catch(() => null);
    if (mine === checkRound) domainCheck = found ?? 'failed';
  }
  /** A segmented choice moved with the arrow keys (Home and End to its ends): the choice and the focus go together. */
  function arrows<T>(event: KeyboardEvent, list: readonly T[], current: T, pick: (next: T) => void) {
    const at = list.indexOf(current);
    const to = event.key === 'ArrowRight' || event.key === 'ArrowDown' ? Math.min(list.length - 1, at + 1)
      : event.key === 'ArrowLeft' || event.key === 'ArrowUp' ? Math.max(0, at - 1)
        : event.key === 'Home' ? 0 : event.key === 'End' ? list.length - 1 : -1;
    if (to < 0) return;
    event.preventDefault();
    pick(list[to]);
    const group = event.currentTarget as HTMLElement;
    requestAnimationFrame(() => group.querySelectorAll<HTMLElement>('[role="radio"]')[to]?.focus());
  }
  /** 开始 can be pressed (and Return starts it): an address typed, and a port typed or known (the setup read), none on
   *  its way. A port out of range is said on the press, under the form (R12-8). */
  const canStart = $derived(!domainStarting && !starting && (portInput.trim() !== '' || !!setup));
  const enter = (event: KeyboardEvent) => { if (event.key === 'Enter' && !composing(event) && canStart) applyNear(); };
  const enterPort = (event: KeyboardEvent) => { if (event.key === 'Enter' && !composing(event) && !starting) void startNear(); };
  const ownLink = $derived(view?.domain ?? null);
  $effect(() => {
    if (ownLink && !nearStarting) void checkDomain();
    else if (!ownLink) { checkRound++; domainCheck = null; }
  });

  /** The page shown: looking starts nothing and stops nothing. */
  function choose(next: 'local' | 'public') {
    phone.reach = next;
    if (next === 'public' && !outside && !opening) void checkTunnel();
  }
  /** From a link that failed or dropped: the same network's page, as it is. */
  function useLocal() {
    lost = null;
    publicFailed = null;
    choose('local');
  }
  /** Cloudflare not answering this once is often over by the next try: asked again once, quietly, before saying so. */
  const passing = (error: unknown) => String(error).includes('这次没有连上');
  /** 任何网络's 开始 (重新开始, 重试, 重新生成). */
  async function openPublic() {
    checking++;
    const mine = ++publicRound;
    epoch++;
    busy = true;
    publicFailed = null;
    lost = null;
    for (let attempt = 0; ; attempt++) {
      try {
        const made = await sent(shell.phonePublic());
        if (mine !== publicRound) return;
        setPhone(made);
        break;
      } catch (error) {
        // A 自有域名 start moving the server closes this link: whether it did is known once that start is done.
        if (moving) await moving;
        if (mine !== publicRound) return;
        if (attempt === 0 && passing(error)) continue;
        // Said on 任何网络's page, whichever page is shown now: the same network's code stays as it is (PHONE-12).
        publicFailed = String(error);
        lost = 'failed';
        break;
      }
    }
    busy = false;
  }
  /** 任何网络's 停止 (a link still being made is closed as it comes): its page says so; 同一个网络 stays as it is. */
  async function stopPublic() {
    const mine = ++publicRound;
    epoch++;
    busy = false;
    lost = null;
    publicFailed = null;
    phone.publicStopped = true;
    without({ public: null, secondsLeft: 0, publicOpening: false });
    void checkTunnel();
    const left = await sent(shell.phonePrivate()).catch(() => phone.view);
    if (mine === publicRound) setPhone(left);
  }
  async function copy(key: string, link: string) {
    clearTimeout(copiedTimer);
    // 已复制链接 only once the clipboard has taken it.
    const ok = await (navigator.clipboard?.writeText(link).then(() => true, () => false) ?? Promise.resolve(false));
    copied = { key, state: ok ? 'ok' : 'failed' };
    if (ok) copiedTimer = setTimeout(() => (copied = null), 2000);
  }
  /** What follows a link's # (#k=…&l=zh), in the pieces it may break between under its code: the key in two halves
   *  (side by side there is room for half of it on a line), then each & part. */
  function linkParts(url: string): string[] {
    const rest = url.match(/#.*$/)?.[0] ?? '';
    if (!rest) return [];
    const [first, ...more] = rest.split('&');
    const at = 3 + Math.ceil((first.length - 3) / 2);
    return [first.slice(0, at), first.slice(at), ...more.map((part) => '&' + part)].filter((part) => part !== '');
  }
  // Refused: the whole link, key and all, selected for a copy by hand.
  $effect(() => { if (copied?.state === 'failed' && manual) { manual.focus(); manual.select(); } });
  // A new link (another network, a public link) starts afresh.
  $effect(() => { void lan; void tailnet; void wireguard; void outside; void own; void proxyTail; copied = null; });

  let body: HTMLDivElement | undefined = $state();
  $effect(() => {
    const box = body;
    if (!box) return;
    let frame = 0;
    const observer = new ResizeObserver(() => { cancelAnimationFrame(frame); frame = requestAnimationFrame(() => onheight(58 + box.offsetHeight)); });
    observer.observe(box);
    return () => { observer.disconnect(); cancelAnimationFrame(frame); };
  });
</script>

{#snippet link(key: string, url: string, short = false, inline = false)}
  <!-- Under a code, the whole link as the phone opens it, key and all (用户：二维码下方你直接显示长链接吧，这样用户就知道
       我们帮他拼了一个key一样的东西。否则现在没概念；the address alone stood there, the key only in the code and in what
       a click copied): the address on its line, then what follows the # on the next (the key, the language and look
       the page opens in), in a fixed-width face, breaking between its parts where the room ends. A click copies it;
       已复制链接 then stands on the address's line for a moment, the lines under it kept (nothing below moves). Side by
       side, the address without its http://. In a row (inline: where a proxy forwards to, no key in it), the address
       alone, and the full link to copy by hand goes under the row instead (byHand). -->
  {@const done = copied?.key === key && copied.state === 'ok'}
  {@const parts = inline ? [] : linkParts(url)}
  {@const shown = short ? url.replace(/^https?:\/\//, '').replace(/\/?#.*/, '') : url.replace(/#.*/, '')}
  <!-- Copied, 已复制链接 stands in the middle of the link's whole box (用户：已复制链接上下居中), the address and the key
       kept in their places unseen, so the box is the same size and nothing under it moves. -->
  <button class="copy-link pressable" class:done={done && parts.length > 0} onclick={() => copy(key, url)} title={url}>
    {#if done && parts.length}<span class="room" aria-hidden="true">{shown}</span>{/if}<span class="address">{done ? t('已复制链接') : shown}</span>{#if parts.length}<span
      class="secret" class:gone={done}>{#each parts as part, i (i)}{#if i}<wbr />{/if}{part}{/each}</span>{/if}
  </button>
  {#if !inline}{@render byHand(key, url)}{/if}
{/snippet}

{#snippet byHand(key: string, url: string)}
  {#if copied?.key === key && copied.state === 'failed'}
    <p class="manual-note">{t('没能复制，下面是完整链接，选中后手动复制')}</p>
    <input class="manual" bind:this={manual} value={url} readonly aria-label={t('完整链接')} onfocus={(event) => event.currentTarget.select()} />
  {/if}
{/snippet}

{#snippet portRow()}
  <!-- 同一网络's port (用户：同一网络也支持指定端口号吧), one quiet line in the middle under the code's place, where the
       address stands once it is on (用户：换端口那里，感觉布局不优雅不好看: it was a wide box of its own, a small word at
       one end and the field at the other): the port as it is and 换端口, which turns it into a field; emptied, the app
       picks again. -->
  <div class="port-line">
    <span class="port-word">{t('端口')}</span>
    {#if portOpen}
      <input class="field-input num port-field" type="text" inputmode="numeric" autocomplete="off" autocorrect="off" spellcheck="false"
        maxlength="5" placeholder={t('自动')} aria-label={t('端口')} bind:this={portField} bind:value={portInput} oninput={() => (portEdited = true)} onkeydown={enterPort} />
    {:else}
      <span class="port-now num">{setup?.port ?? ''}</span>
      <button class="change pressable" onclick={openPort}>{t('换端口')}</button>
    {/if}
  </div>
  {#if portSaid}<p class="failed">{t(portSaid)}</p>{/if}
  {#if movesServer}<p class="note caution">{t('换端口后二维码要重新扫，Cloudflare 链接会关闭')}</p>{/if}
{/snippet}

{#snippet domainForm()}
  <!-- The address the user's proxy opens, and the address its proxy or tunnel forwards to, with the server's port,
       changed only when asked (the same network's codes use the same port). -->
  <div class="setup">
    <label class="field">
      <span class="field-label">{t('域名')}</span>
      <input class="field-input" type="text" autocomplete="off" autocapitalize="off" autocorrect="off" spellcheck="false"
        placeholder={t('https://starmeter.example.com（可不填）')} bind:value={domainInput} oninput={() => (domainEdited = true)} onkeydown={enter}
        onchange={() => { if (proxyOn && domainEdited && !domainStarting) void startDomain(); }} />
    </label>
    <div class="forward">
      <span class="field-label">{t('转发到')}</span>
      {#if forwardTo}{@render link('target', forwardTo, true, true)}{/if}
      {#if portOpen}
        <input class="field-input num port-field" type="text" inputmode="numeric" autocomplete="off" autocorrect="off" spellcheck="false"
          maxlength="5" placeholder={t('自动')} aria-label={t('端口')} bind:this={portField} bind:value={portInput} oninput={() => (portEdited = true)} onkeydown={enter} />
      {:else}
        <button class="change pressable" onclick={openPort}>{t('换端口')}</button>
      {/if}
    </div>
    {#if forwardTo}{@render byHand('target', forwardTo)}{/if}
  </div>
  {#if domainFailed}<p class="failed">{t(domainFailed)}</p>{/if}
  {#if movesServer}<p class="note caution">{t('换端口后二维码要重新扫，Cloudflare 链接会关闭')}</p>{/if}
  {#if phone.domainStopped}<p class="note">{t('重新开启后，原来的反代链接还能用。')}</p>{/if}
{/snippet}

{#snippet proxyRecipe(tail: string)}
  <!-- 反代 on with no domain given: how its link goes together, http(s):// your domain or IP (用户：http(s)这样写), then this (用户：域名也可以不写，但是开始
       之后，告诉用户如何拼域名/IP+后面的key). A click copies what goes after it. -->
  {@const done = copied?.key === 'recipe' && copied.state === 'ok'}
  <div class="recipe">
    <span class="card-title">{t('反代')}</span>
    <button class="copy-link pressable" class:done onclick={() => copy('recipe', tail)} title={tail}>
      {#if done}<span class="room" aria-hidden="true">http(s)://{t('你的域名或 IP')}</span>{/if}<span class="address">{#if done}{t('已复制')}{:else}http(s)://<span
            class="slot">{t('你的域名或 IP')}</span>{/if}</span><span class="secret" class:gone={done}>{#each linkParts('#' + tail.replace(/^\/#/, '')) as part, i (i)}{#if i}<wbr />{/if}{i ? part : '/' + part}{/each}</span>
    </button>
    <span class="card-detail">{t('把这一段接在你的域名或 IP 后面，手机就能打开')}</span>
    {@render byHand('recipe', tail)}
  </div>
{/snippet}

{#snippet domainCheckLine()}
  <!-- Whether the domain opens this server from here, asked once it is on. -->
  {#if own}
    {#if domainCheck === 'checking'}
      <p class="check"><span class="spinner small" aria-hidden="true"></span>{t('正在确认手机能打开…')}</p>
    {:else if checked?.opens}
      <p class="check ok">{checkText}</p>
    {:else if domainCheck}
      <div class="check caution" role="status"><span>{checkText}</span><button class="again pressable" onclick={checkDomain}>{t('再检查')}</button></div>
    {/if}
  {/if}
{/snippet}

{#snippet linkLine(of: 'near' | 'vpn')}
  <!-- Under a page's codes while it is on, one quiet line as the port's is: 换链接. The links keep their key until it is
       pressed (用户：那个key可不可以固定啊？用户可以手动刷新，刷新之前就不要改变), and it asks once more in its place
       first: the codes from before stop opening. 停止 stays the page's one button. -->
  <div class="link-line" aria-live="polite">
    {#if renewing === of}
      <span>{t('原来的二维码将不能再用')}</span>
      <button class="change sure pressable" onclick={() => newLink(of)}>{t('确定换')}</button>
      <button class="change pressable" onclick={() => (renewing = null)}>{t('取消')}</button>
    {:else if renewed === of}
      <span>{t('已换成新链接，手机重新扫码')}</span>
    {:else}
      <button class="change pressable" onclick={() => (renewing = of)}>{t('换链接')}</button>
    {/if}
  </div>
{/snippet}

{#snippet halted(title: string, detail: string)}
  <!-- In the code's place, as large: what was there says it is over. -->
  <div class="code">
    <div class="stopped" role="status">
      <span class="stopped-glyph"><Icon name="iphone" size={22} /></span>
      <span class="stopped-title">{t(title)}</span>
      <span class="stopped-detail">{t(detail)}</span>
    </div>
  </div>
{/snippet}

<div class="phone-page">
  <div class="grip" class:movable={panel.pinned} aria-hidden="true"
    onmousedown={grabWindow}></div>
  <span class="back"><HintButton icon="chevron.left" size={17} hint={t('返回')} label={t('返回')} onclick={onback} /></span>
  <span class="page-glyph"><Icon name="iphone" size={17} /></span>
  <h2 class="title">{t('在手机上看')}</h2>
  <span class="rule"></span>
  <div class="scroll">
    <Scroller onswipeback={onback}>
      <div class="content" bind:this={body}>
        <!-- The two pages: looking starts nothing and stops nothing; a green dot after each way that is on. -->
        <div class="reach" role="radiogroup" tabindex="-1" aria-label={t('手机在哪')} onkeydown={(event) => arrows(event, choices, reach, choose)}>
          <span class="pill" style:transform="translateX({reach === 'public' ? 100 : 0}%)"></span>
          {#each choices as choice (choice)}
            <button class="choice pressable" class:chosen={reach === choice} role="radio" aria-checked={reach === choice}
              tabindex={reach === choice ? 0 : -1} onclick={() => choose(choice)}>{t(choice === 'local' ? '同一网络 / 反代' : '任何网络')}{#if on[choice]}<span class="on" aria-hidden="true"></span>{/if}</button>
          {/each}
        </div>

        {#if reach === 'local'}
          <!-- 同一网络 / 反代: one page (用户：可以只开同一网络，或者同一网络和反代一起开两种模式。显示在一个页面就好啊，不要分开
               显示): the same network, and with 同时开启反代 the user's own domain through their own reverse proxy beside it,
               on the same page, started and stopped together. The network gets in through the same network only, so a proxy
               on a NAS or the router reaches this computer while it is on. -->
          <button class="proxy-row pressable" role="switch" aria-checked={phone.near === 'domain'}
            onclick={() => pickNear(phone.near === 'domain' ? 'local' : 'domain')}>
            <span class="proxy-text">
              <span class="proxy-title">{t('同时开启反代')}</span>
              <span class="proxy-detail">{t('用你自己的域名，手机在哪都能打开')}</span>
            </span>
            <span class="switch" class:on={phone.near === 'domain'}><span class="knob"></span></span>
          </button>
          {#if localFailed}
            {@render halted('没能开启', '原因在下面，可以再试一次')}
            <p class="failed">{t(localFailed)}</p>
            {#if phone.near === 'domain'}{@render domainForm()}{:else}{@render portRow()}{/if}
            <div class="actions"><button class="action primary pressable" disabled={phone.near === 'domain' && !canStart} onclick={startNear}>{t('重试')}</button></div>
          {:else if localOn || proxyOn}
            {#if nearCodes.length > 1}
              <!-- The network's code and the domain's side by side, the same server. -->
              <div class="codes">
                {#each nearCodes as code (code.key)}
                  <div class="card">
                    <span class="card-title">{t(code.title)}</span>
                    <QrCode text={code.url} size={132} label={t(code.label)} />
                    {@render link(code.key, code.url, true)}
                    <span class="card-detail">{t(code.detail)}</span>
                  </div>
                {/each}
              </div>
            {:else if nearCodes.length === 1}
              <div class="code"><QrCode text={nearCodes[0].url} size={188} label={t(nearCodes[0].label)} /></div>
              {@render link(nearCodes[0].key, nearCodes[0].url)}
            {:else}
              <div class="code"><span class="wait">{t('这台电脑没有连上网络')}</span></div>
            {/if}
            {#if proxyTail}{@render proxyRecipe(proxyTail)}{/if}
            {#if nearCodes.length || proxyTail}{@render linkLine('near')}{/if}
            {@render domainCheckLine()}
            {#if phone.near === 'domain'}
              <!-- 反代 on (or starting) with the switch: its domain and where it forwards to, set here, applied on Return. -->
              {@render domainForm()}
            {/if}
            <!-- Short (用户：图中说明能省就省，简洁点，字太多): what to scan, and that it stays on. -->
            {#if own && lan}
              <p class="note">{t('同一个 Wi-Fi 扫“局域网”，在别处扫“反代”。只读，点“停止”前一直开着。')}</p>
            {:else if own}
              <p class="note">{t('手机在哪都能扫。只读，点“停止”前一直开着。')}</p>
            {:else if lan}
              <p class="note">{t('手机连同一个 Wi-Fi 后扫码。只读，点“停止”前一直开着。')}</p>
            {/if}
            <div class="actions"><button class="action danger pressable" onclick={stopNear}>{t('停止')}</button></div>
          {:else if starting || domainStarting}
            <div class="code"><span class="spinner" aria-label={t('正在开启')}></span></div>
          {:else}
            <!-- Off, as it starts: what its 开始 gives, its port (with 反代, its fields in the port's place, on the same page),
                 and the button. -->
            {#if phone.localStopped}
              {@render halted('已停止', '手机上的页面已断开')}
            {:else}
              {@render halted('未开始', '在同一个网络里直连这台电脑')}
            {/if}
            {#if phone.near === 'domain'}{@render domainForm()}{:else}{@render portRow()}{/if}
            <p class="note centre">{t(phone.localStopped ? '重新开始后，原来的二维码还能用。' : '手机连同一个 Wi-Fi，扫码就能看。')}</p>
            <div class="actions"><button class="action primary pressable" disabled={phone.near === 'domain' && !canStart} onclick={startNear}>{t('开始')}</button></div>
          {/if}
        {:else}
          <!-- 任何网络: Cloudflare's link for an hour, or the VPNs this computer is on (Tailscale, WireGuard), each a page of
               its own and on and off by itself, so both can be on at once. -->
          <div class="via" role="radiogroup" tabindex="-1" aria-label={t('外网链接从哪来')} onkeydown={(event) => arrows(event, vias, phone.via, chooseVia)}>
            <span class="via-pill" style:transform="translateX({vias.indexOf(phone.via) * 100}%)"></span>
            {#each vias as choice (choice)}
              <button class="via-choice pressable" class:chosen={phone.via === choice} role="radio" aria-checked={phone.via === choice}
                tabindex={phone.via === choice ? 0 : -1} onclick={() => chooseVia(choice)}>{t(choice === 'cloudflare' ? 'Cloudflare 临时链接' : 'Tailscale 等 VPN')}{#if viaOn[choice]}<span class="on" aria-hidden="true"></span>{/if}</button>
            {/each}
          </div>
          {#if phone.via === 'vpn'}
            <!-- A phone on the same tailnet or WireGuard network opens this computer's address on it from anywhere, with
                 the VPN page's own key; only connections to those addresses are taken, and only while it is on. -->
            {#if vpnFailed}
              {@render halted('没能开启', '原因在下面，可以再试一次')}
              <p class="failed">{t(vpnFailed)}</p>
              {#if /^端口 \d+ 被别的程序占用$/.test(vpnFailed)}<p class="note">{t('在“同一网络 / 反代”里换端口，或退出占用它的程序。')}</p>{/if}
              <div class="actions"><button class="action primary pressable" onclick={startVpn}>{t('重试')}</button></div>
            {:else if vpnOn}
              {#if vpnCodes.length > 1}
                <div class="codes">
                  {#each vpnCodes as code (code.key)}
                    <div class="card">
                      <span class="card-title">{t(code.title)}</span>
                      <QrCode text={code.url} size={132} label={t(code.label)} />
                      {@render link(code.key, code.url, true)}
                      <span class="card-detail">{t(code.detail)}</span>
                    </div>
                  {/each}
                </div>
                {@render linkLine('vpn')}
                <p class="note">{t('手机连着哪个 VPN 就扫哪个，不经过 Cloudflare。只读，点“停止”前一直开着。')}</p>
              {:else if vpnCodes.length === 1}
                <div class="code"><QrCode text={vpnCodes[0].url} size={188} label={t(vpnCodes[0].label)} /></div>
                {@render link(vpnCodes[0].key, vpnCodes[0].url)}
                {@render linkLine('vpn')}
                <p class="note">{t(vpnNote(vpnCodes[0].key))}</p>
              {:else}
                {@render halted('没有连着 VPN', '这台电脑连上 Tailscale 或 WireGuard 后，这里就有二维码')}
              {/if}
              <div class="actions"><button class="action danger pressable" onclick={stopVpn}>{t('停止')}</button></div>
            {:else if vpnStarting}
              <div class="code"><span class="spinner" aria-label={t('正在开启')}></span></div>
            {:else}
              {#if phone.vpnStopped}
                {@render halted('已停止', '手机上的页面已断开')}
                <p class="note">{t('重新开始后，原来的二维码还能用。')}</p>
              {:else}
                {@render halted('未开始', '经 Tailscale 或 WireGuard 直连这台电脑')}
                <p class="note">{t('手机和电脑连着同一个 Tailscale 或 WireGuard 时，在哪都能打开，不经过 Cloudflare。')}</p>
              {/if}
              {#if setup && !setup.tailnet && !setup.wireguard}<p class="note caution">{t('这台电脑还没连上 Tailscale 或 WireGuard。')}</p>{/if}
              <div class="actions"><button class="action primary pressable" onclick={startVpn}>{t('开始')}</button></div>
            {/if}
          {:else if opening}
            <div class="code">
              <span class="spinner" aria-label={t('生成链接')}></span>
              <span class="wait">{t('正在向 Cloudflare 要链接…')}</span>
              <!-- PHONE-11: shown once it opens from outside, so a phone scanning it never meets a name not there yet. -->
              <span class="wait-detail">{t('确认手机能打开后再显示，通常十几秒')}</span>
            </div>
            <div class="actions"><button class="action danger pressable" onclick={stopPublic}>{t('停止')}</button></div>
          {:else if lost}
            {@render halted(lost === 'dropped' ? '外网链接断开了' : '没能生成外网链接', lost === 'dropped' ? '可以重新生成一个新链接' : '原因在下面，可以再试一次')}
            {#if publicFailed}<p class="failed">{t(publicFailed)}</p>{/if}
            {#if publicFailed && /^端口 \d+ 被别的程序占用$/.test(publicFailed)}<p class="note">{t('在“同一网络 / 反代”里换端口，或退出占用它的程序。')}</p>{/if}
            <div class="actions">
              <button class="action pressable" onclick={useLocal}>{t('改用同一个网络')}</button>
              <button class="action primary pressable" onclick={openPublic}>{t(lost === 'dropped' ? '重新生成' : '重试')}</button>
            </div>
          {:else if outside}
            <div class="code"><QrCode text={outside} size={188} label={t('手机扫这个二维码')} /></div>
            {@render link('public', outside)}
            {#if view?.publicChecked === false}
              <!-- The wait for it to open ran out: shown all the same, with what to do if it doesn't open yet. -->
              <p class="note caution">{t('还没确认手机能打开：扫码打不开就等一分钟再试')}</p>
            {/if}
            <p class="note">{t('任何网络都能打开，还能用 {n} 分钟。只读。', { n: minutes })}</p>
            <p class="note">{t('网页右上角的太阳可让屏幕常亮。')}</p>
            <div class="actions"><button class="action danger pressable" onclick={stopPublic}>{t('停止')}</button></div>
          {:else}
            <!-- Off, as it starts: what its 开始 does (the figures go through Cloudflare) said before it is pressed. -->
            {#if phone.publicStopped}
              {@render halted('已停止', '外网链接已关闭')}
              <p class="note">{t('原来的外网链接已失效，重新开始后再扫。')}</p>
            {:else}
              {@render halted('未开始', '经 Cloudflare 的临时链接')}
            {/if}
            <p class="note">{t('经 Cloudflare 的临时 https 链接打开：数字会经过 Cloudflare，一小时后失效。')}</p>
            <!-- Measured from 29 points across mainland China, the link failed at most of them: said before it starts. -->
            <p class="note">{t('中国大陆部分地区打不开；能连同一个 Wi-Fi 时用“同一网络”更稳。')}</p>
            {#if download}<p class="note">{t('第一次要下载 cloudflared（约 20 MB）。')}</p>{/if}
            <div class="actions"><button class="action primary pressable" onclick={openPublic}>{t(download ? '下载并开始' : '开始')}</button></div>
          {/if}
        {/if}
      </div>
    </Scroller>
  </div>
</div>

<style>
  .phone-page { position: relative; width: 100%; height: 100%; }
  .grip { position: absolute; left: 0; top: 0; width: 100%; height: 48px; }
  .grip.movable { cursor: grab; }
  .back { position: absolute; left: 10px; top: 11px; }
  .page-glyph { position: absolute; left: 48px; top: 16px; pointer-events: none; }
  .title { position: absolute; left: 73px; top: 15px; margin: 0; font-size: 14px; font-weight: 600; line-height: 20px; pointer-events: none; }
  .rule { position: absolute; left: 16px; right: 16px; top: 49px; height: 1px; background: var(--separator); }
  .scroll { position: absolute; left: 0; right: 0; top: 58px; bottom: 0; display: flex; }
  /* Symmetrical 16px inset on left, right, and bottom edges. */
  .content { padding: 0 16px 16px; display: flex; flex-direction: column; align-items: stretch; gap: 12px; }
  .reach {
    position: relative; display: flex; height: 28px; padding: 2px; box-sizing: border-box; border-radius: 8px;
    background: color-mix(in srgb, var(--label) 6%, transparent);
  }
  .pill {
    position: absolute; top: 2px; bottom: 2px; left: 2px; width: calc(50% - 2px); border-radius: 6px; background: var(--solid);
    box-shadow: 0 0 0 0.5px var(--separator), 0 1px 3px rgba(0, 0, 0, 0.1); transition: transform 0.22s var(--page-curve), width 0.22s var(--page-curve);
  }
  .choice { --press-scale: 0.96; position: relative; flex: 1; display: flex; align-items: center; justify-content: center; font-size: 12px; color: var(--secondary); border-radius: 6px; }
  .choice.chosen { color: var(--label); font-weight: 600; }
  .on { width: 6px; height: 6px; margin-left: 5px; border-radius: 50%; background: var(--green); flex: none; }
  .code { display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 8px; min-height: 196px; }
  .stopped {
    width: 188px; height: 188px; box-sizing: border-box; border-radius: 30px; display: flex; flex-direction: column;
    align-items: center; justify-content: center; gap: 4px; border: 1px dashed color-mix(in srgb, var(--label) 22%, transparent);
    color: var(--secondary);
  }
  .stopped-glyph { opacity: 0.7; margin-bottom: 4px; }
  .stopped-title { font-size: 14px; font-weight: 600; color: var(--label); padding: 0 12px; text-align: center; }
  .stopped-detail { font-size: 11px; line-height: 1.4; padding: 0 16px; text-align: center; }
  .wait { font-size: 11.5px; color: var(--secondary); }
  .wait-detail { max-width: 240px; font-size: 10.5px; line-height: 1.4; color: var(--tertiary); text-align: center; text-wrap: balance; }
  /* A code's link, whole: the address, then its key under it; long ones break where they must. */
  .copy-link {
    --press-scale: 0.98; position: relative; align-self: center; box-sizing: border-box; max-width: 100%; padding: 4px 8px; border-radius: 7px; font-size: 11px; line-height: 1.4;
    color: var(--secondary); font-variant-numeric: tabular-nums; text-align: center; overflow-wrap: anywhere;
  }
  .copy-link:hover { background: var(--hover); }
  .address, .secret, .room { display: block; }
  .secret { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 10px; }
  /* Copied: 已复制链接 in the middle of the whole box, over the address and the key kept unseen in their places. */
  .secret.gone, .room { visibility: hidden; }
  .copy-link.done .address { position: absolute; inset: 0; display: flex; align-items: center; justify-content: center; }
  .manual-note { margin: 0; font-size: 10.5px; color: var(--orange); text-align: center; }
  .manual {
    align-self: stretch; height: 26px; padding: 0 8px; border: 0; border-radius: 7px; font: inherit; font-size: 11px;
    color: var(--label); background: color-mix(in srgb, var(--label) 6%, transparent); user-select: text; -webkit-user-select: text;
  }
  .note { margin: 0; font-size: 11px; line-height: 1.5; color: var(--secondary); }
  /* Two to a row, the same size; a third under them in the middle. */
  .codes { display: flex; flex-wrap: wrap; justify-content: center; gap: 10px; }
  .card {
    flex: 0 0 calc(50% - 5px); box-sizing: border-box; min-width: 0; display: flex; flex-direction: column; align-items: center; gap: 6px; padding: 10px 6px 9px;
    border-radius: 12px; background: color-mix(in srgb, var(--label) 4%, transparent);
  }
  .card-title { font-size: 12px; font-weight: 600; line-height: 16px; color: var(--label); }
  .card-detail { max-width: 100%; font-size: 10.5px; line-height: 14px; color: var(--tertiary); text-align: center; }
  .card .copy-link { margin: -2px 0; padding: 4px 2px; }
  .note.caution { color: var(--orange); }
  .failed { margin: 0; padding: 8px 10px; border-radius: 8px; font-size: 11.5px; color: var(--label); background: color-mix(in srgb, var(--orange) 18%, transparent); }
  /* Centered half-width tinted actions that blend into the panel's material; two actions share the row evenly. */
  .actions { display: flex; gap: 8px; justify-content: center; }
  .action {
    --press-scale: 0.98; flex: 1; min-width: 0; height: 32px; padding: 0 14px; border-radius: 8px;
    display: flex; align-items: center; justify-content: center; white-space: nowrap;
    font-size: 12px; font-weight: 600; color: var(--label);
    background: color-mix(in srgb, var(--label) 7%, transparent);
    box-shadow: inset 0 0 0 0.5px var(--separator);
  }
  .action:only-child { flex: 0 0 50%; }
  .action:disabled { opacity: 0.42; pointer-events: none; }
  .action:hover { background: color-mix(in srgb, var(--label) 12%, transparent); }
  .action.danger {
    color: var(--red);
    background: color-mix(in srgb, var(--red) 14%, transparent);
    box-shadow: inset 0 0 0 0.5px color-mix(in srgb, var(--red) 30%, transparent);
  }
  .action.danger:hover { background: color-mix(in srgb, var(--red) 22%, transparent); }
  .action.primary {
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 16%, transparent);
    box-shadow: inset 0 0 0 0.5px color-mix(in srgb, var(--accent) 32%, transparent);
  }
  .action.primary:hover { background: color-mix(in srgb, var(--accent) 24%, transparent); }
  .spinner { width: 16px; height: 16px; border-radius: 50%; border: 2px solid var(--separator); border-top-color: var(--secondary); animation: spin 0.8s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
  /* 任何网络's two links, under the pages' choice and lined up with it, a little lower. */
  .via {
    position: relative; display: flex; height: 26px; padding: 2px; box-sizing: border-box; border-radius: 8px;
    background: color-mix(in srgb, var(--label) 4%, transparent);
  }
  .via-pill {
    position: absolute; top: 2px; bottom: 2px; left: 2px; width: calc(50% - 2px); border-radius: 6px; background: var(--solid);
    box-shadow: 0 0 0 0.5px var(--separator), 0 1px 2px rgba(0, 0, 0, 0.08); transition: transform 0.22s var(--page-curve);
  }
  .via-choice {
    --press-scale: 0.96; position: relative; flex: 1; min-width: 0; display: flex; align-items: center; justify-content: center;
    font-size: 11.5px; color: var(--secondary); border-radius: 6px; white-space: nowrap;
  }
  .via-choice.chosen { color: var(--label); font-weight: 600; }
  /* 同时开启反代: a row with its switch, as the settings' rows have. */
  .proxy-row {
    --press-scale: 0.99; display: flex; align-items: center; gap: 10px; padding: 9px 10px 9px 12px; border-radius: 10px; text-align: left;
    background: color-mix(in srgb, var(--label) 4%, transparent);
  }
  .proxy-row:hover { background: color-mix(in srgb, var(--label) 7%, transparent); }
  .proxy-text { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 2px; }
  .proxy-title { font-size: 12px; font-weight: 600; color: var(--label); }
  .proxy-detail { font-size: 10.5px; color: var(--secondary); }
  .switch {
    position: relative; flex: none; width: 30px; height: 18px; border-radius: 9px;
    background: color-mix(in srgb, var(--label) 19%, transparent); transition: background-color 0.18s var(--page-curve);
  }
  .switch.on { background: var(--accent); }
  .knob {
    position: absolute; left: 2px; top: 2px; width: 14px; height: 14px; border-radius: 50%; background: white;
    box-shadow: 0 0.5px 2px rgba(0, 0, 0, 0.25); transition: transform 0.18s cubic-bezier(0.33, 1, 0.68, 1);
  }
  .switch.on .knob { transform: translateX(12px); }
  /* 自有域名's fields, in the code's place: the address, and the address to forward to with the port. */
  .setup {
    display: flex; flex-direction: column; gap: 10px; padding: 10px 10px 8px; border-radius: 12px;
    background: color-mix(in srgb, var(--label) 4%, transparent);
  }
  .field { display: flex; flex-direction: column; gap: 4px; min-width: 0; }
  .field-label { font-size: 10.5px; color: var(--secondary); }
  .field-input {
    width: 100%; height: 28px; padding: 0 8px; box-sizing: border-box; border: 0; border-radius: 7px; outline: none;
    font: inherit; font-size: 12px; color: var(--label); background: color-mix(in srgb, var(--label) 6%, transparent);
    box-shadow: inset 0 0 0 0.5px var(--separator); user-select: text; -webkit-user-select: text;
  }
  .field-input:focus { box-shadow: inset 0 0 0 1.2px var(--accent); }
  .field-input::placeholder { color: var(--tertiary); }
  .field-input.num { font-variant-numeric: tabular-nums; }
  .forward { display: flex; align-items: center; gap: 4px; min-width: 0; min-height: 26px; }
  .forward .field-label { flex: none; }
  .forward .copy-link { margin-left: auto; font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 10.5px; white-space: nowrap; overflow: hidden; overflow: clip visible; text-overflow: ellipsis; }
  .change {
    --press-scale: 0.95; flex: none; height: 22px; padding: 0 7px; border-radius: 6px; font-size: 10.5px; color: var(--accent);
  }
  .change:hover { background: var(--hover); }
  .port-field { flex: none; width: 62px; height: 24px; text-align: center; }
  /* 同一网络's port: one line in the middle, in the address's place and its quiet look. */
  .port-line, .link-line { display: flex; align-items: center; justify-content: center; gap: 5px; min-height: 26px; margin: -2px 0; font-size: 11px; color: var(--secondary); }
  .port-now { font-variant-numeric: tabular-nums; }
  /* 换链接 under a page's codes: the same line, a little closer to the address over it; asked about, its words may wrap. */
  .link-line { flex-wrap: wrap; margin: -8px 0 -4px; }
  .change.sure { color: var(--red); }
  .note.centre { text-align: center; }
  /* 反代 with no domain: its title, your domain or IP then the part to put after it, and one line on what to do. */
  .recipe { display: flex; flex-direction: column; align-items: center; gap: 4px; padding: 10px 8px 9px; border-radius: 12px; background: color-mix(in srgb, var(--label) 4%, transparent); }
  .slot { padding: 0 4px; border-radius: 4px; border: 1px dashed var(--separator); color: var(--tertiary); }
  .check { margin: -4px 0 0; display: flex; align-items: center; justify-content: center; gap: 6px; font-size: 11px; line-height: 1.45; color: var(--secondary); text-align: center; }
  .check.ok { color: var(--green); }
  .check.caution {
    margin: 0; justify-content: space-between; text-align: left; padding: 7px 8px 7px 10px; border-radius: 8px; color: var(--label);
    background: color-mix(in srgb, var(--orange) 16%, transparent);
  }
  .again {
    --press-scale: 0.95; flex: none; height: 22px; padding: 0 8px; border-radius: 6px; font-size: 10.5px; font-weight: 600; color: var(--label);
    background: color-mix(in srgb, var(--label) 8%, transparent);
  }
  .again:hover { background: color-mix(in srgb, var(--label) 13%, transparent); }
  .spinner.small { width: 10px; height: 10px; border-width: 1.5px; flex: none; }
</style>
