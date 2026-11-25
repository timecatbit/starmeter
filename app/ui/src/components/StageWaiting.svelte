<script lang="ts">
  // 网页版 before the computer has any figures (SwiftBar's plugin not written yet, no tool on the homepage): its own
  // sky, in the computer's theme and look, the Starmeter mark breathing, and what it waits for. The page asks again on
  // its own (App.svelte) and turns to 网页版 as soon as there are figures; nothing to choose meanwhile. The browser's
  // bars take the sky's edges as 网页版's do.
  import { paintEdges } from '../lib/edges';
  import { t } from '../lib/i18n.svelte';
  import { style } from '../lib/style.svelte';
  import { panel } from '../lib/ui.svelte';

  const DOTS: [number, number, number][] = [[12, 2.67, 1.58], [7.33, 7.33, 0.95], [12, 7.33, 2.15], [16.67, 7.33, 0.95], [2.67, 12, 1.58], [7.33, 12, 2.15],
    [12, 12, 2.15], [16.67, 12, 2.15], [21.33, 12, 1.58], [7.33, 16.67, 0.95], [12, 16.67, 2.15], [16.67, 16.67, 0.95], [12, 21.33, 1.58]];
  const sketch = $derived(style.current === 'sketch');
  let box: HTMLDivElement | undefined = $state();
  $effect(() => {
    void panel.dark; void sketch;
    const here = box;
    if (!here) return;
    const frame = requestAnimationFrame(() => {
      const css = getComputedStyle(here);
      paintEdges(css.getPropertyValue('--edge-top').trim(), css.getPropertyValue('--edge-bottom').trim());
    });
    return () => cancelAnimationFrame(frame);
  });
</script>

<div class="waiting" class:light={!panel.dark} class:sketch class:still={panel.reduceMotion} bind:this={box} role="status">
  <svg class="mark" viewBox="0 0 24 24" width="44" height="44" aria-hidden="true">
    {#each DOTS as [cx, cy, r], i (i)}<circle {cx} {cy} {r} style:--d={Math.hypot(cx - 12, cy - 12) / 4.67} />{/each}
  </svg>
  <p class="title">{t('等电脑读到用量')}</p>
  <p class="body">{t('电脑上的 Starmeter 还没有今天的数字（SwiftBar 插件还没写入，或主页上还没有勾选工具）。一读到，这里就是实时用量。')}</p>
</div>

<style>
  .waiting {
    --ink: rgba(240, 245, 255, 0.95);
    --ink-2: rgba(214, 222, 255, 0.7);
    --edge-top: #1F3A7A;
    --edge-bottom: #050A24;
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    padding: calc(24px + env(safe-area-inset-top)) 32px calc(24px + env(safe-area-inset-bottom));
    text-align: center;
    color: var(--ink);
    background: linear-gradient(180deg, var(--edge-top) 0, #1F3A7A 72px, #0E1D4C 48%, #050A24 calc(100% - 90px), var(--edge-bottom) 100%);
  }
  .waiting.light {
    --ink: rgba(14, 24, 66, 0.94);
    --ink-2: rgba(14, 24, 66, 0.62);
    --edge-top: #DCE3FF;
    --edge-bottom: #F7F6FF;
    background: linear-gradient(180deg, var(--edge-top) 0, #DCE3FF 72px, #EEF0FF 48%, #F7F6FF calc(100% - 90px), var(--edge-bottom) 100%);
  }
  .waiting.sketch {
    --ink: var(--label);
    --ink-2: var(--secondary);
    --edge-top: var(--paper);
    --edge-bottom: var(--paper);
    background-color: var(--paper);
    background-image: var(--grain), var(--glow), var(--shade), radial-gradient(circle, var(--dots) 0.75px, transparent 1.1px);
    background-size: 180px 180px, 100% 100%, 100% 100%, 15px 15px;
  }
  .mark { fill: var(--ink); opacity: 0.85; }
  .mark circle { transform-box: fill-box; transform-origin: center; animation: breathe 1.6s ease-in-out infinite; animation-delay: calc(var(--d) * 0.16s); }
  .still .mark circle { animation: none; }
  @keyframes breathe { 0%, 100% { opacity: 1; transform: scale(1); } 45% { opacity: 0.3; transform: scale(0.6); } }
  .title { margin: 6px 0 0; font-size: 17px; font-weight: 600; }
  .body { margin: 0; max-width: 30em; font-size: 13px; line-height: 1.55; color: var(--ink-2); }
</style>
