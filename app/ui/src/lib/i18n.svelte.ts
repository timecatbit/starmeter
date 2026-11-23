// The panel in Simplified Chinese, Traditional Chinese or English (通用 → 语言). Simplified Chinese is the source: the
// engine and the panel write it, English comes from the dictionary in en.ts at the edges and 繁體中文 from the converter
// in hant.ts (the system's ICU, character by character in context, built by scripts/make_hant.py). localize() turns a snapshot's text
// into the language as it arrives, so every page lays out and measures what it shows; t() does the panel's own words.
// Text the English dictionary doesn't know (a session's title, a project's folder, a new message) stays as it is; the
// English test lists any of the panel's own text left untranslated. The converter takes any Chinese.
import { withUpdateHints } from './updateHints.svelte';
import { shell } from './backend';
import { phrases, rules, whole } from './en';
import { toHant } from './hant';
import { phoneRow } from './phone.svelte';
import { shortcutRow } from './shortcut.svelte';
import { withStyle } from './style.svelte';
import type { Dashboard, Entry, ProviderUsage, Snapshot } from './types';

export type Language = 'zh' | 'zh-Hant' | 'en';
const KEY = 'starmeter.language';
const known = (value: string | null): value is Language => value === 'zh' || value === 'zh-Hant' || value === 'en';
/** Each language by its own name, as the choices show it whatever the language shown. */
export const NAMES: Record<Language, string> = { zh: '简体中文', 'zh-Hant': '繁體中文', en: 'English' };

function stored(): Language {
  try {
    // A phone opened from 在手机上看 starts in the computer's language (…&l=en in the address's fragment).
    const asked = typeof location === 'undefined' ? null : new URLSearchParams(location.hash.slice(1)).get('l');
    if (known(asked)) return asked;
    const kept = typeof localStorage !== 'undefined' ? localStorage.getItem(KEY) : null;
    return known(kept) ? kept : 'zh';
  } catch {
    return 'zh';
  }
}

export const language = $state({ current: stored() });
export const english = () => language.current === 'en';
export const traditional = () => language.current === 'zh-Hant';

/** The page's language, for the fonts' forms (PingFang TC for 繁體中文) and assistive technology. */
function markPage() {
  if (typeof document !== 'undefined') document.documentElement.lang = { zh: 'zh-Hans', 'zh-Hant': 'zh-Hant', en: 'en' }[language.current];
}
markPage();

export function chooseLanguage(next: string) {
  language.current = known(next) ? next : 'zh';
  markPage();
  try { localStorage.setItem(KEY, language.current); } catch { /* the choice lasts this session */ }
  void shell.setLanguage(language.current);
}

/** The shell's own words (the star's right-click menu) follow the panel: told at each choice, and as the panel loads
 *  when this panel ever had one. A panel that never chose leaves the shell as it is, so SwiftBar's panel, with
 *  storage of its own, doesn't turn the app's menu back to Chinese. */
export function restoreLanguage() {
  let chosen = false;
  try { chosen = localStorage.getItem(KEY) !== null; } catch { /* nothing kept */ }
  if (chosen) void shell.setLanguage(language.current);
}

const CJK = /[㐀-鿿（-？、-。]/;
const extra: Record<string, string> = {};
const cache = new Map<string, string>();

/** More phrases, for text only some hosts show (the fixtures' made-up session titles). */
export function addPhrases(more: Record<string, string>) {
  Object.assign(extra, more);
  cache.clear();
}

/** A piece of the engine's text in English: the whole as one phrase or by a rule for the whole (已用 本周 38% · 5 小时
 *  12%); then each part between " · " on its own; then the part before a "：" and the rest; then a rule that knows the
 *  part's shape. What none of these know stays Chinese. */
/** A figure in tokens as the engine writes it (2.41M Token): English counts them in the plural. */
const TOKENS = /(\d[\d.,]*[KMB]?) Token\b(?!\/)/g;

export function translate(text: string): string {
  // Fields the engine leaves empty (null) pass through as they are.
  if (typeof text !== 'string') return text;
  if (!CJK.test(text)) return text.replace(TOKENS, '$1 tokens');
  const known = cache.get(text);
  if (known !== undefined) return known;
  const rule = (list: typeof rules) => {
    for (const [pattern, write] of list) {
      const match = text.match(pattern);
      if (match) return write(match, translate);
    }
    return null;
  };
  let out = phrases[text] ?? extra[text] ?? rule(whole);
  if (out === null && text.includes(' · ')) out = text.split(' · ').map(translate).join(' · ');
  if (out === null && text.includes('：')) {
    const at = text.indexOf('：'), head = translate(text.slice(0, at));
    if (head !== text.slice(0, at)) out = `${head}: ${translate(text.slice(at + 1))}`;
  }
  if (out === null) out = rule(rules) ?? text;
  cache.set(text, out);
  return out;
}

/** The panel's own words, in the language chosen; {name} marks where a value goes. A count of one takes the phrase's
 *  singular, keyed "…|1", where English has one. */
export function t(zh: string, values?: Record<string, string | number>): string {
  const one = values?.n !== undefined && String(values.n) === '1' ? phrases[zh + '|1'] : undefined;
  const text = english() ? (one ?? phrases[zh] ?? translate(zh)) : zh;
  const filled = values ? text.replace(/\{(\w+)\}/g, (whole, name) => (name in values ? String(values[name]) : whole)) : text;
  return traditional() ? shown(filled) : filled;
}

