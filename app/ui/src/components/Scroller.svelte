<script lang="ts">
  // A scroll area with a thin knob at its edge shown only while scrolling (no track, and never the wide always-on bar),
  // no scrolling at all while the content fits, and a two-finger swipe to the right to go back.
  import type { Snippet } from 'svelte';

  let { children, onswipeback, onheight, scrollTop = $bindable(0) }:
    { children: Snippet; onswipeback?: () => void; onheight?: (height: number) => void; scrollTop?: number } = $props();

  let viewport: HTMLDivElement | undefined = $state();
  let content: HTMLDivElement | undefined = $state();
  let viewHeight = $state(0), contentHeight = $state(0), top = $state(0), scrolling = $state(false);
  let hide: ReturnType<typeof setTimeout> | undefined;
  const scrollable = $derived(contentHeight > viewHeight + 0.5);

  $effect(() => {
    if (!content || !viewport) return;
    // Read in the observer, applied on the next frame: the panel's new size would otherwise resize what is observed
    // within the same notification.
    let frame = 0;
    const observer = new ResizeObserver(() => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => {
        if (!content || !viewport) return;
        contentHeight = content.offsetHeight;
        viewHeight = viewport.clientHeight;
        onheight?.(contentHeight);
      });
    });
    observer.observe(content);
    observer.observe(viewport);
    return () => { cancelAnimationFrame(frame); observer.disconnect(); };
  });

  // Restoring a position (coming back to the homepage) once the content is there to scroll.
  $effect(() => {
    if (viewport && contentHeight && Math.abs(viewport.scrollTop - scrollTop) > 1) viewport.scrollTop = scrollTop;
  });

  function scrolled() {
    if (!viewport) return;
    top = viewport.scrollTop;
    scrollTop = top;
    scrolling = true;
    clearTimeout(hide);
    hide = setTimeout(() => (scrolling = false), 900);
  }

  let sideways = 0, swiped = false, quiet: ReturnType<typeof setTimeout> | undefined;
  function wheel(event: WheelEvent) {
    if (onswipeback && Math.abs(event.deltaX) > Math.abs(event.deltaY)) {
      clearTimeout(quiet);
      quiet = setTimeout(() => { sideways = 0; swiped = false; }, 250);
      sideways += event.deltaX;
      if (!swiped && sideways < -60) { swiped = true; onswipeback(); }
      event.preventDefault();
      return;
    }
    if (!scrollable) event.preventDefault();
  }

  const knob = $derived.by(() => {
    if (!scrollable) return null;
    const height = Math.max(24, (viewHeight * viewHeight) / contentHeight);
    return { top: (top / (contentHeight - viewHeight)) * (viewHeight - height), height };
  });
</script>

<div class="scroller">
  <div class="viewport" class:scrollable bind:this={viewport} onscroll={scrolled} onwheel={wheel}>
    <div bind:this={content}>{@render children()}</div>
  </div>
  {#if knob}
    <span class="knob" class:shown={scrolling} style:top="{knob.top + 2}px" style:height="{Math.max(0, knob.height - 4)}px"></span>
  {/if}
</div>

<style>
  .scroller {
    position: relative;
    flex: 1;
    min-height: 0;
  }
  .viewport {
    position: absolute;
    inset: 0;
    overflow: hidden;
    overscroll-behavior: none;
    scrollbar-width: none;
  }
  .viewport.scrollable { overflow-y: auto; }
  .viewport::-webkit-scrollbar { display: none; }
  .knob {
    position: absolute;
    right: 0.5px;
    width: 3px;
    border-radius: 1.5px;
    background: var(--knob);
    opacity: 0;
    transition: opacity 0.3s;
    pointer-events: none;
  }
  .knob.shown { opacity: 1; transition-duration: 0.1s; }
</style>
