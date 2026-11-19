// The panel in Chinese or English (外观与设置 → 语言). Chinese is the source: the engine and the panel write Chinese,
// and English comes from the dictionary in en.ts at the edges. localize() turns a snapshot's text into English as it
// arrives, so every page lays out and measures the English it shows; t() does the panel's own words. Text the
// dictionary doesn't know (a session's title, a project's folder, a new message) stays as it is; the English test lists
// any of the panel's own text left untranslated.
import { phrases, rules, whole } from './en';
import { phoneRow } from './phone.svelte';
import { shortcutRow } from './shortcut.svelte';
import type { Dashboard, Entry, ProviderUsage, Snapshot } from './types';

export type Language = 'zh' | 'en';
const KEY = 'starmeter.language';

function stored(): Language {
  try {
    // A phone opened from 在手机上看 starts in the computer's language (…&l=en in the address's fragment).
    const asked = typeof location === 'undefined' ? null : new URLSearchParams(location.hash.slice(1)).get('l');
    if (asked === 'en' || asked === 'zh') return asked;
    return typeof localStorage !== 'undefined' && localStorage.getItem(KEY) === 'en' ? 'en' : 'zh';
  } catch {
    return 'zh';
  }
}

export const language = $state({ current: stored() });
export const english = () => language.current === 'en';

export function chooseLanguage(next: string) {
  language.current = next === 'en' ? 'en' : 'zh';
  try { localStorage.setItem(KEY, language.current); } catch { /* the choice lasts this session */ }
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
  return values ? text.replace(/\{(\w+)\}/g, (whole, name) => (name in values ? String(values[name]) : whole)) : text;
}

/** Whether the engine's text reads as this Chinese phrase, in either language. */
export const reads = (text: string | null | undefined, zh: string) => text === zh || (text !== null && text !== undefined && text === translate(zh));

/** What a row said in Chinese before it was translated: the panel's logic reads rows by their Chinese wording. */
export const source = (e: Entry) => e.source ?? e;

function entry(e: Entry): Entry {
  const label = translate(e.label), value = e.value === undefined ? undefined : translate(e.value);
  const detail = e.detail === undefined ? undefined : translate(e.detail);
  const changed = label !== e.label || value !== e.value || detail !== e.detail;
  return {
    ...e, label, value, detail,
    ...(e.fallbacks ? { fallbacks: e.fallbacks.map(translate) } : {}),
    ...(changed ? { source: { label: e.label, value: e.value, detail: e.detail } } : {}),
    children: e.children.map(entry),
  };
}

function provider(p: ProviderUsage): ProviderUsage {
  return {
    ...p, name: translate(p.name), subtitle: translate(p.subtitle), status: translate(p.status),
    priceNote: p.priceNote ? translate(p.priceNote) : p.priceNote, serviceNote: p.serviceNote ? translate(p.serviceNote) : p.serviceNote,
    models: p.models.map((m) => ({ ...m, name: translate(m.name), priceNote: m.priceNote ? translate(m.priceNote) : m.priceNote })),
    clients: p.clients ? p.clients.map((c) => ({ ...c, name: translate(c.name) })) : p.clients,
  };
}

function dashboard(d: Dashboard): Dashboard {
  const h = d.heatmap;
  return {
    ...d, rangeLabel: translate(d.rangeLabel), chartUnit: translate(d.chartUnit), chartSpan: translate(d.chartSpan),
    chartStatus: translate(d.chartStatus), updated: d.updated === undefined ? undefined : translate(d.updated),
    dates: d.dates.map(translate), period: d.period?.map(translate), groupNote: d.groupNote ? translate(d.groupNote) : d.groupNote,
    groupings: d.groupings?.map((g) => ({ ...g, label: translate(g.label) })),
    currency: d.currency ? { ...d.currency, note: d.currency.note === undefined ? undefined : translate(d.currency.note) } : d.currency,
    providers: d.providers.map(provider), cards: d.cards ? d.cards.map(provider) : d.cards,
    heatmap: h ? {
      ...h, rowLabels: h.rowLabels?.map(translate), labels: h.labels.map(translate), hint: translate(h.hint),
      status: h.status === undefined ? undefined : translate(h.status), notice: h.notice === undefined ? undefined : translate(h.notice),
      streak: h.streak ? translate(h.streak) : h.streak,
    } : h,
  };
}

/** The 语言 row, first under 显示 in 外观与设置: 中文 / English. The choice is the panel's own (kept in the web view's
 *  storage), not the engine's, so the engine's other output (menus, notifications) stays Chinese. The 新手指引 row
 *  under 帮助 is the panel's own too. */
function withLanguage(list: Entry[]): Entry[] {
  return list.map((e) => {
    if (e.label !== '外观与设置') return e;
    const children = [...e.children], at = children.findIndex((c) => c.section && c.label === '显示');
    const radio = (label: string, key: Language): Entry => ({ label, children: [], radio: true, checked: language.current === key, action: ['set-language', key] });
    children.splice(at + 1, 0, { label: '语言 Language', symbol: 'globe', value: language.current === 'en' ? 'English' : '中文',
      children: [radio('中文', 'zh'), radio('English', 'en')] }, shortcutRow(), phoneRow());
    // 帮助, last: the first-run tour, again.
    children.push({ label: '帮助', section: true, children: [] }, { label: '新手指引', symbol: 'questionmark.circle', action: ['show-guide'], children: [] });
    return { ...e, children };
  });
}

/** The snapshot as the panel shows it: the language row added, and in English every text the dictionary knows. */
export function localize(s: Snapshot): Snapshot {
  const options = s.moduleOptions ? withLanguage(s.moduleOptions) : s.moduleOptions;
  if (!english()) return { ...s, moduleOptions: options };
  return {
    ...s, entries: s.entries.map(entry), moduleOptions: options?.map(entry), rangeOptions: s.rangeOptions?.map(entry),
    dashboard: s.dashboard ? dashboard(s.dashboard) : s.dashboard,
  };
}
