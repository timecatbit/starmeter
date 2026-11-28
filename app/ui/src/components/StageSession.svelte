<script lang="ts">
  // A session on 网页版, as 会话 lists it on the panel (SessionList.svelte reads the same rows): its tool's mark, the
  // title and what it cost in the range; its state under it, a running one with a light that breathes (it is working
  // now); the latest turn after that. A tap slides its last turns open.
  import { untrack } from 'svelte';
  import { slide } from 'svelte/transition';
  import { figureParts } from '../lib/format';
  import { t } from '../lib/i18n.svelte';
  import { pageCurve } from '../lib/motion';
  import { panel } from '../lib/ui.svelte';
  import type { Entry } from '../lib/types';
  import Glyph from './Glyph.svelte';

  let { s, open = false, glow = false, order = 0, enter = true, ontoggle }:
    { s: Entry; open?: boolean; glow?: boolean; order?: number; enter?: boolean; ontoggle: () => void } = $props();

  const running = $derived(s.statusLight === 'new');
  const turns = $derived(s.children.filter((c) => c.detail !== undefined));
  const latest = $derived.by(() => {
    const turn = turns[0];
    if (!turn) return null;
    const [tokens, cost] = turn.label.split(' · ').slice(-2);
    return `${t(running ? '本轮' : '上一轮')} ${tokens} · ${cost} · ${turn.detail}`;
  });
  /** The amount as the cards write theirs, any words around it (已知) smaller; a value with no number as it is. */
  const figure = $derived.by(() => {
    const found = /^(.*?)([$¥€£]?\d[\d,]*(?:\.\d+)?[KMB]?)(.*)$/.exec(s.value ?? '');
    return found ? { before: found[1].trim(), parts: figureParts(found[2]), after: found[3].trim() } : null;
  });
  const nothing = $derived(/^\D*0[.,]00$/.test(s.value ?? ''));
  /** Risen in only when it first shows and the page asks (the opening, a new grouping or range): a card that stays
   *  through a reading doesn't rise again. */
  const entering = untrack(() => enter) && !panel.reduceMotion;
  const reveal = (node: Element) => slide(node, { duration: panel.reduceMotion ? 0 : 320, easing: pageCurve });
</script>

<div class="session" class:open class:glow class:enter={entering} style:--order={order}>
  <button class="head" aria-expanded={open} onclick={ontoggle}>
    <span class="mark"><Glyph image={s.image} template={s.template} symbol={s.symbol ?? 'terminal'} size={16} tint="var(--ink)" /></span>
    <span class="title">{s.label}</span>
    <span class="value" class:nothing>
      {#if figure}{#if figure.before}<span class="words">{figure.before}</span>{/if}<span class="figure num">{#if figure.parts.sign}<span
            class="sign">{figure.parts.sign}</span>{/if}{figure.parts.body}{#if figure.parts.unit}<span class="unit">{figure.parts.unit}</span>{/if}</span>{#if figure.after}<span
          class="words">{figure.after}</span>{/if}{:else}<span class="words">{s.value ?? ''}</span>{/if}
    </span>
  </button>
  <div class="state"><span class="light" class:running></span><span class="text">{s.detail ?? ''}</span></div>
  {#if latest}<div class="latest">{latest}</div>{/if}
  {#if open}
    <div class="turns" transition:reveal>
      {#each s.children as turn, i (i)}
        {#if turn.detail === undefined}
          <div class="turn-heading">{turn.label}</div>
        {:else}
          <div class="turn"><span class="turn-label num">{turn.label}</span><span class="turn-detail">{turn.detail}</span></div>
        {/if}
      {/each}
    </div>
  {/if}
</div>

<style>
  .session {
    border-radius: 18px;
    background: var(--glass);
    box-shadow: inset 0 0 0 0.5px var(--glass-edge), var(--lift);
    padding: 4px 14px 11px;
    transition: box-shadow 1.2s ease-out;
  }
  /* With the page's first frame, as the tools' cards are (StageCard). */
  .session.enter { animation: card-in 0.5s cubic-bezier(0.22, 1, 0.36, 1) calc(min(var(--order), 8) * 0.035s) backwards; }
  @keyframes card-in { from { opacity: 0; transform: translateY(14px) scale(0.985); } }
  .session.glow { box-shadow: inset 0 0 0 1px var(--star), 0 0 22px -4px var(--star), var(--lift); transition: none; }
  .head { width: 100%; display: flex; align-items: center; gap: 10px; min-height: 42px; text-align: left; }
  .mark {
    flex: none; width: 28px; height: 28px; border-radius: 9px; display: flex; align-items: center; justify-content: center;
    background: var(--glass-strong);
  }
  .title { flex: 1; min-width: 0; font-size: var(--type-title, 15px); font-weight: 600; color: var(--ink); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .value { flex: none; white-space: nowrap; color: var(--ink); }
  .figure { font-family: var(--rounded); font-size: var(--fig-m, 17px); font-weight: 600; }
  /* As the panel's figures: the sign smaller, lighter and raised. */
  .sign { font-size: 0.62em; font-weight: 500; color: var(--ink-2); vertical-align: 0.48em; margin-right: 0.04em; }
  .unit { font-size: 0.74em; }
  .words { font-size: 11px; color: var(--ink-2); }
  .words + .figure { margin-left: 3px; }
  .figure + .words { margin-left: 2px; }
  .nothing .figure { color: var(--ink-3); font-weight: 500; }
  .state { display: flex; align-items: center; gap: 7px; padding-left: 38px; font-size: 11.5px; color: var(--ink-2); min-width: 0; }
  .text, .latest { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .light { flex: none; width: 7px; height: 7px; border-radius: 50%; background: var(--ink-3); }
  .light.running { background: var(--good); box-shadow: 0 0 0 0 var(--good); animation: working 2s ease-out infinite; }
  @keyframes working {
    0% { box-shadow: 0 0 0 0 color-mix(in srgb, var(--good) 70%, transparent); }
    80%, 100% { box-shadow: 0 0 0 7px color-mix(in srgb, var(--good) 0%, transparent); }
  }
  .latest { padding: 3px 0 0 38px; font-size: 11px; color: var(--ink-3); }
  .turns { margin: 8px 0 0 38px; padding-left: 10px; border-left: 1px solid var(--line); display: flex; flex-direction: column; gap: 4px; }
  .turn-heading { font-size: 10.5px; color: var(--ink-3); }
  .turn { display: flex; flex-direction: column; }
  .turn-label { font-size: 11.5px; color: var(--ink); }
  .turn-detail { font-size: 10.5px; color: var(--ink-3); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
</style>
