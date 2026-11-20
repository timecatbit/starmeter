// The fixture backend for a plain browser (dev server, Playwright). It serves app/ui/fixtures/*.json and keeps the
// choices made in the panel, as the Tauri app's fixture backend (app/src-tauri/src/fixtures.rs) does; the two follow
// the same rules. Pick a scenario with ?fixture=failed, ?fixture=pending or ?fixture=empty (a first run that found nothing).
import type { ActionReceipt, Dashboard, Entry, PresentationPreferences, ProviderUsage, Question, Snapshot } from './types';
import { addPhrases } from './i18n.svelte';

// The made-up session titles in English, for the English screenshots; a real title is the user's own and stays as it is.
addPhrases({ 重做设置页的分段控件: 'Redo the settings page segments', 修订论文的评估章节: 'Revise the evaluation section',
  迁移博客的图片目录: "Move the blog's image folder" });

interface Fixture {
  snapshot: Snapshot;
  entryModules: (string | null)[];
  dashboards: Record<string, Dashboard>;
  cards: Record<string, { model: ProviderUsage[]; project: ProviderUsage[] }>;
}

const files = import.meta.glob<Fixture>('../../fixtures/*.json', { import: 'default' });
const logos = import.meta.glob<string>('../../../../assets/**/*.svg', { query: '?raw', import: 'default', eager: true });

