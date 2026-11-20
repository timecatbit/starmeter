<script lang="ts">
  // 分享用量图: the picture small on the left, enough to see what it will look like, and beside it the style, light or
  // dark, how many tools, the range, and what to do with it: copy, save to the computer, or (macOS) the share menu.
  // The page is as tall as the column beside the picture, whatever the picture: changing a setting changes only the
  // picture. The figures are those on screen when the page opened, so live readings don't redraw it under the reader.
  import { onDestroy, untrack } from 'svelte';
  import { t } from '../lib/i18n.svelte';
  import { inTauri, onPhone, shell } from '../lib/backend';
  import * as web from '../lib/webShare';
  import { renderPicture, shareStyles, type Picture, type ShareStyle } from '../lib/share';
  import { SCALE, shownRows } from '../lib/shareCommon';
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
  const storedTools = Math.round(Number(stored(TOOLS)));
  let tools = $state(storedTools >= 1 && storedTools <= SHARE_TOOLS_MOST ? storedTools : SHARE_TOOLS);
  function pickTools(next: number) {
    tools = Math.max(1, Math.min(most, next));
    remember(TOOLS, String(next));
  }
  // The figures as they stood when the page opened (live readings don't redraw the picture under the reader); a range
  // picked here (the homepage's own range, changed with it) brings its own once its figures arrive.
  let frozen = $state(untrack(() => $state.snapshot(dashboard)) as Dashboard);
  /** No more than the tools used in the range (six at most): a number past them would change nothing. The choice kept is
   *  the reader's; a range with fewer tools shows fewer. */
  const most = $derived(Math.max(1, Math.min(SHARE_TOOLS_MOST, shownRows(frozen).length)));
  const shownTools = $derived(Math.min(tools, most));
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
  // ?mac draws the Mac's share button in a browser too (the fixtures, the screenshots).
  const canShare = onPhone ? web.canSharePicture() : mac && (inTauri || (typeof location !== 'undefined' && new URLSearchParams(location.search).has('mac')));

  // Each style drawn once per range, light or dark and number of tools, then kept while the page is open.
  const pictureKey = (range: string) => `${range}:${style}${dark ? ':dark' : ':light'}:${shownTools}`;
  /** Which picture is on screen: until the one for the current choices is drawn, the last one stays in sight. */
  let drawnKey = $state<string | null>(null);
  $effect(() => {
    const figures = frozen, count = shownTools, key = pictureKey(figures.rangeKey), wantsDark = dark;
    remember(STYLE, style);
    const known = pictures.get(key);
    if (known) { picture = known; drawnStyle = style; drawnKey = key; failed = false; return; }
    let current = true;
    const wanted = style;
    void renderPicture(wanted, figures, project, wantsDark, count).then((drawn) => {
      if (!current) return;
      if (drawn) pictures.set(key, drawn);
      picture = drawn;
      drawnStyle = wanted;
      drawnKey = drawn ? key : null;
      failed = !drawn;
    });
    return () => { current = false; };
  });
  /** Copy, save and share give the picture of what the page says: the range picked (its figures arrived), the style,
   *  the colours and the number of tools. Until it is drawn they wait, rather than give the last one away. */
  const ready = $derived(picture !== null && frozen.rangeKey === chosenRange && drawnKey === pictureKey(chosenRange));
  onDestroy(() => { for (const p of pictures.values()) URL.revokeObjectURL(p.url); clearTimeout(resultTimer); });

  // The picture on a stage as tall as the column beside it, fitted whole (never cut): changing the style, the colours
  // or the number of tools changes only the picture, nothing around it moves.
  const INSET = 10;
  let stageWidth = $state(0), stageHeight = $state(0);
  const thumb = $derived.by(() => {
    if (!picture || stageWidth <= 2 * INSET || stageHeight <= 2 * INSET) return { width: 0, height: 0 };
    const scale = Math.min((stageHeight - 2 * INSET) / picture.height, (stageWidth - 2 * INSET) / picture.width);
    return { width: Math.round(picture.width * scale), height: Math.round(picture.height * scale) };
  });
  // The page follows its sheet: the header, then the stage beside its column.
  let sheet: HTMLDivElement | undefined = $state();
  $effect(() => {
    const box = sheet;
    if (!box) return;
    let frame = 0;
    const measure = () => onheight(58 + box.offsetHeight);
    const observer = new ResizeObserver(() => { cancelAnimationFrame(frame); frame = requestAnimationFrame(measure); });
    observer.observe(box);
    measure();
    return () => { observer.disconnect(); cancelAnimationFrame(frame); };
  });
  const styleName = $derived(shareStyles.find((s) => s.key === style)?.name ?? '');
  const fileName = $derived(`Starmeter ${frozen.date} ${styleName}.png`);

  function settle(op: 'copy' | 'save', ok: boolean) {
    clearTimeout(resultTimer);
    result = { op, ok };
    resultTimer = setTimeout(() => (result = null), 2000);
  }
  async function copy() {
    if (!picture || !ready || busy) return;
    busy = 'copy';
    const ok = onPhone ? await web.copyPicture(picture.png) : await shell.copyPicture(picture.png).catch(() => false);
    busy = null;
    settle('copy', ok);
  }
  async function save() {
    if (!picture || !ready || busy) return;
    busy = 'save';
    const ok = onPhone ? web.savePicture(picture.png, fileName) : await shell.savePicture(picture.png, fileName).catch(() => false);
    busy = null;
    // On a phone the browser shows the download itself, and the reader may still turn it down.
    if (ok && !onPhone) settle('save', true);
  }
  async function share() {
    if (!picture || !ready || busy || !shareButton) return;
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
  /** The ways out, the strongest first: on a phone the share sheet (how a picture reaches a chat there), else copy. */
  const actions = $derived.by(() => {
    const list: ('share' | 'copy' | 'save')[] = [];
    if (onPhone && canShare) list.push('share');
    if (canCopy) list.push('copy');
    list.push('save');
    if (!onPhone && canShare) list.push('share');
    return list;
  });
  const copied = $derived(result?.op === 'copy' ? (result.ok ? '已复制' : '复制失败') : null);
  const saved = $derived(result?.op === 'save' ? '已保存' : null);
</script>

{#snippet act(which: 'share' | 'copy' | 'save', primary: boolean)}
  {#if which === 'share'}
    <button class="action pressable" class:primary bind:this={shareButton} disabled={!ready} onclick={share}>
      <Icon name="square.and.arrow.up" size={14} system />{t('分享…')}
    </button>
  {:else if which === 'copy'}
    <button class="action pressable" class:primary class:done={copied === '已复制'} class:failed={copied === '复制失败'} disabled={!ready} onclick={copy}>
      <Icon name={copied === '已复制' ? 'checkmark' : 'doc.on.doc'} size={14} system />{t(copied ?? '复制图片')}
    </button>
  {:else}
    <button class="action pressable" class:primary class:done={!!saved} disabled={!ready} onclick={save}>
      <Icon name={saved ? 'checkmark' : 'square.and.arrow.down'} size={14} system />{t(saved ?? (onPhone ? '保存图片' : '保存…'))}
    </button>
  {/if}
{/snippet}

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
      <div class="sheet" bind:this={sheet}>
      <div class="stage" bind:clientWidth={stageWidth} bind:clientHeight={stageHeight}>
        {#if picture && thumb.width}
          <img src={picture.url} alt={t('{style}风格的用量图预览', { style: t(shareStyles.find((s) => s.key === drawnStyle)?.name ?? '') })} width={thumb.width} height={thumb.height} />
        {:else if failed}
          <span class="note">{t('没能画出来')}</span>
        {:else}
          <span class="spinner" aria-label={t('正在生成')}></span>
        {/if}
      </div>
      <!-- Beside the picture, one under another: the settings, then the size and what to do with the picture, the
           strongest action first; the column sets the page's height, the stage takes it. -->
      <div class="side">
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
          <!-- One less or one more, in the same field as the menus above and below. -->
          <div class="stepper" role="group" aria-label={t('工具数量')}>
            <button class="step pressable" disabled={shownTools <= 1} aria-label={t('少一个')} onclick={() => pickTools(shownTools - 1)}><Icon name="minus" size={13} /></button>
            <span class="count" aria-live="polite">{shownTools}</span>
            <button class="step pressable" disabled={shownTools >= most} aria-label={t('多一个')} onclick={() => pickTools(shownTools + 1)}><Icon name="plus" size={13} /></button>
          </div>
        </div>
        {#if rangeOptions.length}
          <div class="setting">
            <span class="setting-name">{t('时间范围')}</span>
            <Dropdown label={t('时间范围')} value={chosenRange} size={12} align="left" field
              options={rangeOptions.map((o) => ({ value: o.action?.[1] ?? '', label: o.label }))} onpick={pickRange} />
          </div>
        {/if}
        <div class="export">
          <p class="size">{#if picture}{Math.round(picture.width * SCALE)} × {Math.round(picture.height * SCALE)} {t('像素')} · PNG{/if}</p>
          {#if onPhone}<p class="hint">{t('长按图片也能存到相册')}</p>{/if}
          <div class="actions">
            {#each actions as which, i (which)}{@render act(which, i === 0)}{/each}
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
  /* The stage on the left takes what the column beside it leaves, and its height: its own content has no say
     (contain: size), so a taller or shorter picture moves nothing. */
  .sheet { display: grid; grid-template-columns: minmax(0, 1fr) 166px; gap: 12px; min-height: 300px; padding: 0 16px 16px; }
  .stage {
    contain: size;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 12px;
    background: color-mix(in srgb, var(--label) 4%, transparent);
  }
  .stage img { display: block; border-radius: 5px; box-shadow: 0 0 0 0.5px var(--separator), 0 4px 14px rgba(0, 0, 0, 0.16); }
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
  .side { display: flex; flex-direction: column; gap: 10px; min-width: 0; }
  .setting { display: flex; flex-direction: column; align-items: stretch; gap: 5px; min-width: 0; }
  .setting-name { font-size: 11px; color: var(--secondary); }
  /* The size and the actions at the column's foot, level with the stage's bottom edge. */
  .export { margin-top: auto; padding-top: 4px; display: flex; flex-direction: column; gap: 6px; }
  .size, .hint { margin: 0; font-size: 10px; line-height: 13px; color: var(--tertiary); font-variant-numeric: tabular-nums; }
  .size { min-height: 13px; }
  .actions { display: flex; flex-direction: column; gap: 8px; }
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
  .stepper {
    display: flex;
    align-items: center;
    height: 28px;
    padding: 2px;
    box-sizing: border-box;
    border-radius: 8px;
    background: color-mix(in srgb, var(--label) 6%, transparent);
  }
  .step {
    --press-scale: 0.92;
    flex: none;
    width: 30px;
    height: 24px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 6px;
    color: var(--label);
  }
  .step:hover:not(:disabled) { background: var(--hover); }
  .step:disabled { color: var(--tertiary); }
  .count { flex: 1; text-align: center; font-size: 12.5px; font-weight: 600; font-variant-numeric: tabular-nums; }
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
  .action {
    --press-scale: 0.97;
    flex: none;
    min-width: 0;
    white-space: nowrap;
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
