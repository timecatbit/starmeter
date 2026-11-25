<script lang="ts">
  // A choice from a short list, drawn by the panel rather than the system: bare until pointed at, it opens a small menu
  // right under itself (a check by the current choice, a soft highlight under the pointer), follows the arrow keys,
  // Return and Esc, and closes on a click elsewhere. The system's own menu covered its button and looked foreign here.
  import { onDestroy } from 'svelte';
  import { pageCurve } from '../lib/motion';
  import { panel } from '../lib/ui.svelte';

  let { options, value, label, size = 11, align = 'right', field = false, onpick, onopen }: {
    options: { value: string; label: string }[]; value: string; label: string; size?: number; align?: 'left' | 'right';
    /** Drawn as a full-width field (a soft fill, the chevron at its right end) where it sits among other controls. */
    field?: boolean; onpick: (value: string) => void;
    /** The menu opening: the range menu has its ranges read ahead while the user looks it over. */
    onopen?: () => void;
  } = $props();

  let open = $state(false);
  let lit = $state(0);
  /** The options' ids: the list tells assistive technology which one the arrows light (aria-activedescendant), the
   *  focus staying on the list so Return still picks it. */
  const uid = $props.id();
  let root: HTMLSpanElement | undefined = $state();
  let trigger: HTMLButtonElement | undefined = $state();
  let menu: HTMLDivElement | undefined = $state();
  /** As tall as the window leaves below the button; the list scrolls inside past that. */
  let room = $state(0);
  const current = $derived(options.find((o) => o.value === value)?.label ?? '');

  function outside(event: PointerEvent) {
    if (root && !root.contains(event.target as Node)) close();
  }
  function show() {
    // Opened again before the menu that went away was taken out, the same menu comes back: shown again.
    quick = false;
    menu?.style.removeProperty('display');
    open = true;
    panel.menuOpen = true;
    onopen?.();
    lit = Math.max(0, options.findIndex((o) => o.value === value));
    // The bottom of what scrolls around it (else the panel's): past it, the menu would make the page scroll to it.
    const bottom = (root?.closest('.viewport') ?? root?.closest('.window'))?.getBoundingClientRect().bottom ?? window.innerHeight;
    room = root ? Math.max(96, bottom - root.getBoundingClientRect().bottom - 10) : 0;
    window.addEventListener('pointerdown', outside, true);
    // The panel going away (a click on the desktop hides it) closes the menu too, as a system menu would: opened again,
    // the panel shows the range as it was, folded, and refreshes again.
    window.addEventListener('blur', away);
    // Focusing would scroll the page to show the whole menu, jolting it up under the pointer.
    queueMicrotask(() => { menu?.focus({ preventScroll: true }); reveal(); });
  }
  function close() {
    if (!open) return;
    open = false;
    panel.menuOpen = false;
    window.removeEventListener('pointerdown', outside, true);
    window.removeEventListener('blur', away);
  }
  /** The panel going away (hidden, or the host letting go of the focus as it closes): the menu goes at once, with no
   *  fade. A hidden window stops the fade halfway, and the rest of it played as the panel opened again, a flash of
   *  the menu over the homepage. */
  function away() {
    if (!open) return;
    quick = true;
    menu?.style.setProperty('display', 'none');
    close();
  }
  /** The next fold has no fade (the panel is going away). */
  let quick = false;
  /** Keeps the lit option in view inside the menu, never scrolling the page. */
  function reveal() {
    const option = menu?.children[lit] as HTMLElement | undefined;
    if (!menu || !option) return;
    if (option.offsetTop < menu.scrollTop) menu.scrollTop = option.offsetTop - 4;
    else if (option.offsetTop + option.offsetHeight > menu.scrollTop + menu.clientHeight) menu.scrollTop = option.offsetTop + option.offsetHeight - menu.clientHeight + 4;
  }
  function pick(choice: string) {
    close();
    if (choice !== value) onpick(choice);
  }
  /** Esc or Return closes the menu with the focus back on its button, so Return opens it again; left on the menu as it
   *  went away, the focus fell to the page. Leaving by Tab, a click elsewhere or the window going keeps its own way. */
  function settle(choice?: string) {
    if (choice === undefined) close();
    else pick(choice);
    trigger?.focus({ preventScroll: true });
  }
  function keydown(event: KeyboardEvent) {
    if (!open) return;
    // The panel's own Esc (back) and Return stay out of it while the menu is open.
    event.stopPropagation();
    if (event.key === 'Escape') { event.preventDefault(); settle(); }
    // Tab or Shift+Tab leaves the menu: it closes with the choice as it was, and the focus goes on its way.
    else if (event.key === 'Tab') close();
    // A menu with nothing in it has nothing to light or pick.
    else if (!options.length) return;
    else if (event.key === 'ArrowDown') { event.preventDefault(); lit = (lit + 1) % options.length; reveal(); }
    else if (event.key === 'ArrowUp') { event.preventDefault(); lit = (lit - 1 + options.length) % options.length; reveal(); }
    else if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); settle(options[lit].value); }
    // The ends of the list (and its pages) are the menu's, not the page's: left to the browser, they scrolled what is
    // around the menu, and the menu with it, out of sight.
    else if (event.key === 'Home' || event.key === 'PageUp') { event.preventDefault(); lit = 0; reveal(); }
    else if (event.key === 'End' || event.key === 'PageDown') { event.preventDefault(); lit = options.length - 1; reveal(); }
  }
  onDestroy(close);

  const drop = (_node: Element) => ({
    duration: panel.reduceMotion || quick ? 0 : 140, easing: pageCurve,
    css: (t: number) => `opacity: ${t}; transform: translateY(${-4 * (1 - t)}px) scale(${0.97 + 0.03 * t})`,
  });
