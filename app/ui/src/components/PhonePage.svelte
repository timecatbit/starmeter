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
  import { chooseVia, phone, secondsLeft, setPhone, type Via } from '../lib/phone.svelte';
  import { composing, grabWindow, panel } from '../lib/ui.svelte';
  import HintButton from './HintButton.svelte';
  import Icon from './Icon.svelte';
  import QrCode from './QrCode.svelte';
  import Scroller from './Scroller.svelte';

  let { onback, onheight }: { onback: () => void; onheight: (height: number) => void } = $props();

  /** 同一个网络 being started. */
  let starting = $state(false);
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
  /** 自有域名 as set (its address, the port its proxy forwards to, this computer's network address), and its page's two
   *  fields: what the user typed, as they left it. */
  let setup = $state<DomainSetup | null>(null);
  let domainInput = $state('');
  let portInput = $state('');
  /** 自有域名 being started; its start that failed (the address, or the port taken): the reason, under its fields. */
  let domainStarting = $state(false);
  let domainFailed = $state<string | null>(null);
  /** Whether its address opens this server from here, asked once it is on. */
  let domainCheck = $state<'checking' | DomainCheck | null>(null);

  const view = $derived(phone.view);
  const reach = $derived(phone.reach);
  // The phone opens in this panel's language, and its look (手绘).
  const looked = (base: string | null | undefined) => (base ? `${base}&l=${language.current}${style.current === 'sketch' ? '&s=sketch' : ''}` : null);
  const localOn = $derived(!localFailed && !!(view?.localOpen ?? view?.local));
  const lan = $derived(localOn ? looked(view?.local) : null);
  const tailnet = $derived(localOn ? looked(view?.tailnet) : null);
  const wireguard = $derived(localOn ? looked(view?.wireguard) : null);
  /** 同一个网络's codes: the network's, then Tailscale's and WireGuard's where this computer is on them (用户：如果有
   *  wireguard也和tailscale一样处理哈). */
  const codes = $derived([
    lan && { key: 'local', url: lan, title: '局域网', label: '手机扫这个二维码', detail: '手机连同一个 Wi-Fi' },
    tailnet && { key: 'tailnet', url: tailnet, title: 'Tailscale', label: '连着 Tailscale 的手机扫这个二维码', detail: '手机开着 Tailscale' },
    wireguard && { key: 'wireguard', url: wireguard, title: 'WireGuard', label: '连着 WireGuard 的手机扫这个二维码', detail: '手机开着 WireGuard' },
  ].filter((code) => !!code));
  /** What the codes side by side say under them: the system's question the first time, and which open from anywhere. */
  const codesNote = $derived(lan
    ? tailnet && wireguard ? '第一次打开时系统可能问是否允许 Starmeter 接收连接，选允许；Tailscale 和 WireGuard 的二维码在手机连着对应的网络时打开，不经过 Cloudflare'
      : wireguard ? '第一次打开时系统可能问是否允许 Starmeter 接收连接，选允许；WireGuard 的二维码在手机连着同一个 WireGuard 网络时打开，不经过 Cloudflare'
        : '第一次打开时系统可能问是否允许 Starmeter 接收连接，选允许；Tailscale 的二维码在哪都能打开，不经过 Cloudflare'
    : '手机连着 Tailscale 或 WireGuard 时扫对应的二维码，不经过 Cloudflare');
  /** And what one code alone says. */
  const codeNote = (key: string) => key === 'local' ? '手机和电脑连同一个网络（Wi-Fi）后扫码；第一次打开时系统可能问是否允许 Starmeter 接收连接，选允许'
    : key === 'tailnet' ? '手机连上同一个 Tailscale 网络时扫这个，在哪都能打开，不经过 Cloudflare'
      : '手机连上同一个 WireGuard 网络时扫这个，不经过 Cloudflare';
  const outside = $derived(looked(view?.public));
  /** 自有域名's link while it is on, its address, and where its proxy forwards to (this computer's port). */
  const own = $derived(looked(view?.domain));
  const ownOrigin = $derived(view?.domain ? view.domain.replace(/\/?#.*$/, '') : null);
  const ownHost = $derived(ownOrigin?.replace(/^https?:\/\//, '') ?? '');
  const portTyped = $derived(/^\d+$/.test(portInput.trim()) ? Number(portInput.trim()) : null);
  const forwardPort = $derived(own ? view?.port : portTyped ?? setup?.port);
  const forwardTo = $derived(forwardPort ? `http://127.0.0.1:${forwardPort}` : null);
  /** What the check found, said with what to look at. */
  const checkText = $derived.by(() => {
    if (!domainCheck || domainCheck === 'checking') return '';
    const target = forwardTo?.replace(/^http:\/\//, '') ?? '';
    if (domainCheck.opens) return t('已确认能通过 {host} 打开', { host: ownHost });
    if (domainCheck.status && domainCheck.status >= 300 && domainCheck.status < 400 && ownOrigin?.startsWith('http:')) {
      return t('{host} 把请求转到了别处（HTTP {n}）：检查反代的设置；总是跳到 https 时，把域名改成 https://{host}', { host: ownHost, n: domainCheck.status });
    }
    if (domainCheck.status) return t('{host} 有回应（HTTP {n}），但不是 Starmeter：检查反代是否转发到 {target}', { host: ownHost, n: domainCheck.status, target });
    return t('这台电脑没能打开 {host}（{reason}）：手机也打不开时，检查域名解析和反代是否转发到 {target}', { host: ownHost, reason: domainCheck.reason ?? '', target });
  });
  const opening = $derived(busy || !!view?.publicOpening);
  const minutes = $derived(view ? Math.max(1, Math.round(secondsLeft() / 60)) : 0);
  const choices = ['local', 'public'] as const;
  const vias: Via[] = ['cloudflare', 'domain'];
  /** Which of 任何网络's two links are on. */
  const viaOn = $derived({ cloudflare: !!outside || opening, domain: !!own });
  /** 自有域名 started on another port than the server's: the same network's codes and Cloudflare's link go with it. */
  const movesServer = $derived(!own && !!view && !!view.port && portTyped !== null && portTyped !== view.port
    && (localOn || !!outside || opening));
  /** Which ways are on, a green dot after each one's name. */
  const on = $derived({ local: localOn, public: !!outside || opening || !!own });

  /** Each way's steps belong to its own round, so what one answers late can't undo a later step of the same way: a
   *  start after a 停止, a link closed while it was made. Every step also moves `epoch`, so the status asked every ten
   *  seconds, if a step came between its asking and its answer, is dropped instead of putting back what has gone. */
  let localRound = 0, publicRound = 0, domainRound = 0, epoch = 0;

  /** What is left on once a way goes, shown at once: nothing on, nothing at all (the dot on the homepage's button goes
   *  too). The host's own answer follows. */
  function without(gone: Partial<PhoneView>) {
    const left = view ? { ...view, ...gone } : null;
    setPhone(left && (left.localOpen || left.public || left.publicOpening || left.domain) ? left : null);
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
    without({ localOpen: false, local: null, tailnet: null });
    const left = await shell.phoneStop().catch(() => phone.view);
    if (mine === localRound) setPhone(left ?? null);
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
    if (next.domain && !phone.viaChosen) phone.via = 'domain';
  }
  onMount(() => {
    // Nothing starts by opening the page: each way as it was left, off until its 开始.
    void pollStatus();
    void shell.phoneDomain().then((next) => { if (next) settled(next); }).catch(() => {});
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
  onDestroy(() => { localRound++; publicRound++; epoch++; clearInterval(tick); clearInterval(fast); clearTimeout(copiedTimer); });

  /** Whether cloudflared is there, for 任何网络's 开始 to say 下载并开始 when it isn't; an answer that comes back after a
   *  later look is dropped. */
  let checking = 0;
  async function checkTunnel() {
    const mine = ++checking;
    const ready = await shell.tunnelReady().catch(() => false);
    if (mine === checking) download = !ready;
  }
  /** 自有域名's 开始: its address and port kept (the port held from now on), and its link on. */
  async function startDomain() {
    const mine = ++domainRound;
    epoch++;
    domainFailed = null;
    if (portTyped === null || portTyped < 1024 || portTyped > 65535) { domainFailed = '端口填 1024 到 65535 之间的数'; return; }
    domainStarting = true;
    try {
      const started = await shell.phoneDomainStart(domainInput, portTyped);
      if (mine !== domainRound) return;
      phone.domainStopped = false;
      setPhone(started);
      const next = await shell.phoneDomain().catch(() => null);
      if (next && mine === domainRound) settled(next);
    } catch (error) {
      if (mine === domainRound) domainFailed = String(error);
    } finally {
      if (mine === domainRound) domainStarting = false;
    }
  }
  /** 自有域名's 停止: its links stop opening; its address and port stay in its fields for the next 开始. */
  async function stopDomain() {
    const mine = ++domainRound;
    epoch++;
    domainStarting = false;
    domainFailed = null;
    phone.domainStopped = true;
    without({ domain: null });
    const left = await shell.phoneDomainStop().catch(() => phone.view);
    if (mine === domainRound) setPhone(left ?? null);
  }
  /** Whether its address opens this server from here: asked once each link of it is on, and on 再检查. */
  let checkRound = 0;
  async function checkDomain() {
    const mine = ++checkRound;
    domainCheck = 'checking';
    const found = await shell.phoneDomainCheck().catch(() => null);
    if (mine === checkRound) domainCheck = found;
  }
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

{#snippet link(key: string, url: string, short = false)}
  <!-- Side by side, the address alone (192.168.1.20:51234): the key stays in the code and the copied link. -->
  <button class="address pressable" onclick={() => copy(key, url)} title={url}>{copied?.key === key && copied.state === 'ok'
    ? t('已复制链接') : short ? url.replace(/^https?:\/\//, '').replace(/\/?#.*/, '') : url.replace(/#.*/, '')}</button>
  {#if copied?.key === key && copied.state === 'failed'}
    <p class="manual-note">{t('没能复制，下面是完整链接，选中后手动复制')}</p>
    <input class="manual" bind:this={manual} value={url} readonly aria-label={t('完整链接')} onfocus={(event) => event.currentTarget.select()} />
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
        <div class="reach" role="radiogroup" aria-label={t('手机在哪')}>
          <span class="pill" style:transform="translateX({reach === 'public' ? 100 : 0}%)"></span>
          {#each choices as choice (choice)}
            <button class="choice pressable" class:chosen={reach === choice} role="radio" aria-checked={reach === choice}
              onclick={() => choose(choice)}>{t(choice === 'local' ? '同一个网络' : '任何网络')}{#if on[choice]}<span class="on" aria-hidden="true"></span>{/if}</button>
          {/each}
        </div>

        {#if reach === 'local'}
          {#if localFailed}
            {@render halted('没能开启', '原因在下面，可以再试一次')}
            <p class="failed">{t(localFailed)}</p>
            {#if setup?.domain && /^端口 \d+ 被别的程序占用$/.test(localFailed)}
              <p class="note">{t('自有域名固定用这个端口：退出占用它的程序，或在“任何网络 › 自有域名”换一个端口')}</p>
            {/if}
            <div class="actions"><button class="action primary pressable" onclick={startLocal}>{t('重试')}</button></div>
          {:else if localOn}
            {#if codes.length > 1}
              <!-- This computer's network address and its Tailscale and WireGuard ones, side by side: the same server, the
                   same key; a third under the two, in the middle. -->
              <div class="codes">
                {#each codes as code (code.key)}
                  <div class="card">
                    <span class="card-title">{t(code.title)}</span>
                    <QrCode text={code.url} size={132} label={t(code.label)} />
                    {@render link(code.key, code.url, true)}
                    <span class="card-detail">{t(code.detail)}</span>
                  </div>
                {/each}
              </div>
              <p class="note">{t(codesNote)}</p>
            {:else if codes.length === 1}
              <div class="code"><QrCode text={codes[0].url} size={188} label={t(codes[0].label)} /></div>
              {@render link(codes[0].key, codes[0].url)}
              <p class="note">{t(codeNote(codes[0].key))}</p>
            {:else}
              <div class="code"><span class="wait">{t('这台电脑没有连上网络')}</span></div>
            {/if}
            {#if codes.length}
              <p class="note">{t('只读：手机上只能看主页和工具页，不能改设置。点“停止”或退出 App 前一直开着。')}</p>
              <p class="note">{t('网页右上角的太阳可让手机屏幕保持常亮，适合放在桌边一直看；要用“任何网络”的 https 链接打开才能用')}</p>
            {/if}
            <div class="actions"><button class="action danger pressable" onclick={stopLocal}>{t('停止')}</button></div>
          {:else if starting}
            <div class="code"><span class="spinner" aria-label={t('正在开启')}></span></div>
          {:else}
            <!-- Off, as it starts: what its 开始 gives, and the button. -->
            {#if phone.localStopped}
              {@render halted('已停止', '手机上的页面已断开')}
              <p class="note">{t('原来的二维码和链接已失效。点“开始”生成新的二维码，手机再扫一次就能看。')}</p>
            {:else}
              {@render halted('未开始', '在同一个网络里直连这台电脑')}
              <p class="note">{t('点“开始”生成二维码：手机和电脑连同一个网络（Wi-Fi）时扫码就能看；这台电脑连着 Tailscale 或 WireGuard 时，还有它们的二维码。')}</p>
            {/if}
            <div class="actions"><button class="action primary pressable" onclick={startLocal}>{t('开始')}</button></div>
          {/if}
        {:else}
          <!-- 任何网络's two links, each a page of its own and each on and off by itself, so both can be on at once:
               Cloudflare's for an hour, and the user's own domain through their own proxy. Looking starts nothing; a
               green dot after each that is on. -->
          <div class="via" role="radiogroup" aria-label={t('外网链接从哪来')}>
            <span class="via-pill" style:transform="translateX({phone.via === 'domain' ? 100 : 0}%)"></span>
            {#each vias as choice (choice)}
              <button class="via-choice pressable" class:chosen={phone.via === choice} role="radio" aria-checked={phone.via === choice}
                onclick={() => chooseVia(choice)}>{t(choice === 'cloudflare' ? 'Cloudflare 临时链接' : '自有域名')}{#if viaOn[choice]}<span class="on" aria-hidden="true"></span>{/if}</button>
            {/each}
          </div>
          {#if phone.via === 'domain'}
            {#if own}
              <!-- 自有域名 on: its code, whether its address opens this server from here, and where its proxy forwards. -->
              <div class="code"><QrCode text={own} size={188} label={t('手机扫这个二维码')} /></div>
              {@render link('domain', own)}
              {#if domainCheck === 'checking'}
                <p class="check"><span class="spinner small" aria-hidden="true"></span>{t('正在确认手机能打开…')}</p>
              {:else if domainCheck?.opens}
                <p class="check ok">{checkText}</p>
              {:else if domainCheck}
                <div class="check caution" role="status"><span>{checkText}</span><button class="again pressable" onclick={checkDomain}>{t('再检查')}</button></div>
              {/if}
              <p class="note">{t('经你的反代转发到这台电脑的 {target}，不经过 Cloudflare。点“停止”或退出 App 前一直开着。', { target: forwardTo?.replace(/^http:\/\//, '') ?? '' })}</p>
              <p class="note">{t('只读：手机上只能看主页和工具页，不能改设置。')}</p>
              <p class="note">{t(ownOrigin?.startsWith('https:') ? '网页右上角的太阳可让手机屏幕保持常亮，适合放在桌边一直看' : '网页右上角的太阳可让手机屏幕保持常亮，适合放在桌边一直看；域名用 https 时才能用')}</p>
              <div class="actions"><button class="action danger pressable" onclick={stopDomain}>{t('停止')}</button></div>
            {:else}
              <!-- The address the user's proxy opens, the port it forwards to (kept, and held, from its 开始), and that
                   address to copy into the proxy. -->
              <div class="setup">
                <label class="field host">
                  <span class="field-label">{t('域名')}</span>
                  <input class="field-input" type="text" autocomplete="off" autocapitalize="off" autocorrect="off" spellcheck="false" placeholder="meter.example.com"
                    bind:value={domainInput} onkeydown={(event) => { if (event.key === 'Enter' && !composing(event)) void startDomain(); }} />
                </label>
                <label class="field port">
                  <span class="field-label">{t('端口')}</span>
                  <input class="field-input num" type="text" inputmode="numeric" autocomplete="off" autocorrect="off" spellcheck="false" maxlength="5"
                    bind:value={portInput} onkeydown={(event) => { if (event.key === 'Enter' && !composing(event)) void startDomain(); }} />
                </label>
                {#if forwardTo}
                  <div class="forward"><span class="field-label">{t('反代转发到')}</span>{@render link('target', forwardTo, true)}</div>
                {/if}
              </div>
              {#if domainFailed}<p class="failed">{t(domainFailed)}</p>{/if}
              {#if movesServer}<p class="note caution">{t('换了端口：同一个网络的二维码改用新端口，手机要重新扫；Cloudflare 链接会关闭')}</p>{/if}
              {#if phone.domainStopped}<p class="note">{t('原来的二维码和链接已失效。点“开始”生成新的二维码，手机再扫一次就能看。')}</p>{/if}
              <p class="note">{t('用你自己的反代或隧道（Caddy、Nginx、frp、Cloudflare Tunnel、Tailscale Funnel 等）把这个域名转发到上面的地址：手机在哪都能打开，不经过 Starmeter 的 Cloudflare 临时链接，也没有一小时的限制。')}</p>
              {#if setup?.lan && forwardPort}
                <p class="note">{t('反代在另一台设备上（NAS、路由器）时，转发到这台电脑的 {target}。', { target: `${setup.lan}:${forwardPort}` })}</p>
              {/if}
              <div class="actions"><button class="action primary pressable" disabled={domainStarting || !domainInput.trim()} onclick={startDomain}>{t('开始')}</button></div>
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
            <p class="note">{t('网页右上角的太阳可让手机屏幕保持常亮，适合放在桌边一直看；要用“任何网络”的 https 链接打开才能用')}</p>
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
  /* 自有域名's fields, in the code's place: its address and port side by side, the address to forward to under them. */
  .setup {
    display: grid; grid-template-columns: minmax(0, 1fr) 72px; gap: 8px 8px; padding: 10px 10px 8px; border-radius: 12px;
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
  .forward { grid-column: 1 / -1; display: flex; align-items: center; gap: 4px; min-width: 0; }
  .forward .field-label { flex: none; }
  .forward .address { margin-left: auto; font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 10.5px; }
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
