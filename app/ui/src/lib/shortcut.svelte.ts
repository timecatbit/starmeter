// 全局快捷键: a key combination that opens and closes the panel from anywhere, as the star does. The panel's own
// choice, kept in the web view's storage like the language, handed to the shell as the panel loads and whenever it
// changes; off until chosen. Control and Option (Alt) together, which apps rarely take for themselves.
import { shell } from './backend';
import type { Entry } from './types';

const KEY = 'starmeter.shortcut';
export const SHORTCUTS = ['none', 'ctrl-alt-s', 'ctrl-alt-shift-s', 'ctrl-alt-space'] as const;
export type ShortcutChoice = (typeof SHORTCUTS)[number];

const mac = typeof navigator !== 'undefined' && navigator.userAgent.includes('Mac');
/** As each system writes it: the symbols on a Mac, the key names on Windows. */
const LABELS: Record<Exclude<ShortcutChoice, 'none'>, [string, string]> = {
  'ctrl-alt-s': ['⌃⌥S', 'Ctrl+Alt+S'],
  'ctrl-alt-shift-s': ['⌃⌥⇧S', 'Ctrl+Alt+Shift+S'],
  'ctrl-alt-space': ['⌃⌥Space', 'Ctrl+Alt+Space'],
};
export const shortcutLabel = (choice: ShortcutChoice) => (choice === 'none' ? '不使用' : LABELS[choice][mac ? 0 : 1]);

function stored(): ShortcutChoice {
  try {
    const value = localStorage.getItem(KEY);
    return (SHORTCUTS as readonly string[]).includes(value ?? '') ? (value as ShortcutChoice) : 'none';
  } catch {
    return 'none';
  }
}

/** current: the choice kept; taken: the system or another app holds it now, so it does nothing until it is let go. */
export const shortcut = $state({ current: stored(), taken: false });

/** The choice made in 通用: registered first, kept only if the system let us have it. */
export async function chooseShortcut(next: string): Promise<boolean> {
  if (!(SHORTCUTS as readonly string[]).includes(next)) return false;
  const ok = await shell.setShortcut(next).catch(() => false);
  if (!ok) return false;
  shortcut.current = next as ShortcutChoice;
  shortcut.taken = false;
  try { localStorage.setItem(KEY, next); } catch { /* registered for this run */ }
  return true;
}

/** As the panel loads: the shell starts with no shortcut, so the kept one is handed over again. Taken meanwhile by
 *  another app, the choice stays (it is tried again each time the panel loads) and 通用 says it is taken for now. */
export async function restoreShortcut() {
  if (shortcut.current === 'none') { shortcut.taken = false; return; }
  shortcut.taken = !(await shell.setShortcut(shortcut.current).catch(() => false));
}

/** The 全局快捷键 row in 通用, the panel's own like 语言. */
export function shortcutRow(): Entry {
  const radio = (choice: ShortcutChoice): Entry => ({ label: shortcutLabel(choice), children: [], radio: true, checked: shortcut.current === choice,
    action: ['set-shortcut', choice] });
  const taken = shortcut.taken && shortcut.current !== 'none';
  return { label: '全局快捷键', symbol: 'keyboard', value: taken ? `${shortcutLabel(shortcut.current)} · 当前被占用` : shortcutLabel(shortcut.current),
    detail: taken ? '这个组合键现在被系统或其他程序占用，按了没反应；换一个，或退出占用它的程序后再打开面板' : '在任何地方按下即可打开或收起面板',
    children: SHORTCUTS.map(radio) };
}