</script>

<!-- svelte-ignore a11y_no_static_element_interactions (the keys belong to the open menu) -->
<!-- The focus leaving closes the menu. Pressing its button while it is open keeps the focus where it is: WebKit let go
     of the focus on the press, so the menu closed there and the click opened it again. -->
<span class="dropdown" class:field bind:this={root} onkeydown={keydown}
  onfocusout={(event) => {
    if (!open || !root || root.contains(event.relatedTarget as Node | null)) return;
    // The focus going nowhere: the window let go of it (the panel hidden or closing). Tab and a click elsewhere in the
    // panel hand it on, or fold the menu before it moves.
    if (event.relatedTarget === null) away();
    else close();
  }}>
  <button class="trigger pressable" class:open bind:this={trigger} style:font-size="{size}px" aria-haspopup="listbox" aria-expanded={open}
    aria-label="{label} · {current}" onclick={() => (open ? close() : show())}
    onpointerdown={(event) => { if (open) event.preventDefault(); }}>
    <span class="current">{current}</span>
    <svg class="chevron" width="7" height="5" viewBox="0 0 7 5" aria-hidden="true"><path d="M1 1.1 3.5 3.6 6 1.1" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg>
  </button>
  {#if open}
    <div class="menu" class:left={align === 'left'} bind:this={menu} role="listbox" tabindex="-1" aria-label={label} style:max-height="{room}px"
      aria-activedescendant="{uid}-{lit}"
      onwheel={(event) => event.stopPropagation()} transition:drop>
      {#each options as option, i (option.value)}
        <div class="option" id="{uid}-{i}" class:chosen={option.value === value} class:lit={i === lit} role="option" tabindex="-1" aria-selected={option.value === value}
          onpointerenter={() => (lit = i)} onpointerdown={(event) => event.preventDefault()} onclick={() => pick(option.value)} onkeydown={() => {}}>
          {#if option.value === value}
            <svg class="check" width="10" height="8" viewBox="0 0 10 8" aria-hidden="true"><path d="M1 4.2 3.7 6.8 9 1.2" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" /></svg>
          {/if}
          {option.label}
        </div>
      {/each}
    </div>
  {/if}
</span>

<style>
  .dropdown { position: relative; display: inline-flex; }
  .dropdown.field { display: flex; width: 100%; }
  .field .trigger {
    width: 100%;
    justify-content: space-between;
    padding: 0 10px;
    border-radius: 8px;
    background: color-mix(in srgb, var(--label) 6%, transparent);
    font-weight: 600;
  }
  .field .trigger:hover { background: color-mix(in srgb, var(--label) 9%, transparent); }
  .field .trigger.open, .field .trigger:active { background: color-mix(in srgb, var(--label) 12%, transparent); }
  .dropdown.field .menu { left: 0; right: 0; min-width: 0; }
  .trigger {
    --press-scale: 0.96;
    height: 28px;
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 0 8px;
    border-radius: 7px;
    color: var(--label);
    white-space: nowrap;
  }
  .trigger:hover { background: var(--hover); }
  .trigger.open, .trigger:active { background: var(--press); }
  .chevron { flex: none; color: var(--secondary); transition: transform 0.18s var(--page-curve); }
  .trigger.open .chevron { transform: rotate(180deg); }
  .menu {
    position: absolute;
    top: calc(100% + 4px);
    right: 0;
    z-index: 30;
    min-width: 116px;
    padding: 4px;
    border-radius: 10px;
    background: var(--solid);
    box-shadow: 0 0 0 0.5px var(--separator), 0 8px 24px rgba(0, 0, 0, 0.16);
    outline: none;
    box-sizing: border-box;
    overflow-y: auto;
    overscroll-behavior: contain;
    scrollbar-width: none;
    transform-origin: top right;
  }
  .menu::-webkit-scrollbar { display: none; }
  .menu.left { right: auto; left: 0; transform-origin: top left; }
  .option {
    position: relative;
    height: 26px;
    display: flex;
    align-items: center;
    padding: 0 12px 0 26px;
    border-radius: 6px;
    font-size: 12px;
    white-space: nowrap;
    cursor: default;
  }
  .option.lit { background: var(--hover); }
  .option.chosen { font-weight: 600; }
  .check { position: absolute; left: 9px; color: var(--accent); }
</style>
