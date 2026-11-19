<script lang="ts">
  // An icon from the panel's set. eye: placed by its centre as the eye sees it (not its box), on whole device pixels,
  // so a row of different icons (the header's) lines up. system: on a Mac, the SF Symbol of that name as macOS draws it
  // (in the text colour, at its own proportions, centred in the box); elsewhere, or until it arrives, the panel's own.
  import { eyeCentre, iconSvg, systemSymbol } from '../lib/icons';

  let { name, size = 14, eye = false, system = false }: { name: string; size?: number; eye?: boolean; system?: boolean } = $props();

  let symbol = $state<{ url: string; width: number; height: number } | null>(null);
  $effect(() => {
    if (!system) { symbol = null; return; }
    let current = true;
    // A symbol of point size p stands about 1.3p tall (the pin, the share box), so 0.82 of the box keeps it the box's size.
    void systemSymbol(name, Math.round(size * 0.82 * 2) / 2).then((found) => { if (current) symbol = found; });
    return () => { current = false; };
  });

  let offset = $state<[number, number]>([0, 0]);
  $effect(() => {
    if (!eye) { offset = [0, 0]; return; }
    let current = true;
    const pixel = (v: number) => Math.round(v * (window.devicePixelRatio || 1)) / (window.devicePixelRatio || 1);
    void eyeCentre(name).then(([x, y]) => { if (current) offset = [pixel(((10 - x) * size) / 20), pixel(((10 - y) * size) / 20)]; });
    return () => { current = false; };
  });
</script>

{#if symbol}
  <span class="icon system" aria-hidden="true" data-icon={name} style:width="{size}px" style:height="{size}px">
    <span class="mask" style:width="{symbol.width}px" style:height="{symbol.height}px"
      style:mask-image="url('{symbol.url}')" style:-webkit-mask-image="url('{symbol.url}')"></span>
  </span>
{:else}
  <svg class="icon" viewBox="0 0 20 20" width={size} height={size} aria-hidden="true" data-icon={name}
    style:transform={offset[0] || offset[1] ? `translate(${offset[0]}px, ${offset[1]}px)` : undefined}>{@html iconSvg(name)}</svg>
{/if}

<style>
  .icon {
    display: block;
    flex: none;
  }
  .system { display: flex; align-items: center; justify-content: center; overflow: visible; }
  .mask {
    flex: none;
    background-color: currentColor;
    mask-size: 100% 100%;
    -webkit-mask-size: 100% 100%;
    mask-repeat: no-repeat;
    -webkit-mask-repeat: no-repeat;
  }
</style>
