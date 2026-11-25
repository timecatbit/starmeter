<script lang="ts">
  // What the AI is doing, under the curve. 正在运行: each running session, its tool's mark in a ring that turns while it
  // works, the title and this turn's clock going second by second; its model and how full its context is; this turn's
  // tokens and cost, and the tools it has called (a tool called since the last reading lights for a moment: that is
  // what it is doing now). Nothing running, the session last at work and since when. 动态: what happened, newest first,
  // each sliding in at the top: a request landing (+12.3K Token from Claude), a tool called, a turn begun or done.
  import { flip } from 'svelte/animate';
  import { fly } from 'svelte/transition';
  import { toolColour } from '../lib/colors';
  import { t } from '../lib/i18n.svelte';
  import { turnClock, type PhoneLive } from '../lib/phoneLive.svelte';
  import { panel } from '../lib/ui.svelte';
  import Glyph from './Glyph.svelte';

  let { live, now, shown = 6 }: { live: PhoneLive; now: number; shown?: number } = $props();

  const running = $derived(live.sessions.filter((s) => s.running));
  const latest = $derived(live.sessions.find((s) => !s.running) ?? null);
  const events = $derived(live.events.slice(0, shown));
  const pad = (n: number) => String(n).padStart(2, '0');
  const clock = (at: number) => { const d = new Date(at); return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`; };
  /** A turn's clock (turnClock, as the panel's 实时 writes it): each figure in the figures' face, each word in the
   *  text's (4 分钟; 1 小时 23 分钟, the 23 a figure too: STAGE-19). */
  const timer = (began: number | null, exact: boolean) => turnClock(began, exact, now);
  const motion = (ms: number) => (panel.reduceMotion ? 0 : ms);
  const tint = $derived(panel.dark ? 'rgb(235, 238, 255)' : 'rgb(30, 40, 80)');
</script>

<div class="activity">
  <div class="head"><span>{t('正在运行')}</span>{#if running.length}<span class="count num">{running.length}</span>{/if}</div>
  {#if running.length}
    <div class="sessions">
      {#each running as s (s.key)}
        <div class="session" in:fly={{ y: 10, duration: motion(400) }}>
          <div class="top">
            <span class="mark"><span class="orbit" aria-hidden="true"></span><Glyph image={s.image} template={s.template} symbol={s.symbol ?? 'terminal'} size={17} {tint} /></span>
            <span class="title">{s.title}</span>
            <span class="timer" title={t('本轮用时')}>{#each timer(s.began, s.exact) as piece, i (i)}<span
                class={piece.figure ? 'timer-figure num' : 'timer-words'}>{piece.text}</span>{/each}</span>
          </div>
          {#if s.model || s.context !== null}
            <!-- The model and effort, then how full the context is: its meter and share (as the panel's 实时 has them). -->
            <div class="about">
              {#if s.model}<span class="about-text">{s.model}</span>{/if}
              {#if s.context !== null}<span class="context" title={t('上下文 {n}%', { n: Math.round(s.context * 100) })}><span class="meter" aria-hidden="true"><span style:width="{Math.round(s.context * 100)}%"></span></span><span
                  class="share num">{Math.round(s.context * 100)}%</span></span>{/if}
            </div>
          {/if}
          <div class="turn">
            {#if s.turnTokens}<span class="turn-figures num">{t('本轮') + ' ' + s.turnTokens + (s.turnCost ? ' · ' + s.turnCost : '')}</span>{/if}
            {#each s.tools.slice(0, 4) as [name, count] (name)}
              <span class="chip num" class:lit={now - (s.called[name] ?? 0) < 1800}>{name} ×{count}</span>
            {/each}
          </div>
        </div>
      {/each}
    </div>
  {:else}
    <!-- Before the first answer, the sessions are still to come, not none. -->
    <p class="quiet">{!live.answered ? t('正在读取…') : latest ? t('没有在运行的会话 · 最近：{title}{idle}', { title: latest.title, idle: latest.idle ? ' · ' + latest.idle : '' }) : t('没有在运行的会话')}</p>
  {/if}

  <div class="head feed-head"><span>{t('动态')}</span></div>
  {#if events.length}
    <!-- data-moving: a new row comes in every couple of seconds and moves the rest (textFits.ts knows). -->
    <ol class="feed" data-moving>
      {#each events as e (e.id)}
        <li class="event {e.kind}" in:fly={{ y: -12, duration: motion(380) }} animate:flip={{ duration: motion(300) }}>
          <span class="when num">{clock(e.at)}</span>
          {#if e.kind === 'done'}
            <svg class="sign" width="12" height="12" viewBox="0 0 12 12" aria-hidden="true"><circle cx="6" cy="6" r="5.5" fill="var(--good)" /><path d="M3.4 6.1 5.2 7.9 8.6 4.3" fill="none" stroke="#06281A" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg>
          {:else}
            <span class="dot" style:background={e.tool ? toolColour(e.tool, panel.dark) : e.kind === 'wake' ? 'var(--good)' : 'var(--star)'}></span>
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
  .activity { display: flex; flex-direction: column; gap: 10px; min-width: 0; }
  .head { display: flex; align-items: center; gap: 8px; font-size: var(--type-head, 13px); font-weight: 600; color: var(--ink-2); margin: 4px 2px -2px; }
  .feed-head { margin-top: 10px; }
  .count { min-width: 18px; height: 18px; padding: 0 5px; box-sizing: border-box; border-radius: 9px; font-size: 11px; line-height: 18px; text-align: center; color: #06281A; background: var(--good); }
  .sessions { display: flex; flex-direction: column; gap: 10px; }
  .session {
    padding: 12px 14px;
    border-radius: 20px;
    background: var(--glass);
    box-shadow: inset 0 0 0 0.5px var(--glass-edge), inset 0 0 0 1px color-mix(in srgb, var(--good) 22%, transparent), var(--lift);
    display: flex;
    flex-direction: column;
    gap: 7px;
  }
  /* At work: a light runs along the card's foot, again and again. */
  .session { position: relative; overflow: hidden; }
  .session::after {
    content: '';
    position: absolute;
    left: 0;
    bottom: 0;
    width: 40%;
    height: 1.5px;
    background: linear-gradient(90deg, transparent, var(--good), transparent);
    opacity: 0.55;
    transform: translateX(-100%);
    animation: at-work 2.8s cubic-bezier(0.45, 0, 0.55, 1) infinite;
  }
  @keyframes at-work { to { transform: translateX(250%); } }
  :global(.asleep) .session::after { animation-play-state: paused; }
  :global(.still) .session::after { animation: none; opacity: 0; }
  .top { display: flex; align-items: center; gap: 10px; min-width: 0; }
  .mark { position: relative; flex: none; width: 34px; height: 34px; border-radius: 50%; display: flex; align-items: center; justify-content: center; background: var(--glass-strong); }
  /* Turning while the session works: an arc of light round its mark. */
  .orbit {
    position: absolute;
    inset: -2px;
    border-radius: 50%;
    background: conic-gradient(from 0deg, transparent 0deg, var(--good) 70deg, transparent 140deg);
    mask: radial-gradient(farthest-side, transparent calc(100% - 2.5px), #000 calc(100% - 2px));
    -webkit-mask: radial-gradient(farthest-side, transparent calc(100% - 2.5px), #000 calc(100% - 2px));
    animation: orbit 1.6s linear infinite;
  }
  @keyframes orbit { to { transform: rotate(360deg); } }
  :global(.still) .orbit { animation: none; background: conic-gradient(var(--good) 0deg 360deg); opacity: 0.6; }
  .title { flex: 1; min-width: 0; font-size: var(--type-title, 15px); font-weight: 600; color: var(--ink); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .timer { flex: none; display: inline-flex; align-items: baseline; gap: 3px; color: var(--good); }
  .timer-figure { font-family: var(--rounded); font-size: var(--fig-m, 17px); font-weight: 600; }
  .timer-words { font-size: var(--type-small, 11px); font-weight: 500; }
  .about { display: flex; align-items: center; gap: 10px; padding-left: 44px; font-size: 11.5px; color: var(--ink-2); min-width: 0; }
  .about-text { min-width: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .context { flex: none; display: inline-flex; align-items: center; gap: 5px; }
  .meter { width: 40px; height: 4px; border-radius: 2px; background: var(--line-faint); overflow: hidden; }
  .meter span { display: block; height: 100%; border-radius: 2px; background: var(--star); }
  .share { font-family: var(--rounded); font-weight: 600; color: var(--ink-2); }
  .turn { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; padding-left: 44px; font-size: 11.5px; color: var(--ink-2); }
  .turn-figures { margin-right: 4px; color: var(--ink); font-weight: 600; }
  .chip {
    padding: 2px 8px;
    border-radius: 999px;
    font-size: 11px;
    background: var(--glass-strong);
    color: var(--ink-2);
    transition: background-color 1.2s ease-out, color 1.2s ease-out, box-shadow 1.2s ease-out;
  }
  /* Just called: lit, then easing back. */
  .chip.lit { background: color-mix(in srgb, var(--good) 30%, transparent); color: var(--ink); box-shadow: 0 0 12px -2px var(--good); transition: none; }
  .quiet { margin: 0 2px; font-size: 12.5px; color: var(--ink-3); line-height: 1.5; }
  .feed { list-style: none; margin: 0; padding: 4px 14px; border-radius: 20px; background: var(--glass); box-shadow: inset 0 0 0 0.5px var(--glass-edge), var(--lift); }
  .event { display: grid; grid-template-columns: auto auto minmax(0, auto) minmax(0, 1fr); align-items: center; gap: 8px; min-height: 34px; font-size: 12.5px; border-bottom: 0.5px solid var(--line-faint); }
  .event:last-child { border-bottom: 0; }
  .when { font-size: 11px; color: var(--ink-3); }
  .dot { width: 8px; height: 8px; border-radius: 50%; }
  .sign { display: block; }
  .who { color: var(--ink); font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; max-width: 11em; }
  .what { color: var(--ink-2); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .event.done .what, .event.wake .what { color: var(--good); font-weight: 600; }
</style>
