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
  import { onDestroy, onMount } from 'svelte';
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
  let portOpen = $state(false);
  /** The port's field to take the focus once it is there: as 换端口 opens it, or a start fails on the port. */
  let focusPort = $state(false);
  let portField: HTMLInputElement | undefined = $state();
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
    ? '手机连上同一个 Tailscale 网络后扫这个，在哪都能打开，不经过 Cloudflare。点“停止”或退出 App 前一直开着。'
    : '手机连上同一个 WireGuard 网络后扫这个，在哪都能打开，不经过 Cloudflare。点“停止”或退出 App 前一直开着。';
  const outside = $derived(looked(view?.public));
  /** 自有域名's link while it is on, its address, and where its proxy forwards to (this computer's port). */
  const own = $derived(looked(view?.domain));
  const ownOrigin = $derived(view?.domain ? view.domain.replace(/\/?#.*$/, '') : null);
  const ownHost = $derived(ownOrigin?.replace(/^https?:\/\//, '') ?? '');
  const portTyped = $derived(/^\d+$/.test(portInput.trim()) ? Number(portInput.trim()) : null);
  const forwardPort = $derived(own ? view?.port : portTyped ?? setup?.port);
  /** Where the user's proxy or tunnel on this computer hands requests on to: this computer itself (not 0.0.0.0: that is
   *  where the server listens, not an address to send to). A proxy on a NAS or the router comes in from the network,
   *  which only 同一个网络 lets in: it forwards to this computer's network address, with 同一个网络 on. */
  const forwardTo = $derived(forwardPort ? `http://127.0.0.1:${forwardPort}` : null);
  const forwardLan = $derived(forwardPort && setup?.lan ? `${setup.lan}:${forwardPort}` : null);
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
  const movesServer = $derived(!own && !!view && !!view.port && portTyped !== null && portTyped !== view.port
    && (localOn || vpnOn || !!outside || opening));
  /** Which ways are on, a green dot after each one's name. */
  const on = $derived({ local: localOn || !!own, public: !!outside || opening || vpnOn });

  /** Each way's steps belong to its own round, so what one answers late can't undo a later step of the same way: a
   *  start after a 停止, a link closed while it was made. Every step also moves `epoch`, so the status asked every ten
   *  seconds, if a step came between its asking and its answer, is dropped instead of putting back what has gone. */
  let localRound = 0, vpnRound = 0, publicRound = 0, domainRound = 0, epoch = 0;
  /** A 自有域名 start moving the server on its way: Cloudflare's link waits for it before saying it failed (the move
   *  closes it, and the host answers that the phone view closed). */
  let moving: Promise<void> | null = null;

  /** What is left on once a way goes, shown at once: nothing on, nothing at all (the dot on the homepage's button goes
   *  too). The host's own answer follows. */
  function without(gone: Partial<PhoneView>) {
    const left = view ? { ...view, ...gone } : null;
    setPhone(left && (left.localOpen || left.vpnOpen || left.public || left.publicOpening || left.domain) ? left : null);
  }

  /** 同一个网络's 开始 (重新开始, 重试). A start that fails stays in the code's place with its reason and 重试, instead of
   *  leaving a page with neither a code nor a way on. */
  async function startLocal() {
    const mine = ++localRound;
    epoch++;
    localFailed = null;
    starting = true;
    try {
      const started = await shell.phoneStart();
      if (mine === localRound) setPhone(started);
    } catch (error) {
      if (mine === localRound) localFailed = String(error);
    }
    if (mine === localRound) starting = false;
  }
  /** 同一个网络's 停止: its codes stop opening; the public link, if up, stays as it is. */
  async function stopLocal() {
    const mine = ++localRound;
    epoch++;
    starting = false;
    localFailed = null;
    phone.localStopped = true;
    without({ localOpen: false, local: null });
    const left = await shell.phoneStop().catch(() => phone.view);
    if (mine === localRound) setPhone(left ?? null);
  }

  /** The VPN page's 开始 (重试): on with its own key; a start that fails stays in the code's place with its reason. */
  async function startVpn() {
    const mine = ++vpnRound;
    epoch++;
    vpnFailed = null;
    vpnStarting = true;
    try {
      const started = await shell.phoneVpnStart();
      if (mine === vpnRound) { phone.vpnStopped = false; setPhone(started); }
    } catch (error) {
      if (mine === vpnRound) vpnFailed = String(error);
    }
    if (mine === vpnRound) vpnStarting = false;
  }
  /** The VPN page's 停止: its codes stop opening; the other ways as they are. */
  async function stopVpn() {
    const mine = ++vpnRound;
    epoch++;
    vpnStarting = false;
    vpnFailed = null;
    phone.vpnStopped = true;
    without({ vpnOpen: false, tailnet: null, wireguard: null });
    const left = await shell.phoneVpnStop().catch(() => phone.view);
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
  }
  /** 自有域名 as the shell has it: its fields filled from it, and its page chosen when a domain is set and nothing else has
   *  been chosen. */
  function settled(next: DomainSetup) {
    setup = next;
    domainInput = next.domain ? next.domain.replace(/^https:\/\//, '') : domainInput;
    portInput = String(next.port);
    if (next.domain && !phone.nearChosen) phone.near = 'domain';
  }
  onMount(() => {
    // Nothing starts by opening the page: each way as it was left, off until its 开始. What is set for 自有域名, unless
    // a 开始 or 停止 of it came first (its answer is newer).
    void pollStatus();
    const asked = domainRound;
    void shell.phoneDomain().then((next) => { if (next && asked === domainRound) settled(next); }).catch(() => {});
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
  onDestroy(() => { localRound++; vpnRound++; publicRound++; epoch++; clearInterval(tick); clearInterval(fast); clearTimeout(copiedTimer); });

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
    const port = portTyped;
    if (port === null || port < 1024 || port > 65535) { domainFailed = '端口填 1024 到 65535 之间的数'; openPort(); return; }
    const mine = ++domainRound;
    epoch++;
    // A port move closes Cloudflare's link (and one still being made). Its round is held meanwhile, so the host's
    // answer to it (手机查看已经关闭) isn't said as a failure; ended as its 停止 would once the move is done, handed
    // back as it was when the start fails before anything moved.
    const moves = movesServer && (outside || opening);
    const publicBefore = publicRound;
    let moved = () => {};
    if (moves) moving = new Promise((done) => (moved = done));
    domainStarting = true;
    try {
      const started = await serial(() => shell.phoneDomainStart(domainInput, port));
      if (moves && publicRound === publicBefore) {
        publicRound++;
        busy = false;
        lost = null;
        publicFailed = null;
        phone.publicStopped = true;
      }
      if (mine !== domainRound) return;
      phone.domainStopped = false;
      setPhone(started);
      const next = await shell.phoneDomain().catch(() => null);
      if (next && mine === domainRound) settled(next);
    } catch (error) {
      // Nothing moved: Cloudflare's link as the host has it now (an answer held meanwhile is in its status).
      if (moves) void pollStatus();
      if (mine !== domainRound) return;
      domainFailed = String(error);
      // A port another app holds: the port's field open, to pick another.
      if (/^端口 \d+ 被别的程序占用$/.test(domainFailed)) openPort();
    } finally {
      if (moves) { moved(); moving = null; }
      if (mine === domainRound) domainStarting = false;
    }
  }
  /** The kind's 开始: the same network, and in the second mode the domain with it (the network first, so a proxy on a NAS
   *  or the router gets in). */
  async function startNear() {
    if (!localOn) await startLocal();
    if (phone.near === 'domain' && !localFailed && !own) await startDomain();
  }
  /** The kind's 停止: whatever of it is on. */
  async function stopNear() {
    await Promise.all([own ? stopDomain() : null, localOn || localFailed ? stopLocal() : null]);
  }
  /** 同时开启反代: off takes the domain's link down with it; on again, its 开始 is on the form, on the same page. */
  function pickNear(next: Near) {
    chooseNear(next);
    if (next === 'local' && own) void stopDomain();
  }
  /** 自有域名's 停止: its links stop opening; its address and port stay for the next 开始. */
  async function stopDomain() {
    const mine = ++domainRound;
    epoch++;
    domainStarting = false;
    domainFailed = null;
    phone.domainStopped = true;
    without({ domain: null });
    const left = await serial(() => shell.phoneDomainStop()).catch(() => phone.view);
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
  /** 开始 can be pressed (and Return starts it): an address typed, and the port known (the setup read), none on its way. */
  const canStart = $derived(!domainStarting && !starting && !!domainInput.trim() && !!forwardPort);
  const enter = (event: KeyboardEvent) => { if (event.key === 'Enter' && !composing(event) && canStart) void startNear(); };
  const ownLink = $derived(view?.domain ?? null);
  $effect(() => {
    if (ownLink) void checkDomain();
    else { checkRound++; domainCheck = null; }
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
        const opened = await shell.phonePublic();
        if (mine !== publicRound) return;
        setPhone(opened);
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
    const left = await shell.phonePrivate().catch(() => phone.view);
    if (mine === publicRound) setPhone(left);
  }
  async function copy(key: string, link: string) {
    clearTimeout(copiedTimer);
    // 已复制链接 only once the clipboard has taken it.
    const ok = await (navigator.clipboard?.writeText(link).then(() => true, () => false) ?? Promise.resolve(false));
    copied = { key, state: ok ? 'ok' : 'failed' };
    if (ok) copiedTimer = setTimeout(() => (copied = null), 2000);
  }
  // Refused: the whole link, key and all, selected for a copy by hand.
  $effect(() => { if (copied?.state === 'failed' && manual) { manual.focus(); manual.select(); } });
  // A new link (another network, a public link) starts afresh.
  $effect(() => { void lan; void tailnet; void wireguard; void outside; void own; copied = null; });

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
  <!-- Side by side, the address alone (192.168.1.20:51234): the key stays in the code and the copied link. In a row
       (inline), the full link to copy by hand goes under the row instead (byHand). -->
  <button class="address pressable" onclick={() => copy(key, url)} title={url}>{copied?.key === key && copied.state === 'ok'
    ? t('已复制链接') : short ? url.replace(/^https?:\/\//, '').replace(/\/?#.*/, '') : url.replace(/#.*/, '')}</button>
  {#if !inline}{@render byHand(key, url)}{/if}
{/snippet}

{#snippet byHand(key: string, url: string)}
  {#if copied?.key === key && copied.state === 'failed'}
    <p class="manual-note">{t('没能复制，下面是完整链接，选中后手动复制')}</p>
    <input class="manual" bind:this={manual} value={url} readonly aria-label={t('完整链接')} onfocus={(event) => event.currentTarget.select()} />
  {/if}
{/snippet}

{#snippet domainForm()}
  <!-- The address the user's proxy opens, and the address its proxy or tunnel forwards to, with the server's port,
       changed only when asked (the same network's codes use the same port). -->
  <div class="setup">
    <label class="field">
      <span class="field-label">{t('域名')}</span>
      <input class="field-input" type="text" autocomplete="off" autocapitalize="off" autocorrect="off" spellcheck="false"
        placeholder="meter.example.com" bind:value={domainInput} onkeydown={enter} />
    </label>
    <div class="forward">
      <span class="field-label">{t('转发到')}</span>
      {#if forwardTo}{@render link('target', forwardTo, true, true)}{/if}
      {#if portOpen}
        <input class="field-input num port-field" type="text" inputmode="numeric" autocomplete="off" autocorrect="off" spellcheck="false"
          maxlength="5" aria-label={t('端口')} bind:this={portField} bind:value={portInput} onkeydown={enter} />
      {:else}
        <button class="change pressable" onclick={openPort}>{t('换端口')}</button>
      {/if}
    </div>
    {#if forwardTo}{@render byHand('target', forwardTo)}{/if}
  </div>
  {#if domainFailed}<p class="failed">{t(domainFailed)}</p>{/if}
  {#if movesServer}<p class="note caution">{t('换了端口：同一个网络和 VPN 的二维码改用新端口，手机要重新扫；Cloudflare 链接会关闭')}</p>{/if}
  {#if phone.domainStopped}<p class="note">{t('原来的二维码和链接已失效。点“开始”生成新的二维码，手机再扫一次就能看。')}</p>{/if}
  <p class="note">{t('用你的反代或隧道（Caddy、Nginx、frp、Cloudflare Tunnel、Tailscale Funnel 等）把域名转发到上面的地址，手机在哪都能打开，也没有一小时的限制。')}</p>
  {#if forwardLan}
    <p class="note">{t('反代在 NAS 或路由器上时，转发到这台电脑的 {target}。', { target: forwardLan })}</p>
  {/if}
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
            {#if setup?.domain && /^端口 \d+ 被别的程序占用$/.test(localFailed)}
              <p class="note">{t('自有域名固定用这个端口：退出占用它的程序，或在“同时开启反代”下面换一个端口')}</p>
            {/if}
            <div class="actions"><button class="action primary pressable" onclick={startNear}>{t('重试')}</button></div>
          {:else if localOn || own}
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
            {@render domainCheckLine()}
            {#if phone.near === 'domain' && !own}
              <!-- The same network on, 反代 chosen since: its form, and its own 开始. -->
              {@render domainForm()}
              <div class="actions"><button class="action primary pressable" disabled={!canStart} onclick={startDomain}>{t('开启反代')}</button></div>
            {/if}
            {#if own}
              <p class="note">{t('手机连同一个 Wi-Fi 时扫“局域网”，在别处时扫“反代”：经你的反代转发到这台电脑的 {target}。', { target: forwardTo?.replace(/^http:\/\//, '') ?? '' })}</p>
            {:else if lan}
              <p class="note">{t('手机和电脑连同一个网络（Wi-Fi）后扫码；第一次打开时系统可能问是否允许 Starmeter 接收连接，选允许')}</p>
            {/if}
            {#if nearCodes.length}
              <p class="note">{t('只读：手机上只能看主页和工具页，不能改设置。点“停止”或退出 App 前一直开着。')}</p>
              <p class="note">{t(ownOrigin?.startsWith('https:') ? '网页右上角的太阳可让手机屏幕保持常亮，适合放在桌边一直看；扫“反代”打开时能用' : '网页右上角的太阳可让手机屏幕保持常亮，适合放在桌边一直看；要用 Cloudflare 临时链接或 https 的自有域名打开才能用')}</p>
            {/if}
            <div class="actions"><button class="action danger pressable" onclick={stopNear}>{t('停止')}</button></div>
          {:else if starting || domainStarting}
            <div class="code"><span class="spinner" aria-label={t('正在开启')}></span></div>
          {:else if phone.near === 'domain'}
            <!-- Off, the second mode: the form, then one 开始 for both. -->
            {@render domainForm()}
            <div class="actions"><button class="action primary pressable" disabled={!canStart} onclick={startNear}>{t('开始')}</button></div>
          {:else}
            <!-- Off, as it starts: what its 开始 gives, and the button. -->
            {#if phone.localStopped}
              {@render halted('已停止', '手机上的页面已断开')}
              <p class="note">{t('原来的二维码和链接已失效。点“开始”生成新的二维码，手机再扫一次就能看。')}</p>
            {:else}
              {@render halted('未开始', '在同一个网络里直连这台电脑')}
              <p class="note">{t('点“开始”生成二维码：手机和电脑连同一个网络（Wi-Fi）时扫码就能看。有自己的域名和反代时，打开“同时开启反代”；手机不在身边时，也可以用“任何网络”。')}</p>
            {/if}
            <div class="actions"><button class="action primary pressable" onclick={startNear}>{t('开始')}</button></div>
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
                <p class="note">{t('手机连着哪个 VPN 就扫哪个二维码，在哪都能打开，不经过 Cloudflare。点“停止”或退出 App 前一直开着。')}</p>
              {:else if vpnCodes.length === 1}
                <div class="code"><QrCode text={vpnCodes[0].url} size={188} label={t(vpnCodes[0].label)} /></div>
                {@render link(vpnCodes[0].key, vpnCodes[0].url)}
                <p class="note">{t(vpnNote(vpnCodes[0].key))}</p>
              {:else}
                {@render halted('没有连着 VPN', '这台电脑连上 Tailscale 或 WireGuard 后，这里就有二维码')}
              {/if}
              {#if vpnCodes.length}
                <p class="note">{t('只读：手机上只能看主页和工具页，不能改设置。')}</p>
                <p class="note">{t('网页右上角的太阳可让手机屏幕保持常亮，适合放在桌边一直看；要用 Cloudflare 临时链接或 https 的自有域名打开才能用')}</p>
              {/if}
              <div class="actions"><button class="action danger pressable" onclick={stopVpn}>{t('停止')}</button></div>
            {:else if vpnStarting}
              <div class="code"><span class="spinner" aria-label={t('正在开启')}></span></div>
            {:else}
              {#if phone.vpnStopped}
                {@render halted('已停止', 'VPN 的链接已失效')}
                <p class="note">{t('原来的二维码和链接已失效。点“开始”生成新的二维码，手机再扫一次就能看。')}</p>
              {:else}
                {@render halted('未开始', '经 Tailscale 或 WireGuard 直连这台电脑')}
              {/if}
              <p class="note">{t('点“开始”生成二维码：手机和这台电脑连着同一个 Tailscale 或 WireGuard 网络时，在哪都能打开，数据不经过 Cloudflare。')}</p>
              {#if setup && !setup.tailnet && !setup.wireguard}<p class="note caution">{t('这台电脑现在没有连着 Tailscale 或 WireGuard：先连上再开始。')}</p>{/if}
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
            <p class="note">{t('任何网络都能打开，链接带一次性口令，还能用 {n} 分钟', { n: minutes })}</p>
            <p class="note">{t('只读：手机上只能看主页和工具页，不能改设置。')}</p>
            <p class="note">{t('网页右上角的太阳可让手机屏幕保持常亮，适合放在桌边一直看')}</p>
            <div class="actions"><button class="action danger pressable" onclick={stopPublic}>{t('停止')}</button></div>
          {:else}
            <!-- Off, as it starts: what its 开始 does (the figures go through Cloudflare) said before it is pressed. -->
            {#if phone.publicStopped}
              {@render halted('已停止', '外网链接已关闭')}
              <p class="note">{t('原来的外网链接已失效。点“开始”要一个新链接，手机再扫一次就能看。')}</p>
            {:else}
              {@render halted('未开始', '经 Cloudflare 的临时链接')}
            {/if}
            <p class="note">{t('手机不在同一个网络时用：Cloudflare 给一个临时的 https 地址，转到这台电脑。主页的数字会经过 Cloudflare；链接带一次性口令，停止或一小时后失效。')}</p>
            <!-- Measured from 29 points across mainland China, the link failed at most of them: said before it starts. -->
            <p class="note">{t('中国大陆部分地区打不开 Cloudflare 的链接。手机和电脑能连同一个 Wi-Fi 时，用“同一个网络”更稳。')}</p>
            {#if download}<p class="note">{t('第一次要从 Cloudflare 的 GitHub 发布页下载 cloudflared（约 20 MB），校验后存在 App 的数据文件夹。')}</p>{/if}
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
  .address {
    --press-scale: 0.98; align-self: center; max-width: 100%; padding: 4px 8px; border-radius: 7px; font-size: 11px;
    color: var(--secondary); font-variant-numeric: tabular-nums; white-space: nowrap; overflow: hidden; overflow: clip visible; text-overflow: ellipsis;
  }
  .address:hover { background: var(--hover); }
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
  .card .address { margin: -2px 0; }
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
  .forward .address { margin-left: auto; font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 10.5px; }
  .change {
    --press-scale: 0.95; flex: none; height: 22px; padding: 0 7px; border-radius: 6px; font-size: 10.5px; color: var(--accent);
  }
  .change:hover { background: var(--hover); }
  .port-field { flex: none; width: 62px; height: 24px; text-align: center; }
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
