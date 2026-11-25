// The panel's look: 标准, or 手绘 (外观 → 风格): drawn by hand, as the 手绘 share picture is. By day coloured ink on
// dotted journal paper, by night chalk on a blackboard; every line a little unsteady (an SVG filter on the lines only,
// so the words stay sharp), the figures in a hand's letters. A choice of this panel's own, kept in the web view's
// storage like the language; a phone opened from 在手机上看 starts in the computer's (…&s=sketch in its address).
import { shell } from './backend';
import type { Entry } from './types';

export type Style = 'standard' | 'sketch';
const KEY = 'starmeter.style';

function stored(): Style {
  try {
    const asked = typeof location === 'undefined' ? null : new URLSearchParams(location.hash.slice(1)).get('s');
    if (asked === 'sketch' || asked === 'standard') return asked;
    return typeof localStorage !== 'undefined' && localStorage.getItem(KEY) === 'sketch' ? 'sketch' : 'standard';
  } catch {
    return 'standard';
  }
}

export const style = $state({ current: stored() });

/** The page's look, for the stylesheet (app.css, :root[data-style='sketch']). */
function markPage() {
  if (typeof document === 'undefined') return;
  if (style.current === 'sketch') document.documentElement.dataset.style = 'sketch';
  else delete document.documentElement.dataset.style;
}
markPage();

export function chooseStyle(next: string) {
  style.current = next === 'sketch' ? 'sketch' : 'standard';
  markPage();
  try { localStorage.setItem(KEY, style.current); } catch { /* the choice lasts this session */ }
  void shell.setStyle(style.current);
}

/** The shell is told the look as the panel loads, so a phone follows the computer's (its snapshot carries it). */
export function restoreStyle() {
  void shell.setStyle(style.current);
}

/** On a phone: the computer's look, as its snapshot says, taken at once (the phone's own storage keeps it too). */
export function followStyle(look: string | undefined) {
  if ((look === 'sketch' || look === 'standard') && look !== style.current) chooseStyle(look);
}

/** 外观 with its 风格 choice, after the background's: 标准 / 手绘. */
export function withStyle(list: Entry[]): Entry[] {
  return list.map((e) => {
    if (e.label !== '外观') return e;
    const radio = (label: string, key: Style): Entry => ({ label, children: [], radio: true, checked: style.current === key, action: ['set-style', key] });
    const children = [...e.children];
    const background = children.findIndex((child) => child.action?.[0] === 'set-appearance');
    const after = background < 0 ? children.length : children.findIndex((child, i) => i > background && child.action?.[0] !== 'set-appearance');
    const at = after < 0 ? children.length : after;
    children.splice(at, 0, { label: '', separator: true, children: [] }, radio('标准', 'standard'), radio('手绘', 'sketch'));
    return { ...e, children };
  });
}
