<script lang="ts">
  // A QR code in the logo's look, as the share picture signs with: midnight-navy dots on a pale tile, rounded finder
  // eyes, the app icon in the middle (level Q recovers the modules under it). Dark on light, which every scanner reads,
  // in the dark appearance too.
  import starmeterIcon from '../../../../assets/starmeter.svg?url';
  import { qrLook, qrModules } from '../lib/qr';

  let { text, size, label }: { text: string; size: number; label: string } = $props();

  const uid = $props.id();
  const look = $derived.by(() => { const modules = qrModules(text, 'Q'); return modules ? qrLook(modules) : null; });
  /** The quiet zone around the symbol, in modules, as the share picture leaves. */
  const quiet = 2;
  const paper = '#F4F7FF';
</script>

{#if look}
  {@const total = look.n + quiet * 2}
  <svg class="qr" viewBox="{-quiet} {-quiet} {total} {total}" width={size} height={size} role="img" aria-label={label}>
    <defs>
      <linearGradient id="{uid}-ink" gradientUnits="userSpaceOnUse" x1={-quiet} y1={-quiet} x2={-quiet + total * 0.35} y2={-quiet + total}>
        <stop offset="0" stop-color="#1F3A7A" /><stop offset="0.55" stop-color="#0E1D4C" /><stop offset="1" stop-color="#050A24" />
      </linearGradient>
    </defs>
    <rect x={-quiet} y={-quiet} width={total} height={total} rx={total * 0.16} fill={paper}
      stroke="rgba(31, 58, 122, 0.16)" stroke-width="0.8" vector-effect="non-scaling-stroke" />
    <g fill="url(#{uid}-ink)">
      {#each look.dots as [x, y] (x * 1000 + y)}<circle cx={x + 0.5} cy={y + 0.5} r="0.43" />{/each}
      {#each look.eyes as [x, y] (x * 1000 + y)}
        <rect {x} {y} width="7" height="7" rx="2.2" />
        <rect x={x + 1} y={y + 1} width="5" height="5" rx="1.5" fill={paper} />
        <rect x={x + 2} y={y + 2} width="3" height="3" rx="1" />
      {/each}
    </g>
    <!-- The app icon's tile, without the margin the macOS icon grid leaves (100 of 1024). -->
    <svg x={look.icon.at} y={look.icon.at} width={look.icon.size} height={look.icon.size} viewBox="100 100 824 824">
      <image href={starmeterIcon} width="1024" height="1024" />
    </svg>
  </svg>
{/if}

<style>
  .qr { display: block; }
</style>
