<script lang="ts">
  // A page inside the panel: the back button with the row's icon, name and figure (in a shorter form if the name needs
  // the room), a rule, then its rows. Esc, the back button or a two-finger swipe to the right go back.
  import { usageColor } from '../lib/colors';
  import { font, width } from '../lib/measure';
  import type { Entry } from '../lib/types';
  import DetailList from './DetailList.svelte';
  import Glyph from './Glyph.svelte';
  import HintButton from './HintButton.svelte';
  import Scroller from './Scroller.svelte';
  import { moveWindow, panel } from '../lib/ui.svelte';

  let { page, open, busy, scrollTop = $bindable(0), onback, onopen, onchoose, ontoggled, onheight }: {
    page: Entry; open: Set<string>; busy: string[] | null; scrollTop?: number;
    onback: () => void; onopen: (e: Entry) => void; onchoose: (e: Entry) => void; ontoggled: (open: Set<string>) => void; onheight: (height: number) => void;
  } = $props();

  const W = $derived(panel.width);
  const hasGlyph = $derived(page.image !== undefined || page.symbol !== undefined);
  const titleX = $derived(hasGlyph ? 73 : 48);
  const room = $derived(W - 16 - titleX - width(page.label, font(14, 600)) - 14);
  const shown = $derived(page.value === undefined ? undefined
    : [page.value, ...(page.fallbacks ?? [])].find((v) => width(v, font(11.5, 500)) + 6 <= room) ?? page.fallbacks?.at(-1) ?? page.value);
  const valueWidth = $derived(shown === undefined ? 0 : Math.min(Math.max(60, room), width(shown, font(11.5, 500)) + 6));
</script>

<div class="page">
  <!-- Pinned, the header's free space moves the window, as a title bar would. -->
  <div class="grip" class:movable={panel.pinned} aria-hidden="true"
    onpointerdown={(event) => { if (event.button === 0 && panel.pinned) { event.preventDefault(); void moveWindow(); } }}></div>
  <span class="back"><HintButton icon="chevron.left" hint="返回" label="返回" onclick={onback} /></span>
  {#if hasGlyph}<span class="page-glyph"><Glyph image={page.image} template={page.template} symbol={page.symbol} size={18} /></span>{/if}
  <h2 class="title" style:left="{titleX}px" style:width="{W - 16 - valueWidth - 8 - titleX}px">{page.label}</h2>
  {#if shown !== undefined}
    <span class="value num" style:width="{valueWidth}px" style:color={usageColor(page.usageLevel) ?? 'var(--secondary)'}>{shown}</span>
  {/if}
  <span class="rule"></span>
  <div class="rows">
    <Scroller bind:scrollTop onswipeback={onback} onheight={(h) => onheight(50 + h + 20)}>
      <DetailList entries={page.children} {open} {busy} rowWidth={W - 16} {onopen} {onchoose} {ontoggled} />
    </Scroller>
  </div>
</div>

<style>
  .page {
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
  .back { position: absolute; left: 12px; top: 13px; }
  /* The header's words let the pointer through to the strip under them. */
  .page-glyph, .title, .value { pointer-events: none; }
  .page-glyph { position: absolute; left: 48px; top: 16px; }
  .title {
    position: absolute;
    top: 15px;
    margin: 0;
    font-size: 14px;
    font-weight: 600;
    line-height: 20px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .value {
    position: absolute;
    right: 16px;
    top: 18px;
    font-size: 11.5px;
    font-weight: 500;
    text-align: right;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    line-height: 16px;
  }
  .rule {
    position: absolute;
    left: 16px;
    right: 16px;
    top: 49px;
    height: 1px;
    background: var(--separator);
  }
  .rows {
    position: absolute;
    left: 8px;
    right: 8px;
    top: 54px;
    bottom: 8px;
    display: flex;
  }
</style>
