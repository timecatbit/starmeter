<script lang="ts">
  // An icon from the panel's set. eye: placed by its centre as the eye sees it (not its box), on whole device pixels,
  // so a row of different icons (the header's) lines up.
  import { eyeCentre, iconSvg } from '../lib/icons';

  let { name, size = 14, eye = false }: { name: string; size?: number; eye?: boolean } = $props();

  let offset = $state<[number, number]>([0, 0]);
  $effect(() => {
    if (!eye) { offset = [0, 0]; return; }
    let current = true;
    const pixel = (v: number) => Math.round(v * (window.devicePixelRatio || 1)) / (window.devicePixelRatio || 1);
    void eyeCentre(name).then(([x, y]) => { if (current) offset = [pixel(((10 - x) * size) / 20), pixel(((10 - y) * size) / 20)]; });
    return () => { current = false; };
  });
</script>

<svg class="icon" viewBox="0 0 20 20" width={size} height={size} aria-hidden="true" data-icon={name}
  style:transform={offset[0] || offset[1] ? `translate(${offset[0]}px, ${offset[1]}px)` : undefined}>{@html iconSvg(name)}</svg>

<style>
  .icon {
    display: block;
    flex: none;
  }
</style>
