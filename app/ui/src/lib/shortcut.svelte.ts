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

export const shortcut = $state({ current: stored() });

/** The choice made in 外观与设置: registered first, kept only if the system let us have it. */
export async function chooseShortcut(next: string): Promise<boolean> {
  if (!(SHORTCUTS as readonly string[]).includes(next)) return false;
  const ok = await shell.setShortcut(next).catch(() => false);
  if (!ok) return false;
  shortcut.current = next as ShortcutChoice;
  try { localStorage.setItem(KEY, next); } catch { /* registered for this run */ }
  return true;
}

/** As the panel loads: the shell starts with no shortcut, so the kept one is handed over again. */
export function restoreShortcut() {
  if (shortcut.current !== 'none') void shell.setShortcut(shortcut.current).catch(() => false);
}

/** The 全局快捷键 row under 显示 in 外观与设置, the panel's own like 语言. */
export function shortcutRow(): Entry {
  const radio = (choice: ShortcutChoice): Entry => ({ label: shortcutLabel(choice), children: [], radio: true, checked: shortcut.current === choice,
    action: ['set-shortcut', choice] });
  return { label: '全局快捷键', symbol: 'keyboard', value: shortcutLabel(shortcut.current), detail: '在任何地方按下即可打开或收起面板',
    children: SHORTCUTS.map(radio) };
}
