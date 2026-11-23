#!/usr/bin/env node
// Builds the panel's fixtures: snapshots and dashboards in the 5.x engine's JSON (PanelSnapshot, Dashboard), from
// made-up usage. Nothing here comes from a real machine: the usage is a seeded random walk, the paths, keys, servers
// and sessions are invented. Run `node app/ui/fixtures/make-fixtures.mjs` after changing it; the JSON it writes is
// what the Tauri app's fixture backend and the UI's browser fallback both serve.
import { readFileSync, writeFileSync } from 'node:fs';

const root = new URL('../../../', import.meta.url);
const out = new URL('./', import.meta.url);
// Logos are written as "logo:<file under assets/>"; the fixture backends put the file's base64 there when they load,
// so each logo is stored once rather than in every row and card.
const assetExists = (file) => readFileSync(new URL('assets/' + file, root)).length > 0;

// MARK: Icons, as moduleIcon() gives them

const brandLogos = { ChatGPT: ['openai.svg', true], Claude: ['claude.svg', false], MiniMax: ['minimax.svg', false], Gemini: ['gemini.svg', false],
  Grok: ['grok.svg', true], Kimi: ['kimi.svg', true], Qoder: ['qoder.svg', true], Cursor: ['cursor.svg', true] };
const moduleLogos = { magpie: ['magpie.svg', true], claude: ['claude.svg', false], codex: ['openai.svg', true], opencode: ['opencode.svg', true],
  workbuddy: ['workbuddy.svg', true], qoder: ['qoder.svg', true], gemini: ['gemini.svg', false], cursor: ['cursor.svg', true] };
const moduleSymbols = { summary: 'chart.xyaxis.line', heatmap: 'square.grid.3x3.fill', opencodex: 'network', komari: 'server.rack', updates: 'arrow.down.circle', sessions: 'terminal' };
// In the engine's order (catalog.rs, preferences.rs): the gateways first, then every brand by its name, the same in
// each group.
const services = [
  ['chatgpt', 'ChatGPT', '订阅'], ['claude-subscription', 'Claude', '订阅'], ['cursor-subscription', 'Cursor', '订阅'], ['gemini-subscription', 'Gemini', '订阅'],
  ['grok-subscription', 'Grok', '订阅'], ['kimi-subscription', 'Kimi', '订阅'], ['minimax', 'MiniMax', '订阅'], ['qoder-subscription', 'Qoder', '订阅'],
  ['chatgpt-api', 'ChatGPT', 'API'], ['claude-api', 'Claude', 'API'], ['deepseek', 'DeepSeek', 'API'], ['gemini-api', 'Gemini', 'API'],
  ['grok', 'Grok', 'API'], ['kimi-api', 'Kimi', 'API'], ['minimax-api', 'MiniMax', 'API'], ['qoder-api', 'Qoder', 'API'],
];
const harnessOrder = ['opencodex', 'magpie', 'codex', 'claude', 'cursor', 'deepseek-harness', 'gemini', 'opencode', 'qoder', 'workbuddy'];
const serviceOrder = services.map(([key]) => key);
/** Rows of one group in that order, by the module each opens. */
const inOrder = (rows) => [...rows].sort((a, b) => serviceOrder.indexOf(a.onOpenAction[1]) - serviceOrder.indexOf(b.onOpenAction[1]));
const serviceByKey = Object.fromEntries(services.map(([key, brand, category]) => [key, { key, brand, category, label: brand + (category === '订阅' ? ' 订阅' : ' API') }]));
const logo = (file) => { if (!assetExists(file)) throw new Error('missing ' + file); return 'logo:' + file; };
function moduleIcon(key) {
  const service = serviceByKey[key];
  if (service) {
    if (service.brand === 'DeepSeek') return { image: logo('deepseek-logo.svg'), template: false };
    const brand = brandLogos[service.brand];
    if (brand) return { image: logo('logos/' + brand[0]), template: brand[1] };
    return { symbol: service.category === '订阅' ? 'person.crop.circle' : 'creditcard' };
  }
  if (key === 'deepseek-harness') return { image: logo('deepseek-logo.svg'), template: false };
  const mark = moduleLogos[key];
  if (mark) return { image: logo('logos/' + mark[0]), template: mark[1] };
  return { symbol: moduleSymbols[key] ?? 'square.grid.2x2' };
}

// MARK: Made-up usage

