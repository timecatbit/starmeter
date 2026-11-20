<script lang="ts">
  // 分享用量图: the picture small on the left, enough to see what it will look like, and beside it the style, light or
  // dark, and what to do with it: copy, save to the computer, or (macOS) the share menu. The page is as tall as the
  // picture needs. The figures are those on screen when the page opened, so live readings don't redraw it under the
  // reader.
  import { onDestroy, untrack } from 'svelte';
  import { t } from '../lib/i18n.svelte';
  import { inTauri, onPhone, shell } from '../lib/backend';
  import * as web from '../lib/webShare';
  import { renderPicture, shareStyles, type Picture, type ShareStyle } from '../lib/share';
  import { SCALE } from '../lib/shareCommon';
  import { SHARE_TOOLS, SHARE_TOOLS_MOST } from '../lib/sharePoster';
  import { moveWindow, panel } from '../lib/ui.svelte';
  import type { Dashboard, Entry, Project } from '../lib/types';
  import Dropdown from './Dropdown.svelte';
  import HintButton from './HintButton.svelte';
  import Icon from './Icon.svelte';
  import Scroller from './Scroller.svelte';

  let { dashboard, project, rangeOptions = [], onrange, onback, onheight }: {
    dashboard: Dashboard; project: Project; rangeOptions?: Entry[]; onrange?: (action: string[]) => void;
    onback: () => void; onheight: (height: number) => void;
  } = $props();

  const STYLE = 'starmeter.shareStyle', MODE = 'starmeter.shareMode', TOOLS = 'starmeter.shareTools';
  const stored = (key: string) => (typeof localStorage === 'undefined' ? null : localStorage.getItem(key));
  const remember = (key: string, value: string) => { if (typeof localStorage !== 'undefined') localStorage.setItem(key, value); };
  let style = $state<ShareStyle>(shareStyles.some((s) => s.key === stored(STYLE)) ? (stored(STYLE) as ShareStyle) : 'card');
  /** 浅色 or 深色 for the picture; until chosen, the panel's own. */
  let mode = $state<'light' | 'dark' | null>(stored(MODE) === 'light' || stored(MODE) === 'dark' ? (stored(MODE) as 'light' | 'dark') : null);
  const dark = $derived(mode ? mode === 'dark' : panel.dark);
  /** How many tools have rows of their own in the picture (the ones that used the most); the rest fold into 其他工具. */
  const counts = Array.from({ length: SHARE_TOOLS_MOST }, (_, i) => i + 1);
  const storedTools = Number(stored(TOOLS));
  let tools = $state(counts.includes(storedTools) ? storedTools : SHARE_TOOLS);
  function pickTools(next: number) {
    tools = next;
    remember(TOOLS, String(next));
  }
  // The figures as they stood when the page opened (live readings don't redraw the picture under the reader); a range
  // picked here (the homepage's own range, changed with it) brings its own once its figures arrive.
  let frozen = $state(untrack(() => $state.snapshot(dashboard)) as Dashboard);
  $effect(() => {
    const next = dashboard;
    if (next.rangeKey !== untrack(() => frozen.rangeKey)) frozen = $state.snapshot(next) as Dashboard;
  });
  const chosenRange = $derived(rangeOptions.find((o) => o.checked)?.action?.[1] ?? frozen.rangeKey);
  function pickRange(key: string) {
    const option = rangeOptions.find((o) => o.action?.[1] === key);
    if (option?.action && key !== chosenRange) onrange?.(option.action);
  }
  const pictures = new Map<string, Picture>();
  let picture = $state<Picture | null>(null);
  /** The style the picture on screen was drawn in: the menu changes at once, the picture once it is drawn. */
  let drawnStyle = $state<ShareStyle>('card');
  let failed = $state(false);
  let busy = $state<'copy' | 'save' | 'share' | null>(null);
  let result = $state<{ op: 'copy' | 'save'; ok: boolean } | null>(null);
  let resultTimer: ReturnType<typeof setTimeout> | undefined;
  let shareButton: HTMLButtonElement | undefined = $state();
  const mac = typeof navigator !== 'undefined' && navigator.userAgent.includes('Mac');
  // A phone's browser: copying and the share sheet only on a secure page (the public link), saving always.
  const canCopy = !onPhone || web.canCopyPicture();
  const canShare = onPhone ? web.canSharePicture() : mac && inTauri;

  // Each style drawn once per range, light or dark and number of tools, then kept while the page is open.
  $effect(() => {
    const figures = frozen, count = tools, key = `${figures.rangeKey}:${style}${dark ? ':dark' : ':light'}:${count}`, wantsDark = dark;
    remember(STYLE, style);
    const known = pictures.get(key);
    if (known) { picture = known; drawnStyle = style; failed = false; return; }
    let current = true;
    const wanted = style;
    void renderPicture(wanted, figures, project, wantsDark, count).then((drawn) => {
      if (!current) return;
      if (drawn) pictures.set(key, drawn);
      picture = drawn;
      drawnStyle = wanted;
      failed = !drawn;
    });
    return () => { current = false; };
  });
  onDestroy(() => { for (const p of pictures.values()) URL.revokeObjectURL(p.url); clearTimeout(resultTimer); });

  // The settings at the top of the column beside the picture and the export at its foot need this much height together;
  // taken a frame after they change size (never inside the observer's own call).
  let settingsBox: HTMLDivElement | undefined = $state(), footBox: HTMLDivElement | undefined = $state();
  let natural = $state(300);
  $effect(() => {
    const boxes = [settingsBox, footBox];
    if (boxes.some((b) => !b)) return;
    let frame = 0;
    const measure = () => { natural = settingsBox!.offsetHeight + 24 + footBox!.offsetHeight; };
    const observer = new ResizeObserver(() => { cancelAnimationFrame(frame); frame = requestAnimationFrame(measure); });
    boxes.forEach((b) => observer.observe(b!));
    measure();
    return () => { observer.disconnect(); cancelAnimationFrame(frame); };
  });
  /** Small: enough to see the picture's look and order, not to read it; a short picture is shown a little wider so its
   *  foot meets the column's (124pt wide up to 172). */
  const previewWidth = $derived(picture ? Math.min(172, Math.max(124, Math.round((natural * picture.width) / picture.height))) : 124);
  const previewHeight = $derived(picture ? Math.round((previewWidth * picture.height) / picture.width) : natural);
  /** The picture and the column side by side, as tall as the taller: settings at the top, the export at the foot. */
  const columnHeight = $derived(Math.max(previewHeight, natural));
  const styleName = $derived(shareStyles.find((s) => s.key === style)?.name ?? '');
  const fileName = $derived(`Starmeter ${frozen.date} ${styleName}.png`);
  // The page follows them: header, then the taller of the picture and the column beside it.
  $effect(() => onheight(58 + columnHeight + 16));

  function settle(op: 'copy' | 'save', ok: boolean) {
    clearTimeout(resultTimer);
    result = { op, ok };
    resultTimer = setTimeout(() => (result = null), 2000);
  }
  async function copy() {
    if (!picture || busy) return;
    busy = 'copy';
    const ok = onPhone ? await web.copyPicture(picture.png) : await shell.copyPicture(picture.png).catch(() => false);
    busy = null;
    settle('copy', ok);
  }
  async function save() {
    if (!picture || busy) return;
    busy = 'save';
    const ok = onPhone ? web.savePicture(picture.png, fileName) : await shell.savePicture(picture.png, fileName).catch(() => false);
    busy = null;
    // On a phone the browser shows the download itself, and the reader may still turn it down.
    if (ok && !onPhone) settle('save', true);
  }
  async function share() {
    if (!picture || busy || !shareButton) return;
    busy = 'share';
    const box = shareButton.getBoundingClientRect();
    if (onPhone) await web.sharePicture(picture.png, fileName);
    else await shell.sharePicture(picture.png, fileName, { x: box.left, y: box.top, width: box.width, height: box.height }).catch(() => {});
    busy = null;
  }
  function pickMode(next: 'light' | 'dark') {
    mode = next;
    remember(MODE, next);
  }
  const copied = $derived(result?.op === 'copy' ? (result.ok ? '已复制' : '复制失败') : null);
  const saved = $derived(result?.op === 'save' ? '已保存' : null);