const scenario = new URLSearchParams(location.search).get('fixture') ?? 'default';
const toBase64 = (text: string) => btoa(String.fromCharCode(...new TextEncoder().encode(text)));
const logoData: Record<string, string> = {};
for (const [path, svg] of Object.entries(logos)) logoData[path.replace(/^.*\/assets\//, '')] = toBase64(svg);

/** The fixtures name logos as logo:<file>; the engine sends the file itself, base64. */
function expand<T>(value: T): T {
  if (Array.isArray(value)) return value.map(expand) as T;
  if (value && typeof value === 'object') {
    const out: Record<string, unknown> = {};
    for (const [key, field] of Object.entries(value)) {
      out[key] = key === 'image' && typeof field === 'string' && field.startsWith('logo:') ? logoData[field.slice(5)] ?? '' : expand(field);
    }
    return out as T;
  }
  return value;
}

let data: Fixture | null = null;
async function load(): Promise<Fixture> {
  if (data) return data;
  const file = files[`../../fixtures/${scenario}.json`] ?? files['../../fixtures/default.json'];
  data = expand(await file());
  state.prefs = structuredClone(data.snapshot.preferences!);
  return data;
}

const state = {
  range: 'today',
  grouping: 'tool',
  prefs: null as PresentationPreferences | null,
  pinInterval: 2,
  menuBar: 'none',
  cny: false,
  live: 0,
  /** A just-picked range is still missing Magpie for a moment (the engine reads its sources one by one). */
  settling: false,
};
const listeners = new Set<() => void>();
export const onChange = (changed: () => void) => { listeners.add(changed); return () => listeners.delete(changed); };
const changed = () => setTimeout(() => listeners.forEach((f) => f()), 0);

const same = (a: string[] | undefined, b: string[]) => !!a && a.length === b.length && a.every((x, i) => x === b[i]);

/** A switch or a choice, saved: the same rule the panel shows it by. */
function applyChoice(list: Entry[], action: string[]): boolean {
  const index = list.findIndex((e) => same(e.action, action));
  if (index >= 0) {
    const entry = list[index];
    if (entry.radio) list.forEach((e, i) => { if (e.radio && e.action?.[0] === action[0]) e.checked = i === index; });
    else if (entry.checked !== undefined && (action.at(-1) === 'true' || action.at(-1) === 'false')) {
      entry.checked = action.at(-1) === 'true';
      entry.action = [...action.slice(0, -1), action.at(-1) === 'true' ? 'false' : 'true'];
    }
    return true;
  }
  return list.some((e) => applyChoice(e.children, action));
}

/** A parent row's value follows its chosen child (金额显示 · 人民币, 钉住时刷新 · 每 5 秒). */
function syncValues(list: Entry[]) {
  for (const e of list) {
    const chosen = e.children.find((c) => c.radio && c.checked);
    if (chosen && e.value !== undefined) e.value = chosen.label.replace(/（.*）$/, '');
    syncValues(e.children);
  }
}

/** Every range that reaches now creeps up on each live tick (what each tick adds adds up), so the pinned chart has
 *  something to draw whatever the range; yesterday stays as it was. */
function bump(d: Dashboard) {
  if (!state.live || d.rangeKey === 'yesterday') return;
  const grow: Record<string, number> = { claude: 41_000, codex: 23_000, magpie: 9_000 };
  let ticks = 0;
  for (let k = 1; k <= state.live; k++) ticks += 0.6 + ((k * 7919) % 13) / 13;
  for (const p of d.providers) {
    const step = grow[p.id];
    if (!step) continue;
    const added = step * ticks;
    p.tokens += added;
    // Most of what a request adds is context read from the cache; a sliver is what the model wrote.
    p.output += added * 0.004;
    if (p.cost !== null) p.cost += added * 0.38e-6;
    p.requests += Math.round(state.live / 3);
    p.history[p.history.length - 1] += added;
    if (p.counted) { p.counted.tokens += added; p.counted.history[p.counted.history.length - 1] += added; }
  }
}

export async function dashboard(range: string): Promise<Dashboard> {
  const f = await load(), prefs = state.prefs!;
  const d = structuredClone(f.dashboards[range] ?? f.dashboards.today);
  bump(d);
  if (state.settling) {
    // Magpie not read yet: its card waits, and the clients it carried count all their own requests for now.
    for (const p of d.providers) {
      if (p.id === 'magpie') { p.available = false; p.status = '待同步'; }
      else delete p.counted;
    }
  }
  d.providers = d.providers.filter((p) => prefs.visible.includes(p.id));
  d.showSummary = prefs.visible.includes('summary');
  if (!prefs.visible.includes('heatmap')) d.heatmap = null;
  d.allowCollapse = prefs.allowCollapse;
  d.expandedProviders = prefs.expandedProviders;
  d.grouping = state.grouping;
  if (state.grouping !== 'tool' && d.providers.length) {
    d.cards = structuredClone(f.cards[range]?.[state.grouping as 'model' | 'project'] ?? []);
    d.groupNote = state.grouping === 'project' ? '按客户端自己的记录' : null;
  }
  if (state.cny) d.currency = { code: 'CNY', symbol: '¥', rate: 7.1, note: '欧洲央行 2026-11-18 参考汇率 7.1000' };
  return d;
}

export async function snapshot(): Promise<Snapshot> {
  const f = await load(), prefs = state.prefs!;
  const s = structuredClone(f.snapshot);
  const entries: Entry[] = [];
  f.snapshot.entries.forEach((e, i) => {
    const key = f.entryModules[i];
    if (key && !prefs.visible.includes(key)) return;
    if (e.separator && (!entries.length || entries.at(-1)!.separator)) return;
    entries.push(s.entries[i]);
  });
  while (entries.at(-1)?.separator) entries.pop();
  s.entries = entries;
  s.dashboard = await dashboard(state.range);
  s.rangeOptions = s.rangeOptions?.map((o) => ({ ...o, checked: o.action?.[1] === state.range }));
  s.preferences = structuredClone(prefs);
  s.appearance = prefs.appearance;
  s.theme = prefs.theme;
  s.pinInterval = state.pinInterval;
  const today = state.menuBar === 'cost' ? (await dashboard('today')).providers.reduce((sum, p) => sum + ((p.counted ?? p).cost ?? 0), 0) : 0;
  s.menuBarText = state.menuBar === 'cost' ? (state.cny ? '¥' + (today * 7.1).toFixed(2) : '$' + today.toFixed(2)) : state.menuBar === 'quota' ? '91%' : state.menuBar === 'speed' ? '42 tok/s' : '';
  for (const option of s.moduleOptions ?? []) {
    if (option.action?.[0] === 'set-module') {
      const on = prefs.visible.includes(option.action[1]);
      option.checked = on;
      option.action = ['set-module', option.action[1], on ? 'false' : 'true'];
    }
    if (option.action?.[0] === 'set-collapse') { option.checked = prefs.allowCollapse; option.action = ['set-collapse', prefs.allowCollapse ? 'false' : 'true']; }
  }
  const pages = (s.moduleOptions ?? []).filter((o) => ['外观', '通用', '提醒', '数据'].includes(o.label));
  const settings = pages.flatMap((page) => page.children);
  for (const row of settings) {
    if (row.action?.[0] === 'set-theme') row.checked = row.action[1] === prefs.theme;
    if (row.action?.[0] === 'set-appearance') row.checked = row.action[1] === prefs.appearance;
  }
  const exporting = settings.find((row) => row.label === '导出用量');
  if (exporting) exporting.value = s.dashboard.rangeLabel;
  return s;
}

const wait = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

/** Asks the panel as the core does in the app: a question event, then the answer the panel sends back. */
let asked = 0;
function askPanel(question: Omit<Question, 'id'>): Promise<unknown> {
  const id = ++asked;
  return new Promise((resolve) => {
    const listener = (event: Event) => {
      const detail = (event as CustomEvent<{ id: number; answer: unknown }>).detail;
      if (detail.id !== id) return;
      window.removeEventListener('panel-answer', listener);
      resolve(detail.answer);
    };
    window.addEventListener('panel-answer', listener);
    window.dispatchEvent(new CustomEvent('panel-ask', { detail: { ...question, id } }));
  });
}
const findEntry = (list: Entry[], match: (e: Entry) => boolean): Entry | null => {
  for (const e of list) { if (match(e)) return e; const inner = findEntry(e.children, match); if (inner) return inner; }
  return null;
};

export async function action(action: string[]): Promise<ActionReceipt> {
  const f = await load(), prefs = state.prefs!;
  const [name, a, b] = action;
  const saved = () => { prefs.revision += 1; changed(); return { status: 'done' as const, preferences: structuredClone(prefs) }; };
  switch (name) {
    case 'set-range':
      state.range = a;
      state.settling = true;
      changed();
      setTimeout(() => { state.settling = false; changed(); }, 300);
      return { status: 'done' };
    case 'set-group': state.grouping = a; changed(); return { status: 'done' };
    case 'set-module':
      prefs.visible = b === 'true' ? [...new Set([...prefs.visible, a])] : prefs.visible.filter((v) => v !== a);
      return saved();
    case 'set-collapse': prefs.allowCollapse = a === 'true'; if (prefs.allowCollapse) prefs.expandedProviders = []; return saved();
    case 'set-expanded': prefs.expandedProviders = b === 'true' ? [...new Set([...prefs.expandedProviders, a])].sort() : prefs.expandedProviders.filter((v) => v !== a); return saved();
    case 'set-theme': prefs.theme = a; return saved();
    case 'set-appearance': prefs.appearance = a; return saved();
    case 'reset-modules': prefs.visible = structuredClone(f.snapshot.preferences!.visible); prefs.theme = 'system'; prefs.appearance = 'solid'; return saved();
    case 'refresh-live': state.live += 1; changed(); return { status: 'done' };
    case 'test-api-key': await wait(1200); return { status: 'done' };
    case 'set-subscription-price': {
      const row = findEntry(f.snapshot.entries, (e) => e.action?.[0] === name && e.action[1] === a);
      const current = row?.value?.startsWith('$') ? row.value.slice(1) : '';
      const typed = await askPanel({ kind: 'text', title: '订阅月费', detail: '每月多少美元？用来算本月用量是订阅价的几倍。留空清除。', value: current });
      if (typeof typed !== 'string') return { status: 'done' };
      const value = Number(typed.trim().replace(/^\$/, ''));
      if (typed.trim() && !(value > 0)) { window.dispatchEvent(new CustomEvent('panel-ask', { detail: { kind: 'notice', id: 0, title: '金额无效', detail: '请填写大于 0 的数字（美元），或留空清除。' } })); return { status: 'done' }; }
      if (row) row.value = typed.trim() ? '$' + value.toFixed(2) : '未填写';
      changed();
      return { status: 'done' };
    }
    case 'remove-api-key': {
      const yes = await askPanel({ kind: 'confirm', title: '删除这把 Key？', detail: '只删除 Starmeter 自己保存的副本，不影响服务商那边的 Key。', accept: '删除' });
      if (yes === true) console.info('[fixtures] removed', a, b);
      return { status: 'done' };
    }
  }
  if (name === 'set-pin-interval') state.pinInterval = Number(a);
  if (name === 'set-menu-bar') state.menuBar = a;
  if (name === 'set-currency') state.cny = a === 'CNY';
  if (applyChoice(f.snapshot.entries, action) || applyChoice(f.snapshot.moduleOptions ?? [], action)) {
    syncValues(f.snapshot.moduleOptions ?? []);
    changed();
  }
  console.info('[fixtures] action', action);
  return { status: 'done' };
}
