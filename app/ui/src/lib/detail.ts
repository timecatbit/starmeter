// How a page's rows read: which are plain facts, switches, segmented choices or commands, the icon every action
// carries, and a usage row's share and even-pace mark.
import type { Entry } from './types';

/** A row's identity among its siblings: a module by its id, else its action, link or the first part of its label. */
export function entryKey(e: Entry): string {
  if (e.action?.[0] === 'set-module') return e.action.slice(0, 2).join('\u001f');
  return e.action?.join('\u001f') ?? e.href ?? e.label.split(' · ')[0] ?? e.label;
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
  const symbol = action === 'configure-connection' && e.label.includes('地址') ? 'link' : actionSymbols[action];
  return symbol ? { ...e, symbol } : e;
}

/** Something to do rather than to read: tinted like a link, red when it takes something away. */
export const command = (e: Entry) => !e.children.length && (e.action !== undefined || e.href !== undefined) && e.value === undefined && e.checked === undefined && e.statusLight === undefined;
export const destructive = (e: Entry) => ['remove-api-key', 'clear-connection', 'stop', 'reset-modules'].includes(e.action?.[0] ?? '') || e.label.startsWith('删除') || e.label.startsWith('清除');

/** "已用 38%", "已用 本周 38% · Fable 39%": the tightest share, for the bar under a usage row. */
export function usageShare(e: Entry): number | null {
  if (e.usageLevel === undefined || e.value === undefined) return null;
  const shares = [...e.value.matchAll(/(\d+(?:\.\d+)?)(?=%)/g)].map((m) => Number(m[1]));
  return shares.length ? Math.min(1, Math.max(0, Math.max(...shares) / 100)) : null;
}

/** Where even use would put the bar now: the share used, less how far ahead of even pace the detail says it is. */
export function pace(e: Entry): number | null {
  const used = usageShare(e);
  if (used === null || !e.detail) return null;
  if (e.detail.includes('与匀速持平')) return used;
  const match = e.detail.match(/比匀速(多用|少用) (\d+(?:\.\d+)?)%/);
  if (!match) return null;
  const ahead = match[1] === '多用', number = Number(match[2]);
  return Math.min(1, Math.max(0, used - (ahead ? number : -number) / 100));
}

/** A homepage row with a usage share shows it as a thin bar under its name. */
export const gauge = (e: Entry) => e.children.length > 0 && usageShare(e) !== null;

/** 已配置 / 未配置 draw as a status light, not words. */
export const configured = (value: string | undefined) => (value === '已配置' ? true : value === '未配置' ? false : null);

/** A page with a single row to open (one Key, one session) arrives with it open. */
export function opening(page: Entry): Set<string> {
  const openable = page.children.filter((e) => e.children.length && !e.separator && !e.section);
  return openable.length === 1 ? new Set(['/' + entryKey(openable[0])]) : new Set();
}

// MARK: What the panel does with an action

const quietSettings = new Set(['set-currency', 'set-service-status', 'set-pin-interval', 'set-menu-bar', 'set-usage-alert', 'set-reset-reminder', 'set-session-alerts']);
/** A switch or a choice shows its new state the moment it is clicked; the saved state confirms it after. */
export const choices = new Set([...quietSettings, 'set-cursor-usage', 'set-claude-oauth', 'set-claude-local', 'set-claude-live', 'set-keep-usage', 'set-subscription']);
export const isPreference = (action: string[]) => ['set-module', 'set-appearance', 'set-theme', 'reset-modules', 'set-collapse', 'set-expanded'].includes(action[0]);
export const automatic = (action: string[]) => ['refresh-view', 'refresh-source', 'refresh-live'].includes(action[0]);
export const passive = (action: string[]) =>
  ['refresh-source', 'refresh-view', 'refresh', 'refresh-live', 'refresh-monitor', 'check-app-updates', 'set-range', 'set-group', 'retry-claude-usage'].includes(action[0]);

/** Actions without a dialog keep the panel open so their result appears in place. */
export function keepsPanelOpen(action: string[]): boolean {
  const name = action[0];
  if (!name) return false;
  if (quietSettings.has(name)) return true;
  // These ask before turning on, in a dialog the panel would cover; turning them off asks nothing.
  if (['set-cursor-usage', 'set-claude-oauth', 'set-claude-local', 'set-claude-live'].includes(name) && action.at(-1) === 'false') return true;
  if (name === 'set-keep-usage' && action.at(-1) === 'true') return true;
  return ['refresh-view', 'refresh-source', 'refresh', 'refresh-live', 'refresh-monitor', 'check-app-updates', 'set-subscription', 'set-range', 'set-group',
    'test-api-key', 'retry-claude-usage'].includes(name) || isPreference(action);
}
