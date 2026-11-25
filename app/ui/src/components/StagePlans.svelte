<script lang="ts">
  // The homepage's other rows on 手机版. A plan with a limit (订阅) leads with a dial of its tightest window, which fills
  // to its share when it first shows and takes the colour of its level (orange from 70%, red from 90%), and a bar for
  // each window (five hours, the week) named under it. The rest (API balances, monitors, updates) are tiles of their
  // figure, two to a row. Read-only, as on the panel's phone page: nothing here opens.
  import { onMount } from 'svelte';
  import { barColor, usageColor } from '../lib/colors';
  import { configured, usageShare } from '../lib/detail';
  import { t } from '../lib/i18n.svelte';
  import { panel } from '../lib/ui.svelte';
  import type { Entry } from '../lib/types';
  import Glyph from './Glyph.svelte';

  let { entries, enter = true }: { entries: Entry[]; enter?: boolean } = $props();

  const rows = $derived(entries.filter((e) => !e.separator && !e.section && e.label));
  const plans = $derived(rows.filter((e) => usageShare(e) !== null));
  const tiles = $derived(rows.filter((e) => usageShare(e) === null));
  const R = 22, C = 2 * Math.PI * R;
  /** The dials and bars fill to their shares once the tiles have risen in (at once without the opening). */
  let filled = $state(false);
  onMount(() => {
    if (!enter || panel.reduceMotion) { filled = true; return; }
    const timer = setTimeout(() => (filled = true), 500);
    return () => clearTimeout(timer);
  });
  /** A plan's value as the panel splits it: the words quiet, the share plain (本周已用 76%, 76% used this week). */
  function split(value: string | undefined) {
    if (!value) return { lead: '', figure: '', tail: '' };
    const before = /^(本周已用|本週已用|已用) (.+)$/.exec(value);
    if (before) return { lead: before[1], figure: before[2], tail: '' };
    const after = /^(\d[\d.]*%) (.+)$/.exec(value);
    return after ? { lead: '', figure: after[1], tail: after[2] } : { lead: '', figure: value, tail: '' };
  }
  /** A plan's windows a line each pair, five hours on the left and the week on the right; Gemini 订阅's lines are for its
   *  model groups, named over them with their logos (as the panel draws them). */
  function lines(e: Entry) {
    const bars = e.gauges ?? [];
    const count = Math.max(...bars.map((bar) => Math.floor(bar.place / 2))) + 1;
    return Array.from({ length: count }, (_, line) => {
      const meters = [0, 1].map((column) => bars.find((bar) => bar.place === line * 2 + column));
      const named = meters.find((bar) => bar?.group);
      return { group: named?.group, marks: named?.marks ?? [], meters };
    });
  }
  const tint = $derived(panel.dark ? 'rgb(230,230,240)' : 'rgb(30,40,80)');
</script>