function random(seed) {
  return () => {
    seed |= 0; seed = (seed + 0x6d2b79f5) | 0;
    let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}
const HOUR = 3600, DAY = 86400;
// Wall-clock time kept as if it were UTC, so a fixture reads the same in every time zone.
const NOW = Date.UTC(2026, 10, 19, 15, 40) / 1000;
const dayOf = (moment) => Math.floor(moment / DAY);
const TODAY = dayOf(NOW);
const civil = (day) => { const d = new Date(day * DAY * 1000); return [d.getUTCFullYear(), d.getUTCMonth() + 1, d.getUTCDate()]; };
const pad = (n) => String(n).padStart(2, '0');
const iso = (day) => { const [y, m, d] = civil(day); return `${y}-${pad(m)}-${pad(d)}`; };
const isoDay = (text) => Date.UTC(+text.slice(0, 4), +text.slice(5, 7) - 1, +text.slice(8, 10)) / 1000 / DAY;
const weekdays = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'];
const weekday = (day) => ((day % 7) + 7 + 3) % 7;
const hm = (moment) => { const d = new Date(moment * 1000); return `${pad(d.getUTCHours())}:${pad(d.getUTCMinutes())}`; };
const mdhm = (moment) => { const d = new Date(moment * 1000); return `${pad(d.getUTCMonth() + 1)}/${pad(d.getUTCDate())} ${hm(moment)}`; };
const isoMinute = (moment) => new Date(moment * 1000).toISOString().slice(0, 16);
const dayName = (day) => { const [, m, d] = civil(day); return `${m}月${d}日 ${weekdays[weekday(day)]}`; };
const round6 = (x) => Math.round(x * 1e6) / 1e6;
const capped = (x) => (x < 1 ? x : 1);

// Each tool: when it started, how much it uses on a working day, what a token costs, and its models' shares.
const tools = [
  { id: 'magpie', name: 'Magpie', start: '2026-06-10', base: 15e6, rate: 0.31e-6, perM: 3.4, output: 0.018, cache: 0.88, sessions: 0,
    models: [['gpt-5.5-codex', 0.57], ['claude-sonnet-5-5', 0.32], ['deepseek-v4', 0.11]],
    clients: [['codex', 0.62], ['claude-code', 0.3], ['pi', 0.08]] },
  { id: 'claude', name: 'Claude', start: '2026-03-02', base: 68e6, rate: 0.36e-6, perM: 2.9, output: 0.0034, cache: 0.972, sessions: 3,
    models: [['claude-opus-5-5', 0.9], ['claude-fable-5-2', 0.07], ['claude-haiku-5-5', 0.03]],
    projects: [['starmeter', 0.46], ['harbor-api', 0.24], ['dotfiles', 0.08], ['field-notes', 0.22]] },
  { id: 'codex', name: 'ChatGPT', start: '2026-04-01', base: 27e6, rate: 0.42e-6, perM: 3.1, output: 0.021, cache: 0.913, sessions: 2, carried: 0.3,
    models: [['gpt-5.5-codex', 0.81], ['gpt-5.5', 0.19]],
    projects: [['starmeter', 0.52], ['ChatGPT 对话', 0.18], ['harbor-api', 0.3]] },
  { id: 'opencode', name: 'OpenCode', start: '2026-07-20', base: 6.5e6, rate: 0.17e-6, perM: 4.2, output: 0.03, cache: 0.81, sessions: 1, quiet: 0.35,
    models: [['kimi-k2.5', 0.66], ['glm-5', 0.34]], projects: [['field-notes', 1]] },
  { id: 'qoder', name: 'Qoder', start: '2026-09-01', credits: 14, perCredit: 0.7, sessions: 1, quiet: 0.45,
    models: [['qfmodel-pro', 0.72], ['qwen3-coder-plus', 0.28]], projects: [['harbor-api', 1]] },
  { id: 'gemini', name: 'Gemini', start: '2026-05-12', base: 4.2e6, rate: 0.3e-6, perM: 5, output: 0.04, cache: 0.62, sessions: 1, quiet: 0.3, unpriced: 0.07,
    models: [['gemini-3-pro', 0.74], ['gemini-3-flash', 0.19], ['gemini-3.5-preview', 0.07]] },
  { id: 'cursor', name: 'Cursor', start: '2026-08-15', base: 9e6, rate: 0.4e-6, perM: 3.6, output: 0.026, cache: 0.84, sessions: 1, quiet: 0.25,
    models: [['composer-2', 0.6], ['claude-sonnet-5-5', 0.4]] },
];
const profile = [0.12, 0.05, 0.02, 0, 0, 0, 0, 0.08, 0.45, 0.9, 1, 1, 0.65, 0.8, 1, 1, 0.92, 0.8, 0.55, 0.62, 0.8, 0.72, 0.5, 0.3];
const FIRST = isoDay('2026-03-02');
const HOURS = (TODAY - FIRST + 1) * 24;

// A few days nobody worked, so streaks end somewhere.
const restDays = new Set();
{ const rand = random(7); for (let day = FIRST; day < TODAY; day++) if (rand() < 0.045) restDays.add(day); }
const usage = {};
for (const [index, tool] of tools.entries()) {
  const rand = random(1000 + index * 77);
  const tokens = new Float64Array(HOURS), credits = new Float64Array(HOURS);
  const start = isoDay(tool.start);
  for (let day = FIRST; day <= TODAY; day++) {
    if (day < start) continue;
    const busy = day === TODAY && !tool.quiet;
    const off = !busy && (restDays.has(day) || rand() < 0.07 + (weekday(day) >= 5 ? 0.25 : 0) + (tool.quiet ?? 0));
    const factor = off ? 0 : busy ? 1.1 : (0.35 + rand() * 1.3) * (weekday(day) >= 5 ? 0.55 : 1);
    const sum = profile.reduce((a, b) => a + b, 0);
    for (let h = 0; h < 24; h++) {
      const at = (day * 24 + h) * HOUR;
      if (at > NOW || (day === TODAY && h < 8)) continue;
      const noise = 0.55 + rand() * 0.9;
      const slot = (day - FIRST) * 24 + h;
      if (tool.credits) credits[slot] = Math.round(tool.credits * factor * (profile[h] / sum) * 24 * noise * 10) / 10;
      else tokens[slot] = Math.round(tool.base * factor * (profile[h] / sum) * noise);
    }
  }
  // The hour still running is only part done.
  const last = (TODAY - FIRST) * 24 + 15;
  tokens[last] = Math.round(tokens[last] * 0.66);
  credits[last] = Math.round(credits[last] * 6.6) / 10;
  usage[tool.id] = { tokens, credits };
}
const slotOf = (moment) => Math.floor(moment / HOUR) - FIRST * 24;

// What a tool used from one moment to another: the summary a range bundle carries.
function tally(tool, start, end) {
  const { tokens, credits } = usage[tool.id];
  let total = 0, credit = 0, active = new Set();
  for (let slot = Math.max(0, slotOf(start)); slot <= Math.min(HOURS - 1, slotOf(end)); slot++) {
    total += tokens[slot]; credit += credits[slot];
    if (tokens[slot] > 0 || credits[slot] > 0) active.add(Math.floor(slot / 24));
  }
  const requests = tool.credits ? Math.round(credit / tool.perCredit) : Math.round((total / 1e6) * tool.perM);
  const unpriced = tool.credits ? requests : Math.round(requests * (tool.unpriced ?? 0));
  const cost = tool.credits ? null : total * tool.rate * (1 - (tool.unpriced ?? 0));
  const days = active.size;
  return { tokens: total, credits: credit, requests, unpriced, cost, output: Math.round(total * (tool.output ?? 0)), cache: tool.cache ?? null,
    sessions: Math.max(days ? 1 : 0, Math.round(days * (tool.sessions ?? 1) * (days > 3 ? 1.6 : 1))) };
}
const slotTokens = (tool, a, b) => {
  const { tokens, credits } = usage[tool.id];
  let sum = 0;
  for (let slot = Math.max(0, slotOf(a)); slot <= Math.min(HOURS - 1, slotOf(b)); slot++) sum += tool.credits ? 0 * credits[slot] : tokens[slot];
  return sum;
};

// MARK: Dashboards, in dashboardPayload()'s shape

const ranges = [['today', '今天'], ['yesterday', '昨天'], ['24h', '近 24 小时'], ['7d', '近 7 天'], ['30d', '近 30 天'], ['week', '本周'], ['month', '本月'], ['all', '全部']];
const shortRanges = new Set(['today', 'yesterday', '24h']);
const hourlyHeatmap = new Set(['today', 'yesterday', '24h', '7d', 'week']);
const streakRanges = new Set(['7d', 'week', '30d', 'month', 'all']);

function bounds(key) {
  const midnight = TODAY * DAY;
  switch (key) {
    case 'today': return [midnight, NOW];
    case 'yesterday': return [midnight - DAY, midnight - 0.001];
    case '24h': return [NOW - DAY, NOW];
    case '7d': return [midnight - 6 * DAY, NOW];
    case '30d': return [midnight - 29 * DAY, NOW];
    case 'week': return [midnight - weekday(TODAY) * DAY, NOW];
    case 'month': { const [y, m] = civil(TODAY); return [Date.UTC(y, m - 1, 1) / 1000, NOW]; }
    default: return [0, NOW];
  }
}

function modelRows(tool, part) {
  const byCredits = part.tokens === 0 && part.credits !== 0;
  return tool.models.slice(0, 3).map(([name, share]) => {
    const row = { name, tokens: byCredits ? 0 : Math.round(part.tokens * share), share };
    if (byCredits) row.credits = Math.round(part.credits * share * 10) / 10;
    if (tool.unpriced && name === 'gemini-3.5-preview') row.priceNote = '未定价';
    return row;
  });
}

function card(tool, part, history, status = '已同步') {
  const card = { id: tool.id, name: tool.name, ...moduleIcon(tool.id) };
  card.subtitle = '';
  card.status = status;
  card.available = true;
  card.credits = part.credits ? Math.round(part.credits * 10) / 10 : null;
  card.clients = tool.clients && part.tokens ? tool.clients.map(([agent, share]) => ({ name: { codex: 'ChatGPT', 'claude-code': 'Claude Code', pi: 'Pi' }[agent], tokens: Math.round(part.tokens * share), share })) : null;
  card.cost = part.cost === null || (part.requests && part.unpriced >= part.requests) ? null : round6(part.cost);
  card.tokens = part.tokens;
  card.requests = part.requests;
  card.output = part.output;
  card.cache = part.tokens ? part.cache : null;
  card.coverage = part.requests ? (part.requests - part.unpriced) / part.requests : 0;
  card.sessions = part.sessions;
  card.unpriced = part.unpriced;
  card.models = part.requests ? modelRows(tool, part) : [];
  card.history = history;
  card.priceNote = part.unpriced > 0 && part.unpriced < part.requests && !part.credits ? `${part.unpriced} 次未定价` : null;
  return card;
}

function heatmap(key, start, end, selected) {
  let cells = [], columns, names;
  if (hourlyHeatmap.has(key)) {
    let moments = [];
    if (key === '24h') {
      const base = Math.floor(NOW / HOUR) * HOUR - 23 * HOUR;
      for (let i = 0; i < 24; i++) moments.push(base + i * HOUR);
      names = ['24小时'];
    } else {
      const back = { today: 0, yesterday: 1, '7d': 6, week: weekday(TODAY) }[key];
      const rows = { today: 1, yesterday: 1, '7d': 7, week: weekday(TODAY) + 1 }[key];
      const first = TODAY - back;
      for (let d = 0; d < rows; d++) for (let h = 0; h < 24; h++) moments.push((first + d) * DAY + h * HOUR);
      names = { today: ['今天'], yesterday: ['昨天'] }[key] ?? Array.from({ length: rows }, (_, i) => weekdays[weekday(first + i)]);
    }
    cells = moments.map((moment) => ({ from: moment, to: moment + HOUR - 1, label: `${dayName(dayOf(moment))} ${hm(moment)}`, later: moment > NOW }));
    columns = 24;
  } else {
    let first, count;
    if (key === '30d') { first = TODAY - 29; count = 30; }
    else if (key === 'month') { const [y, m] = civil(TODAY); first = Date.UTC(y, m - 1, 1) / 1000 / DAY; count = new Date(Date.UTC(y, m, 0)).getUTCDate(); }
    else {
      const since = [];
      for (let day = TODAY - 359; day <= TODAY; day++) if (selected.some((t) => tally(t, day * DAY, day * DAY + DAY - 1).requests > 0)) since.push(day);
      const span = since.length ? TODAY - since[0] + 1 : 1;
      count = Math.min(360, Math.ceil(span / 30) * 30);
      first = TODAY - (count - 1);
    }
    for (let i = 0; i < count; i++) cells.push({ from: (first + i) * DAY, to: (first + i) * DAY + DAY - 1, label: dayName(first + i), later: first + i > TODAY });
    columns = key === 'all' ? 30 : count;
    if (key === 'all') {
      const starts = [];
      for (let i = 0; i < count; i += 30) starts.push(civil(first + i)[1]);
      names = starts.map((m, i) => (i === 0 || m !== starts[i - 1] ? `${m}月` : ''));
    } else names = [key === '30d' ? '30天' : `${civil(TODAY)[1]}月`];
  }
  const values = cells.map((cell) => {
    let tokens = 0, requests = 0, cost = 0, unpriced = 0;
    if (!cell.later) for (const tool of selected) {
      const part = tally(tool, Math.max(cell.from, start), Math.min(cell.to, end));
      tokens += part.tokens; requests += part.requests; cost += part.cost ?? 0; unpriced += part.unpriced;
    }
    return { tokens, requests, cost, unpriced };
  });
  let streak = null;
  if (streakRanges.has(key)) {
    const used = [];
    for (let day = dayOf(Math.max(start, FIRST * DAY)); day <= TODAY; day++) if (selected.some((t) => tally(t, day * DAY, day * DAY + DAY - 1).requests > 0)) used.push(day);
    let longest = 0, run = 0, previous = null;
    for (const day of used) { run = previous !== null && day - previous === 1 ? run + 1 : 1; longest = Math.max(longest, run); previous = day; }
    const present = new Set(used);
    let current = 0, cursor = present.has(TODAY) ? TODAY : TODAY - 1;
    while (present.has(cursor)) { current++; cursor--; }
    if (longest >= 2) streak = current >= 2 ? `连续 ${current} 天 · 最长 ${longest} 天` : `最长连续 ${longest} 天`;
  }
  return { columns, rowLabels: names, labels: cells.map((c) => c.label), tokens: values.map((v) => v.tokens), requests: values.map((v) => v.requests),
    costs: values.map((v) => round6(v.cost)), unpriced: values.map((v) => v.unpriced), future: cells.filter((c) => c.later).length, streak,
    status: '', notice: '', hint: hourlyHeatmap.has(key) ? '每格 1 小时' : '每格 1 天' };
}

function dashboard(key, label) {
  let [start, end] = bounds(key);
  const short = shortRanges.has(key);
  const selected = [...tools].sort((a, b) => harnessOrder.indexOf(a.id) - harnessOrder.indexOf(b.id));
  let slots, labels, span, period;
  if (short) {
    const first = Math.floor(start / HOUR) * HOUR, last = Math.floor(end / HOUR) * HOUR;
    slots = [];
    for (let t = first; t <= last; t += HOUR) slots.push([t, t + HOUR - 1]);
    labels = slots.map(([t]) => hm(t));
    labels[0] = hm(start); labels[labels.length - 1] = hm(end);
    span = mdhm(start) + '–' + (key === 'today' ? hm(end) : mdhm(end));
    period = [isoMinute(start), isoMinute(end)];
    if (key === 'yesterday') {
      labels[labels.length - 1] = '24:00';
      span = iso(dayOf(start)).slice(5).replace('-', '/');
      period = [iso(dayOf(start)), iso(dayOf(start))];
    }
  } else {
    const last = dayOf(end);
    let first = start ? dayOf(start) : FIRST;
    period = [iso(first), iso(last)];
    first = Math.max(first, last - 365);
    slots = [];
    for (let day = first; day <= last; day++) slots.push([day * DAY, day * DAY + DAY - 1]);
    labels = slots.map(([t]) => iso(dayOf(t)).slice(5).replace('-', '/'));
    span = labels[0] + '–' + labels[labels.length - 1];
  }
  const providers = selected.map((tool) => {
    const part = tally(tool, start, end);
    const history = slots.map(([a, b]) => slotTokens(tool, Math.max(a, start), Math.min(b, end)));
    const c = card(tool, part, history);
    c.summaryIncluded = true;
    if (tool.carried && part.requests) {
      const carried = Math.round(part.requests * tool.carried);
      const keep = 1 - tool.carried;
      c.counted = { cost: c.cost === null ? null : round6(c.cost * keep), tokens: Math.round(part.tokens * keep), output: Math.round(c.output * keep), requests: part.requests - carried,
        unpriced: 0, cache: c.cache, history: history.map((v) => Math.round(v * keep)) };
      c.subtitle = `${carried} 次已由 Magpie 计入`;
    }
    return c;
  });
  let chartPositions = null;
  if (key === 'today') {
    const firstIndex = slots.findIndex((_, i) => providers.some((p) => (p.counted ?? p).history[i] > 0));
    if (firstIndex >= 0) {
      const firstCall = Math.min(slots[firstIndex][0] + 12 * 60, end - 0.001);
      const padding = Math.min(300, Math.max(30, (end - firstCall) * 0.06));
      const chartStart = Math.max(start, firstCall - padding);
      const times = [chartStart, firstCall], groups = [];
      for (let i = firstIndex; i < slots.length; i++) {
        const stamp = Math.min(slots[i][0] + HOUR, end);
        if (times.length > 2 && stamp <= times[times.length - 1]) groups[groups.length - 1].push(i); else { times.push(stamp); groups.push([i]); }
      }
      const duration = Math.max(0.001, end - chartStart);
      let positions = times.map((t) => (t - chartStart) / duration);
      if (positions[1] < 0.01) positions = [0, 0.04, ...positions.slice(2).map((p) => 0.04 + 0.96 * p)];
      chartPositions = positions;
      const rebucket = (series) => [0, 0, ...groups.map((g) => g.reduce((s, i) => s + series[i], 0))];
      for (const p of providers) { p.history = rebucket(p.history); if (p.counted) p.counted.history = rebucket(p.counted.history); }
      labels = times.map(hm);
      span = mdhm(chartStart) + '–' + hm(end);
    }
  }
  let unit = short ? '每小时' : '每天';
  if (slots.length > 60) {
    const size = Math.ceil(slots.length / 60), count = slots.length;
    const rebucket = (series) => { const outSeries = []; for (let i = 0; i < count; i += size) outSeries.push(series.slice(i, i + size).reduce((a, b) => a + b, 0)); return outSeries; };
    for (const p of providers) { p.history = rebucket(p.history); if (p.counted) p.counted.history = rebucket(p.counted.history); }
    labels = labels.filter((_, i) => i % size === 0);
    unit = `每 ${size} 天`;
  }
  return {
    date: iso(TODAY), dates: labels, providers, rangeKey: key, rangeLabel: label, grouping: 'tool',
    groupings: [{ key: 'tool', label: '工具' }, { key: 'model', label: '模型' }, { key: 'project', label: '项目' }], cards: null, currency: null, groupNote: null,
    chartUnit: unit, chartSpan: span, period, chartStatus: key === 'all' ? '图表展示最近 366 天' : '', updated: '更新时间：15:40', chartPositions,
    heatmap: heatmap(key, start, end, selected), showSummary: true, allowCollapse: false, expandedProviders: [],
  };
}

// MARK: Model and project cards, in groupCard()'s shape

const modelMakers = [['claude', 'claude'], ['gpt', 'codex'], ['gemini', 'gemini'], ['deepseek', 'deepseek-harness'], ['kimi', 'kimi-api'], ['qwen', 'qoder'],
  ['qfmodel', 'qoder'], ['composer', 'cursor'], ['glm', null]];
function modelTitle(key) {
  for (const family of ['opus', 'sonnet', 'haiku', 'fable']) {
    const prefix = `claude-${family}-`;
    if (key.startsWith(prefix)) return `Claude ${family[0].toUpperCase()}${family.slice(1)} ${key.slice(prefix.length).replaceAll('-', '.')}`;
  }
  return key;
}
function groupCard(id, name, mark, accent, group) {
  const weight = group.tools.reduce((a, [, v]) => a + v, 0) || 1;
  const priced = group.requests > group.unpriced;
  return { id, name, ...mark, accent, subtitle: '', status: priced || group.credits ? '已同步' : '费用待定价', available: true, credits: group.credits || null, clients: null,
    cost: priced ? round6(group.cost) : null, tokens: group.tokens, requests: group.requests, output: group.output, cache: group.inputs ? capped(group.cached / group.inputs) : null,
    coverage: null, sessions: 0, unpriced: group.unpriced,
    models: [...group.tools].sort((a, b) => b[1] - a[1]).slice(0, 3).map(([tool, amount]) => (group.tokens ? { name: tool, tokens: amount, share: capped(amount / weight) }
      : { name: tool, tokens: 0, share: capped(amount / weight), credits: amount })),
    history: [], summaryIncluded: false, priceNote: group.unpriced > 0 && group.unpriced < group.requests && !group.credits ? `${group.unpriced} 次未定价` : null };
}
function addTo(groups, key, toolName, share, part, tool) {
  const g = (groups[key] ??= { tokens: 0, cost: 0, requests: 0, unpriced: 0, output: 0, credits: 0, cached: 0, inputs: 0, tools: [] });
  const tokens = Math.round(part.tokens * share), credits = Math.round(part.credits * share * 10) / 10;
  g.tokens += tokens; g.cost += (part.cost ?? 0) * share; g.requests += Math.round(part.requests * share);
  g.unpriced += tool.unpriced && key.startsWith('gemini-3.5') ? Math.round(part.requests * share) : Math.round(part.unpriced * share * (tool.unpriced ? 0 : 1));
  g.output += Math.round(part.output * share); g.credits += credits; g.cached += tokens * (part.cache ?? 0); g.inputs += tokens * 0.98;
  const existing = g.tools.find(([name]) => name === toolName);
  if (existing) existing[1] += tokens || credits; else g.tools.push([toolName, tokens || credits]);
}
function groupCards(key) {
  const [start, end] = bounds(key);
  const models = {}, projects = {};
  for (const tool of tools) {
    const part = tally(tool, start, end);
    if (!part.requests) continue;
    const counted = tool.carried ? { ...part, tokens: part.tokens * (1 - tool.carried), cost: part.cost * (1 - tool.carried), requests: Math.round(part.requests * (1 - tool.carried)) } : part;
    for (const [model, share] of tool.models) addTo(models, model, tool.name, share, counted, tool);
    if (tool.id === 'magpie') continue;
    for (const [project, share] of tool.projects ?? [['', 1]]) addTo(projects, project, tool.name, share, part, tool);
  }
  const rank = (groups) => Object.entries(groups).filter(([, g]) => g.requests).sort((a, b) => b[1].cost - a[1].cost || b[1].tokens - a[1].tokens || b[1].credits - a[1].credits);
  const model = rank(models).map(([name, g]) => {
    const maker = modelMakers.find(([prefix]) => name.startsWith(prefix))?.[1];
    return groupCard('model:' + name, modelTitle(name), maker ? moduleIcon(maker) : { symbol: 'cpu' }, maker ?? null, g);
  });
  const lead = (g) => { const best = [...g.tools].sort((a, b) => b[1] - a[1])[0]?.[0]; return tools.find((t) => t.name === best)?.id ?? null; };
  const unmarked = projects[''];
  delete projects[''];
  const project = rank(projects).map(([name, g]) => groupCard('project:' + name, name, { symbol: 'folder' }, lead(g), g));
  if (unmarked?.requests) project.push(groupCard('project:', '未标注项目', { symbol: 'questionmark.folder' }, lead(unmarked), unmarked));
  return { model, project };
}

// MARK: The homepage rows and the module list

const entry = (label, fields = {}, children = []) => ({ label, children, ...fields });
const separator = () => ({ label: '', separator: true, children: [] });
const location = (label, paths) => entry(label, { detail: paths.join('\n'), verbatim: true });
const HOME = '/Users/demo';
const STATE = HOME + '/Library/Application Support/SwiftBarToken';

function subscriptionRows(state) {
  const failed = state === 'failed', pending = state === 'pending';
  const updated = (text) => entry('更新时间', { value: pending ? '待同步' : text });
  const window = (label, used, detail) => {
    const level = used >= 90 ? 'critical' : used >= 70 ? 'warning' : 'normal';
    return entry(label, { ...(failed ? {} : { usageLevel: level }), value: `已用 ${used}%`, ...(failed ? {} : { detail }) });
  };
  // The homepage's bars for a plan with more than one limit, as the engine's panelgauges give them: five hours on the
  // left, the week on the right, a line per model group.
  const gauges = (...bars) => (pending || failed ? {} : {
    gauges: bars.map(([label, used, group, marks], place) => ({ label, used, level: used >= 90 ? 'critical' : used >= 70 ? 'warning' : 'normal', place,
      ...(group ? { group } : {}), ...(marks ? { marks } : {}) })) });
  const price = (key, value) => entry('订阅月费…', { symbol: 'dollarsign.circle', value, action: ['set-subscription-price', key] });
  return [
    entry('ChatGPT 订阅', { ...moduleIcon('chatgpt'), ...(pending ? {} : { usageLevel: 'normal' }), value: pending ? '待同步' : '已用 38%',
      ...gauges(['5 小时', 12], ['本周', 38]), onOpenAction: ['refresh-source', 'chatgpt'] }, pending ? [entry('来源 · 本机登录 · 编程工具额度'), updated('')] : [
      entry('来源 · 本机登录 · 编程工具额度'),
      window('5 小时', 12, '重置于 18:20 · 比匀速少用 9%'),
      window('本周', 38, '重置于 11/23 09:00 · 比匀速多用 6%'),
      window('Codex Spark', 4, '重置于 11/23 09:00 · 与匀速持平'),
      updated('刚刚更新'),
      entry('打开 ChatGPT', { ...moduleIcon('chatgpt'), href: 'https://chatgpt.com' }),
      price('chatgpt', '$20.00'),
      entry('本月回本 6.2 倍', { detail: '本月 API 等值 $124.40 · 月费 $20.00' }),
    ]),
    entry('Claude 订阅', { ...moduleIcon('claude-subscription'), ...(pending || failed ? {} : { usageLevel: 'warning' }),
      value: pending ? '待同步' : failed ? '已用 76% · 刷新失败' : '已用 76%', ...(failed ? { fallbacks: ['刷新失败'] } : {}), ...gauges(['5 小时', 22], ['本周', 76]),
      onOpenAction: ['refresh-source', 'claude-subscription'] }, [
      entry('来源 · Code 登录'),
      window('本周', 76, '重置于 11/20 22:00 · 比匀速多用 12%'),
      window('Fable', 41, '重置于 11/20 22:00 · 比匀速少用 23%'),
      window('5 小时', 22, '重置于 17:00 · 与匀速持平'),
      entry('额外用量', { value: '$4.20 / $50.00', detail: '本月已用 · 剩余 $45.80' }),
      failed ? entry('更新时间', { value: '15 分钟前更新', detail: '本次读取失败：令牌已过期，请在 Claude Code 里运行 /login · 保留最近一次成功的数据 · 15:25' })
        : updated('2 分钟前更新'),
      entry('立即刷新用量', { symbol: 'arrow.clockwise', action: ['retry-claude-usage'] }),
      entry('打开 Claude', { ...moduleIcon('claude-subscription'), href: 'file:///Applications/Claude.app' }),
      entry('打开订阅用量', { symbol: 'arrow.up.right', href: 'https://claude.ai/settings/usage' }),
      price('claude-subscription', '$100.00'),
      entry('本月回本 4.8 倍', { detail: '本月 API 等值 $482.10 · 月费 $100.00' }),
    ]),
    entry('Cursor 订阅', { ...moduleIcon('cursor-subscription'), ...(pending ? {} : { usageLevel: 'critical' }), value: pending ? '待同步' : '已用 91%',
      onOpenAction: ['refresh-source', 'cursor-subscription'] }, [
      entry('来源 · Cursor 账户（cursor.com）'),
      window('本周期', 91, '重置于 12/03 16:50 · 比匀速多用 43%'),
      entry('套餐额度', { value: '$18.20 / $20.00 · 剩余 $1.80', fallbacks: ['$18.20 / $20.00'] }),
      entry('按需用量', { value: '未开启' }),
      updated('4 分钟前更新'),
      entry('打开 Cursor', { ...moduleIcon('cursor-subscription'), href: 'file:///Applications/Cursor.app' }),
      entry('打开用量与账单', { symbol: 'arrow.up.right', href: 'https://cursor.com/dashboard' }),
      price('cursor-subscription', '未填写'),
    ]),
    entry('Qoder 订阅', { ...moduleIcon('qoder-subscription'), ...(failed || pending ? {} : { usageLevel: 'normal' }),
      value: pending ? '待同步' : failed ? '已用 58% 截至 09:12' : '已用 58%', fallbacks: ['已用 58%'], onOpenAction: ['refresh-source', 'qoder-subscription'] }, [
      entry('来源 · Qoder 桌面 App 的本机日志'),
      entry('套餐 Credits', { ...(failed ? {} : { usageLevel: 'normal' }), value: '已用 58%', detail: '1,160 / 2,000 Credits · 到期 12/08' }),
      entry('加购 Credits', { ...(failed ? {} : { usageLevel: 'normal' }), value: '已用 10%', detail: '50 / 500 Credits · 到期 2027-02-01' }),
      updated(failed ? '6 小时前更新' : '12 分钟前更新'),
      entry('在 Qoder 窗口右下角打开 Credits 用量后更新'),
    ]),
    // Antigravity's quota, read from its app on this computer.
    entry('Gemini 订阅', { ...moduleIcon('gemini-subscription'), ...(pending ? {} : { usageLevel: 'normal' }), value: pending ? '待同步' : '已用 24%',
      ...gauges(['5 小时', 24, 'Gemini', [{ image: 'logo:logos/gemini.svg', template: false }]], ['本周', 9],
        ['5 小时', 6, 'Claude 和 GPT', [{ image: 'logo:logos/claude.svg', template: false }, { image: 'logo:logos/openai.svg', template: true }]], ['本周', 18]), onOpenAction: ['refresh-source', 'gemini-subscription'] }, [
      entry('来源 · Antigravity 本机额度'),
      window('5 小时', 24, '重置于 12/01 18:20 · 比匀速少用 12%'),
      window('本周', 9, '重置于 12/06 09:40 · 比匀速少用 20%'),
      window('5 小时 · Claude 和 GPT', 6, '重置于 12/01 19:05'),
      window('本周 · Claude 和 GPT', 18, '重置于 12/06 10:15'),
      updated('刚刚更新'),
      entry('打开 Antigravity', { symbol: 'macwindow', href: 'file:///Applications/Antigravity.app' }),
      entry('打开订阅管理', { symbol: 'arrow.up.right', href: 'https://gemini.google.com' }),
      price('gemini-subscription', '未填写'),
    ]),
  ];
}

function apiKey(service, id, name, value, rows, removable = true) {
  return entry(name, { value }, [
    ...rows,
    location('来源', [id === 'default' ? STATE + '/config.local.sh' : `${STATE}/keys/${service}/${id}.key`]),
    entry('重命名…', { action: ['rename-api-key', service, id] }),
    ...(removable ? [entry('删除…', { action: ['remove-api-key', service, id] })] : []),
  ]);
}

function apiRows(state) {
  const failed = state === 'failed', pending = state === 'pending';
  const value = (text) => (pending ? '待同步' : text);
  return [
    entry('Claude API', { ...moduleIcon('claude-api'), value: value('本月已用 $12.40'), onOpenAction: ['refresh-source', 'claude-api'] }, [
      apiKey('claude-api', 'default', '默认', value('本月已用 $12.40'), [
        entry('今天已用（UTC）', { value: '$1.05' }),
        entry('更新时间', { value: '7 分钟前更新' }),
        entry('测试 Key', { value: 'Key 可用', action: ['test-api-key', 'claude-api', 'default'] }),
      ]),
      entry('添加 Key…', { symbol: 'plus', action: ['add-api-key', 'claude-api'] }),
      entry('打开用量与账单', { symbol: 'arrow.up.right', href: 'https://platform.claude.com/usage' }),
    ]),
    entry('DeepSeek API', { ...moduleIcon('deepseek'), value: value(failed ? '余额 ¥186.40 · 刷新失败' : '余额 ¥229.15 (2)'), fallbacks: ['余额 ¥229.15'],
      onOpenAction: ['refresh-source', 'deepseek'] }, [
      apiKey('deepseek', 'default', '默认', value('余额 ¥186.40'), [
        failed ? entry('更新时间', { value: '1 小时前更新', detail: '本次读取失败：网络连接超时 · 保留最近一次成功的数据 · 14:31' }) : entry('更新时间', { value: '4 分钟前更新' }),
      ], false),
      apiKey('deepseek', '7c41e2aa', 'demo', value('余额 ¥42.75'), [entry('更新时间', { value: '4 分钟前更新' })]),
      entry('添加 Key…', { symbol: 'plus', action: ['add-api-key', 'deepseek'] }),
      entry('余额提醒…', { symbol: 'bell', value: '¥20.00', action: ['set-balance-alert', 'deepseek', 'CNY'] }),
      entry('打开用量与账单', { symbol: 'arrow.up.right', href: 'https://platform.deepseek.com/usage' }),
    ]),
    entry('Kimi API', { ...moduleIcon('kimi-api'), value: value('余额 ¥86.30 · $18.20'), fallbacks: ['余额 ¥86.30'], onOpenAction: ['refresh-source', 'kimi-api'] }, [
      apiKey('kimi-api', 'default', '默认', value('余额 ¥86.30'), [entry('更新时间', { value: '9 分钟前更新' })], false),
      apiKey('kimi-api', '0f93ab10', '国际站', value('余额 $18.20'), [entry('更新时间', { value: '9 分钟前更新' })]),
      entry('添加 Key…', { symbol: 'plus', action: ['add-api-key', 'kimi-api'] }),
      entry('人民币余额提醒…', { symbol: 'bell', value: '关闭', action: ['set-balance-alert', 'kimi-api', 'CNY'] }),
      entry('美元余额提醒…', { symbol: 'bell', value: '$5.00', action: ['set-balance-alert', 'kimi-api', 'USD'] }),
      entry('打开用量与账单', { symbol: 'arrow.up.right', href: 'https://platform.moonshot.cn/console/account' }),
    ]),
    entry('ChatGPT API', { ...moduleIcon('chatgpt-api'), value: value('Key 可用'), onOpenAction: ['refresh-source', 'chatgpt-api'] }, [
      apiKey('chatgpt-api', 'default', '默认', value('Key 可用'), [
        entry('测试 Key', { value: 'Key 可用 · 11/18 10:20', fallbacks: ['Key 可用'], action: ['test-api-key', 'chatgpt-api', 'default'] }),
      ]),
      entry('添加 Key…', { symbol: 'plus', action: ['add-api-key', 'chatgpt-api'] }),
      entry('打开用量与账单', { symbol: 'arrow.up.right', href: 'https://platform.openai.com/usage' }),
    ]),
  ];
}

function sessionRows(state) {
  const alert = (on) => entry('这一轮完成时提醒', { symbol: 'bell', detail: '运行中的会话这一轮完成时发通知', checked: on, action: ['set-session-alerts', on ? 'false' : 'true'] });
  if (state === 'pending') return entry('会话', { symbol: 'terminal', value: '没有会话', onOpenAction: ['refresh-source', 'sessions'] }, [
    entry('今天没有会话'), separator(), alert(false)]);
  const round = (label, detail) => entry(label, { detail });
  const claude = moduleIcon('claude-subscription'), chatgpt = moduleIcon('chatgpt');
  // Seven sessions active today (the range shown), the running ones first: five show, the rest behind 查看更多. Each
  // card's figure is what the session cost today; what the whole session cost heads its turns. The last two share a
  // name (two runs of a program in one folder have no title): the engine adds when each began, and the key tells them
  // apart. The last changed today without asking anything, and a note under the list says what is listed and counted.
  return entry('会话', { symbol: 'terminal', value: '3 运行中 · 4 空闲', onOpenAction: ['refresh-source', 'sessions'] }, [
    entry('重做设置页的分段控件', { ...claude, key: 'session-1', statusLight: 'new', value: '$6.90', detail: '运行中 4 分钟 · Opus 5.5 · xhigh · 上下文 62%' }, [
      entry('starmeter · 最近 3 轮 · 累计 $6.90'),
      round('15:36 · 2.41M Token · $0.92', '9 次请求 · Edit ×6 · Bash ×4 · Read ×3 · update_plan ×1'),
      round('15:12 · 5.07M Token · $1.88', '21 次请求 · Bash ×12 · Read ×7 · Edit ×5 · Grep ×2'),
      round('更早开始 · 11.3M Token · $4.10', '44 次请求 · Bash ×30 · Write ×3 · Read ×2 · Edit ×2'),
    ]),
    entry('harbor-api', { ...chatgpt, key: 'session-2', statusLight: 'new', value: '已知 $1.71', detail: '运行中 · gpt-5.5-codex · high · 上下文 38%' }, [
      entry('最近 2 轮 · 累计 已知 $1.71'),
      round('15:31 · 1.20M Token · $0.51', '6 次请求 · shell ×5 · apply_patch ×2'),
      round('14:58 · 3.62M Token · 已知 $1.20', '14 次请求 · shell ×9 · apply_patch ×4 · web_search ×1'),
    ]),
    entry('修订论文的评估章节', { ...claude, key: 'session-3', statusLight: 'new', value: '$0.21', detail: '运行中 1 分钟 · Opus 5.5 · high · 上下文 17%' }, [
      entry('NetServe · 最近 1 轮 · 累计 $0.21'),
      round('15:35 · 318.4K Token · $0.21', '3 次请求 · Read ×4 · Grep ×1'),
    ]),
    entry('迁移博客的图片目录', { ...claude, key: 'session-4', statusLight: 'unknown', value: '$2.36', detail: '空闲 6 分钟 · Fable 5.2 · medium · 上下文 21%' }, [
      entry('field-notes · 最近 1 轮 · 累计 $14.08'),
      round('15:29 · 640.2K Token · $0.12', '4 次请求 · Bash ×3 · Read ×2'),
    ]),
    entry('整理本周的周报', { ...chatgpt, key: 'session-5', statusLight: 'unknown', value: '$0.48', detail: '空闲 12 分钟 · gpt-5.5 · medium · 上下文 9%' }, [
      entry('最近 1 轮 · 累计 $0.48'),
      round('15:23 · 402.7K Token · $0.48', '5 次请求 · shell ×2'),
    ]),
    entry('修好 CI 的缓存 · 13:02', { ...claude, key: 'session-6', statusLight: 'unknown', value: '$1.04', detail: '空闲 2 小时 · Fable 5.2 · medium · 上下文 14%' }, [
      entry('最近 2 轮 · 累计 $1.04'),
      round('13:17 · 520.1K Token · $0.31', '6 次请求 · Edit ×3 · Bash ×2'),
      round('13:02 · 1.31M Token · $0.73', '11 次请求 · Bash ×6 · Read ×4'),
    ]),
    entry('修好 CI 的缓存 · 11-20 21:10', { ...claude, key: 'session-7', statusLight: 'unknown', value: '$0.00', detail: '空闲 3 小时 · Opus 5.5 · high · 上下文 44%' }, [
      entry('harbor-api · 最近 1 轮 · 累计 $3.12'),
      round('更早开始 · 2.02M Token · $0.86', '12 次请求 · Bash ×9 · Edit ×2'),
    ]),
    entry('今天活动过的会话 · 金额只算今天'),
    separator(),
    alert(true),
  ]);
}

function komariRows(state) {
  const failed = state === 'failed';
  const server = (name, ok, detail) => entry(name, { statusLight: ok === null ? 'unknown' : ok ? 'ok' : 'issue', href: `https://komari.example.com/instance/${name}`, detail });
  return entry('Komari 监控', { symbol: 'server.rack', value: state === 'pending' ? '待同步' : failed ? '4/6 在线' : '5/6 在线', onOpenAction: ['refresh-source', 'komari'] }, [
    server('tokyo-edge', true, '在线 · CPU 4% · 内存 22% · 到期 2027-03-02'),
    server('hk-relay', true, '在线 · CPU 11% · 内存 41% · 到期 长期'),
    server('fra-db', !failed, failed ? '离线 · 最后在线 14:52 · 到期 长期' : '在线 · CPU 27% · 内存 63% · 到期 长期'),
    server('sjc-build', true, '在线 · CPU 63% · 内存 58% · 到期 2026-12-30'),
    server('home-nas', true, '在线 · CPU 2% · 内存 17% · 到期 长期'),
    server('lab-pi', false, '离线 · 最后在线 11/17 08:10 · 到期 长期'),
    separator(),
    entry('更新时间', { value: '刚刚更新' }),
    entry('打开 Komari 面板', { href: 'https://komari.example.com' }),
    entry('刷新监控数据', { action: ['refresh-monitor'] }),
  ]);
}

// As the engine writes them (more_rows.rs, app_update_lines): each tool's name and status, then its versions, channel
// and what a click does; a check that failed gives its reason. The manual ones say where to look.
function updateRows(state) {
  const failed = state === 'failed', count = failed ? 2 : 3;
  return entry('工具更新', { symbol: 'arrow.down.circle', attention: true, value: `${count} 项待更新`, fallbacks: [`${count} 项`], onOpenAction: ['refresh-source', 'updates'] }, [
    entry('Claude · 有新版本', { statusLight: 'new', detail: '6.6395.1 → 6.7012.0 · 点击打开 App 更新', href: 'file:///Applications/Claude.app' }),
    entry('Claude Code · 有新版本', { statusLight: 'new', detail: '2.1.327 → 2.1.330 · Claude 官方 · 点击运行 claude update', action: ['update-cli', 'claude-code'] }),
    failed
      ? entry('Codex · 检查失败，稍后重试', { statusLight: 'issue', detail: '0.170.0 · Homebrew Cask · formulae.brew.sh 返回 HTTP 503' })
      : entry('Codex · 有新版本', { statusLight: 'new', detail: '0.170.0 → 0.171.2 · Homebrew Cask · 点击运行 codex update', action: ['update-cli', 'codex-cli'] }),
    entry('ChatGPT · 已是最新', { statusLight: 'ok', detail: '26.1118.10442', href: 'file:///Applications/ChatGPT.app' }),
    entry('Cursor · 在 App 内检查更新', { statusLight: 'unknown', detail: '尚未接入自动版本检查', href: 'file:///Applications/Cursor.app' }),
    entry('Gemini CLI · 在官网查看更新', { statusLight: 'unknown', detail: '尚未接入自动版本检查', href: 'https://github.com/google-gemini/gemini-cli/releases' }),
    separator(),
    entry('立即检查更新', { symbol: 'arrow.clockwise', action: ['check-app-updates'] }),
    entry('查看最近 CLI 更新日志', { symbol: 'doc.text', action: ['open-cli-update-log'] }),
    entry('选择工具…', { symbol: 'slider.horizontal.3', action: ['panel-modules'] }),
  ]);
}

// The SwiftBar plugin's own update, as the engine writes it (more_rows.rs, self_update_lines): a new version, the
// update under way (pending) or one that failed (failed).
function selfUpdateRow(state) {
  const running = state === 'pending', failed = state === 'failed';
  return entry('Starmeter', { image: logo('starmeter-mark.svg'), template: true, attention: !failed, value: running ? '正在更新' : failed ? '更新失败' : '可更新 6.0.9' }, [
    entry('当前 6.0.8 → 最新 6.0.9'),
    ...(running ? [entry('正在下载并安装，完成后自动刷新')] : [
      ...(failed ? [entry('安装包校验失败', { statusLight: 'issue', detail: `日志：${HOME}/Library/Application Support/SwiftBarToken/logs/self-update.log` })] : []),
      entry(failed ? '重试更新到 6.0.9…' : '更新到 6.0.9…', { symbol: 'arrow.down.circle', action: ['update-self'] }),
    ]),
    entry('查看更新内容', { symbol: 'arrow.up.right', href: 'https://github.com/timecatbit/starmeter/releases/tag/v6.0.9' }),
  ]);
}

const modules = [
  ['summary', 'Summary', '概览'], ['heatmap', '用量热力图', '概览'],
  ['opencodex', 'OpenCodex', 'Harness'], ['magpie', 'Magpie', 'Harness'], ['codex', 'ChatGPT', 'Harness'], ['claude', 'Claude', 'Harness'],
  ['cursor', 'Cursor', 'Harness'], ['deepseek-harness', 'DeepSeek Harness', 'Harness'], ['gemini', 'Gemini', 'Harness'], ['opencode', 'OpenCode', 'Harness'],
  ['qoder', 'Qoder', 'Harness'], ['workbuddy', 'WorkBuddy', 'Harness'],
  ...services.map(([key]) => [key, serviceByKey[key].label, serviceByKey[key].category]),
  ['sessions', '会话', '其他'], ['komari', 'Komari 监控', '其他'], ['updates', '工具更新', '其他'],
];
const groupLabels = { Harness: '工具 Token 用量', 订阅: '订阅用量', API: 'API' };
const setupNotes = {
  minimax: '需要 MiniMax Token Plan 的 Key，显示套餐各窗口的用量', 'chatgpt-api': 'OpenAI 没有余额接口：保存或测试时校验一次 Key，只表示 Key 可用',
  'claude-api': '管理员 Key 显示本月花费；个人 Key 只校验是否可用', deepseek: '显示账户余额', 'minimax-api': '保存或测试时校验一次 Key，只表示 Key 可用',
  'gemini-api': 'Google 没有余额接口：保存或测试时校验一次 Key，只表示 Key 可用', grok: '需要 xAI 管理 Key 和 Team ID，显示团队余额',
  'kimi-api': '显示账户余额（国内站与国际站都可以）', 'qoder-api': '仅团队版/企业版：组织管理员的 OpenAPI Key 和组织 ID，显示组织共享的加购 Credits；个人账号没有 API',
};
const configuredModules = new Set(['magpie', 'claude', 'codex', 'opencode', 'qoder', 'gemini', 'cursor', 'chatgpt', 'claude-subscription', 'cursor-subscription',
  'qoder-subscription', 'gemini-subscription', 'claude-api', 'deepseek', 'kimi-api', 'chatgpt-api', 'komari', 'opencodex']);

function connection(key, home) {
  const service = serviceByKey[key];
  const configured = configuredModules.has(key);
  if (key === 'opencodex') return [location('配置目录', [HOME + '/.opencodex']), location('管理地址', ['http://127.0.0.1:10100']),
    location('管理令牌文件', [HOME + '/.opencodex/admin-api-token']), entry('选择配置目录…', { symbol: 'folder', action: ['choose-client-home', 'opencodex'] }),
    entry('修改管理地址…', { action: ['set-proxy-address'] }), entry('选择管理令牌文件…', { action: ['choose-admin-token'] }),
    entry('恢复自动读取', { action: ['reset-client-connection', 'opencodex'] }), entry('在 Finder 中打开', { symbol: 'folder', action: ['reveal-client-home', 'opencodex'] })];
  if (key === 'cursor' || key === 'cursor-subscription') return [
    entry('读取 Cursor 用量记录', { detail: '用本机 Cursor 登录查询 cursor.com，令牌只在内存中使用', checked: true, action: ['set-cursor-usage', 'false'] }),
    entry('来源 · Cursor 桌面版与 Cursor CLI'), entry('两者同一账户，用量都在账户记录里'),
    location('登录来源', [HOME + '/Library/Application Support/Cursor/User/globalStorage/state.vscdb', HOME + '/.cursor/cli-config/auth.json']),
    entry('打开 Cursor', { symbol: 'macwindow', href: 'file:///Applications/Cursor.app' })];
  if (key === 'chatgpt') return [entry('当前来源 · 本机 ChatGPT 登录'), entry('勾选 OpenCodex 后改用 OpenCodex 账户'), separator(),
    location('本机登录目录', [HOME + '/.codex']), entry('选择配置目录…', { symbol: 'folder', action: ['choose-client-home', 'chatgpt'] }),
    entry('恢复自动读取', { action: ['reset-client-connection', 'chatgpt'] }), entry('在 Finder 中打开', { symbol: 'folder', action: ['reveal-client-home', 'chatgpt'] })];
  if (key === 'claude-subscription') return [entry('Claude Code 登录查询', { checked: true, action: ['set-claude-oauth', 'false'] }),
    location('Code 登录来源', [HOME + '/.claude']), entry('启用本机用量回传…', { action: ['set-claude-local', 'true'] }),
    location('App 快照文件', [HOME + '/Library/Application Support/Claude/plan-usage-history.json']),
    entry('选择 Claude 数据目录…', { action: ['choose-source', 'claude'] }), entry('打开 Claude', { action: ['open-claude'] }),
    entry('打开订阅用量', { href: 'https://claude.ai/settings/usage' })];
  if (key === 'komari') return [location('面板地址', ['https://komari.example.com']), entry('设置面板地址…', { action: ['configure-connection', 'komari'] }),
    entry('清除连接设置…', { action: ['clear-connection', 'komari'] })];
  if (key === 'qoder-subscription') return [entry('来源 · Qoder 桌面 App 的本机日志'), entry('在 Qoder 窗口右下角打开 Credits 用量后更新'),
    location('日志目录', [HOME + '/Library/Application Support/com.qodercn.app.stable/logs']), entry('打开订阅用量', { symbol: 'arrow.up.right', href: 'https://qoder.cn/account/usage' })];
  if (key === 'gemini-subscription') return [entry('来源 · Antigravity 本机额度'), entry('Antigravity 开着时每分钟更新；关着时保留最近一次读到的额度'),
    entry('打开 Antigravity', { symbol: 'macwindow', href: 'file:///Applications/Antigravity.app' }), entry('打开订阅管理', { symbol: 'arrow.up.right', href: 'https://gemini.google.com' })];
  if (['grok-subscription', 'kimi-subscription'].includes(key)) return [entry('手动标记，未验证套餐'),
    ...(configured ? [location('本机标记文件', [STATE + '/preferences.json'])] : []),
    entry(configured ? '清除配置标记' : '标记为已配置', { action: ['set-subscription', key, configured ? 'false' : 'true'] }),
    entry('打开订阅管理', { symbol: 'arrow.up.right', href: 'https://example.com/' + key })];
  if (service?.category === 'API' || key === 'minimax') {
    const shown = home.find((row) => row.onOpenAction?.[1] === key);
    const rows = shown ? shown.children.slice() : [entry('添加 Key…', { symbol: 'plus', action: ['add-api-key', key] }), entry('打开用量与账单', { symbol: 'arrow.up.right', href: 'https://example.com/' + key })];
    rows.splice(Math.max(0, rows.length - 1), 0, entry('选择密钥文件…', { action: ['configure-connection', key] }));
    if (!configured && setupNotes[key]) rows.unshift(entry(setupNotes[key]));
    return rows;
  }
  if (['summary', 'heatmap', 'sessions', 'updates'].includes(key)) return [];
  // A Harness: where its records are read.
  const again = entry('重新查找', { symbol: 'sparkle.magnifyingglass', action: ['detect-source', key] });
  if (!configured) return [entry(key === 'gemini' ? '未发现 Gemini CLI 或 Antigravity 的记录' : '未发现客户端或用量记录'), again, entry('选择记录目录…', { action: ['choose-source', key] })];
  const folders = { magpie: [HOME + '/.config/magpie'], claude: [HOME + '/.claude/projects', HOME + '/Library/Application Support/Claude/claude-code-sessions'],
    codex: [HOME + '/.codex/sessions', HOME + '/.codex/archived_sessions'], opencode: [HOME + '/.local/share/opencode'], qoder: [HOME + '/.qoder-cn/projects'],
    gemini: [HOME + '/.gemini/tmp'] }[key] ?? [HOME + '/.' + key];
  const first = { magpie: [entry('来源 · Magpie 本机网关记录'), entry('只读取 usage.jsonl')], codex: [entry('来源 · 编程客户端的本机记录'), entry('仅编程会话，不含普通聊天')],
    claude: [entry('来源 · Claude 桌面 App 与 Claude Code'), entry('含 VS Code、JetBrains 与终端里的会话')],
    gemini: [entry('来源 · Gemini CLI 与 Antigravity'), entry('含 Antigravity 的 App、agy CLI 与 IDE')] }[key] ?? [entry('来源 · 自动读取本机客户端')];
  return [...first, location('读取目录', folders), entry('在 Finder 中打开', { symbol: 'folder', action: ['reveal-source', key] }), again,
    entry(key === 'claude' ? '选择 Claude 数据目录…' : '选择记录目录…', { action: ['choose-source', key] }),
    ...(key === 'claude' ? [entry('打开 Claude', { symbol: 'macwindow', action: ['open-claude'] })] : [])];
}

/** The settings, a page each as the engine sends them: 外观, 通用, 提醒 and 数据. */
function settingsPages(prefs) {
  const radio = (label, action, checked) => entry(label, { checked, radio: true, action });
  const appearance = [
    radio('跟随系统', ['set-theme', 'system'], prefs.theme === 'system'), radio('浅色', ['set-theme', 'light'], prefs.theme === 'light'),
    radio('深色', ['set-theme', 'dark'], prefs.theme === 'dark'), separator(),
    radio('实色', ['set-appearance', 'solid'], prefs.appearance === 'solid'), radio('磨砂', ['set-appearance', 'frosted'], prefs.appearance === 'frosted'), radio('玻璃', ['set-appearance', 'glass'], prefs.appearance === 'glass'), separator(),
    entry('允许折叠', { detail: '卡片折成两行，点卡片就地展开', symbol: 'rectangle.compress.vertical', checked: prefs.allowCollapse, action: ['set-collapse', prefs.allowCollapse ? 'false' : 'true'] }),
    entry('金额显示', { symbol: 'dollarsign.circle', value: '美元' }, [radio('美元（$）', ['set-currency', 'USD'], true), radio('人民币（¥）', ['set-currency', 'CNY'], false),
      separator(), entry('汇率…', { value: '每天自动更新', detail: 'API 等值估算按这个汇率换成人民币；订阅月费与 API 实际花费仍是美元', action: ['set-cny-rate'] })]),
  ];
  const general = [
    entry('菜单栏显示', { symbol: 'menubar.rectangle', value: '只显示星星' }, [radio('只显示星星', ['set-menu-bar', 'none'], true),
      radio('今天的 API 估算', ['set-menu-bar', 'cost'], false), radio('最紧张的订阅额度', ['set-menu-bar', 'quota'], false)]),
    entry('钉住时刷新', { symbol: 'pin', value: '每 2 秒' }, [1, 2, 5].map((s) => radio(`每 ${s} 秒`, ['set-pin-interval', String(s)], s === 2))),
    entry('模块与配置', { section: true }),
    entry('自动检测', { symbol: 'sparkle.magnifyingglass' }, [entry('把新找到的加到主页，已勾选的不变；不读取对话正文，不显示 Key'), separator(),
      entry('开始检测…', { symbol: 'sparkle.magnifyingglass', action: ['detect-modules'] })]),
    entry('主页顺序', { symbol: 'arrow.up.arrow.down', value: '默认' },
      [entry('在主页按住一行，拖到想要的位置；键盘选中一行后按 ⌥↑ 或 ⌥↓。订阅、API 和其他各自排序，工具卡片按用量排')]),
    entry('高级配置文件…', { symbol: 'doc.text', action: ['open-settings'] }),
    entry('恢复默认模块', { action: ['reset-modules'] }),
  ];
  const alerts = [
    entry('用量提醒', { detail: '订阅窗口用到所选比例时通知一次', symbol: 'bell', value: '90%' }, [
      ...[70, 80, 90, 0].map((v) => radio(v ? `${v}%` : '关闭', ['set-usage-alert', String(v)], v === 90)), separator(),
      entry('重置前提醒', { detail: '每周窗口用了不到 85%，重置前 12 小时提醒', checked: false, action: ['set-reset-reminder', 'true'] })]),
    entry('服务状态', { detail: '每 5 分钟查一次 Claude、OpenAI、Cursor 的官方状态页，出问题时标在卡片上', symbol: 'waveform.path.ecg', checked: false,
      action: ['set-service-status', 'true'] }),
  ];
  const data = [
    entry('保留已清理记录的用量', { detail: '工具删掉旧记录后仍计入“全部”等统计', symbol: 'archivebox', checked: true, action: ['set-keep-usage', 'false'] }),
    entry('导出用量', { symbol: 'square.and.arrow.up', value: '今天' }, [entry('CSV（每天每个工具一行）…', { action: ['export-usage', 'csv'] }),
      entry('JSON…', { action: ['export-usage', 'json'] })]),
    entry('多台电脑合并', { symbol: 'arrow.left.arrow.right', value: '未设置',
      detail: '每台电脑都选同一个同步文件夹（iCloud 云盘、Dropbox、OneDrive…），各自写一份用量，合起来显示；文件里只有工具、模型和用量数字' },
      [entry('选择同步文件夹…', { symbol: 'folder', action: ['choose-sync-folder'] })]),
  ];
  return [entry('外观', { symbol: 'circle.lefthalf.filled' }, appearance), entry('通用', { symbol: 'slider.horizontal.3' }, general),
    entry('提醒', { symbol: 'bell' }, alerts), entry('数据', { symbol: 'externaldrive' }, data)];
}

function moduleOptions(prefs, home) {
  const options = [
    // The settings first, a group of their own like the modules' below.
    entry('设置', { section: true }), ...settingsPages(prefs)];
  let group = null;
  for (const [key, label, category] of modules) {
    if (category !== group) { options.push(entry(groupLabels[category] ?? category, { section: true })); group = category; }
    const enabled = prefs.visible.includes(key);
    const toggle = ['set-module', key, enabled ? 'false' : 'true'];
    // A module's page leads with the switch the grid's checkbox is: whether it shows on the homepage.
    const page = connection(key, home);
    if (page.length) page.unshift(entry('在主页显示', { checked: enabled, action: [...toggle] }), separator());
    const row = entry(label, { checked: enabled, action: toggle, ...moduleIcon(key) }, page);
    if (!['summary', 'heatmap', 'sessions', 'updates'].includes(key)) row.value = configuredModules.has(key) ? '已配置' : '未配置';
    options.push(row);
  }
  return options;
}

// MARK: Scenarios

const visible = ['summary', 'heatmap', 'magpie', 'claude', 'codex', 'opencode', 'qoder', 'gemini', 'cursor', 'chatgpt', 'claude-subscription', 'cursor-subscription',
  'qoder-subscription', 'gemini-subscription', 'claude-api', 'deepseek', 'kimi-api', 'chatgpt-api', 'sessions', 'komari', 'updates'];

function homeEntries(state) {
  const rows = [], keys = [];
  // Each row names its module, as the engine does, so the panel can drag it to another place in its group.
  const add = (row, key) => { rows.push(key ? { ...row, module: key } : row); keys.push(key); };
  for (const row of inOrder(subscriptionRows(state))) add(row, row.onOpenAction[1]);
  add(separator(), null);
  for (const row of inOrder(apiRows(state))) add(row, row.onOpenAction[1]);
  add(separator(), null);
  add(sessionRows(state), 'sessions');
  add(komariRows(state), 'komari');
  add(updateRows(state), 'updates');
  add(selfUpdateRow(state), null);
  return { rows, keys };
}

/** A first run on a computer where nothing was found: Summary, the heatmap and 工具更新 with no tool to check. */
function emptyHomeEntries() {
  const updates = entry('工具更新', { symbol: 'arrow.down.circle', onOpenAction: ['refresh-source', 'updates'] }, [
    entry('尚未勾选工具'), separator(),
    entry('立即检查更新', { symbol: 'arrow.clockwise', action: ['check-app-updates'] }),
    entry('选择工具…', { symbol: 'slider.horizontal.3', action: ['panel-modules'] }),
  ]);
  return { rows: [{ ...updates, module: 'updates' }], keys: ['updates'] };
}

function scenario(state) {
  const empty = state === 'empty';
  const prefs = { revision: 1, visible: empty ? ['summary', 'heatmap', 'updates'] : visible, appearance: 'solid', theme: 'system', allowCollapse: false, expandedProviders: [], homeOrder: [] };
  const { rows, keys } = empty ? emptyHomeEntries() : homeEntries(state);
  const dashboards = {}, cards = {};
  for (const [key, label] of ranges) {
    const d = dashboard(key, label);
    if (state === 'pending') {
      d.providers = d.providers.map((p) => ({ ...p, subtitle: '', status: '待同步', available: false, cost: null, tokens: 0, requests: 0, output: 0, cache: null,
        coverage: null, sessions: 0, unpriced: 0, credits: null, clients: null, models: [], history: p.history.map(() => 0), priceNote: null, counted: undefined }));
      d.chartStatus = '待同步'; d.updated = ''; d.chartPositions = null;
      d.heatmap = { ...d.heatmap, tokens: d.heatmap.tokens.map(() => 0), requests: d.heatmap.requests.map(() => 0), costs: d.heatmap.costs.map(() => 0),
        unpriced: d.heatmap.unpriced.map(() => 0), status: '待同步', streak: null };
    }
    if (empty) {
      d.providers = []; d.chartStatus = ''; d.updated = ''; d.chartPositions = null;
      d.heatmap = { ...d.heatmap, tokens: d.heatmap.tokens.map(() => 0), requests: d.heatmap.requests.map(() => 0), costs: d.heatmap.costs.map(() => 0),
        unpriced: d.heatmap.unpriced.map(() => 0), status: null, streak: null };
    }
    if (state === 'failed') {
      d.chartStatus = '部分图表数据不可用'; d.updated = '更新时间：15:25';
      d.providers = d.providers.map((p) => p.id === 'gemini' ? { ...p, available: false, status: '读取失败：~/.gemini/tmp 不可读', cost: null, models: [], priceNote: null }
        : p.id === 'codex' ? { ...p, status: '刷新失败 · 保留原数据' } : p.id === 'qoder' ? { ...p, status: '部分记录未读取' } : p.id === 'claude' ? { ...p, serviceNote: '服务部分降级' } : p);
    }
    for (const p of d.providers) if (p.counted === undefined) delete p.counted;
    dashboards[key] = d;
    cards[key] = state === 'pending' || empty ? { model: [], project: [] } : groupCards(key);
  }
  return {
    snapshot: { schema: 1, project: { name: 'Starmeter', url: 'https://github.com/timecatbit/starmeter' }, dashboard: null, menuBarText: '', pinInterval: 2,
      entries: rows, rangeOptions: ranges.map(([key, label]) => entry(label, { action: ['set-range', key], checked: key === 'today' })),
      moduleOptions: moduleOptions(prefs, rows).map((o) => (empty && o.value === '已配置' ? { ...o, value: '未配置' } : o)), appearance: prefs.appearance,
      theme: prefs.theme, preferences: prefs, updateCount: empty ? 0 : 3 },
    // Fixture-only: which module each homepage row belongs to (null for separators and the self-update row), so the
    // fixture backends can hide a row when its module is turned off, as the engine does.
    entryModules: keys,
    dashboards,
    cards,
  };
}

for (const state of ['default', 'failed', 'pending', 'empty']) {
  writeFileSync(new URL(state + '.json', out), JSON.stringify(scenario(state)) + '\n');
  console.log('wrote', state + '.json');
}
