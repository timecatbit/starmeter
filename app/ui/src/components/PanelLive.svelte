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
  import { duration, PhoneLive } from '../lib/phoneLive.svelte';
  import { panel } from '../lib/ui.svelte';
  import Glyph from './Glyph.svelte';

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

  const SHOWN = 12;
  const running = $derived(live.sessions.filter((s) => s.running));
  const latest = $derived(live.sessions.find((s) => !s.running) ?? null);
  const events = $derived(live.events.slice(0, SHOWN));
  const pad = (n: number) => String(n).padStart(2, '0');
  const clock = (at: number) => { const d = new Date(at); return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`; };
  /** This turn's clock: m:ss when the computer gave its start to the second, else its minutes. */
  function timer(began: number | null, exact: boolean): string {
    // No start known (a turn begun before the logs read): it is running, said so, never an empty space.
    if (began === null) return t('运行中');
    const seconds = Math.max(0, Math.floor((now - began) / 1000));
    if (!exact) return duration(seconds * 1000, false);
    const h = Math.floor(seconds / 3600), m = Math.floor((seconds % 3600) / 60), s = seconds % 60;
    return h ? `${h}:${pad(m)}:${pad(s)}` : `${m}:${pad(s)}`;
  }
  /** The model and effort, as 会话 writes them before the context's share. */
  const model = (about: string) => about.split(' · ').filter((part) => !/%/.test(part)).slice(0, 2).join(' · ');
  const motion = (ms: number) => (panel.reduceMotion ? 0 : ms);
  const tint = $derived(panel.dark ? 'rgb(235, 238, 255)' : 'rgb(30, 40, 80)');
</script>

<div class="panel-live">
  <div class="head"><span>{t('正在运行')}</span>{#if running.length}<span class="count num">{running.length}</span>{/if}</div>
  {#each running as s (s.key)}
    <div class="live-session" in:fly={{ y: 6, duration: motion(240) }}>
      <div class="top">
        <span class="mark"><Glyph image={s.image} template={s.template} symbol={s.symbol ?? 'terminal'} size={12} {tint} /></span>
        <span class="title">{s.title}</span>
        <span class="clock num">{timer(s.began, s.exact)}</span>
      </div>
      <div class="about">
        {#if model(s.about)}<span class="model">{model(s.about)}</span>{/if}
        {#if s.turnTokens}<span class="spent num">{t('本轮') + ' ' + s.turnTokens + (s.turnCost ? ' · ' + s.turnCost : '')}</span>{/if}
      </div>
    </div>
  {/each}
  {#if !running.length}
    <p class="quiet">{latest ? t('没有在运行的会话 · 最近：{title}{idle}', { title: latest.title, idle: latest.idle ? ' · ' + latest.idle : '' }) : t('没有在运行的会话')}</p>
  {/if}

  <div class="head feed-head"><span>{t('动态')}</span></div>
  {#if events.length}
    <!-- data-moving: a new row comes in every couple of seconds and moves the rest (textFits.ts knows). -->
    <ol class="feed" data-moving>
      {#each events as e (e.id)}
        <li class="event {e.kind}" in:fly={{ y: -8, duration: motion(260) }} animate:flip={{ duration: motion(220) }}>
          <span class="when num">{clock(e.at)}</span>
          <span class="dot" style:background={e.tool ? toolColour(e.tool, panel.dark) : e.kind === 'wake' || e.kind === 'done' ? 'var(--green)' : 'var(--accent)'}></span>
          <span class="who">{e.who}</span>
          <span class="what num">{e.what}</span>
        </li>
      {/each}
    </ol>
  {:else}
    <p class="quiet">{live.state === 'working' ? t('等下一笔请求…') : t('一有动静就会出现在这里')}</p>
  {/if}
</div>

<style>
  .panel-live { padding: 0 12px 6px; display: flex; flex-direction: column; font-size: 11px; color: var(--label); }
  .head { display: flex; align-items: center; gap: 6px; height: 22px; font-weight: 600; color: var(--secondary); }
  .feed-head { margin-top: 6px; }
  .count { min-width: 15px; height: 15px; padding: 0 4px; box-sizing: border-box; border-radius: 8px; font-size: 10px; font-weight: 600; line-height: 15px; text-align: center; color: white; background: var(--green); }
  .live-session { padding: 4px 0 5px; border-bottom: 0.5px solid var(--separator); }
  .live-session:last-of-type { border-bottom: 0; }
  .top { display: flex; align-items: center; gap: 6px; min-width: 0; height: 18px; }
  .mark { flex: none; width: 16px; height: 16px; border-radius: 5px; display: flex; align-items: center; justify-content: center; background: var(--hover); }
  .title { flex: 1; min-width: 0; font-size: 12px; font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .clock { flex: none; font-family: var(--rounded); font-size: 12px; font-weight: 600; color: var(--green); }
  .about { display: flex; gap: 8px; min-width: 0; padding-left: 22px; height: 16px; line-height: 16px; color: var(--secondary); white-space: nowrap; }
  .model { flex: 0 1 auto; min-width: 0; overflow: hidden; text-overflow: ellipsis; }
  .spent { flex: none; color: var(--label); }
  .quiet { margin: 2px 0 4px; line-height: 18px; color: var(--secondary); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .feed { list-style: none; margin: 0; padding: 0; }
  .event { display: flex; align-items: center; gap: 6px; height: 20px; min-width: 0; color: var(--secondary); }
  .when { flex: none; font-size: 10px; color: var(--tertiary); }
  .dot { flex: none; width: 6px; height: 6px; border-radius: 50%; }
  .who { flex: none; max-width: 45%; font-weight: 600; color: var(--label); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .what { min-width: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .event.done .what, .event.wake .what { color: var(--green); font-weight: 600; }
</style>
