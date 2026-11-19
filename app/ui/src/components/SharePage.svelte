<script lang="ts">
  // 分享用量图: pick a style from the menu at the top right, see the picture exactly as it will be sent (small, so most
  // of it shows at once), then copy it, save it to the computer, or (on macOS) hand it to the share menu. The figures are those on screen when the page opened, so live readings don't
  // redraw the picture under the reader.
  import { onDestroy, untrack } from 'svelte';
  import { inTauri, shell } from '../lib/backend';
  import { renderPicture, shareStyles, type Picture, type ShareStyle } from '../lib/share';
  import { moveWindow, panel } from '../lib/ui.svelte';
  import type { Dashboard, Project } from '../lib/types';
  import HintButton from './HintButton.svelte';
  import Icon from './Icon.svelte';
  import Scroller from './Scroller.svelte';

  let { dashboard, project, onback, onheight }: { dashboard: Dashboard; project: Project; onback: () => void; onheight: (height: number) => void } = $props();

  const KEY = 'starmeter.shareStyle';
  const remembered = typeof localStorage === 'undefined' ? null : localStorage.getItem(KEY);
  let style = $state<ShareStyle>(shareStyles.some((s) => s.key === remembered) ? (remembered as ShareStyle) : 'card');
  const frozen = untrack(() => $state.snapshot(dashboard)) as Dashboard;
  const pictures = new Map<string, Picture>();
  let picture = $state<Picture | null>(null);
  let failed = $state(false);
  let busy = $state<'copy' | 'save' | 'share' | null>(null);
  let result = $state<{ op: 'copy' | 'save'; ok: boolean } | null>(null);
  let resultTimer: ReturnType<typeof setTimeout> | undefined;
  let shareButton: HTMLButtonElement | undefined = $state();
  const mac = typeof navigator !== 'undefined' && navigator.userAgent.includes('Mac');

  // Each style drawn once, in this page's theme, then kept while the page is open.
  $effect(() => {
    const key = style + (panel.dark ? ':dark' : ':light');
    if (typeof localStorage !== 'undefined') localStorage.setItem(KEY, style);
    const known = pictures.get(key);
    if (known) { picture = known; failed = false; return; }
    let current = true;
    void renderPicture(style, frozen, project, panel.dark).then((drawn) => {
      if (!current) return;
      if (drawn) pictures.set(key, drawn);
      picture = drawn;
      failed = !drawn;
    });
    return () => { current = false; };
  });
  onDestroy(() => { for (const p of pictures.values()) URL.revokeObjectURL(p.url); clearTimeout(resultTimer); });

  const previewWidth = $derived(Math.min(280, Math.round(panel.width * 0.62)));
  const styleName = $derived(shareStyles.find((s) => s.key === style)?.name ?? '');
  let picking = $state(false);
  function pick(event: Event) {
    style = (event.target as HTMLSelectElement).value as ShareStyle;
    panel.menuOpen = picking = false;
  }
  const previewHeight = $derived(picture ? Math.round((previewWidth * picture.height) / picture.width) : 420);
  const fileName = $derived(`Starmeter ${frozen.date} ${shareStyles.find((s) => s.key === style)?.name ?? ''}.png`);

  function settle(op: 'copy' | 'save', ok: boolean) {
    clearTimeout(resultTimer);
    result = { op, ok };
    resultTimer = setTimeout(() => (result = null), 2000);
  }
  async function copy() {
    if (!picture || busy) return;
    busy = 'copy';
    const ok = await shell.copyPicture(picture.png).catch(() => false);
    busy = null;
    settle('copy', ok);
  }
  async function save() {
    if (!picture || busy) return;
    busy = 'save';
    const ok = await shell.savePicture(picture.png, fileName).catch(() => false);
    busy = null;
    if (ok) settle('save', true);
  }
  async function share() {
    if (!picture || busy || !shareButton) return;
    busy = 'share';
    const box = shareButton.getBoundingClientRect();
    await shell.sharePicture(picture.png, fileName, { x: box.left, y: box.top, width: box.width, height: box.height }).catch(() => {});
    busy = null;
  }
  const copied = $derived(result?.op === 'copy' ? (result.ok ? '已复制' : '复制失败') : null);
  const saved = $derived(result?.op === 'save' ? '已保存' : null);
</script>

