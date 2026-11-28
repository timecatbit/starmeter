<script lang="ts">
  // 实时, the homepage's fifth tab (beside 会话): what the AI is doing now, as 网页版 has it (phoneLive.svelte.ts, from
  // the app's live_now: today by tool, 会话's sessions, each turn's start). 正在运行: each session at work in two lines,
  // its tool's mark, title and this turn's clock going second by second, then its model, this turn's tokens and cost;
  // 动态: what happened, newest first, each sliding in at the top. Read every couple of seconds while it is on screen,
  // pinned or not (the user looks at it; SwiftBar's figures come at its plugin's pace). Pinning turns to it.
  import { flip } from 'svelte/animate';
  import { fly } from 'svelte/transition';
  import { toolColour } from '../lib/colors';
  import { t } from '../lib/i18n.svelte';
  import { LIVE_EVENTS_SHOWN, liveClock, liveSessionView, liveTurnText } from '../lib/liveView';
  import { PhoneLive, turnClock } from '../lib/phoneLive.svelte';
  import { panel } from '../lib/ui.svelte';
  import Glyph from './Glyph.svelte';

  let { interval = 2, onrunning }: { interval?: number; onrunning?: (tools: string[], started: Record<string, number>) => void } = $props();

  const live = new PhoneLive();
  let now = $state(Date.now());
  $effect(() => { live.setIntervalSec(interval); });
  // Read only while the panel is on screen (STAGE-11).
  $effect(() => {
    if (!panel.shown) return;
    live.start();
    return () => live.stop();
  });

  const view = $derived(liveSessionView(live.sessions, live.answered));
  const running = $derived(view.running);
  $effect(() => {
    const tools: string[] = [];
    const started: Record<string, number> = {};
    for (const s of running) {
      if (!s.tool) continue;
      if (!tools.includes(s.tool)) tools.push(s.tool);
      if (s.began !== null) started[s.tool] = started[s.tool] !== undefined ? Math.min(started[s.tool], s.began) : s.began;
    }
    onrunning?.(tools, started);
  });
  // Only run the 1s clock interval while at least one session is actively running (PERF-4).
  $effect(() => {
    if (!panel.shown || !running.length) return;
    now = Date.now();
    const ticking = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(ticking);
  });
  const shown = $derived(view.shown);
  const events = $derived(live.events.slice(0, LIVE_EVENTS_SHOWN));
  /** This turn's clock as 网页版 writes it (turnClock): figures in the rounded face, words in the text's. */
  const timer = (began: number | null, exact: boolean) => turnClock(began, exact, now);
  const motion = (ms: number) => (panel.reduceMotion ? 0 : ms);
  const tint = $derived(panel.dark ? 'rgb(235, 238, 255)' : 'rgb(30, 40, 80)');
</script>

