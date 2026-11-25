<script lang="ts">
  // 网页版's 星环: each tool's share of the tokens Summary counts, as arcs round a ring that sweep in clockwise from the
  // top when a range first shows (shares fill a whole), with the total in the middle. A tap on an arc or on its line in
  // the legend lights that tool and puts its own figures in the middle; another tap, or a tap on the middle, lets go.
  // The first five keep their arcs; the rest are one, 其他工具.
  import { onDestroy } from 'svelte';
  import { toolColour } from '../lib/colors';
  import { legendName, money, share, summaryPart, tokens } from '../lib/format';
  import { t } from '../lib/i18n.svelte';
  import { panel } from '../lib/ui.svelte';
  import type { Dashboard } from '../lib/types';

  let { data, draw = true, size = 148 }: { data: Dashboard; draw?: boolean; size?: number } = $props();

  const SHOWN = 5;
  type Slice = { id: string; name: string; tokens: number; cost: number | null; colour: string };
  const slices = $derived.by((): Slice[] => {
    const counted = data.providers.map(summaryPart).filter((p) => p !== null && p.available && p.tokens > 0)
      .sort((a, b) => b!.tokens - a!.tokens) as NonNullable<ReturnType<typeof summaryPart>>[];
    const listed = counted.slice(0, counted.length > SHOWN + 1 ? SHOWN : SHOWN + 1);
    const rest = counted.slice(listed.length);
    const out: Slice[] = listed.map((p) => ({ id: p.id, name: legendName(p), tokens: p.tokens, cost: p.cost, colour: toolColour(p.id, panel.dark) }));
    if (rest.length) {
      const costs = rest.map((p) => p.cost).filter((c): c is number => c !== null);
      out.push({ id: 'other', name: t('其他工具'), tokens: rest.reduce((s, p) => s + p.tokens, 0), cost: costs.length ? costs.reduce((a, b) => a + b, 0) : null,
        colour: panel.dark ? 'rgba(255,255,255,0.35)' : 'rgba(14,29,76,0.3)' });
    }
    return out;
  });
  const total = $derived(slices.reduce((s, p) => s + p.tokens, 0));
  const stroke = $derived(Math.round(size * 0.1));
  const radius = $derived((size - stroke) / 2 - 2);
  const circumference = $derived(2 * Math.PI * radius);
  /** A hairline of the ring left between arcs, so neighbours read apart. */
  const gap = $derived(slices.length > 1 ? 3 : 0);
  const arcs = $derived.by(() => {
    let start = 0;
    return slices.map((s, i) => {
      const length = total > 0 ? (s.tokens / total) * circumference : 0;
      const arc = { ...s, start, length: Math.max(0.5, length - gap), share: total > 0 ? s.tokens / total : 0, order: i };
      start += length;
      return arc;
    });
  });
  /** How far round the sweep has come (0 to 1): one front going clockwise from the top, filling the arcs in order. */
  let swept = $state(1);
  /** The range last swept: a new reading of the same range moves the arcs, only a new range sweeps again. */
  let sweptRange: string | null = null;
  let frame = 0;
  $effect(() => {
    const key = data.rangeKey;
    if (key === sweptRange) return;
    const first = sweptRange === null;
    sweptRange = key;
    cancelAnimationFrame(frame);
    if ((first && !draw) || panel.reduceMotion) { swept = 1; return; }
    swept = 0;
    let start = 0;
    const step = (now: number) => {
      start ||= now;
      const t = Math.min(1, Math.max(0, (now - start - 150) / 950));
      swept = 1 - Math.pow(1 - t, 3);
      if (t < 1) frame = requestAnimationFrame(step);
    };
    frame = requestAnimationFrame(step);
  });
  onDestroy(() => cancelAnimationFrame(frame));
  const shown = (arc: { start: number; length: number }) => Math.max(0, Math.min(arc.length, swept * circumference - arc.start));
  let lit = $state<string | null>(null);
  let ringBox: HTMLButtonElement | undefined = $state();
  /** A tap on the ring: the arc under it by its angle from the top, clockwise; the middle lets go. */
  function tapRing(event: MouseEvent) {
    if (!ringBox) return;
    const box = ringBox.getBoundingClientRect();
    const dx = event.clientX - box.left - box.width / 2, dy = event.clientY - box.top - box.height / 2;
    if (Math.hypot(dx, dy) < radius - stroke) { lit = null; return; }
    const along = (((Math.atan2(dx, -dy) + 2 * Math.PI) % (2 * Math.PI)) / (2 * Math.PI)) * circumference;
    const arc = arcs.find((a) => along >= a.start && along < a.start + a.length + gap);
    if (arc) pick(arc.id);
  }
  // A tool that has left the ring (another range) lets go of it.
  $effect(() => { if (lit && !slices.some((s) => s.id === lit)) lit = null; });
  const focus = $derived(lit ? arcs.find((a) => a.id === lit) ?? null : null);
  /** The middle's figure as large as the hole in the ring lets it be. */
  const middleSize = (text: string) => Math.min(20, ((size - 2 * stroke - 8) * 0.84) / (Math.max(3, text.length) * 0.6));
  const pick = (id: string) => (lit = lit === id ? null : id);
