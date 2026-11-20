<script lang="ts">
  // 在手机上看: a QR code a phone scans to open the homepage in its browser, read-only. On the same network the app
  // serves it itself; 任何网络 opens a public link through Cloudflare (the user's yes first, and a download of
  // cloudflared the first time). The server stays on after this page is left, an hour at most, until 停止. Stopped, the
  // page stays, saying so in the code's place, with 重新开始 for a new code (the old one's key no longer opens).
  import { onDestroy, onMount } from 'svelte';
  import { language, t } from '../lib/i18n.svelte';
  import { shell } from '../lib/backend';
  import { phone, secondsLeft, setPhone } from '../lib/phone.svelte';
  import { moveWindow, panel } from '../lib/ui.svelte';
  import HintButton from './HintButton.svelte';
  import Icon from './Icon.svelte';
  import QrCode from './QrCode.svelte';
  import Scroller from './Scroller.svelte';

  let { onback, onheight }: { onback: () => void; onheight: (height: number) => void } = $props();

  /** 同一网络 or 任何网络: which address the code holds. */
  let reach = $state<'local' | 'public'>('local');
  let failed = $state<string | null>(null);
  let busy = $state(false);
  /** 停止 pressed: the server is off and its code no longer opens, until 重新开始. */
  let stopped = $state(false);
  /** The public link's yes, asked in place before anything leaves the computer. */
  let asking = $state<null | { download: boolean }>(null);
  /** The link copied, or the clipboard refused it (the full link is then shown, selected, to copy by hand). */
  let copied = $state<'ok' | 'failed' | null>(null);
  let manual: HTMLInputElement | undefined = $state();
  let copiedTimer: ReturnType<typeof setTimeout> | undefined;

  const view = $derived(phone.view);
  // The phone opens in this panel's language.
  const address = $derived.by(() => {
    const base = reach === 'public' ? view?.public ?? null : view?.local ?? null;
    return base ? `${base}&l=${language.current}` : null;
  });
  const minutes = $derived(view ? Math.max(1, Math.round(secondsLeft() / 60)) : 0);

  async function start() {
    failed = null;
    try { setPhone(await shell.phoneStart()); } catch (error) { failed = String(error); }
  }
  let tick: ReturnType<typeof setInterval> | undefined;
  onMount(() => {
    void start();
    // Asked after while the page is open: a public link whose cloudflared has ended is taken down, and said so.
    tick = setInterval(async () => {
      if (stopped) return;
      const had = !!phone.view?.public;
      setPhone(await shell.phoneStatus().catch(() => phone.view));
      if (had && !phone.view?.public && reach === 'public') { reach = 'local'; failed = '外网链接断开了，可以重新生成'; }
    }, 10_000);
  });
  onDestroy(() => { clearInterval(tick); clearTimeout(copiedTimer); });

  /** The latest choice. 任何网络 asks the host whether cloudflared is there before its question; an answer that comes
   *  back after a later choice (同一个网络 again, or the link asked for) is dropped, so it can't take the local code
   *  away or open the question under the wrong radio. */
  let choosing = 0;
  async function choose(next: 'local' | 'public') {
    const mine = ++choosing;
    // A new choice starts afresh: the last one's failure is not left under it.
    failed = null;
    reach = next;
    if (next !== 'public' || view?.public) { asking = null; return; }
    const ready = await shell.tunnelReady().catch(() => false);
    if (mine !== choosing) return;
    asking = { download: !ready };
  }
  async function openPublic() {
    choosing++;
    asking = null;
    busy = true;
    failed = null;
    try { setPhone(await shell.phonePublic()); } catch (error) { failed = String(error); reach = 'local'; }
    busy = false;
  }
  async function closePublic() {
    setPhone(await shell.phonePrivate().catch(() => phone.view));
    reach = 'local';
  }
  async function stop() {
    await shell.phoneStop();
    setPhone(null);
    choosing++;
    asking = null;
    failed = null;
    reach = 'local';
    stopped = true;
  }
  async function restart() {
    stopped = false;
    await start();
  }
  async function copy() {
    if (!address) return;
    clearTimeout(copiedTimer);
    // 已复制链接 only once the clipboard has taken it.
    const ok = await (navigator.clipboard?.writeText(address).then(() => true, () => false) ?? Promise.resolve(false));
    copied = ok ? 'ok' : 'failed';
    if (ok) copiedTimer = setTimeout(() => (copied = null), 2000);
  }
  // Refused: the whole link, key and all, selected for a copy by hand.
  $effect(() => { if (copied === 'failed' && manual) { manual.focus(); manual.select(); } });
  // A new link (another network, a public link) starts afresh.
  $effect(() => { void address; copied = null; });

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

