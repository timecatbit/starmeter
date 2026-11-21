<script lang="ts">
  // A Harness card: the tool's mark and name with its notes, the cost on the right; then its tokens in bold and the
  // counts in grey; a gateway's clients; then each model with a short share bar. Folded (允许折叠), only the first two
  // lines show, and a click on the card unfolds it in place (again folds it); the arrow at its right is a button of its
  // own that opens the tool's page. Not folded, a click anywhere on the card opens the page. One gesture each: the
  // card's click never means one thing first and another the second time.
  import { alpha, blend, label as labelColour, toolColour } from '../lib/colors';
  import { t } from '../lib/i18n.svelte';
  import { clientLine, colourOf, headline, isGrouped, modelAmount, modelName, notes, share, statsForms, tokenLine } from '../lib/format';
  import { figureWidth, font, ROUNDED, width } from '../lib/measure';
  import { handRing, seeded } from '../lib/sketchStrokes';
  import { style } from '../lib/style.svelte';
  import { panel } from '../lib/ui.svelte';
  import type { ProviderUsage } from '../lib/types';
  import Figure from './Figure.svelte';
  import Glyph from './Glyph.svelte';

  let { p, collapsible = false, expanded = true, glow = 0, ontoggle, onopen }:
    { p: ProviderUsage; collapsible?: boolean; expanded?: boolean; glow?: number; ontoggle?: () => void; onopen?: () => void } = $props();

  const left = 12;
  const right = $derived(panel.width - 28);
  /** A model's share bar: 44pt at the panel's own width, longer in a pinned panel made wider, so the row fills. */
  const bar = $derived(Math.round(Math.min(200, 44 + (panel.width - 360) * 0.45)));
  const colour = $derived(toolColour(colourOf(p), panel.dark));
  const headerRight = $derived(collapsible ? right - 21 : right);
  const cost = $derived(p.available ? headline(p) : '—');
  const costWidth = $derived(Math.min(130, figureWidth(cost.slice(0, 1), font(16 * 0.62, 500, ROUNDED)) + figureWidth(cost.slice(1), font(16, 600, ROUNDED)) + 1));
  const nameWidth = $derived(Math.min(headerRight - costWidth - 14 - (left + 21), width(p.name, font(13, 600)) + 1));
  const shownNotes = $derived.by(() => {
    let x = left + 21 + nameWidth + 7;
    const out: { text: string; warning: boolean; x: number; w: number }[] = [];
    for (const [text, warning] of notes(p)) {
      const room = headerRight - costWidth - 10 - x;
      if (room <= 12) break;
      const w = Math.min(room, width(text, font(9.5)) + 1);
      out.push({ text, warning, x, w });
      x += w + 6;
    }
    return out;
  });
  const lead = $derived(tokenLine(p));
  const leadWidth = $derived(width(lead, font(11, 600)) + 1);
  const stats = $derived.by(() => {
    const room = Math.max(0, right - left - 21 - leadWidth - 4), forms = statsForms(p);
    return forms.find((form) => width('· ' + form, font(10)) <= room) ?? forms[forms.length - 1];
  });
  const showStats = $derived(!collapsible || expanded);
  const showModels = $derived(!collapsible || expanded);
  const clients = $derived(clientLine(p));
  const clientHeight = $derived(clients ? 16 : 0);
  const single = $derived(isGrouped(p) && p.models.length === 1 && p.models[0].share >= 0.999 && !p.models[0].priceNote);
  const mid = 15.8;
  /** 手绘: the mark in a ring of the tool's pen, drawn by hand as the picture rings it (seeded by the tool, so steady). */
  const ring = $derived(style.current === 'sketch' ? handRing(seeded([...p.id].reduce((n, ch) => n + ch.charCodeAt(0), 7)), 12, 12, 9.5) : '');
  const glowing = $derived(blend(labelColour(panel.dark, 0.85), colour, glow * 0.9));
  /** A figure too wide for its column shrinks rather than cuts, down to 9pt. */
  const fitted = (value: string, size: number, room: number) => { const w = figureWidth(value, font(size)) * 1.02; return w > room ? Math.max(8, (size * room) / w) : size; };
  const height = $derived(collapsible && !expanded ? 52 : p.available ? 52 + clientHeight + Math.max(1, p.models.length) * 18 + 8 : 52);
