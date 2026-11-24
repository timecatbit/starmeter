<script lang="ts">
  // 会话: the sessions of every tool on the homepage that keeps them (Claude Code, Codex, Gemini, OpenCode, Qoder, WorkBuddy,
  // DeepSeek) as cards, the fourth choice beside 工具 / 模型 / 项目.
  // A session is running or it is not. Its card leads with its tool's logo, its title and what it cost in the range
  // shown, as the other tabs count (the whole session's under 全部), read again with every pinned tick so the number
  // moves as the session works; nothing spent in the range reads quieter. A click opens its last turns, headed by what
  // the whole session has cost. A note under the list says what the figures count. The alert switch closes the list;
  // a pinned panel fits down to the third session.
  import { panel } from '../lib/ui.svelte';
  import { t } from '../lib/i18n.svelte';
  import type { Entry } from '../lib/types';
  import DetailList from './DetailList.svelte';
  import Glyph from './Glyph.svelte';
  import MoreButton from './MoreButton.svelte';
  import { closeList, listOpen, openList, sessionOpen, toggleSession } from '../lib/expanded.svelte';
  import StatusLight from './StatusLight.svelte';

  let { entry, busy, onopen, onchoose, onfit }: {
    entry: Entry; busy: string[] | null; onopen: (e: Entry) => void; onchoose: (e: Entry) => void;
    /** The list's height down to the end of its third session (all of it with three or fewer). */
    onfit?: (height: number) => void;
  } = $props();

  const FIT = 3;
  /** Five at first, the running ones leading; the rest one click away, then the page scrolls to them. */
  const SHOWN = 5;
  // 查看更多 and each opened session stay so (expanded.svelte.ts), whatever the list comes back through.
  const everything = $derived(listOpen('sessions'));
  const sessions = $derived(entry.children.filter((c) => c.statusLight !== undefined));
  /** A session's identity: its key (two sessions can share a name), else its name and how many before it had it. */
  const keys = $derived.by(() => {
    const seen = new Map<string, number>();
    return new Map(sessions.map((s) => {
      const base = s.key ?? s.label, count = seen.get(base) ?? 0;
      seen.set(base, count + 1);
      return [s, count ? `${base}\u001f${count}` : base] as const;
    }));
  });
  const keyOf = (s: Entry) => keys.get(s) ?? s.label;
  /** Nothing spent in the range shown: the amount is all zeros (¥0.00, $0.00), drawn quieter. */
  const spentNothing = (s: Entry) => /^\D*0[.,]00$/.test(s.value ?? '');
  /** A card's figure as the tool cards write theirs: rounded digits, the $ and the K/M/B unit smaller, any words around
   *  it (已知, Token) in the text font; a value with no number (费用待定价) as it is. */
  function amount(value: string): { before: string; sign: string; digits: string; unit: string; after: string } | null {
    const found = /^(.*?)([$¥€£])?(\d[\d,]*(?:\.\d+)?)([KMB])?(.*)$/.exec(value);
    if (!found) return null;
    return { before: found[1], sign: found[2] ?? '', digits: found[3], unit: found[4] ?? '', after: found[5] };
  }
  const listed = $derived(everything ? sessions : sessions.slice(0, SHOWN));
  const notes = $derived(entry.children.filter((c) => c.statusLight === undefined && !c.separator && c.checked === undefined && !c.action));
  const settings = $derived(entry.children.filter((c) => c.checked !== undefined));
  let list: HTMLDivElement | undefined = $state();
  // Taken a frame after the list changes size (an opened session, a new one), never inside the observer's own call.
  $effect(() => {
    const box = list;
    if (!box || !onfit) return;
    let frame = 0;
    const measure = () => {
      const cards = box.querySelectorAll<HTMLElement>('.session');
      const third = cards.length > FIT ? cards[FIT - 1] : null;
      onfit?.(third ? third.offsetTop - box.offsetTop + third.offsetHeight + 4 : box.offsetHeight);
    };
    const observer = new ResizeObserver(() => { cancelAnimationFrame(frame); frame = requestAnimationFrame(measure); });
    observer.observe(box);
    measure();
    return () => { observer.disconnect(); cancelAnimationFrame(frame); };
  });
  let settingsOpen = $state(new Set<string>());

  /** A session's turns: the heading (project · 最近 N 轮), then the turns, newest first. */
  const turnsOf = (s: Entry) => s.children.filter((c) => c.detail !== undefined);
  /** This turn's tokens and cost (its row's last two parts) and requests and tools, under the session's state; the
   *  card's figure on the right is what the session cost in the range shown. */
  function latest(s: Entry): string | null {
    const turn = turnsOf(s)[0];
    if (!turn) return null;
    const [tokens, cost] = turn.label.split(' · ').slice(-2);
    return `${t(s.statusLight === 'new' ? '本轮' : '上一轮')} ${tokens} · ${cost} · ${turn.detail}`;
  }

  // A figure that changed since the last reading glows in the accent colour for a moment.
  let previous = new Map<string, string>();
  let glowing = $state(new Set<string>());
  const timers = new Map<string, ReturnType<typeof setTimeout>>();
  $effect(() => {
    const next = new Map(sessions.map((s) => [keyOf(s), s.value ?? '']));
    for (const [key, value] of next) {
      const before = previous.get(key);
      if (before !== undefined && before !== value && !panel.reduceMotion) {
        glowing = new Set([...glowing, key]);
        clearTimeout(timers.get(key));
        timers.set(key, setTimeout(() => { glowing = new Set([...glowing].filter((k) => k !== key)); }, 1600));
      }
    }
    previous = next;
  });

  const toggle = (key: string) => toggleSession(key);
