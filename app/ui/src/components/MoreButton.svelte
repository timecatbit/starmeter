<script lang="ts">
  // “查看更多 N 个…”: the rest of a list that shows its first five (sessions, models, projects), one click away; the
  // page then scrolls to them. Once the list is whole, the same button at its end says 收起 (its chevron up) and folds
  // it back to five, staying where it was under the pointer instead of the page jumping (用户：展开之后就是不能再折叠回去了).
  import { tick } from 'svelte';

  let { label, open = false, onclick }: { label: string; open?: boolean; onclick: () => void } = $props();
  let button: HTMLButtonElement | undefined = $state();

  async function press() {
    const folding = open;
    const before = button?.getBoundingClientRect().top ?? 0;
    onclick();
    if (!folding || !button) return;
    await tick();
    const viewport = button.closest<HTMLElement>('.viewport');
    if (viewport) viewport.scrollTop += button.getBoundingClientRect().top - before;
  }
</script>

<button class="more pressable" class:open bind:this={button} aria-expanded={open} onclick={press}>
  <span>{label}</span>
  <svg width="10" height="7" viewBox="0 0 10 7" aria-hidden="true"><path d="M1 1.2 5 5.2 9 1.2" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" /></svg>
</button>

<style>
  .more {
    --press-scale: 0.985;
    width: calc(100% - 8px);
    margin: 0 4px 6px;
    height: 28px;
    /* The text on the cards' logo column: 4 + 8 = 12. */
    padding: 0 8px;
    box-sizing: border-box;
    display: flex;
    align-items: center;
    gap: 6px;
    border-radius: 8px;
    font-size: 12px;
    font-weight: 500;
    color: var(--accent);
  }
  .more.open svg { transform: rotate(180deg); }
  .more:hover { background: var(--hover); }
  .more:active { background: var(--press); }
</style>
