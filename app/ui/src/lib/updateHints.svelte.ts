// A newer version of a tool, or of Starmeter itself, is told on the homepage without getting in the way (用户：标题附近
// 加个版本更新提醒，不要太打扰用户，但也要让用户知道). Beside Starmeter: a ring with a down arrow in it, no words (what is new is
// said on hover), until the user has looked at the updates once; then only a dot, until a newer version comes along
// again. The system notification is the engine's, as before. 提醒 → 主页提示工具更新 turns the homepage's part off
// (the pill and the dot, and the 工具更新 row's highlight); the notifications stay. Both are this panel's own choices,
// kept in the web view's storage like the language.
import type { Entry } from './types';

const KEY = 'starmeter.update-hints';
const SEEN = 'starmeter.updates-seen';

function read(key: string): string | null {
  try {
    return typeof localStorage !== 'undefined' ? localStorage.getItem(key) : null;
  } catch {
    return null;
  }
}

function write(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {
    // The choice lasts this session.
  }
}

export const updateHints = $state({ on: read(KEY) !== 'off', seen: read(SEEN) ?? '' });

export function chooseUpdateHints(on: boolean) {
  updateHints.on = on;
  write(KEY, on ? 'on' : 'off');
}

/** What is waiting: the 工具更新 row and its rows with a newer version, Starmeter's own row when it has one, and a key
 *  that changes with every new version (so a newer one shows the pill again). Null when nothing is. */
export function pendingUpdates(entries: Entry[]): { count: number; own: string | null; key: string; row: Entry | null; ownRow: Entry | null } | null {
  const row = entries.find((e) => e.module === 'updates' || e.onOpenAction?.join(' ') === 'refresh-source updates') ?? null;
  const newer = (row?.children ?? []).filter((child) => child.statusLight === 'new');
  const ownRow = entries.find((e) => e.label === 'Starmeter' && /^可更新\s/.test(e.value ?? '')) ?? null;
  const own = ownRow?.value?.replace(/^可更新\s*/, '') ?? null;
  if (!newer.length && !own) return null;
  const key = [own ? 'Starmeter ' + own : '', ...newer.map((child) => `${child.label} ${child.detail?.split('\n')[0] ?? ''}`)]
    .filter(Boolean).sort().join('|');
  return { count: newer.length + (own ? 1 : 0), own, key, row, ownRow };
}

export function markUpdatesSeen(key: string) {
  if (!key || updateHints.seen === key) return;
  updateHints.seen = key;
  write(SEEN, key);
}

/** 提醒 with 主页提示工具更新 last; and, with it off, the 工具更新 row without its highlight. */
export function withUpdateHints(options: Entry[] | undefined, entries: Entry[]): { options: Entry[] | undefined; entries: Entry[] } {
  const row: Entry = {
    label: '主页提示工具更新', detail: '有新版本时在 Starmeter 旁和工具更新那一行提示；关掉后只发系统通知', symbol: 'arrow.down.circle',
    checked: updateHints.on, action: ['set-update-hints', updateHints.on ? 'false' : 'true'], children: [],
  };
  const withRow = options?.map((e) => (e.label === '提醒' ? { ...e, children: [...e.children, row] } : e));
  const quiet = updateHints.on ? entries : entries.map((e) => (e.module === 'updates' || (e.label === 'Starmeter' && e.attention) ? { ...e, attention: false } : e));
  return { options: withRow, entries: quiet };
}
