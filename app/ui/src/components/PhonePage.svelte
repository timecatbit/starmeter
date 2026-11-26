<script lang="ts">
  // 在手机上看: a QR code a phone scans to open the homepage in its browser, read-only. On the same network the app
  // serves it itself, and its Tailscale address beside (the same server, on the same page); 任何网络 opens a public
  // link through Cloudflare (the user's yes first, and a download of cloudflared the first time). The server stays on
  // after this page is left, the public link an hour at most, until 停止. The two choices only show their page: 停止
  // keeps it where it is, saying so in the code's place, and only 重新开始 starts it again, also on opening the page
  // after (用户：停了要手动打开；tailscale 点击停止之后跳到本地网络页面，为什么).
  import { onDestroy, onMount } from 'svelte';
  import { language, t } from '../lib/i18n.svelte';
  import { style } from '../lib/style.svelte';
  import { shell } from '../lib/backend';
  import { keepStopped, phone, secondsLeft, setPhone } from '../lib/phone.svelte';
  import { grabWindow, panel } from '../lib/ui.svelte';
  import HintButton from './HintButton.svelte';
  import Icon from './Icon.svelte';
  import QrCode from './QrCode.svelte';
  import Scroller from './Scroller.svelte';

  let { onback, onheight }: { onback: () => void; onheight: (height: number) => void } = $props();

  /** 同一个网络 (its Tailscale address too, when this computer is on a tailnet) or 任何网络: which page is shown. */
  let reach = $state<'local' | 'public'>('local');
  let failed = $state<string | null>(null);
  /** 任何网络 chosen and its link not there: it couldn't be made, or it dropped. The choice stays, and the code's place
   *  says so, with 重试 (or 重新生成) and 改用同一个网络: the page doesn't go back to the same network on its own
   *  (用户：在所有网络失败，应该停留在所有网络的页面，而不是返回到同一网络显示错误). */
  let lost = $state<null | 'failed' | 'dropped'>(null);
  let busy = $state(false);
  /** 停止 pressed: the server is off and its code no longer opens, until 重新开始. */
  let stopped = $state(false);
  /** The public link's yes, asked in place before anything leaves the computer. */
  let asking = $state<null | { download: boolean }>(null);
  /** Which link was copied, or the clipboard refused (the full link is then shown, selected, to copy by hand). */
  let copied = $state<{ key: string; state: 'ok' | 'failed' } | null>(null);
  let manual: HTMLInputElement | undefined = $state();
  let copiedTimer: ReturnType<typeof setTimeout> | undefined;

  const view = $derived(phone.view);
  // The phone opens in this panel's language, and its look (手绘).
  const looked = (base: string | null | undefined) => (base ? `${base}&l=${language.current}${style.current === 'sketch' ? '&s=sketch' : ''}` : null);
  const lan = $derived(looked(view?.local));
  const tailnet = $derived(looked(view?.tailnet));
  const outside = $derived(looked(view?.public));
  const minutes = $derived(view ? Math.max(1, Math.round(secondsLeft() / 60)) : 0);
  const choices = ['local', 'public'] as const;

  /** Each start (opening the page, 重新开始) begins a round, and 停止 or leaving the page ends it. What a step of an
   *  earlier round answers late (the public link asked for before 停止, the status asked every ten seconds) is
   *  dropped: it can't keep the new code waiting, set its error beside it, or put back the address that has gone. */
  let round = 0;
  /** Starts the server; false, with the reason in `failed`, when it couldn't. */
  async function start(): Promise<boolean> {
    const mine = ++round;
    failed = null;
    busy = false;
    try {
      const started = await shell.phoneStart();
      if (mine === round) setPhone(started);
      return true;
    } catch (error) {
      if (mine === round) failed = String(error);
      return false;
    }
  }
  /** 重新开始 (or 重试): started by hand, it is no longer kept stopped. A start that fails (its port taken for a moment)
   *  stays in the code's place with its reason and 重试, instead of leaving a page with neither a code nor a way on.
   *  Started again on 任何网络, its link is asked for as when that page is chosen, the yes first. */
  async function begin() {
    stopped = false;
    keepStopped(false);
    if (!(await start())) { stopped = true; return; }
    if (reach === 'public' && !phone.view?.public) await choose('public');
  }
  let tick: ReturnType<typeof setInterval> | undefined;
  onMount(() => {
    // Opened again after 停止: it stays off, as it was left, until 重新开始; on, or never stopped, the page starts it
    // (or finds it running).
    if (phone.stopped && !phone.view) stopped = true;
    else void begin();
    // Asked after while the page is open: a public link whose cloudflared has ended is taken down, and said so.
    tick = setInterval(async () => {
      if (stopped) return;
      const mine = round;
      const had = !!phone.view?.public;
      const status = await shell.phoneStatus().catch(() => phone.view);
      if (mine !== round) return;
      setPhone(status);
      if (had && !phone.view?.public && reach === 'public') { failed = null; lost = 'dropped'; }
    }, 10_000);
  });
  onDestroy(() => { round++; clearInterval(tick); clearTimeout(copiedTimer); });

  /** The latest choice. 任何网络 asks the host whether cloudflared is there before its question; an answer that comes
   *  back after a later choice (同一个网络 again, or the link asked for) is dropped, so it can't take the local code
   *  away or open the question under the wrong radio. */
  let choosing = 0;
  async function choose(next: 'local' | 'public') {
    const mine = ++choosing;
    // A new choice starts afresh: the last one's failure is not left under it.
    if (!stopped) failed = null;
    lost = null;
    reach = next;
    // Stopped, a choice only shows that page, still stopped: nothing starts by looking. A link already on its way
    // (任何网络 chosen again while it is made) is waited for, not asked for again (TUNNEL-1).
    if (stopped || next !== 'public' || view?.public || busy) { asking = null; return; }
    const ready = await shell.tunnelReady().catch(() => false);
    if (mine !== choosing) return;
    asking = { download: !ready };
  }
  /** Cloudflare not answering this once is often over by the next try: asked again once, quietly, before saying so. */
  const passing = (error: unknown) => String(error).includes('这次没有连上');
  async function openPublic() {
    choosing++;
    const mine = round;
    asking = null;
    busy = true;
    failed = null;
    lost = null;
    for (let attempt = 0; ; attempt++) {
      try {
        const opened = await shell.phonePublic();
        if (mine !== round) return;
        setPhone(opened);
        break;
      } catch (error) {
        if (mine !== round) return;
        if (attempt === 0 && passing(error) && reach === 'public') continue;
        // Said on 任何网络 only: chosen away while the link was made (同一个网络, Tailscale), that code stays as it is,
        // with nothing of Cloudflare under it (PHONE-12).
        if (reach === 'public') {
          failed = String(error);
          lost = 'failed';
        }
        break;
      }
    }
    busy = false;
  }
  /** The public link closed, the page stays 任何网络's, asking again whether to make one. */
  async function closePublic() {
    const mine = round;
    const closed = await shell.phonePrivate().catch(() => phone.view);
    if (mine !== round) return;
    setPhone(closed);
    lost = null;
    if (reach === 'public') await choose('public');
  }
  async function stop() {
    round++;
    busy = false;
    keepStopped(true);
    await shell.phoneStop();
    setPhone(null);
    choosing++;
    asking = null;
    failed = null;
    lost = null;
    // On the page it was stopped from (用户：点击停止之后跳到本地网络页面，为什么).
    stopped = true;
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
  $effect(() => { void lan; void tailnet; void outside; copied = null; });

  let body: HTMLDivElement | undefined = $state();
  $effect(() => {
    const box = body;
    if (!box) return;
    let frame = 0;
    const observer = new ResizeObserver(() => { cancelAnimationFrame(frame); frame = requestAnimationFrame(() => onheight(58 + box.offsetHeight + 8)); });
    observer.observe(box);
    return () => { observer.disconnect(); cancelAnimationFrame(frame); };
  });
</script>

{#snippet link(key: string, url: string)}
  <button class="address pressable" onclick={() => copy(key, url)} title={url}>{copied?.key === key && copied.state === 'ok' ? t('已复制链接') : url.replace(/#.*/, '')}</button>
  {#if copied?.key === key && copied.state === 'failed'}
    <p class="manual-note">{t('没能复制，下面是完整链接，选中后手动复制')}</p>
    <input class="manual" bind:this={manual} value={url} readonly aria-label={t('完整链接')} onfocus={(event) => event.currentTarget.select()} />
  {/if}
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
        <!-- The two choices only show their page: nothing starts or stops by looking. -->
        <div class="reach" role="radiogroup" aria-label={t('手机在哪')}>
          <span class="pill" style:transform="translateX({reach === 'public' ? 100 : 0}%)"></span>
          {#each choices as choice (choice)}
            <button class="choice pressable" class:chosen={reach === choice} role="radio" aria-checked={reach === choice}
              onclick={() => choose(choice)}>{t(choice === 'local' ? '同一个网络' : '任何网络')}</button>
          {/each}
        </div>

        {#if stopped}
          <!-- In the code's place, as large, on the page it was stopped from: what was there says it is over. -->
          <div class="code">
            <div class="stopped" role="status">
              <span class="stopped-glyph"><Icon name="iphone" size={22} /></span>
              <span class="stopped-title">{t(failed ? '没能开启' : '已停止')}</span>
              <span class="stopped-detail">{t(failed ? '原因在下面，可以再试一次' : '手机上的页面已断开')}</span>
            </div>
          </div>
          {#if failed}<p class="failed">{t(failed)}</p>
          {:else}<p class="note">{t('原来的二维码和链接已失效。点“重新开始”生成新的二维码，手机再扫一次就能看。')}</p>{/if}
          <div class="actions">
            <button class="action primary pressable" onclick={begin}>{t(failed ? '重试' : '重新开始')}</button>
          </div>
        {:else if asking}
          <div class="ask">
            <p class="ask-title">{t('经 Cloudflare 生成外网链接？')}</p>
            <p class="ask-detail">{t('手机不在同一个网络时用：Cloudflare 给一个临时的 https 地址，转到这台电脑。主页的数字会经过 Cloudflare；链接带一次性口令，停止或一小时后失效。')}</p>
            <!-- Measured from 29 points across mainland China, the link failed at most of them: said before the yes. -->
            <p class="ask-detail">{t('中国大陆部分地区打不开 Cloudflare 的链接。手机和电脑能连同一个 Wi-Fi 时，用“同一个网络”更稳。')}</p>
            {#if asking.download}<p class="ask-detail">{t('第一次要从 Cloudflare 的 GitHub 发布页下载 cloudflared（约 20 MB），校验后存在 App 的数据文件夹。')}</p>{/if}
            <div class="ask-actions">
              <button class="button pressable" onclick={() => choose('local')}>{t('取消')}</button>
              <button class="button primary pressable" onclick={openPublic}>{t(asking.download ? '下载并生成' : '生成链接')}</button>
            </div>
          </div>
        {:else if reach === 'public'}
          <div class="code">
            {#if busy}
              <span class="spinner" aria-label={t('生成链接')}></span>
              <span class="wait">{t('正在向 Cloudflare 要链接…')}</span>
              <!-- PHONE-11: shown once it opens from outside, so a phone scanning it never meets a name not there yet. -->
              <span class="wait-detail">{t('确认手机能打开后再显示，通常十几秒')}</span>
            {:else if lost}
              <!-- In the code's place, on 任何网络 still: what happened, and the two ways on. -->
              <div class="stopped" role="status">
                <span class="stopped-glyph"><Icon name="iphone" size={22} /></span>
                <span class="stopped-title">{t(lost === 'dropped' ? '外网链接断开了' : '没能生成外网链接')}</span>
                <span class="stopped-detail">{t(lost === 'dropped' ? '可以重新生成一个新链接' : '原因在下面，可以再试一次')}</span>
              </div>
              <div class="ask-actions">
                <button class="button pressable" onclick={() => choose('local')}>{t('改用同一个网络')}</button>
                <button class="button primary pressable" onclick={openPublic}>{t(lost === 'dropped' ? '重新生成' : '重试')}</button>
              </div>
            {:else if outside}
              <QrCode text={outside} size={188} label={t('手机扫这个二维码')} />
            {:else if !failed}
              <span class="spinner" aria-label={t('正在开启')}></span>
            {/if}
          </div>
          {#if outside && !busy && !lost}{@render link('public', outside)}{/if}
        {:else}
          <!-- 同一个网络: this computer's address on its network, and its Tailscale address under it when it is on a
               tailnet: the same server, both on until 停止. -->
          <div class="code">
            {#if lan}
              <QrCode text={lan} size={188} label={t('手机扫这个二维码')} />
            {:else if view}
              <span class="wait">{t('这台电脑没有连上网络')}</span>
            {:else if !failed}
              <span class="spinner" aria-label={t('正在开启')}></span>
            {/if}
          </div>
          {#if lan}{@render link('local', lan)}{/if}
          {#if view && lan}
            <p class="note">{t('手机和电脑连同一个网络（Wi-Fi）后扫码；第一次打开时系统可能问是否允许 Starmeter 接收连接，选允许')}</p>
          {/if}
          {#if tailnet}
            <div class="tailnet">
              <span class="tailnet-title">Tailscale</span>
              <QrCode text={tailnet} size={150} label={t('连着 Tailscale 的手机扫这个二维码')} />
              {@render link('tailnet', tailnet)}
              <p class="note">{t('手机连上同一个 Tailscale 网络时扫这个，在哪都能打开，不经过 Cloudflare')}</p>
            </div>
          {/if}
        {/if}

        {#if failed && !stopped}<p class="failed">{t(failed)}</p>{/if}

        {#if view && !stopped}
          {#if !asking && !(busy && reach === 'public')}
            {#if reach === 'public' && outside && view.publicChecked === false}
              <!-- The wait for it to open ran out: shown all the same, with what to do if it doesn't open yet. -->
              <p class="note caution">{t('还没确认手机能打开：扫码打不开就等一分钟再试')}</p>
            {/if}
            {#if reach === 'public' && outside}
              <p class="note">{t('任何网络都能打开，链接带一次性口令，还能用 {n} 分钟', { n: minutes })}</p>
            {/if}
            <!-- With a code on screen only: not under a link that failed. -->
            {#if (reach === 'local' && (lan || tailnet)) || (reach === 'public' && outside && !lost)}
              <p class="note">{t('只读：手机上只能看主页和工具页，不能改设置。点“停止”或退出 App 前一直开着。')}</p>
              <p class="note">{t('网页右上角的太阳可让手机屏幕保持常亮，适合放在桌边一直看；要用“任何网络”的 https 链接打开才能用')}</p>
            {/if}
          {/if}
          <div class="actions">
            {#if view.public}<button class="action pressable" onclick={closePublic}>{t('关闭外网链接')}</button>{/if}
            <button class="action danger pressable" onclick={stop}>{t('停止')}</button>
          </div>
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
  .content { padding: 0 16px 12px; display: flex; flex-direction: column; align-items: stretch; gap: 12px; }
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
  .tailnet { display: flex; flex-direction: column; align-items: center; gap: 8px; padding-top: 12px; border-top: 1px solid var(--separator); }
  .tailnet-title { align-self: flex-start; font-size: 12px; font-weight: 600; color: var(--label); }
  .tailnet .note { align-self: stretch; }
  .note.caution { color: var(--orange); }
  .failed { margin: 0; padding: 8px 10px; border-radius: 8px; font-size: 11.5px; color: var(--label); background: color-mix(in srgb, var(--orange) 18%, transparent); }
  .ask { display: flex; flex-direction: column; gap: 6px; padding: 10px 12px; border-radius: 10px; background: color-mix(in srgb, var(--label) 5%, transparent); }
  .ask-title { margin: 0; font-size: 12.5px; font-weight: 600; }
  .ask-detail { margin: 0; font-size: 11.5px; line-height: 1.5; color: var(--secondary); }
  .ask-actions { display: flex; justify-content: flex-end; gap: 6px; margin-top: 4px; }
  .button { --press-scale: 0.96; height: 26px; padding: 0 12px; border-radius: 7px; font-size: 12px; font-weight: 600; background: color-mix(in srgb, var(--label) 8%, transparent); }
  .button.primary { color: var(--on-accent); background: var(--accent); }
  .actions { display: flex; gap: 8px; justify-content: flex-end; }
  .action { --press-scale: 0.97; height: 28px; padding: 0 14px; border-radius: 8px; font-size: 12px; font-weight: 600; background: color-mix(in srgb, var(--label) 8%, transparent); }
  .action.danger { color: var(--red, #e5484d); }
  .action.primary { color: var(--on-accent); background: var(--accent); }
  .spinner { width: 16px; height: 16px; border-radius: 50%; border: 2px solid var(--separator); border-top-color: var(--secondary); animation: spin 0.8s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
</style>