{#if plans.length}
  <div class="plans">
    {#each plans as e, i (e.module ?? e.label)}
      {@const used = usageShare(e) ?? 0}
      {@const level = e.shareLevel ?? e.usageLevel}
      {@const words = split(e.value)}
      <div class="plan" class:enter={enter && !panel.reduceMotion} style:--order={i}>
        <div class="dial" style:--level={barColor(level, panel.dark)}>
          <svg width="56" height="56" viewBox="0 0 56 56" aria-hidden="true">
            <circle cx="28" cy="28" r={R} class="dial-track" />
            <circle cx="28" cy="28" r={R} class="dial-fill" transform="rotate(-90 28 28)"
              style:stroke-dasharray="{(filled ? Math.max(0.02, used) : 0) * C} {C}" style:transition-delay="{i * 90}ms" />
          </svg>
          <span class="dial-mark"><Glyph image={e.image} template={e.template} symbol={e.symbol} size={18} {tint} /></span>
        </div>
        <div class="plan-body">
          <div class="plan-top">
            <span class="plan-name">{e.label}</span>
            <span class="plan-value num" style:color={usageColor(e.usageLevel) === 'var(--label)' ? 'var(--ink)' : usageColor(e.usageLevel) ?? 'var(--ink)'}>{#if words.lead}<span
                class="said lead">{words.lead}</span>{/if}{words.figure}{#if words.tail}<span class="said tail">{words.tail}</span>{/if}</span>
          </div>
          {#if e.gauges && e.gauges.length > 1}
            {#each lines(e) as line, l (l)}
              {#if line.group}
                <div class="group">{line.group}{#each line.marks as mark, m (m)}<Glyph image={mark.image} template={mark.template} size={10} {tint} />{/each}</div>
              {/if}
              <div class="windows">
                {#each line.meters as g, k (k)}
                  <div class="window">
                    {#if g}
                      <span class="window-bar"><span class="window-fill" style:width="{filled ? Math.max(2, g.used) : 0}%" style:background={barColor(g.level, panel.dark)}
                        style:transition-delay="{i * 90 + (l * 2 + k) * 60}ms"></span></span>
                      <span class="window-name">{g.label} <span class="num">{g.used}%</span></span>
                    {/if}
                  </div>
                {/each}
              </div>
            {/each}
          {:else if e.detail}
            <div class="plan-detail">{e.detail}</div>
          {/if}
        </div>
      </div>
    {/each}
  </div>
{/if}
{#if tiles.length}
  <div class="tiles">
    {#each tiles as e, i (e.module ?? e.label)}
      {@const state = configured(e.value)}
      <div class="tile" class:enter={enter && !panel.reduceMotion} class:attention={e.attention} style:--order={plans.length + i}>
        <span class="tile-head"><Glyph image={e.image} template={e.template} symbol={e.symbol} size={15} {tint} /><span class="tile-name">{e.label}</span></span>
        <span class="tile-value num" class:pill={e.attention}>{state === null ? e.value ?? '' : t(state ? '已配置' : '未配置')}</span>
      </div>
    {/each}
  </div>
{/if}

<style>
  .plans { display: flex; flex-direction: column; gap: 10px; }
  .plan {
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 12px 14px;
    border-radius: 20px;
    background: var(--glass);
    box-shadow: inset 0 0 0 0.5px var(--glass-edge), var(--lift);
  }
  .enter { animation: card-in 0.6s cubic-bezier(0.22, 1, 0.36, 1) calc(0.5s + var(--order) * 0.06s) backwards; }
  @keyframes card-in { from { opacity: 0; transform: translateY(14px) scale(0.985); } }
  .dial { position: relative; flex: none; width: 56px; height: 56px; }
  .dial svg { display: block; }
  .dial-track { fill: none; stroke: var(--line-faint); stroke-width: 5; }
  .dial-fill {
    fill: none;
    stroke: var(--level);
    stroke-width: 5;
    stroke-linecap: round;
    transition: stroke-dasharray 1.1s cubic-bezier(0.22, 1, 0.36, 1);
    filter: drop-shadow(0 0 4px color-mix(in srgb, var(--level) 60%, transparent));
  }
  .dial-mark { position: absolute; inset: 0; display: flex; align-items: center; justify-content: center; }
  .plan-body { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 6px; }
  .plan-top { display: flex; align-items: baseline; gap: 8px; }
  .plan-name { flex: 1; min-width: 0; font-size: var(--type-title, 15px); font-weight: 600; color: var(--ink); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .plan-value { flex: none; font-family: var(--rounded); font-size: var(--fig-m, 17px); font-weight: 600; white-space: nowrap; }
  /* The words around the share in the text's own face. */
  .said { font-family: var(--text-face); font-size: var(--type-small, 11px); font-weight: 500; color: var(--ink-2); }
  .said.lead { margin-right: 4px; }
  .said.tail { margin-left: 4px; }
  .plan-detail { font-size: 11.5px; color: var(--ink-2); white-space: pre-line; line-height: 1.4; }
  .windows { display: grid; grid-template-columns: 1fr 1fr; gap: 6px 12px; }
  .window { display: flex; flex-direction: column; gap: 4px; min-width: 0; }
  .window-bar { height: 4px; border-radius: 2px; background: var(--line-faint); overflow: hidden; }
  .window-fill { display: block; height: 100%; border-radius: 2px; transition: width 1.1s cubic-bezier(0.22, 1, 0.36, 1); }
  .window-name { font-size: 11px; color: var(--ink-2); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .window-name .num { color: var(--ink); font-weight: 600; }
  .group { display: flex; align-items: center; gap: 4px; margin-bottom: -2px; font-size: 10.5px; color: var(--ink-3); }
  .tiles { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 10px; margin-top: 10px; }
  /* An odd one out takes the whole last row, so the grid ends level. */
  .tile:last-child:nth-child(odd) { grid-column: 1 / -1; }
  .tile {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 12px 14px;
    border-radius: 18px;
    background: var(--glass);
    box-shadow: inset 0 0 0 0.5px var(--glass-edge), var(--lift);
    min-width: 0;
  }
  .tile-head { display: flex; align-items: center; gap: 7px; min-width: 0; }
  .tile-name { font-size: 12.5px; color: var(--ink-2); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .tile-value { font-size: 15px; font-weight: 600; line-height: 1.3; color: var(--ink); overflow-wrap: anywhere; }
  .tile-value.pill { align-self: flex-start; max-width: 100%; box-sizing: border-box; padding: 2px 10px; border-radius: 999px; font-size: 13px; color: var(--on-star); background: var(--star); }
</style>