<div class="panel-live">
  <div class="head"><span>{t(view.heading)}</span>{#if running.length}<span class="count num">{running.length}</span>{/if}</div>
  {#each live.answered ? shown : [] as s (s.key)}
    {@const spent = liveTurnText(s)}
    <div class="live-session" class:ended={!s.running} in:fly={{ y: 6, duration: motion(240) }}>
      <div class="top">
        <span class="mark"><Glyph image={s.image} template={s.template} symbol={s.symbol ?? 'terminal'} size={12} {tint} /></span>
        <span class="title">{s.title}</span>
        {#if s.running}
          <span class="clock" title={t('本轮用时')}>{#each timer(s.began, s.exact) as piece, i (i)}<span class={piece.figure ? 'clock-figure num' : 'clock-words'}>{piece.text}</span>{/each}</span>
        {:else if s.idle}
          <span class="idle">{s.idle}</span>
        {/if}
      </div>
      <div class="about">
        {#if s.model}<span class="model">{s.model}</span>{/if}
        {#if s.context !== null}<span class="context" title={t('上下文 {n}%', { n: Math.round(s.context * 100) })}><span class="meter" aria-hidden="true"><span style:width="{Math.round(s.context * 100)}%"></span></span><span
            class="share num">{Math.round(s.context * 100)}%</span></span>{/if}
        {#if spent}<span class="spent num">{spent}</span>{/if}
      </div>
    </div>
  {/each}
  {#if !live.answered || !shown.length}
    <p class="quiet">{!live.answered ? t('正在读取…') : t('没有在运行的会话')}</p>
  {/if}

  <div class="head feed-head"><span>{t('动态')}</span></div>
  {#if events.length}
    <!-- data-moving: a new row comes in every couple of seconds and moves the rest (textFits.ts knows). -->
    <ol class="feed" data-moving>
      {#each events as e (e.id)}
        <li class="event {e.kind}" in:fly={{ y: -8, duration: motion(260) }} animate:flip={{ duration: motion(220) }}>
          <span class="when num">{liveClock(e.at)}</span>
          {#if e.kind === 'done'}
            <!-- A turn done: a tick in a green disc, as 网页版 marks it. -->
            <svg class="sign" width="10" height="10" viewBox="0 0 12 12" aria-hidden="true"><circle cx="6" cy="6" r="5.5" fill="var(--green)" /><path d="M3.4 6.1 5.2 7.9 8.6 4.3" fill="none" stroke="white" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg>
          {:else}
            <span class="dot" style:background={e.tool ? toolColour(e.tool, panel.dark) : e.kind === 'wake' ? 'var(--green)' : e.kind === 'rest' ? 'var(--tertiary)' : 'var(--accent)'}></span>
          {/if}
          <span class="who">{e.who}</span>
          <span class="what num">{e.what}</span>
        </li>
      {/each}
    </ol>
  {:else}
    <p class="quiet">{!live.answered ? t('正在读取…') : live.state === 'working' ? t('等下一笔请求…') : t('一有动静就会出现在这里')}</p>
  {/if}
</div>

<style>
  .panel-live { padding: 0 12px 6px; display: flex; flex-direction: column; font-size: 11px; color: var(--label); }
  .head { display: flex; align-items: center; gap: 6px; height: 22px; font-weight: 600; color: var(--secondary); }
  .feed-head { margin-top: 6px; }
  .count { min-width: 15px; height: 15px; padding: 0 4px; box-sizing: border-box; border-radius: 8px; font-size: 10px; font-weight: 600; line-height: 15px; text-align: center; color: white; background: var(--green); }
  .live-session { padding: 4px 0 5px; border-bottom: 0.5px solid var(--separator); }
  .live-session:last-of-type { border-bottom: 0; }
  /* The session last at work while nothing runs: the same row, greyed, lighting up in its place when work comes. */
  .live-session.ended { opacity: 0.6; }
  .idle { flex: none; font-size: 10px; color: var(--tertiary); }
  .top { display: flex; align-items: center; gap: 6px; min-width: 0; height: 18px; }
  .mark { flex: none; width: 16px; height: 16px; border-radius: 5px; display: flex; align-items: center; justify-content: center; background: var(--hover); }
  .title { flex: 1; min-width: 0; font-size: 12px; font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .clock { flex: none; display: inline-flex; align-items: baseline; gap: 2px; color: var(--green); }
  .clock-figure { font-family: var(--rounded); font-size: 12px; font-weight: 600; }
  .clock-words { font-size: 10px; font-weight: 500; }
  .about { display: flex; gap: 8px; min-width: 0; padding-left: 22px; height: 16px; line-height: 16px; color: var(--secondary); white-space: nowrap; }
  .model { flex: 0 1 auto; min-width: 0; overflow: hidden; text-overflow: ellipsis; }
  .spent { flex: none; color: var(--label); }
  .context { flex: none; display: inline-flex; align-items: center; gap: 4px; }
  .meter { width: 22px; height: 3px; border-radius: 1.5px; background: var(--separator); overflow: hidden; }
  .meter span { display: block; height: 100%; border-radius: 1.5px; background: var(--accent); }
  .share { font-family: var(--rounded); font-size: 10px; font-weight: 600; }
  .sign { flex: none; }
  .quiet { margin: 2px 0 4px; line-height: 18px; color: var(--secondary); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .feed { list-style: none; margin: 0; padding: 0; }
  .event { display: flex; align-items: center; gap: 6px; height: 20px; min-width: 0; color: var(--secondary); }
  .when { flex: none; font-size: 10px; color: var(--tertiary); }
  .dot { flex: none; width: 6px; height: 6px; border-radius: 50%; }
  .who { flex: none; max-width: 45%; font-weight: 600; color: var(--label); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .what { min-width: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .event.done .what, .event.wake .what { color: var(--green); font-weight: 600; }
  .event.rest .what { color: var(--tertiary); }
</style>