</script>

<div class="card" class:opens={!!onopen || collapsible} style:height="{height}px" style:--pen={colour}>
  {#if collapsible}
    <button class="open pressable" aria-label={t(expanded ? '收起 {name} 明细' : '展开 {name} 明细', { name: p.name })} aria-expanded={expanded} onclick={ontoggle}></button>
    {#if onopen}
      <!-- Centred on the amount's digits, which sit 0.8pt above the header line's middle. -->
      <button class="enter pressable" style:left="{right - 16}px" style:top="{mid - 12.8}px" aria-label={t('{name} 详情', { name: p.name })} onclick={onopen}>
        <svg width="8" height="8" viewBox="0 0 8 8" aria-hidden="true">
          <path d="M2.5 1 5.5 4 2.5 7" fill="none" stroke="var(--secondary)" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" />
        </svg>
      </button>
    {/if}
  {:else if onopen}
    <button class="open pressable" aria-label={t('{name} 详情', { name: p.name })} onclick={onopen}></button>
  {/if}
  {#if ring}
    <svg class="ring" style:left="{left - 5}px" style:top="{mid - 12}px" width="24" height="24" aria-hidden="true">
      <path d={ring} fill="none" stroke={colour} stroke-width="1.6" stroke-linecap="round" />
    </svg>
  {/if}
  <span class="mark" style:left="{left}px" style:top="{mid - 7}px">
    {#if p.image || p.symbol}
      <Glyph image={p.image} template={p.template} symbol={p.symbol} size={14} />
    {:else}
      <span class="dot" style:background={colour}></span>
    {/if}
  </span>
  <span class="name" style:left="{left + 21}px" style:width="{nameWidth}px">{p.name}</span>
  {#each shownNotes as note (note.text)}
    <span class="card-note" class:warning={note.warning} style:left="{note.x}px" style:width="{note.w}px">{note.text}</span>
  {/each}
  <span class="cost" style:left="{headerRight - costWidth - 2}px">
    <Figure value={cost} size={16} room={costWidth + 2} align="right" color={glowing} />
  </span>
  {#if p.available}
    <span class="lead" style:left="{left + 21}px" style:width="{leadWidth}px" style:color={glowing}>{lead}</span>
    {#if showStats}
      <span class="stats" style:left="{left + 21 + leadWidth + 4}px" style:width="{Math.max(0, right - left - 21 - leadWidth - 4)}px">· {stats}</span>
    {/if}
  {:else}
    <span class="waiting" style:left="{left + 21}px">{statsForms(p)[0]}</span>
  {/if}
  {#if p.available && showModels}
    {#if clients}<span class="clients" style:left="{left + 21}px" style:width="{right - left - 21}px">{clients}</span>{/if}
    <div class="models" style:top="{52 + clientHeight}px">
      {#if !p.models.length}
        <span class="empty">{t('此时段暂无模型调用')}</span>
      {:else if single}
        <span class="empty from">{t('全部来自 {name}', { name: modelName(p.models[0].name) })}</span>
      {:else}
        {#each p.models as m, i (m.name + i)}
          {@const barX = right - 34 - 62 - bar - 4}
          {@const room = barX - left - 21 - 10}
          {@const noteWidth = m.priceNote ? Math.min(room * 0.55, width(m.priceNote, font(9)) + 1) : 0}
          {@const nameRoom = m.priceNote ? Math.min(room - noteWidth - 5, width(modelName(m.name), font(10.5)) + 1) : room}
          <div class="model" style:top="{i * 18}px">
            <span class="model-name" style:width="{nameRoom}px">{modelName(m.name)}</span>
            {#if m.priceNote}<span class="model-note" style:left="{nameRoom + 5}px" style:width="{noteWidth}px">{m.priceNote}</span>{/if}
            <span class="track" style:left="{barX - left - 21}px" style:width="{bar}px" style:background={labelColour(panel.dark, panel.dark ? 0.1 : 0.07)}>
              <span class="fill" style:width="{Math.max(4, bar * Math.min(1, m.share))}px" style:background={alpha(colour, 0.85)}></span>
            </span>
            <span class="amount num" style:left="{barX + bar + 4 - left - 21}px" style:font-size="{fitted(modelAmount(m), 10.5, 58)}px">{modelAmount(m)}</span>
            <span class="share num" style:left="{right - 34 - left - 21}px" style:font-size="{fitted(share(m.share), 10, 34)}px">{share(m.share)}</span>
          </div>
        {/each}
      {/if}
    </div>
  {/if}
</div>

<style>
  .card {
    position: relative;
    width: 100%;
    transition: height 0.26s var(--page-curve);
    overflow: hidden;
  }
  .card > span, .model > span {
    position: absolute;
    white-space: nowrap;
    overflow: hidden;
    overflow: clip visible;
    text-overflow: ellipsis;
    line-height: 1.2;
  }
  .open {
    --press-scale: 0.99;
    position: absolute;
    left: 4px;
    top: 2px;
    width: calc(100% - 8px);
    height: calc(100% - 6px);
    border-radius: 8px;
  }
  .open:hover { background: var(--hover); }
  .open:active { background: var(--press); }
  /* The page's own door on a folded card: drawn as a button, apart from the card's unfolding. */
  .enter {
    --press-scale: 0.9;
    position: absolute;
    width: 24px;
    height: 24px;
    border-radius: 7px;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .enter:hover { background: color-mix(in srgb, var(--label) 9%, transparent); }
  .enter:active { background: color-mix(in srgb, var(--label) 14%, transparent); }
  /* The words let the pointer through to the card's button under them. */
  .card.opens > span, .card.opens .models { pointer-events: none; }
  .mark { width: 14px; height: 14px; pointer-events: none; }
  .ring { position: absolute; overflow: visible; pointer-events: none; }
  .dot {
    position: absolute;
    left: 4.5px;
    top: 4.5px;
    width: 5px;
    height: 5px;
    border-radius: 50%;
  }
  .name {
    top: 8px;
    font-size: 13px;
    font-weight: 600;
    pointer-events: none;
  }
  .card-note {
    top: 10.5px;
    font-size: 9.5px;
    color: var(--secondary);
    pointer-events: none;
  }
  .card-note.warning { color: var(--orange); }
  .cost { top: 5px; pointer-events: none; overflow: visible !important; }
  .lead {
    top: 29px;
    font-size: 11px;
    font-weight: 600;
    pointer-events: none;
  }
  .stats {
    top: 30px;
    font-size: 10px;
    color: var(--secondary);
    pointer-events: none;
  }
  .waiting {
    top: 29px;
    font-size: 10.5px;
    color: var(--secondary);
  }
  .clients {
    top: 45px;
    font-size: 10px;
    color: var(--secondary);
  }
  .models {
    position: absolute;
    left: 33px;
    right: 12px;
  }
  .empty {
    position: absolute;
    font-size: 10px;
    color: var(--secondary);
    white-space: nowrap;
  }
  .empty.from { font-size: 10.5px; }
  .model {
    position: absolute;
    left: 0;
    right: 0;
    height: 18px;
  }
  .model-name { left: 0; font-size: 10.5px; }
  .model-note { font-size: 9px; color: var(--orange); top: 1.5px; }
  .track {
    top: 4.3px;
    height: 4px;
    border-radius: 2px;
  }
  .fill {
    position: absolute;
    left: 0;
    top: 0;
    height: 4px;
    border-radius: 2px;
  }
  .amount {
    text-overflow: clip !important;
    width: 58px;
    font-size: 10.5px;
    text-align: right;
  }
  .share {
    width: 34px;
    font-size: 10px;
    color: var(--secondary);
    text-align: right;
  }
</style>