<div class="share-page">
  <!-- Pinned, the header's free space moves the window, as a title bar would. -->
  <div class="grip" class:movable={panel.pinned} aria-hidden="true"
    onpointerdown={(event) => { if (event.button === 0 && panel.pinned) { event.preventDefault(); void moveWindow(); } }}></div>
  <span class="back"><HintButton icon="chevron.left" hint="返回" label="返回" onclick={onback} /></span>
  <span class="page-glyph"><Icon name="square.and.arrow.up" size={17} /></span>
  <h2 class="title">分享用量图<span class="range">{frozen.rangeLabel}</span></h2>
  <!-- The style, as the range picker on the homepage: bare until pointed at, the system menu when clicked. -->
  <span class="picker" class:open={picking}>
    <span class="picker-label">{styleName}</span>
    <svg class="picker-arrows" width="7" height="5" viewBox="0 0 7 5" aria-hidden="true"><path d="M1 1.1 3.5 3.6 6 1.1" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg>
    <select aria-label="风格 · {styleName}" value={style} onchange={pick}
      onpointerdown={() => (panel.menuOpen = picking = true)} onblur={() => (panel.menuOpen = picking = false)}>
      {#each shareStyles as s (s.key)}
        <option value={s.key}>{s.name}</option>
      {/each}
    </select>
  </span>
  <span class="rule"></span>
  <div class="preview">
    <Scroller onswipeback={onback} onheight={(h) => onheight(62 + h + 64)}>
      <div class="frame" style:width="{previewWidth}px" style:height="{previewHeight}px">
        {#if picture}
          <img src={picture.url} alt="{styleName}风格的用量图预览" width={previewWidth} height={previewHeight} />
        {:else if failed}
          <span class="note">这张图没能画出来</span>
        {:else}
          <span class="spinner" aria-label="正在生成"></span>
        {/if}
      </div>
    </Scroller>
  </div>
  <div class="actions">
    <button class="action primary pressable" class:done={copied === '已复制'} class:failed={copied === '复制失败'} disabled={!picture} onclick={copy}>
      <Icon name={copied === '已复制' ? 'checkmark' : 'doc.on.doc'} size={14} />{copied ?? '复制图片'}
    </button>
    <button class="action pressable" class:done={!!saved} disabled={!picture} onclick={save}>
      <Icon name={saved ? 'checkmark' : 'square.and.arrow.down'} size={14} />{saved ?? '保存…'}
    </button>
    {#if mac && inTauri}
      <button class="action pressable" bind:this={shareButton} disabled={!picture} onclick={share}>
        <Icon name="square.and.arrow.up" size={14} />分享…
      </button>
    {/if}
  </div>
</div>

<style>
  .share-page {
    position: relative;
    width: 100%;
    height: 100%;
  }
  .grip {
    position: absolute;
    left: 0;
    top: 0;
    width: 100%;
    height: 48px;
  }
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
  .range {
    margin-left: 8px;
    font-size: 11.5px;
    font-weight: 500;
    color: var(--secondary);
  }
  .picker {
    position: absolute;
    right: 12px;
    top: 11px;
    height: 28px;
    display: flex;
    align-items: center;
    gap: 4px;
    box-sizing: border-box;
    padding: 0 8px;
    border-radius: 7px;
    font-size: 12px;
    font-weight: 500;
    color: var(--label);
  }
  .picker:hover { background: var(--hover); }
  .picker.open, .picker:active { background: var(--press); }
  .picker-arrows { flex: none; color: var(--secondary); }
  .picker select {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    opacity: 0;
    font-size: 12px;
    cursor: default;
  }
  .rule {
    position: absolute;
    left: 16px;
    right: 16px;
    top: 49px;
    height: 1px;
    background: var(--separator);
  }
  .preview {
    position: absolute;
    left: 0;
    right: 0;
    top: 62px;
    bottom: 60px;
    display: flex;
  }
  .frame {
    margin: 0 auto 4px;
    border-radius: 8px;
    overflow: hidden;
    box-shadow: 0 0 0 0.5px var(--separator), 0 4px 14px rgba(0, 0, 0, 0.14);
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .frame img { display: block; }
  .note { font-size: 11px; color: var(--secondary); }
  .spinner {
    width: 16px;
    height: 16px;
    border-radius: 50%;
    border: 2px solid var(--separator);
    border-top-color: var(--secondary);
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin { to { transform: rotate(360deg); } }
  .actions {
    position: absolute;
    left: 16px;
    right: 16px;
    bottom: 14px;
    height: 32px;
    display: flex;
    gap: 8px;
  }
  .action {
    --press-scale: 0.96;
    flex: 1;
    height: 32px;
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
  .action.primary { flex: 1.3; color: white; background: var(--accent); }
  .action.primary:hover:not(:disabled) { background: color-mix(in srgb, var(--accent) 88%, black); }
  .action.done { color: var(--green); }
  .action.primary.done { color: white; background: var(--green); }
  .action.primary.failed { color: white; background: var(--orange); }
</style>
