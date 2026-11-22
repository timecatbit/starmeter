// How a page's rows read: which are plain facts, switches, segmented choices or commands, the icon every action
// carries, and a usage row's share and even-pace mark.
import { reads, source } from './i18n.svelte';
import type { Entry } from './types';

/** A row's identity among its siblings: a module by its id, else its action, link or the first part of its label. */
export function entryKey(e: Entry): string {
  if (e.action?.[0] === 'set-module') return e.action.slice(0, 2).join('\u001f');
  // A switch keeps its identity when its action flips from true to false, so it slides instead of being replaced.
  if (e.checked !== undefined && !e.radio && (e.action?.at(-1) === 'true' || e.action?.at(-1) === 'false')) return e.action.slice(0, -1).join('\u001f');
  // By its Chinese label, so a page stays open when the language changes.
  const label = source(e).label;
  return e.action?.join('\u001f') ?? e.href ?? label.split(' · ')[0] ?? label;
}

export const actionable = (e: Entry) => e.children.length > 0 || e.href !== undefined || e.action !== undefined;
export const isModule = (e: Entry) => e.action?.[0] === 'set-module';

/** A plain fact (where data comes from, a note): quieter than rows that can be acted on or carry a figure. */
export const info = (e: Entry) => !actionable(e) && e.value === undefined && e.statusLight === undefined && e.checked === undefined && e.usageLevel === undefined;
/** An on/off setting reads as a switch on the right, not a checkbox. */
export const toggle = (e: Entry) => e.checked !== undefined && e.radio !== true && (e.action?.at(-1) === 'true' || e.action?.at(-1) === 'false');
export const leads = (e: Entry) => (e.checked !== undefined && !toggle(e)) || e.statusLight !== undefined || e.image !== undefined || e.symbol !== undefined;

const actionSymbols: Record<string, string> = {
  'refresh-monitor': 'arrow.clockwise', 'check-app-updates': 'arrow.clockwise', 'retry-claude-usage': 'arrow.clockwise',
  'choose-source': 'folder', 'choose-client-home': 'folder', 'reveal-source': 'folder', 'reveal-client-home': 'folder',
  'auto-source': 'arrow.uturn.backward', 'reset-client-connection': 'arrow.uturn.backward', 'set-proxy-address': 'link',
  'choose-admin-token': 'key', 'configure-connection': 'key', 'test-api-key': 'checkmark.shield', 'rename-api-key': 'pencil',
  'remove-api-key': 'trash', 'clear-connection': 'trash', 'reset-modules': 'arrow.counterclockwise', 'open-claude': 'macwindow',
  'set-subscription': 'checkmark.circle', 'set-claude-local': 'dot.radiowaves.left.and.right', 'set-claude-live': 'xmark.circle',
  'export-usage': 'square.and.arrow.down', 'set-cny-rate': 'arrow.left.arrow.right', 'detect-modules': 'sparkle.magnifyingglass',
  'choose-sync-folder': 'folder', 'set-device-name': 'pencil', 'clear-sync-folder': 'trash',
};

/** Every action on a page carries an icon, so a level's text starts in one column; a web link's icon is a globe, its
 *  small arrow staying on the right. */
export function iconed(e: Entry): Entry {
  if (e.separator || e.section || e.statusLight !== undefined) return e;
  if (e.href?.startsWith('http') && !e.children.length) return { ...e, image: undefined, symbol: 'globe' };
  if (e.image !== undefined || e.symbol !== undefined || e.children.length || e.checked !== undefined) return e;
  if (e.href) return { ...e, symbol: e.href.startsWith('file:') ? 'macwindow' : 'globe' };
  const action = e.action?.[0];
  if (!action) return e;
  const symbol = action === 'configure-connection' && source(e).label.includes('地址') ? 'link' : actionSymbols[action];
  return symbol ? { ...e, symbol } : e;
}

/** Something to do rather than to read: tinted like a link, red when it takes something away. */
export const command = (e: Entry) => !e.children.length && (e.action !== undefined || e.href !== undefined) && e.value === undefined && e.checked === undefined && e.statusLight === undefined;
export const destructive = (e: Entry) => ['remove-api-key', 'clear-connection', 'stop', 'reset-modules', 'clear-sync-folder'].includes(e.action?.[0] ?? '') || source(e).label.startsWith('删除') || source(e).label.startsWith('清除');