/** A text as the language chosen shows it: English by the dictionary, 繁體中文 by the converter; a language's own name
 *  (简体中文, 繁體中文) stays as it is. */
export function shown(text: string): string {
  if (typeof text !== 'string' || language.current === 'zh') return text;
  if (english()) return translate(text);
  return SELF.has(text) ? text : toHant(text);
}
const SELF = new Set(Object.values(NAMES));

/** Whether the engine's text reads as this Chinese phrase, in any language. */
export const reads = (text: string | null | undefined, zh: string) =>
  text === zh || (text !== null && text !== undefined && (text === translate(zh) || text === toHant(zh)));

/** What a row said in Chinese before it was translated: the panel's logic reads rows by their Chinese wording. */
export const source = (e: Entry) => e.source ?? e;

function entry(e: Entry): Entry {
  const label = shown(e.label), value = e.value === undefined ? undefined : shown(e.value);
  // A path or address stays as it is on disk (LANG-1): the converter can't tell where one ends.
  const detail = e.detail === undefined || e.verbatim ? e.detail : shown(e.detail);
  const changed = label !== e.label || value !== e.value || detail !== e.detail;
  return {
    ...e, label, value, detail,
    ...(e.fallbacks ? { fallbacks: e.fallbacks.map(shown) } : {}),
    ...(e.gauges ? { gauges: e.gauges.map((bar) => ({ ...bar, label: shown(bar.label), ...(bar.group ? { group: shown(bar.group) } : {}) })) } : {}),
    ...(changed ? { source: { label: e.label, value: e.value, detail: e.detail } } : {}),
    children: e.children.map(entry),
  };
}

function provider(p: ProviderUsage): ProviderUsage {
  return {
    ...p, name: shown(p.name), subtitle: shown(p.subtitle), status: shown(p.status),
    priceNote: p.priceNote ? shown(p.priceNote) : p.priceNote, serviceNote: p.serviceNote ? shown(p.serviceNote) : p.serviceNote,
    models: p.models.map((m) => ({ ...m, name: shown(m.name), priceNote: m.priceNote ? shown(m.priceNote) : m.priceNote })),
    clients: p.clients ? p.clients.map((c) => ({ ...c, name: shown(c.name) })) : p.clients,
  };
}

function dashboard(d: Dashboard): Dashboard {
  const h = d.heatmap;
  return {
    ...d, rangeLabel: shown(d.rangeLabel), chartUnit: shown(d.chartUnit), chartSpan: shown(d.chartSpan),
    chartStatus: shown(d.chartStatus), updated: d.updated === undefined ? undefined : shown(d.updated),
    dates: d.dates.map(shown), period: d.period?.map(shown), groupNote: d.groupNote ? shown(d.groupNote) : d.groupNote,
    groupings: d.groupings?.map((g) => ({ ...g, label: shown(g.label) })),
    currency: d.currency ? { ...d.currency, note: d.currency.note === undefined ? undefined : shown(d.currency.note) } : d.currency,
    providers: d.providers.map(provider), cards: d.cards ? d.cards.map(provider) : d.cards,
    heatmap: h ? {
      ...h, rowLabels: h.rowLabels?.map(shown), labels: h.labels.map(shown), hint: shown(h.hint),
      status: h.status === undefined ? undefined : shown(h.status), notice: h.notice === undefined ? undefined : shown(h.notice),
      streak: h.streak ? shown(h.streak) : h.streak,
    } : h,
  };
}

/** The 语言 row, first in 通用: 简体中文 / 繁體中文 / English. The choice is the panel's own (kept in the web view's
 *  storage), told to the shell: the star's right-click menu follows it (restoreLanguage), and so do the engine's
 *  notifications and SwiftBar's menu for 繁體中文. The 新手指引 row under 帮助 is the panel's own too. */
function withLanguage(list: Entry[]): Entry[] {
  const radio = (label: string, key: Language): Entry => ({ label, children: [], radio: true, checked: language.current === key, action: ['set-language', key] });
  const out = list.map((e) => {
    if (e.label !== '通用') return e;
    // 语言 and 全局快捷键 first; 帮助, last: the first-run tour, again.
    return { ...e, children: [
      { label: '语言 Language', symbol: 'globe', value: NAMES[language.current], children: [radio(NAMES.zh, 'zh'), radio(NAMES['zh-Hant'], 'zh-Hant'), radio(NAMES.en, 'en')] },
      shortcutRow(), ...e.children,
      { label: '帮助', section: true, children: [] }, { label: '新手指引', symbol: 'questionmark.circle', action: ['show-guide'], children: [] }] };
  });
  const data = out.findIndex((e) => e.label === '数据');
  if (data >= 0) out.splice(data + 1, 0, phoneRow());
  return out;
}

/** The snapshot as the panel shows it: the language row added, and every text in the language chosen (in English,
 *  what the dictionary knows). */
export function localize(s: Snapshot): Snapshot {
  const hinted = withUpdateHints(s.moduleOptions, s.entries);
  const options = hinted.options ? withStyle(withLanguage(hinted.options)) : hinted.options;
  if (language.current === 'zh') return { ...s, entries: hinted.entries, moduleOptions: options };
  return {
    ...s, entries: hinted.entries.map(entry), moduleOptions: options?.map(entry), rangeOptions: s.rangeOptions?.map(entry),
    dashboard: s.dashboard ? dashboard(s.dashboard) : s.dashboard,
  };
}
