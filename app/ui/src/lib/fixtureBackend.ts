// The fixture backend for a plain browser (dev server, Playwright). It serves app/ui/fixtures/*.json and keeps the
// choices made in the panel, as the Tauri app's fixture backend (app/src-tauri/src/fixtures.rs) does; the two follow
// the same rules. Pick a scenario with ?fixture=failed or ?fixture=pending.
import type { ActionReceipt, Dashboard, Entry, PresentationPreferences, ProviderUsage, Snapshot } from './types';

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

/** Today's figures creep up on each live tick, so the pinned chart has something to draw. */
function bump(d: Dashboard) {
  if (!state.live || d.rangeKey !== 'today') return;
  const grow: Record<string, number> = { claude: 41_000, codex: 23_000, magpie: 9_000 };
  for (const p of d.providers) {
    const step = grow[p.id];
    if (!step) continue;
    const added = step * state.live * (0.6 + ((state.live * 7919) % 13) / 13);
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
  s.menuBarText = state.menuBar === 'cost' ? (state.cny ? '¥' + (today * 7.1).toFixed(2) : '$' + today.toFixed(2)) : state.menuBar === 'quota' ? '91%' : '';
  for (const option of s.moduleOptions ?? []) {
    if (option.action?.[0] === 'set-module') {
      const on = prefs.visible.includes(option.action[1]);
      option.checked = on;
      option.action = ['set-module', option.action[1], on ? 'false' : 'true'];
    }
    if (option.action?.[0] === 'set-collapse') { option.checked = prefs.allowCollapse; option.action = ['set-collapse', prefs.allowCollapse ? 'false' : 'true']; }
  }
  const settings = s.moduleOptions?.find((o) => o.label === '外观与设置');
  for (const row of settings?.children ?? []) {
    if (row.action?.[0] === 'set-theme') row.checked = row.action[1] === prefs.theme;
    if (row.action?.[0] === 'set-appearance') row.checked = row.action[1] === prefs.appearance;
  }
  const exporting = settings?.children.find((row) => row.label === '导出用量');
  if (exporting) exporting.value = s.dashboard.rangeLabel;
  return s;
}

const wait = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

export async function action(action: string[]): Promise<ActionReceipt> {
  const f = await load(), prefs = state.prefs!;
  const [name, a, b] = action;
  const saved = () => { prefs.revision += 1; changed(); return { status: 'done' as const, preferences: structuredClone(prefs) }; };
  switch (name) {
    case 'set-range': state.range = a; changed(); return { status: 'done' };
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
