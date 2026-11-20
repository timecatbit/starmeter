<script lang="ts">
  // 会话: the Claude Code and Codex sessions of the last day as cards, the fourth choice beside 工具 / 模型 / 项目.
  // A session is running or it is not. Its card leads with its tool's logo, its title and this turn's tokens (the last
  // turn's once it has finished), read again with every pinned tick so the number moves as the session works; a click
  // opens its last turns. The alert switch closes the list; a pinned panel fits down to the third session.
  import { panel } from '../lib/ui.svelte';
  import { t } from '../lib/i18n.svelte';
  import type { Entry } from '../lib/types';
  import DetailList from './DetailList.svelte';
  import Glyph from './Glyph.svelte';
  import MoreButton from './MoreButton.svelte';
  import StatusLight from './StatusLight.svelte';

  let { entry, busy, onopen, onchoose, onfit }: {
    entry: Entry; busy: string[] | null; onopen: (e: Entry) => void; onchoose: (e: Entry) => void;
    /** The list's height down to the end of its third session (all of it with three or fewer). */
    onfit?: (height: number) => void;
  } = $props();

  const FIT = 3;
  /** Five at first, the running ones leading; the rest one click away, then the page scrolls to them. */
  const SHOWN = 5;
  let everything = $state(false);
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
  let opened = $state(new Set<string>());
  let settingsOpen = $state(new Set<string>());

  /** A session's turns: the heading (project · 最近 N 轮), then the turns, newest first. */
  const turnsOf = (s: Entry) => s.children.filter((c) => c.detail !== undefined);
  /** This turn's tokens and cost (its row's last two parts) and requests and tools, under the session's state; the
   *  card's figure on the right is what the whole session has cost. */
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

  function toggle(key: string) {
    const next = new Set(opened);
    if (next.has(key)) next.delete(key);
    else next.add(key);
    opened = next;
  }
</script>

<div class="session-list" bind:this={list}>
  {#each listed as s (keyOf(s))}
    {@const more = latest(s)}
    {@const open = opened.has(keyOf(s))}
    <div class="session" class:open>
      <button class="head pressable" aria-expanded={open} onclick={() => toggle(keyOf(s))}>
        <span class="logo"><Glyph image={s.image} template={s.template} symbol={s.symbol ?? 'terminal'} size={14} /></span>
        <span class="title">{s.label}</span>
        <span class="value" class:glow={glowing.has(keyOf(s))}>{s.value ?? ''}</span>
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
  {#if listed.length < sessions.length}
    <MoreButton label={t('查看更多 {n} 个会话', { n: sessions.length - listed.length })} onclick={() => (everything = true)} />
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
    text-overflow: ellipsis;
  }
  .value {
    flex: none;
    font-size: 12.5px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    font-family: ui-rounded, -apple-system, sans-serif;
    transition: color 1.2s ease-out;
  }
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
  .text { min-width: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .turns {
    margin: 4px 12px 0 18px;
    padding-left: 10px;
    border-left: 1px solid var(--separator);
  }
  .turn-heading { font-size: 10px; color: var(--secondary); height: 16px; line-height: 16px; }
  .turn { display: flex; flex-direction: column; padding: 2px 0; }
  .turn-label { font-size: 11px; font-variant-numeric: tabular-nums; }
  .turn-detail { font-size: 10px; color: var(--secondary); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .empty { padding: 2px 12px 10px; font-size: 10.5px; color: var(--secondary); }
  .settings { margin-top: 2px; }
</style>