<div class="phone-page">
  <div class="grip" class:movable={panel.pinned} aria-hidden="true"
    onpointerdown={(event) => { if (event.button === 0 && panel.pinned) { event.preventDefault(); void moveWindow(); } }}></div>
  <span class="back"><HintButton icon="chevron.left" size={17} hint={t('返回')} label={t('返回')} onclick={onback} /></span>
  <span class="page-glyph"><Icon name="iphone" size={17} /></span>
  <h2 class="title">{t('在手机上看')}</h2>
  <span class="rule"></span>
  <div class="scroll">
    <Scroller onswipeback={onback}>
      <div class="content" bind:this={body}>
        <div class="reach" class:off={stopped} role="radiogroup" aria-label={t('手机在哪')} aria-disabled={stopped}>
          <span class="pill" class:right={reach === 'public'}></span>
          <button class="choice pressable" class:chosen={reach === 'local'} role="radio" aria-checked={reach === 'local'} disabled={stopped} onclick={() => choose('local')}>{t('同一个网络')}</button>
          <button class="choice pressable" class:chosen={reach === 'public'} role="radio" aria-checked={reach === 'public'} disabled={stopped} onclick={() => choose('public')}>{t('任何网络')}</button>
        </div>

        {#if stopped}
          <!-- In the code's place, as large: the page keeps its shape, and what was there says it is over. -->
          <div class="code">
            <div class="stopped" role="status">
              <span class="stopped-glyph"><Icon name="iphone" size={22} /></span>
              <span class="stopped-title">{t('已停止')}</span>
              <span class="stopped-detail">{t('手机上的页面已断开')}</span>
            </div>
          </div>
          <p class="note">{t('原来的二维码和链接已失效。点“重新开始”生成新的二维码，手机再扫一次就能看。')}</p>
          <div class="actions">
            <button class="action primary pressable" onclick={restart}>{t('重新开始')}</button>
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
        {:else}
          <div class="code">
            {#if busy}
              <span class="spinner" aria-label={t('生成链接')}></span>
              <span class="wait">{t('正在向 Cloudflare 要链接…')}</span>
            {:else if address}
              <QrCode text={address} size={188} label={t('手机扫这个二维码')} />
            {:else if view && !view.local && reach === 'local'}
              <span class="wait">{t('这台电脑没有连上网络')}</span>
            {:else if !failed}
              <span class="spinner" aria-label={t('正在开启')}></span>
            {/if}
          </div>
          {#if address}
            <button class="address pressable" onclick={copy} title={address}>{copied === 'ok' ? t('已复制链接') : address.replace(/#.*/, '')}</button>
            {#if copied === 'failed'}
              <p class="manual-note">{t('没能复制，下面是完整链接，选中后手动复制')}</p>
              <input class="manual" bind:this={manual} value={address} readonly aria-label={t('完整链接')} onfocus={(event) => event.currentTarget.select()} />
            {/if}
          {/if}
        {/if}

        {#if failed}<p class="failed">{t(failed)}</p>{/if}

        {#if view && !stopped}
          <!-- What the code is for: on the same network until 停止; the public link, while it is up, for its hour. -->
          {#if !asking && !busy}
            <p class="note">
              {reach === 'public' && view.public ? t('任何网络都能打开，链接带一次性口令，还能用 {n} 分钟', { n: minutes })
                : t('手机和电脑连同一个网络（Wi-Fi）后扫码；第一次打开时系统可能问是否允许 Starmeter 接收连接，选允许')}
            </p>
            <p class="note">{t('只读：手机上只能看主页和工具页，不能改设置。点“停止”或退出 App 前一直开着。')}</p>
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
    box-shadow: 0 0 0 0.5px var(--separator), 0 1px 3px rgba(0, 0, 0, 0.1); transition: transform 0.22s var(--page-curve);
  }
  .pill.right { transform: translateX(100%); }
  .choice { --press-scale: 0.96; position: relative; flex: 1; display: flex; align-items: center; justify-content: center; font-size: 12px; color: var(--secondary); border-radius: 6px; }
  .choice.chosen { color: var(--label); font-weight: 600; }
  .code { display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 8px; min-height: 196px; }
  .stopped {
    width: 188px; height: 188px; box-sizing: border-box; border-radius: 30px; display: flex; flex-direction: column;
    align-items: center; justify-content: center; gap: 4px; border: 1px dashed color-mix(in srgb, var(--label) 22%, transparent);
    color: var(--secondary);
  }
  .stopped-glyph { opacity: 0.7; margin-bottom: 4px; }
  .stopped-title { font-size: 14px; font-weight: 600; color: var(--label); }
  .stopped-detail { font-size: 11px; }
  .reach.off { opacity: 0.45; }
  .wait { font-size: 11.5px; color: var(--secondary); }
  .address {
    --press-scale: 0.98; align-self: center; max-width: 100%; padding: 4px 8px; border-radius: 7px; font-size: 11px;
    color: var(--secondary); font-variant-numeric: tabular-nums; white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
  }
  .address:hover { background: var(--hover); }
  .manual-note { margin: 0; font-size: 10.5px; color: var(--orange); text-align: center; }
  .manual {
    align-self: stretch; height: 26px; padding: 0 8px; border: 0; border-radius: 7px; font: inherit; font-size: 11px;
    color: var(--label); background: color-mix(in srgb, var(--label) 6%, transparent); user-select: text; -webkit-user-select: text;
  }
  .note { margin: 0; font-size: 11px; line-height: 1.5; color: var(--secondary); }
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
