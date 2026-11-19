<script lang="ts">
  // A bare header icon, as magpie's: a soft rounded fill under the pointer that deepens while pressed, and a short hint
  // drawn by the panel below it (the system tooltip only shows once the app is active). The pin can show its live ring.
  // Each icon sits by its centre as the eye sees it.
  import { hoverHint, panel } from '../lib/ui.svelte';
  import Icon from './Icon.svelte';

  let { icon, hint = '', label, onclick, ring = null, tint = 'var(--label)', width = 27 }: {
    icon: string; hint?: string; label: string; onclick: (event: MouseEvent) => void;
    /** Pinned: the interval of the ring that fills once per live reading, and when the current one started. */
    ring?: { interval: number; since: number } | null;
    tint?: string; width?: number;
  } = $props();

  let button: HTMLButtonElement | undefined = $state();

  const ringDelay = $derived(ring ? -(((Date.now() - ring.since) % (ring.interval * 1000)) / 1000) : 0);
</script>

<button bind:this={button} class="hint-button pressable" style:width="{width}px" aria-label={label}
  onpointerenter={() => hoverHint(button ?? null, hint)} onpointerleave={() => hoverHint(null, '')}
  onpointerdown={() => hoverHint(null, '')} onclick={onclick}>
  <span class="glyph" style:color={tint}><Icon name={icon} size={15} eye /></span>
  {#if ring}
    {#key ring.since}
      <svg class="ring" viewBox="0 0 {width} 25" aria-hidden="true">
        <rect x="0.75" y="0.75" width={width - 1.5} height="23.5" rx="7" ry="7" pathLength="100"
          style:animation-duration="{ring.interval}s" style:animation-delay="{ringDelay}s" class:still={panel.reduceMotion} />
      </svg>
    {/key}
  {/if}
</button>

<style>
  .hint-button {
    --press-scale: 0.88;
    position: relative;
    height: 25px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 7px;
  }
  .hint-button:hover { background: var(--hover); }
  .hint-button:active { background: var(--press); }
  .glyph { display: block; }
  .ring {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    pointer-events: none;
    overflow: visible;
  }
  .ring rect {
    fill: none;
    stroke: var(--accent);
    stroke-width: 1.5;
    stroke-linecap: round;
    stroke-dasharray: 100;
    stroke-dashoffset: 100;
    animation-name: sweep;
    animation-timing-function: linear;
    animation-iteration-count: infinite;
  }
  .ring rect.still {
    animation: none;
    stroke-dashoffset: 0;
  }
  @keyframes sweep {
    to { stroke-dashoffset: 0; }
  }
</style>