</script>

{#if slices.length}
  <div class="ring-room"><div class="ring-block">
    <div class="ring" style:width="{size}px" style:height="{size}px">
      <svg width={size} height={size} viewBox="0 0 {size} {size}" aria-hidden="true">
        <circle class="track" cx={size / 2} cy={size / 2} r={radius} stroke-width={stroke} />
        <g transform="rotate(-90 {size / 2} {size / 2})">
          {#each arcs as arc (arc.id)}
            <circle class="arc" class:dim={lit !== null && lit !== arc.id} class:moving={swept === 1} cx={size / 2} cy={size / 2} r={radius}
              stroke={arc.colour} stroke-width={lit === arc.id ? stroke + 4 : stroke}
              style:stroke-dasharray="{shown(arc)} {circumference}" style:stroke-dashoffset={-arc.start} />
          {/each}
        </g>
      </svg>
      <button class="middle" bind:this={ringBox} onclick={tapRing} aria-label={t('工具占比')}>
        {#if focus}
          <span class="big num" style:font-size="{middleSize(share(focus.share))}px">{share(focus.share)}</span>
          <span class="small">{focus.name}</span>
        {:else}
          <span class="big num" style:font-size="{middleSize(tokens(total))}px">{tokens(total)}</span>
          <span class="small">Token</span>
        {/if}
      </button>
    </div>
    <div class="legend-room"><ul class="legend">
      {#each arcs as arc (arc.id)}
        <li>
          <button class="entry pressable" class:on={lit === arc.id} class:dim={lit !== null && lit !== arc.id} onclick={() => pick(arc.id)}>
            <span class="swatch" style:background={arc.colour}></span>
            <span class="name">{arc.name}</span>
            <span class="share num">{share(arc.share)}</span>
            <span class="cost num">{arc.cost === null ? '—' : money(arc.cost)}</span>
          </button>
        </li>
      {/each}
    </ul></div>
  </div></div>
{/if}

<style>
  /* Narrow (a small phone on its side), the costs give way; narrower still, the legend goes under the ring. */
  /* Narrow (a phone on its side, its right column), the legend goes under the ring, the whole width; narrow beside the
     ring, the costs give way first. */
  .ring-room { container-type: inline-size; }
  .ring-block { display: flex; align-items: center; gap: 18px; }
  .legend-room { flex: 1; min-width: 0; container-type: inline-size; }
  @container (max-width: 330px) { .ring-block { flex-direction: column; gap: 12px; } .legend-room { width: 100%; } }
  @container (max-width: 170px) { .cost { display: none; } }
  .ring { position: relative; flex: none; }
  svg { display: block; overflow: visible; }
  .track { fill: none; stroke: var(--line-faint); }
  .arc {
    fill: none;
    stroke-linecap: butt;
    transition: stroke-width 0.25s cubic-bezier(0.22, 1, 0.36, 1), opacity 0.25s;
  }
  /* A new reading moves the arcs to their new shares rather than jumping (the sweep itself is drawn frame by frame). */
  .arc.moving { transition: stroke-width 0.25s cubic-bezier(0.22, 1, 0.36, 1), opacity 0.25s, stroke-dasharray 0.6s cubic-bezier(0.22, 1, 0.36, 1),
      stroke-dashoffset 0.6s cubic-bezier(0.22, 1, 0.36, 1); }
  .arc.dim { opacity: 0.28; }
  .middle {
    position: absolute;
    inset: 0;
    border-radius: 50%;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    text-align: center;
    gap: 2px;
  }
  .big { font-family: var(--rounded); font-size: var(--fig-l, 20px); font-weight: 600; color: var(--ink); }
  .small { font-size: 11px; color: var(--ink-2); max-width: 100%; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .legend { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 2px; }
  .entry {
    --press-scale: 0.98;
    width: 100%;
    display: flex;
    align-items: center;
    gap: 8px;
    height: 26px;
    padding: 0 8px;
    margin: 0 -8px;
    border-radius: 9px;
    box-sizing: content-box;
    font-size: 12.5px;
    transition: opacity 0.25s, background-color 0.2s;
  }
  .entry.on { background: var(--glass-strong); }
  .entry.dim { opacity: 0.45; }
  .swatch { width: 8px; height: 8px; border-radius: 3px; flex: none; }
  .name { flex: 1; min-width: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; color: var(--ink); }
  .share { color: var(--ink); font-weight: 600; min-width: 3.2em; text-align: right; }
  .cost { color: var(--ink-3); min-width: 3.8em; text-align: right; font-size: 11.5px; }
</style>