/** "已用 38%", "已用 main 76% · alt 20%": the tightest share, for the bar under a usage row. */
export function usageShare(e: Entry): number | null {
  const value = source(e).value;
  if (e.usageLevel === undefined || value === undefined) return null;
  const shares = [...value.matchAll(/(\d+(?:\.\d+)?)(?=%)/g)].map((m) => Number(m[1]));
  return shares.length ? Math.min(1, Math.max(0, Math.max(...shares) / 100)) : null;
}

/** Where even use would put the bar now: the share used, less how far ahead of even pace the detail says it is. */
export function pace(e: Entry): number | null {
  const used = usageShare(e);
  const detail = source(e).detail;
  if (used === null || !detail) return null;
  if (detail.includes('与匀速持平')) return used;
  const match = detail.match(/比匀速(多用|少用) (\d+(?:\.\d+)?)%/);
  if (!match) return null;
  const ahead = match[1] === '多用', number = Number(match[2]);
  return Math.min(1, Math.max(0, used - (ahead ? number : -number) / 100));
}

/** A homepage row with a usage share shows it as a thin bar under its name. */
export const gauge = (e: Entry) => e.children.length > 0 && usageShare(e) !== null;

/** 已配置 / 未配置 draw as a status light, not words. */
export const configured = (value: string | undefined) => (reads(value, '已配置') ? true : reads(value, '未配置') ? false : null);

/** A page with a single row to open (one Key, one session) arrives with it open. */
export function opening(page: Entry): Set<string> {
  const openable = page.children.filter((e) => e.children.length && !e.separator && !e.section);
  return openable.length === 1 ? new Set(['/' + entryKey(openable[0])]) : new Set();
}

// MARK: What the panel does with an action

const quietSettings = new Set(['set-currency', 'set-service-status', 'set-pin-interval', 'set-menu-bar', 'set-usage-alert', 'set-reset-reminder', 'set-session-alerts']);
/** A switch or a choice shows its new state the moment it is clicked; the saved state confirms it after. */
export const choices = new Set([...quietSettings, 'set-cursor-usage', 'set-claude-oauth', 'set-claude-local', 'set-claude-live', 'set-keep-usage', 'set-subscription']);
export const isPreference = (action: string[]) =>
  ['set-module', 'set-appearance', 'set-theme', 'reset-modules', 'set-collapse', 'set-expanded', 'move-module', 'reset-module-order'].includes(action[0]);
export const automatic = (action: string[]) => ['refresh-view', 'refresh-source', 'refresh-live'].includes(action[0]);
export const passive = (action: string[]) =>
  ['refresh-source', 'refresh-view', 'refresh', 'refresh-live', 'refresh-monitor', 'check-app-updates', 'set-range', 'set-group', 'retry-claude-usage'].includes(action[0]);

/** Actions that hand the user to another app (an App, a log or the config file in an editor, Finder, a terminal). */
const handOff = new Set(['open-claude', 'open-cli-update-log', 'open-home', 'open-settings', 'reveal-source', 'reveal-client-home', 'update-cli', 'logs', 'panel',
  'panel-modules']);
/** Actions that may open a file picker: in SwiftBar's panel AppleScript draws it under the panel, which steps aside. */
const pickers = new Set(['choose-source', 'choose-client-home', 'choose-admin-token', 'configure-connection', 'export-usage', 'choose-sync-folder']);

/** The panel stays open for everything but a hand-off to another app: the core's questions (and the plugin's, in
 *  SwiftBar's panel) are answered in it, and the app's file pickers show above it. */
export function keepsPanelOpen(action: string[], swiftbar = false): boolean {
  if (!action[0] || handOff.has(action[0])) return false;
  return !(swiftbar && pickers.has(action[0]));
}

/** The 会话 module's row on the homepage (its sessions become the dashboard's fourth choice), or null. */
export function sessionsEntry(entries: Entry[]): Entry | null {
  return entries.find((e) => e.onOpenAction?.[0] === 'refresh-source' && e.onOpenAction[1] === 'sessions')
    ?? entries.find((e) => source(e).label === '会话' && e.symbol === 'terminal') ?? null;
}

/** The homepage's rows without one of them, and without the divider it leaves doubled or dangling. */
export function without(entries: Entry[], gone: Entry | null): Entry[] {
  if (!gone) return entries;
  const kept = entries.filter((e) => e !== gone);
  return kept.filter((e, i) => !e.separator || (i > 0 && i < kept.length - 1 && !kept[i - 1].separator));
}
