<script lang="ts">
  // A tool's card on 网页版 (or a model's, a project's): its mark in a halo of its colour, the name and any note that
  // changes how to read it, the amount on the right; its tokens and its share of the list, with a bar of that share
  // that grows in when the card first shows. A tap opens it in place, sliding open to the counts and each model with
  // its own bar; another tap closes it. A card whose figures just grew glows at its edge in its colour for a moment:
  // that tool is at work now.
  import { untrack } from 'svelte';
  import { slide } from 'svelte/transition';
  import { alpha, toolColour } from '../lib/colors';
  import { clientLine, colourOf, headline, modelAmount, modelName, notes, share, statsForms, tokenLine } from '../lib/format';
  import { t } from '../lib/i18n.svelte';
  import { pageCurve } from '../lib/motion';
  import { panel } from '../lib/ui.svelte';
  import type { ProviderUsage } from '../lib/types';
  import Glyph from './Glyph.svelte';
  import StageNumber from './StageNumber.svelte';

  let { p, part, open = false, glow = false, order = 0, enter = true, ontoggle }:
    { p: ProviderUsage; part: number; open?: boolean; glow?: boolean; order?: number; enter?: boolean; ontoggle: () => void } = $props();

  const colour = $derived(toolColour(colourOf(p), panel.dark));
  const amount = $derived(p.available ? headline(p) : '—');
  /** What changes how to read the card (unpriced requests, counted by a gateway, a problem): a warning in orange. */
  const said = $derived(notes(p));
  const stats = $derived(statsForms(p)[0]);
  const clients = $derived(clientLine(p));
  /** Risen in only when it first shows and the page asks (the opening, a new grouping or range): a card that stays
   *  through a reading doesn't rise again. */
  const entering = untrack(() => enter) && !panel.reduceMotion;
  const reveal = (node: Element) => slide(node, { duration: panel.reduceMotion ? 0 : 320, easing: pageCurve });
</script>

<div class="card" class:open class:glow class:off={!p.available} class:enter={entering} style:--tint={colour} style:--pen={colour}
  style:--halo={alpha(colour, panel.dark ? 0.22 : 0.16)} style:--order={order}>
  <button class="head" aria-expanded={open} onclick={ontoggle}>
    <span class="mark">{#if p.image || p.symbol}<Glyph image={p.image} template={p.template} symbol={p.symbol} size={18} tint={colour} />{:else}<span
        class="initial">{[...p.name][0] ?? '·'}</span>{/if}</span>
    <span class="who">
      <span class="name">{p.name}</span>
      {#if said.length}<span class="notes">{#each said as [text, warning], i (i)}<span class="note" class:warning>{text}</span>{/each}</span>{/if}
    </span>
    <span class="amount num"><StageNumber value={amount} /></span>
    <svg class="chevron" width="10" height="10" viewBox="0 0 10 10" aria-hidden="true"><path d="M3 2l3 3-3 3" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg>
  </button>
  {#if p.available}
    <div class="line">
      <span class="tokens">{tokenLine(p)}</span>
      {#if part > 0}<span class="part num">{t('占 {n}', { n: share(part) })}</span>{/if}
    </div>
    {#if part > 0}<div class="bar"><span class="fill" style:transform="scaleX({Math.max(0.012, Math.min(1, part))})"></span></div>{/if}
  {/if}
  {#if open && p.available}
    <div class="more" transition:reveal>
      <div class="stats">{stats}</div>
      {#if clients}<div class="stats">{clients}</div>{/if}
      {#each p.models as m (m.name)}
        <div class="model">
          <span class="model-name">{modelName(m.name)}{#if m.priceNote}<span class="unpriced">{t('未定价')}</span>{/if}</span>
          <span class="model-bar"><span class="model-fill" style:width="{Math.max(2, m.share * 100)}%"></span></span>
          <span class="model-amount num">{modelAmount(m)}</span>
          <span class="model-share num">{share(m.share)}</span>
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .card {
    position: relative;
    border-radius: 20px;
    background: var(--glass);
    box-shadow: inset 0 0 0 0.5px var(--glass-edge), var(--lift);
    padding: 4px 14px 12px;
    transition: box-shadow 1.2s ease-out;
  }
  /* Rising in, one after another down the list. */
  .card.enter { animation: card-in 0.6s cubic-bezier(0.22, 1, 0.36, 1) calc(var(--lead, 0.35s) + var(--order) * 0.06s) backwards; }
  @keyframes card-in { from { opacity: 0; transform: translateY(14px) scale(0.985); } }
  .card.glow { box-shadow: inset 0 0 0 1px var(--tint), 0 0 22px -4px var(--tint), var(--lift); transition: none; }
  .card.off { opacity: 0.62; }
  .head {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 48px;
    text-align: left;
  }
  .mark {
    flex: none;
    width: 34px;
    height: 34px;
    border-radius: 11px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--halo);
    box-shadow: inset 0 0 0 0.5px color-mix(in srgb, var(--tint) 40%, transparent);
  }
  .initial { font-family: var(--rounded); font-size: 15px; font-weight: 600; color: var(--tint); text-transform: uppercase; }
  .who { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 1px; }
  .name { font-size: var(--type-title, 15px); font-weight: 600; color: var(--ink); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .notes { display: flex; gap: 6px; min-width: 0; font-size: 11px; white-space: nowrap; overflow: hidden; }
  .note { color: var(--ink-3); overflow: hidden; text-overflow: ellipsis; }
  .note.warning { color: var(--warn); }
  .amount { font-family: var(--rounded); font-size: var(--fig-m, 17px); font-weight: 600; color: var(--ink); white-space: nowrap; }
  .chevron { flex: none; color: var(--ink-3); transition: transform 0.3s cubic-bezier(0.22, 1, 0.36, 1); }
  .open .chevron { transform: rotate(90deg); }
  .line { display: flex; align-items: baseline; gap: 8px; padding-left: 44px; font-size: 12px; }
  .tokens { color: var(--ink); font-weight: 600; }
  .part { color: var(--ink-2); }
  .bar {
    margin: 7px 0 0 44px;
    height: 4px;
    border-radius: 2px;
    background: var(--line-faint);
    overflow: hidden;
  }
  .fill {
    display: block;
    height: 100%;
    border-radius: 2px;
    background: linear-gradient(90deg, color-mix(in srgb, var(--tint) 55%, transparent), var(--tint));
    transform-origin: left;
    transition: transform 0.8s cubic-bezier(0.22, 1, 0.36, 1);
  }
  .enter .fill { animation: grow 0.9s cubic-bezier(0.22, 1, 0.36, 1) calc(var(--lead, 0.35s) + 0.2s + var(--order) * 0.06s) backwards; }
  @keyframes grow { from { transform: scaleX(0); } }
  .more { padding: 10px 0 0 44px; display: flex; flex-direction: column; gap: 6px; }
  .stats { font-size: 11.5px; color: var(--ink-2); line-height: 1.45; }
  .model { display: grid; grid-template-columns: minmax(0, 1fr) 64px auto 3.3em; align-items: center; gap: 8px; font-size: 12px; }
  .model-name { color: var(--ink); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .unpriced { margin-left: 6px; font-size: 10.5px; color: var(--warn); }
  .model-bar { height: 4px; border-radius: 2px; background: var(--line-faint); overflow: hidden; }
  .model-fill { display: block; height: 100%; border-radius: 2px; background: var(--tint); }
  .model-amount { color: var(--ink-2); text-align: right; }
  .model-share { color: var(--ink-3); text-align: right; }
</style>
