<script lang="ts">
  // Pinned, under the live curve: what the AI is doing now, as 网页版 has it (phoneLive.svelte.ts, from the app's
  // live_now: today by tool, 会话's sessions, each turn's start). The sessions running a line each: the tool's mark, the
  // title, this turn's tokens and cost, its clock going second by second. Under them 动态, the latest of what happened
  // on one line; 动态 at the right opens the last six (and is remembered). Nothing running: the session last at work.
  // Compact, the curve above it untouched: the panel stays a panel.
  import { onMount } from 'svelte';
  import { toolColour } from '../lib/colors';
  import { t } from '../lib/i18n.svelte';
  import { duration, PhoneLive } from '../lib/phoneLive.svelte';
  import { panel } from '../lib/ui.svelte';
  import Glyph from './Glyph.svelte';

  let { onheight }: { onheight: (height: number) => void } = $props();

  const KEY = 'starmeter.pinned-feed';
  const live = new PhoneLive();
  let now = $state(Date.now());
  let open = $state(typeof localStorage !== 'undefined' && localStorage.getItem(KEY) === 'open');
  onMount(() => {
    live.start();
    const ticking = setInterval(() => (now = Date.now()), 1000);
    return () => { live.stop(); clearInterval(ticking); };
  });
  function toggle() {
    open = !open;
    try { localStorage.setItem(KEY, open ? 'open' : 'closed'); } catch { /* this panel only */ }
  }

  /** Its height, taken a frame after it changes (set in the observer's own callback it would move what other observers
   *  watch in the same frame), for the dashboard to make room. */
  let box: HTMLDivElement | undefined = $state();

  const SHOWN = 3;
  const running = $derived(live.sessions.filter((s) => s.running));
  const latest = $derived(live.sessions.find((s) => !s.running) ?? null);
  const events = $derived(live.events.slice(0, open ? 6 : 1));
  const pad = (n: number) => String(n).padStart(2, '0');
  const clock = (at: number) => { const d = new Date(at); return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`; };
  /** This turn's clock: m:ss when the computer gave its start to the second, else its minutes. */
  function timer(began: number | null, exact: boolean): string {
    if (began === null) return '';
    const seconds = Math.max(0, Math.floor((now - began) / 1000));
    if (!exact) return duration(seconds * 1000, false);
    const h = Math.floor(seconds / 3600), m = Math.floor((seconds % 3600) / 60), s = seconds % 60;
    return h ? `${h}:${pad(m)}:${pad(s)}` : `${m}:${pad(s)}`;
  }
  const tint = $derived(panel.dark ? 'rgb(235, 238, 255)' : 'rgb(30, 40, 80)');
  /** Anything to say: a session (running or last at work), or something that happened. */
  const any = $derived(running.length > 0 || !!latest || live.events.length > 0);
  $effect(() => {
    const element = box;
    if (!element) { onheight(0); return; }
    let frame = 0;
    const observer = new ResizeObserver(() => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => onheight(element.offsetHeight));
    });
    observer.observe(element);
    onheight(element.offsetHeight);
    return () => { observer.disconnect(); cancelAnimationFrame(frame); };
  });
</script>

{#if any}
  <div class="pinned-now" bind:this={box}>
    <div class="head">
      <span class="label">{t('正在运行')}</span>
      {#if running.length}<span class="count num">{running.length}</span>{/if}
      <span class="space"></span>
      {#if live.events.length > 1}
        <button class="toggle" aria-expanded={open} onclick={toggle}>{t('动态')}<svg width="8" height="8" viewBox="0 0 8 8" aria-hidden="true"><path d="M1.5 2.75 4 5.25 6.5 2.75" /></svg></button>
      {/if}
    </div>
    {#each running.slice(0, SHOWN) as s (s.key)}
      <div class="pinned-session">
        <span class="mark"><Glyph image={s.image} template={s.template} symbol={s.symbol ?? 'terminal'} size={11} {tint} /></span>
        <span class="title">{s.title}</span>
        {#if s.turnTokens}<span class="spent num">{s.turnTokens}{s.turnCost ? ' · ' + s.turnCost : ''}</span>{/if}
        <span class="clock num">{timer(s.began, s.exact)}</span>
      </div>
    {/each}
    {#if running.length > SHOWN}<div class="more">{t('还有 {n} 个会话在运行', { n: running.length - SHOWN })}</div>{/if}
    {#if !running.length && latest}
      <div class="quiet">{t('没有在运行的会话 · 最近：{title}{idle}', { title: latest.title, idle: latest.idle ? ' · ' + latest.idle : '' })}</div>
    {/if}
    {#each events as e (e.id)}
      <div class="pinned-event {e.kind}">
        <span class="when num">{clock(e.at)}</span>
        <span class="dot" style:background={e.tool ? toolColour(e.tool, panel.dark) : e.kind === 'wake' || e.kind === 'done' ? 'var(--green)' : 'var(--accent)'}></span>
        <span class="who">{e.who}</span>
        <span class="what num">{e.what}</span>
      </div>
    {/each}
  </div>
{/if}

<style>
  .pinned-now { padding: 2px 12px 6px; display: flex; flex-direction: column; gap: 1px; font-size: 11px; color: var(--label); }
  .head { display: flex; align-items: center; gap: 6px; height: 20px; }
  .label { font-weight: 600; color: var(--secondary); }
  .count { min-width: 15px; height: 15px; padding: 0 4px; box-sizing: border-box; border-radius: 8px; font-size: 10px; font-weight: 600; line-height: 15px; text-align: center; color: white; background: var(--green); }
  .space { flex: 1; }
  .toggle { display: inline-flex; align-items: center; gap: 3px; height: 18px; padding: 0 6px; border-radius: 6px; font-size: 11px; color: var(--secondary); }
  .toggle:hover { background: var(--hover); }
  .toggle svg { fill: none; stroke: currentColor; stroke-width: 1.3; stroke-linecap: round; stroke-linejoin: round; transform: rotate(-90deg); transition: transform 0.18s ease-out; }
  .toggle[aria-expanded='true'] svg { transform: none; }
  .pinned-session, .pinned-event { display: flex; align-items: center; gap: 6px; height: 20px; min-width: 0; }
  .mark { flex: none; width: 16px; height: 16px; border-radius: 5px; display: flex; align-items: center; justify-content: center; background: var(--hover); }
  .title { flex: 1; min-width: 0; font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .spent { flex: none; color: var(--secondary); }
  .clock { flex: none; min-width: 30px; text-align: right; font-family: var(--rounded); font-weight: 600; color: var(--green); }
  .more, .quiet { height: 20px; line-height: 20px; color: var(--secondary); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .pinned-event { color: var(--secondary); }
  .when { flex: none; font-size: 10px; color: var(--tertiary); }
  .dot { flex: none; width: 6px; height: 6px; border-radius: 50%; }
  .who { flex: none; max-width: 45%; font-weight: 600; color: var(--label); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .what { min-width: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .pinned-event.done .what, .pinned-event.wake .what { color: var(--green); font-weight: 600; }
</style>
