<script lang="ts">
  // 实时, the homepage's fifth tab (beside 会话): what the AI is doing now, as 网页版 has it (phoneLive.svelte.ts, from
  // the app's live_now: today by tool, 会话's sessions, each turn's start). 正在运行: each session at work in two lines,
  // its tool's mark, title and this turn's clock going second by second, then its model, this turn's tokens and cost;
  // 动态: what happened, newest first, each sliding in at the top. Read every couple of seconds while it is on screen,
  // pinned or not (the user looks at it; SwiftBar's figures come at its plugin's pace). Pinning turns to it.
  import { flip } from 'svelte/animate';
  import { fly } from 'svelte/transition';
  import { toolColour } from '../lib/colors';
  import { closeList, listOpen, openList } from '../lib/expanded.svelte';
  import { t } from '../lib/i18n.svelte';
  import { PhoneLive, turnClock } from '../lib/phoneLive.svelte';
  import { panel } from '../lib/ui.svelte';
  import Glyph from './Glyph.svelte';
  import MoreButton from './MoreButton.svelte';

  const live = new PhoneLive();
  let now = $state(Date.now());
  // Read and ticking only while the panel is on screen (STAGE-11: closed, it went on asking every couple of seconds and
  // counting the seconds in a hidden window). Back on screen, the minutes it missed come in at once from the app's
  // last minutes (PhoneLive asks for them after being away), 动态 too.
  $effect(() => {
    if (!panel.shown) return;
    live.start();
    now = Date.now();
    const ticking = setInterval(() => (now = Date.now()), 1000);
    return () => { live.stop(); clearInterval(ticking); };
  });

  /** 动态 shows its newest five, the rest behind 查看更多 (as the sessions' and projects' lists: kept open or folded,
   *  expanded.svelte.ts); folded, what comes in keeps it at five. */
  const SHOWN = 5;
  const running = $derived(live.sessions.filter((s) => s.running));
  const latest = $derived(live.sessions.find((s) => !s.running) ?? null);
  /** The cards: the sessions running, else the one last at work, greyed (as 网页版's 正在运行). */
  const shown = $derived(running.length ? running : latest ? [latest] : []);
  const feedOpen = $derived(listOpen('feed'));
  const events = $derived(feedOpen ? live.events : live.events.slice(0, SHOWN));
  const pad = (n: number) => String(n).padStart(2, '0');
  const clock = (at: number) => { const d = new Date(at); return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`; };
  /** This turn's clock as 网页版 writes it (turnClock): figures in the rounded face, words in the text's. */
  const timer = (began: number | null, exact: boolean) => turnClock(began, exact, now);
  const motion = (ms: number) => (panel.reduceMotion ? 0 : ms);
  const tint = $derived(panel.dark ? 'rgb(235, 238, 255)' : 'rgb(30, 40, 80)');
</script>

<div class="panel-live">
  <div class="head"><span>{t('正在运行')}</span>{#if running.length}<span class="count num">{running.length}</span>{/if}</div>
  {#each live.answered ? shown : [] as s (s.key)}
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
        {#if s.turnTokens}<span class="spent num">{t(s.running ? '本轮' : '上一轮') + ' ' + s.turnTokens + (s.turnCost ? ' · ' + s.turnCost : '')}</span>{/if}
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
          <span class="when num">{clock(e.at)}</span>
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
    {#if live.events.length > SHOWN}
      <MoreButton open={feedOpen} label={feedOpen ? t('收起') : t('查看更多 {n} 条动态', { n: live.events.length - SHOWN })}
        onclick={() => (feedOpen ? closeList : openList)('feed')} />
    {/if}
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
