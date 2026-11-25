<script lang="ts">
  // 网页版's choices in a row (the range, the grouping): one tap picks, and a lit pill slides from the
  // old choice to the new, so the eye follows where it went. A row wider than the screen scrolls sideways and brings
  // the choice into view. `busy`: the choice is picked but its figures are still on their way; a light runs across its
  // pill until they come. The arrow keys move to the next choice and pick it, as the panel's grouping tabs do.
  import { panel } from '../lib/ui.svelte';

  let { options, chosen, label, onpick, busy = false, scroll = false, small = false, bare = false }: {
    options: { value: string; label: string }[]; chosen: string; label: string; onpick: (value: string) => void;
    busy?: boolean; scroll?: boolean; small?: boolean;
    /** No tray under the row: the choices stand on the sky, only the lit one on its pill. */
    bare?: boolean;
  } = $props();

  let row: HTMLDivElement | undefined = $state();
  let pill = $state<{ x: number; w: number } | null>(null);
  /** The first placing of the pill doesn't slide (it has nowhere to slide from). */
  let placed = $state(false);
  function place() {
    const button = row?.querySelector<HTMLButtonElement>(`[data-value="${CSS.escape(chosen)}"]`);
    if (!button) { pill = null; return; }
    pill = { x: button.offsetLeft, w: button.offsetWidth };
  }
  $effect(() => {
    void chosen; void options; void row;
    place();
    const frame = requestAnimationFrame(() => (placed = true));
    const button = row?.querySelector<HTMLButtonElement>(`[data-value="${CSS.escape(chosen)}"]`);
    if (scroll && button && row) {
      const left = button.offsetLeft - 16, right = button.offsetLeft + button.offsetWidth + 16;
      if (left < row.scrollLeft) row.scrollTo({ left, behavior: panel.reduceMotion || !placed ? 'auto' : 'smooth' });
      else if (right > row.scrollLeft + row.clientWidth) row.scrollTo({ left: right - row.clientWidth, behavior: panel.reduceMotion || !placed ? 'auto' : 'smooth' });
    }
    return () => cancelAnimationFrame(frame);
  });
  // Laid out again when the row changes size (a turn of the phone, the fonts arriving).
  $effect(() => {
    if (!row) return;
    const observer = new ResizeObserver(() => place());
    observer.observe(row);
    return () => observer.disconnect();
  });

  function keydown(event: KeyboardEvent) {
    const step = event.key === 'ArrowRight' || event.key === 'ArrowDown' ? 1 : event.key === 'ArrowLeft' || event.key === 'ArrowUp' ? -1 : 0;
    const at = options.findIndex((o) => o.value === chosen);
    let next = step ? at + step : event.key === 'Home' ? 0 : event.key === 'End' ? options.length - 1 : null;
    if (next === null) return;
    event.preventDefault();
    next = Math.max(0, Math.min(options.length - 1, next));
    if (next === at) return;
    onpick(options[next].value);
    requestAnimationFrame(() => row?.querySelector<HTMLButtonElement>(`[data-value="${CSS.escape(options[next].value)}"]`)?.focus());
  }
</script>

<div class="pills" class:sideways={scroll} class:small class:bare bind:this={row} role="radiogroup" aria-label={label} tabindex="-1" onkeydown={keydown}>
  {#if pill}
    <span class="pill" class:placed class:busy style:transform="translateX({pill.x}px)" style:width="{pill.w}px" aria-hidden="true"></span>
  {/if}
  {#each options as option (option.value)}
    <button class="choice" class:on={option.value === chosen} data-value={option.value} role="radio" aria-checked={option.value === chosen}
      tabindex={option.value === chosen ? 0 : -1} onclick={() => { if (option.value !== chosen) onpick(option.value); }}>{option.label}</button>
  {/each}
</div>

<style>
  .pills {
    position: relative;
    display: flex;
    gap: 2px;
    padding: 3px;
    border-radius: 999px;
    background: var(--glass);
    box-shadow: inset 0 0 0 0.5px var(--glass-edge);
    width: max-content;
    max-width: 100%;
    box-sizing: border-box;
  }
  .pills.bare { background: none; box-shadow: none; border-radius: 0; }
  .pills.sideways {
    overflow-x: auto;
    scrollbar-width: none;
    -webkit-overflow-scrolling: touch;
  }
  .pills.sideways::-webkit-scrollbar { display: none; }
  .pill {
    position: absolute;
    left: 0;
    top: 3px;
    bottom: 3px;
    border-radius: 999px;
    background: var(--pill);
    box-shadow: 0 1px 8px var(--pill-glow), inset 0 0 0 0.5px var(--glass-edge);
    overflow: hidden;
  }
  .pill.placed { transition: transform 0.42s cubic-bezier(0.22, 1, 0.36, 1), width 0.42s cubic-bezier(0.22, 1, 0.36, 1); }
  /* Picked, still being read: a light runs across it until the figures come. */
  .pill.busy::after {
    content: '';
    position: absolute;
    inset: 0;
    background: linear-gradient(100deg, transparent 20%, var(--shine) 50%, transparent 80%);
    transform: translateX(-100%);
    animation: shine 1.1s ease-in-out infinite;
  }
  @keyframes shine { to { transform: translateX(100%); } }
  .choice {
    position: relative;
    flex: none;
    height: 32px;
    padding: 0 14px;
    border-radius: 999px;
    font-size: 13.5px;
    font-weight: 500;
    color: var(--ink-2);
    white-space: nowrap;
    transition: color 0.25s;
    -webkit-tap-highlight-color: transparent;
  }
  .small .choice { height: 28px; padding: 0 12px; font-size: 12.5px; }
  .choice.on { color: var(--ink); font-weight: 600; }
  :global(:root.keyboard) .choice:focus-visible { outline: 2px solid var(--star); outline-offset: -2px; }
</style>