</script>

<div class="share-page">
  <!-- Pinned, the header's free space moves the window, as a title bar would. -->
  <div class="grip" class:movable={panel.pinned} aria-hidden="true"
    onpointerdown={(event) => { if (event.button === 0 && panel.pinned) { event.preventDefault(); void moveWindow(); } }}></div>
  <span class="back"><HintButton icon="chevron.left" size={17} hint={t('返回')} label={t('返回')} onclick={onback} /></span>
  <span class="page-glyph"><Icon name="square.and.arrow.up" size={17} system /></span>
  <h2 class="title">{t('分享用量图')}<span class="range">{frozen.rangeLabel}</span></h2>
  <span class="rule"></span>
  <div class="body">
    <Scroller onswipeback={onback}>
      <div class="columns">
        <div class="frame" style:width="{previewWidth}px" style:height="{previewHeight}px">
          {#if picture}
            <img src={picture.url} alt={t('{style}风格的用量图预览', { style: t(shareStyles.find((s) => s.key === drawnStyle)?.name ?? '') })} width={previewWidth} height={previewHeight} />
          {:else if failed}
            <span class="note">{t('没能画出来')}</span>
          {:else}
            <span class="spinner" aria-label={t('正在生成')}></span>
          {/if}
        </div>
        <div class="controls" style:height="{columnHeight}px">
          <div class="settings" bind:this={settingsBox}>
          <div class="setting">
            <span class="setting-name">{t('风格')}</span>
            <Dropdown label={t('风格')} value={style} size={12} align="left" field options={shareStyles.map((s) => ({ value: s.key, label: t(s.name) }))}
              onpick={(next) => (style = next as ShareStyle)} />
          </div>
          <div class="setting">
            <span class="setting-name">{t('色彩')}</span>
            <div class="modes" role="radiogroup" aria-label={t('色彩')}>
              <span class="pill" class:right={dark}></span>
              <button class="mode pressable" class:chosen={!dark} role="radio" aria-checked={!dark} onclick={() => pickMode('light')}>{t('浅色')}</button>
              <button class="mode pressable" class:chosen={dark} role="radio" aria-checked={dark} onclick={() => pickMode('dark')}>{t('深色')}</button>
            </div>
          </div>
          <div class="setting">
            <span class="setting-name">{t('工具数量')}</span>
            <div class="modes counts" role="radiogroup" aria-label={t('工具数量')} style:--segments={counts.length}>
              <span class="pill" style:transform="translateX({(tools - 1) * 100}%)"></span>
              {#each counts as n (n)}
                <button class="mode pressable" class:chosen={tools === n} role="radio" aria-checked={tools === n} onclick={() => pickTools(n)}>{n}</button>
              {/each}
            </div>
          </div>
          {#if rangeOptions.length}
            <div class="setting">
              <span class="setting-name">{t('时间范围')}</span>
              <Dropdown label={t('时间范围')} value={chosenRange} size={12} align="left" field
                options={rangeOptions.map((o) => ({ value: o.action?.[1] ?? '', label: o.label }))} onpick={pickRange} />
            </div>
          {/if}
          </div>
          <div class="spacer"></div>
          <div class="foot" bind:this={footBox}>
          {#if picture}<p class="size">{Math.round(picture.width * SCALE)} × {Math.round(picture.height * SCALE)} {t('像素')} · PNG</p>{/if}
          {#if onPhone}<p class="size">{t('长按左边的图也能存到相册')}</p>{/if}
          <div class="actions">
            <!-- On a phone the share sheet comes first: it is how a picture reaches a chat there. -->
            {#if onPhone && canShare}
              <button class="action primary pressable" bind:this={shareButton} disabled={!picture} onclick={share}>
                <Icon name="square.and.arrow.up" size={14} system />{t('分享…')}
              </button>
            {/if}
            {#if canCopy}
              <button class="action pressable" class:primary={!onPhone || !canShare} class:done={copied === '已复制'} class:failed={copied === '复制失败'} disabled={!picture} onclick={copy}>
                <Icon name={copied === '已复制' ? 'checkmark' : 'doc.on.doc'} size={14} system />{t(copied ?? '复制图片')}
              </button>
            {/if}
            <button class="action pressable" class:primary={onPhone && !canShare && !canCopy} class:done={!!saved} disabled={!picture} onclick={save}>
              <Icon name={saved ? 'checkmark' : 'square.and.arrow.down'} size={14} system />{t(saved ?? (onPhone ? '保存图片' : '保存…'))}
            </button>
            {#if !onPhone && canShare}
              <button class="action pressable" bind:this={shareButton} disabled={!picture} onclick={share}>
                <Icon name="square.and.arrow.up" size={14} system />{t('分享…')}
              </button>
            {/if}
          </div>
          </div>
        </div>
      </div>
    </Scroller>
  </div>
</div>

<style>
  .share-page {
    position: relative;
    width: 100%;
    height: 100%;
  }
  .grip { position: absolute; left: 0; top: 0; width: 100%; height: 48px; }
  .grip.movable { cursor: grab; }
  .grip.movable:active { cursor: grabbing; }
  .back { position: absolute; left: 10px; top: 11px; }
  .page-glyph { position: absolute; left: 48px; top: 16px; pointer-events: none; }
  .title {
    position: absolute;
    left: 73px;
    top: 15px;
    margin: 0;
    font-size: 14px;
    font-weight: 600;
    line-height: 20px;
    pointer-events: none;
  }
  .range { margin-left: 8px; font-size: 11.5px; font-weight: 500; color: var(--secondary); }
  .rule { position: absolute; left: 16px; right: 16px; top: 49px; height: 1px; background: var(--separator); }
  .body { position: absolute; left: 0; right: 0; top: 58px; bottom: 0; display: flex; }
  .columns { display: flex; gap: 16px; padding: 0 16px 16px; }
  .frame {
    flex: none;
    border-radius: 6px;
    overflow: hidden;
    box-shadow: 0 0 0 0.5px var(--separator), 0 3px 10px rgba(0, 0, 0, 0.14);
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .frame img { display: block; }
  .note { font-size: 10.5px; color: var(--secondary); }
  .spinner {
    width: 14px;
    height: 14px;
    border-radius: 50%;
    border: 2px solid var(--separator);
    border-top-color: var(--secondary);
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin { to { transform: rotate(360deg); } }
  /* Settings at the top, what to do with the picture at the bottom, level with the picture's foot. */
  .controls { flex: 1; min-width: 0; display: flex; flex-direction: column; }
  .settings { display: flex; flex-direction: column; gap: 14px; }
  .foot { display: flex; flex-direction: column; gap: 14px; }
  .spacer { flex: 1; }

  .setting { display: flex; flex-direction: column; align-items: stretch; gap: 5px; }
  .setting-name { font-size: 11px; color: var(--secondary); }
  .size { margin: 0 0 -6px; font-size: 10px; color: var(--tertiary); font-variant-numeric: tabular-nums; }
  .modes {
    position: relative;
    display: flex;
    width: 100%;
    height: 28px;
    padding: 2px;
    box-sizing: border-box;
    border-radius: 8px;
    background: color-mix(in srgb, var(--label) 6%, transparent);
  }
  .pill {
    position: absolute;
    top: 2px;
    bottom: 2px;
    left: 2px;
    width: calc(50% - 2px);
    border-radius: 6px;
    background: var(--solid);
    box-shadow: 0 0 0 0.5px var(--separator), 0 1px 3px rgba(0, 0, 0, 0.1);
    transition: transform 0.22s var(--page-curve);
  }
  .pill.right { transform: translateX(100%); }
  .counts .pill { width: calc((100% - 4px) / var(--segments)); }
  .counts .mode { font-variant-numeric: tabular-nums; }
  .mode {
    --press-scale: 0.96;
    position: relative;
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 11.5px;
    color: var(--secondary);
    border-radius: 6px;
  }
  .mode.chosen { color: var(--label); font-weight: 600; }
  .actions { display: flex; flex-direction: column; gap: 8px; }
  .action {
    --press-scale: 0.97;
    height: 30px;
    border-radius: 8px;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    font-size: 12px;
    font-weight: 600;
    color: var(--label);
    background: color-mix(in srgb, var(--label) 8%, transparent);
  }
  .action:hover:not(:disabled) { background: color-mix(in srgb, var(--label) 12%, transparent); }
  .action:disabled { opacity: 0.5; }
  .action.primary { color: var(--on-accent); background: var(--accent); }
  .action.primary:hover:not(:disabled) { background: color-mix(in srgb, var(--accent) 88%, black); }
  .action.done { color: var(--green); }
  .action.primary.done { color: white; background: var(--green); }
  .action.primary.failed { color: white; background: var(--orange); }
</style>