</script>

<div class="session-list" bind:this={list}>
  {#each listed as s (keyOf(s))}
    {@const more = latest(s)}
    {@const open = sessionOpen(keyOf(s))}
    {@const figure = amount(s.value ?? '')}
    <div class="session" class:open>
      <button class="head pressable" aria-expanded={open} onclick={() => toggle(keyOf(s))}>
        <span class="logo"><Glyph image={s.image} template={s.template} symbol={s.symbol ?? 'terminal'} size={14} /></span>
        <span class="title">{s.label}</span>
        <span class="value" class:glow={glowing.has(keyOf(s))} class:none={spentNothing(s)}>
          {#if figure}{#if figure.before}<span class="words">{figure.before}</span>{/if}<span class="figure num">{#if figure.sign}<span
                class="sign">{figure.sign}</span>{/if}{figure.digits}{#if figure.unit}<span class="unit">{figure.unit}</span>{/if}</span>{#if figure.after}<span
              class="words">{figure.after}</span>{/if}{:else}<span class="words">{s.value ?? ''}</span>{/if}
        </span>
      </button>
      <div class="line"><StatusLight state={s.statusLight ?? 'unknown'} /><span class="text">{s.detail ?? ''}</span></div>
      {#if more}<div class="line muted"><span class="text">{more}</span></div>{/if}
      {#if open}
        <div class="turns">
          {#each s.children as turn, i (i)}
            {#if turn.detail === undefined}
              <div class="turn-heading">{turn.label}</div>
            {:else}
              <div class="turn"><span class="turn-label">{turn.label}</span><span class="turn-detail">{turn.detail}</span></div>
            {/if}
          {/each}
        </div>
      {/if}
    </div>
  {/each}
  {#if sessions.length > SHOWN}
    <MoreButton open={everything} label={everything ? t('收起') : t('查看更多 {n} 个会话', { n: sessions.length - listed.length })}
      onclick={() => (everything ? closeList : openList)('sessions')} />
  {/if}
  {#each notes as note, i (i)}
    <div class="empty">{note.label}</div>
  {/each}
  {#if settings.length}
    <div class="settings">
      <DetailList entries={settings} open={settingsOpen} {busy} rowWidth={panel.width - 16} {onopen} {onchoose} ontoggled={(next) => (settingsOpen = next)} />
    </div>
  {/if}
</div>

<style>
  .session-list { padding: 0 0 4px; }
  .session { padding: 0 0 8px; }
  .head {
    --press-scale: 0.985;
    width: calc(100% - 8px);
    margin: 0 4px;
    height: 26px;
    padding: 0 8px;
    box-sizing: border-box;
    display: flex;
    align-items: center;
    gap: 7px;
    border-radius: 8px;
    text-align: left;
  }
  .head:hover { background: var(--hover); }
  .head:active { background: var(--press); }
  .logo { flex: none; display: flex; width: 14px; height: 14px; }
  .title {
    flex: 1;
    min-width: 0;
    font-size: 12.5px;
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
    overflow: clip visible;
    text-overflow: ellipsis;
  }
  .value {
    flex: none;
    white-space: nowrap;
    transition: color 1.2s ease-out;
  }
  /* As the tool cards' figures (Figure.svelte), the panel's theme font for numbers (手绘 its hand). */
  .figure {
    font-family: var(--rounded);
    font-size: 14px;
    font-weight: 600;
  }
  .sign {
    font-size: 0.62em;
    font-weight: 500;
    color: var(--secondary);
    vertical-align: 0.48em;
    margin-right: 0.04em;
  }
  .unit { font-size: 0.74em; }
  .words { font-size: 11px; font-weight: 500; color: var(--secondary); }
  .words + .figure { margin-left: 3px; }
  .figure + .words { margin-left: 2px; }
  .value.none .figure { color: var(--secondary); font-weight: 500; }
  .value.glow { color: var(--accent); transition: none; }
  .line {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 0 12px 0 15px;
    height: 16px;
    font-size: 10.5px;
    color: var(--secondary);
  }
  .line.muted { padding-left: 28px; color: var(--tertiary, var(--secondary)); opacity: 0.85; }
  .text { min-width: 0; white-space: nowrap; overflow: hidden; overflow: clip visible; text-overflow: ellipsis; }
  .turns {
    margin: 4px 12px 0 18px;
    padding-left: 10px;
    border-left: 1px solid var(--separator);
  }
  .turn-heading { font-size: 10px; color: var(--secondary); height: 16px; line-height: 16px; }
  .turn { display: flex; flex-direction: column; padding: 2px 0; }
  .turn-label { font-size: 11px; font-variant-numeric: tabular-nums; }
  .turn-detail { font-size: 10px; color: var(--secondary); white-space: nowrap; overflow: hidden; overflow: clip visible; text-overflow: ellipsis; }
  .empty { padding: 2px 12px 10px; font-size: 10.5px; color: var(--secondary); }
  .settings { margin-top: 2px; }
</style>
