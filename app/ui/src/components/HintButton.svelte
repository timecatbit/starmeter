<script lang="ts">
  // A bare header icon, as magpie's: a soft rounded fill under the pointer that deepens while pressed, and a short hint
  // drawn by the panel below it (the system tooltip only shows once the app is active). A toggle that is on (the pin)
  // keeps a soft fill, like a chosen tab. Each icon sits by its centre as the eye sees it.
  import { hoverHint } from '../lib/ui.svelte';
  import Icon from './Icon.svelte';

  let { icon, hint = '', label, onclick, on = false, tint = 'var(--label)', width = 30, size = 15 }: {
    icon: string; hint?: string; label: string; onclick: (event: MouseEvent) => void;
    on?: boolean; tint?: string; width?: number; size?: number;
  } = $props();

  let button: HTMLButtonElement | undefined = $state();

</script>

<button bind:this={button} class="hint-button pressable" class:on style:width="{width}px" aria-label={label} aria-pressed={on || undefined}
  onpointerenter={() => hoverHint(button ?? null, hint)} onpointerleave={() => hoverHint(null, '')}
  onpointerdown={() => hoverHint(null, '')} onclick={onclick}>
  <span class="glyph" style:color={tint}><Icon name={icon} {size} eye /></span>
</button>

<style>
  .hint-button {
    --press-scale: 0.88;
    position: relative;
    height: 28px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 8px;
  }
  .hint-button:hover { background: var(--hover); }
  .hint-button.on { background: color-mix(in srgb, var(--label) 11%, transparent); }
  .hint-button:active { background: var(--press); }
  .glyph { display: block; }
</style>
