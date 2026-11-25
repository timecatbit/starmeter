// The phone server (app/src-tauri/src/phone.rs) answered from the fixtures: the homepage's figures for the key in the
// address, nothing without it, and the snapshot cut down as the app sends it to a phone. The Playwright tests
// (tests/phone.ts) and the dev server (vite.config.ts, for previews and screenshots of 在手机上看) both answer with it.

import { readFileSync } from 'node:fs';

export const PHONE_KEY = '0123456789abcdef0123456789abcdef';

/**
 * The answer to a phone's request: `pathname` /api/snapshot or /api/dashboard, `params` its query, `given` the key it
 * sent. `logo` turns a fixture's logo:<file> into what the app sends (the file, base64); the snapshot the server would
 * send for `?swiftbar` keeps the computer's range and grouping only, as SwiftBar's panel has them. /api/live answers with
 * `live` (liveWork: made-up work going on), or today as it stands.
 * @param {any} data the parsed fixture (default.json)
 * @param {{ pathname: string, params: URLSearchParams, given: string | undefined, key?: string, logo: (file: string) => string, swiftbar?: boolean, live?: ((data: any) => any) | null }} request
 * @returns {{ status: number, type: string, body: string }}
 */
export function phoneAnswer(data, { pathname, params, given, key = PHONE_KEY, logo, swiftbar = false, live = null }) {
  if (given !== key) return { status: 403, type: 'text/plain; charset=utf-8', body: 'key needed' };
  if (pathname.endsWith('live')) {
    const answer = (live ?? still)(data);
    // As the app keeps them (Engine::live): the readings of the last few minutes in short, for ?history=1.
    const recent = (data.recentLive ??= []);
    if (params.get('history') === '1') answer.history = recent.slice();
    const last = recent.at(-1);
    if (answer.dashboard && (!last || answer.at - last.at >= 2 || answer.at < last.at)) {
      const reading = { at: answer.at, date: answer.dashboard.date, rangeKey: answer.dashboard.rangeKey,
        providers: answer.dashboard.providers.map(({ id, available, summaryIncluded, tokens, output, cost, counted }) =>
          ({ id, available, summaryIncluded, tokens, output, cost, ...(counted ? { counted: { tokens: counted.tokens, output: counted.output, cost: counted.cost } } : {}) })) };
      // 会话 in short when it changed, as phone.rs keeps it (short_sessions).
      const sessions = answer.sessions ? shortSessions(answer.sessions) : null;
      const stood = [...recent].reverse().find((r) => r.sessions)?.sessions;
      if (sessions && JSON.stringify(sessions) !== JSON.stringify(stood)) reading.sessions = sessions;
      recent.push(reading);
      let carried = null;
      while (recent.length && (answer.at - recent[0].at > 400 || recent[0].at > answer.at)) carried = recent.shift().sessions ?? carried;
      if (carried && recent.length && !recent[0].sessions) recent[0].sessions = carried;
    }
    const body = JSON.stringify(answer).replace(/"logo:([^"]*)"/g, (_, file) => JSON.stringify(logo(file)));
    return { status: 200, type: 'application/json', body };
  }
  const range = swiftbar ? 'today' : params.get('range') ?? 'today';
  // The phone's own grouping: the cards grouped as asked, the computer's (by tool) otherwise.
  const group = swiftbar ? 'tool' : params.get('group') ?? 'tool';
  const dashboard = data.dashboards[range] ?? data.dashboards.today;
  const grouped = { ...dashboard, grouping: group, cards: group === 'tool' ? null : data.cards[range]?.[group] ?? [],
    groupNote: group === 'project' ? '按客户端自己的记录' : null,
    ...(swiftbar ? { groupings: dashboard.groupings.filter((g) => g.key === group) } : {}) };
  const snapshot = { ...data.snapshot, dashboard: grouped, moduleOptions: [],
    // As phone.rs sends them: 会话 keeps its sessions and its mark, the rest their rows only.
    entries: data.snapshot.entries.map((e) =>
      e.onOpenAction?.[1] === 'sessions'
        ? { ...e, children: e.children.filter((c) => !c.action).filter((c, i, all) => !(c.separator && i === all.length - 1)) }
        : { ...e, children: [], onOpenAction: undefined }),
    rangeOptions: data.snapshot.rangeOptions.map((o) => ({ ...o, checked: o.action[1] === range }))
      .filter((o) => !swiftbar || o.checked) };
  const body = JSON.stringify(pathname.endsWith('dashboard') ? grouped : snapshot)
    .replace(/"logo:([^"]*)"/g, (_, file) => JSON.stringify(logo(file)));
  return { status: 200, type: 'application/json', body };
}

// MARK: 网页版's live view (/api/live: today's figures by tool, 会话, and when each session's turn began)

/** 会话 as the readings keep it: each session's title, key, light, state and latest turn's line. */
function shortSessions(entry) {
  return (entry.children ?? []).filter((row) => row.statusLight !== undefined).map((row) => {
    const turn = (row.children ?? []).find((c) => c.detail !== undefined);
    return { label: row.label, key: row.key, statusLight: row.statusLight, detail: row.detail, children: turn ? [{ label: turn.label, detail: turn.detail }] : [] };
  });
}

/** 会话's row as the phone has it: its sessions, no switch, nothing to do. */
function sessionsOf(data) {
  const entry = data.snapshot.entries.find((e) => e.onOpenAction?.[1] === 'sessions');
  if (!entry) return null;
  const children = entry.children.filter((c) => !c.action).filter((c, i, all) => !(c.separator && i === all.length - 1));
  return { ...entry, children, onOpenAction: undefined };
}

/** The live answer with nothing moving: today as the fixtures have it. */
export function still(data) {
  return { at: Date.now() / 1000, dashboard: structuredClone(data.dashboards.today), sessions: structuredClone(sessionsOf(data)), turns: {} };
}

const pad = (n) => String(n).padStart(2, '0');
const clock = (seconds) => { const d = new Date(seconds * 1000); return `${pad(d.getHours())}:${pad(d.getMinutes())}`; };
const tokensText = (n) => (n >= 1e6 ? (n / 1e6).toFixed(2) + 'M' : n >= 1e3 ? (n / 1e3).toFixed(1) + 'K' : String(Math.round(n)));

/**
 * Made-up work for 网页版 to watch, on `now()`'s clock: a cycle of `length` seconds. Claude Code's session 重做设置页的分段控件
 * runs its turn from the cycle's start to `claudeEnds`, a request landing every 4 seconds, its tools called as it goes;
 * Codex's harbor-api from 2 seconds in to `codexEnds`, a request every 7 seconds; then nothing until the cycle starts
 * again (resting, then waking). Each answer is today's figures with what has landed added, 会话 with the turns as they
 * stand, and when each turn began.
 */
export function liveWork({ now = () => Date.now(), length = 90, claudeEnds = 32, codexEnds = 20 } = {}) {
  const begun = now() / 1000;
  const added = { claude: { tokens: 0, output: 0, cost: 0, requests: 0 }, codex: { tokens: 0, output: 0, cost: 0, requests: 0 } };
  const turn = { claude: { tokens: 0, cost: 0, requests: 0, Edit: 0, Bash: 0, Read: 0 }, codex: { tokens: 0, cost: 0, requests: 0, shell: 0, apply_patch: 0 } };
  let landed = { claude: 0, codex: 0 }, cycle = -1, seed = 7;
  const random = () => { seed = (seed * 16807) % 2147483647; return (seed - 1) / 2147483646; };
  const land = (id) => {
    const tokens = Math.round((id === 'claude' ? 60_000 : 30_000) * (0.6 + random())), output = Math.round(tokens * (0.008 + random() * 0.01));
    const cost = tokens * (id === 'claude' ? 0.0000011 : 0.0000007);
    for (const into of [added[id], turn[id]]) { into.tokens += tokens; into.cost += cost; into.requests += 1; }
    added[id].output += output;
    const tools = id === 'claude' ? ['Edit', 'Bash', 'Read'] : ['shell', 'apply_patch'];
    turn[id][tools[Math.floor(random() * tools.length)]] += 1;
  };
  return (data) => {
    const at = now() / 1000, elapsed = at - begun, n = Math.floor(elapsed / length), phase = elapsed - n * length, start = begun + n * length;
    if (n !== cycle) {
      cycle = n; landed = { claude: 0, codex: 0 };
      turn.claude = { tokens: 0, cost: 0, requests: 0, Edit: 0, Bash: 0, Read: 0 };
      turn.codex = { tokens: 0, cost: 0, requests: 0, shell: 0, apply_patch: 0 };
    }
    // Every request due by now lands, at its own moment in the cycle.
    while (landed.claude * 4 + 1 <= Math.min(phase, claudeEnds)) { landed.claude += 1; land('claude'); }
    while (2 + landed.codex * 7 + 1 <= Math.min(phase, codexEnds)) { landed.codex += 1; land('codex'); }
    const answer = still(data);
    for (const p of answer.dashboard.providers) {
      const more = added[p.id];
      if (!more) continue;
      p.tokens += more.tokens; p.output += more.output; p.cost = (p.cost ?? 0) + more.cost; p.requests += more.requests;
      if (p.history?.length) p.history[p.history.length - 1] += more.tokens;
      // A source a gateway partly counts adds what it alone logged.
      if (p.counted) { p.counted.tokens += more.tokens; p.counted.output = (p.counted.output ?? 0) + more.output; p.counted.cost = (p.counted.cost ?? 0) + more.cost; p.counted.requests += more.requests; }
    }
    const running = { 'session-1': phase < claudeEnds, 'session-2': phase >= 2 && phase < codexEnds };
    const shown = { 'session-1': ['claude', 'Opus 5.5 · xhigh · 上下文 62%', ['Edit', 'Bash', 'Read'], start, claudeEnds],
      'session-2': ['codex', 'gpt-5.5-codex · high · 上下文 38%', ['shell', 'apply_patch'], start + 2, codexEnds] };
    for (const s of answer.sessions?.children ?? []) {
      if (!s.statusLight) continue;
      const mine = shown[s.key];
      if (!mine) { s.statusLight = 'unknown'; s.detail = s.detail.replace(/^运行中( \d+ 分钟)?/, '空闲 3 分钟'); continue; }
      const [id, rest, tools, began, ends] = mine, now = turn[id], on = running[s.key];
      const minutes = Math.floor(Math.max(0, (on ? at : begun + n * length + ends) - began) / 60);
      s.statusLight = on ? 'new' : 'unknown';
      s.detail = on ? `运行中${minutes ? ` ${minutes} 分钟` : ''} · ${rest}` : `空闲 ${Math.max(1, Math.round((phase - ends) / 60))} 分钟 · ${rest}`;
      const label = `${clock(began)} · ${tokensText(now.tokens)} Token · $${now.cost.toFixed(2)}`;
      const detail = `${now.requests} 次请求 · ` + tools.map((t) => [t, now[t]]).filter(([, c]) => c > 0).sort((a, b) => b[1] - a[1]).map(([t, c]) => `${t} ×${c}`).join(' · ');
      // The turn as it stands, from nothing at its start (not the last turn's figures).
      const turns = s.children.filter((c) => c.detail !== undefined);
      if (turns.length) { turns[0].label = label; turns[0].detail = now.requests ? detail : '0 次请求'; }
    }
    answer.at = at;
    answer.turns = { 'session-1': start, 'session-2': start + 2 };
    return answer;
  };
}

/** 在手机上看 while developing: a Vite plugin answering /api/ from the fixtures as the app's phone server would, so the
 *  phone page opens at /#k=0123456789abcdef0123456789abcdef (a page address with ?swiftbar answers as SwiftBar's panel
 *  does). The dev server only: a build has no /api/. */
export function phoneFixtures() {
  const fixture = new URL('./default.json', import.meta.url);
  const logo = (file) => { try { return readFileSync(new URL('../../../assets/' + file, import.meta.url)).toString('base64'); } catch { return ''; } };
  const live = liveWork();
  return {
    name: 'phone-fixtures',
    configureServer(server) {
      server.middlewares.use('/api', (request, response) => {
        const url = new URL(request.url ?? '/', 'http://localhost');
        const header = request.headers['x-starmeter-key'];
        const answer = phoneAnswer(JSON.parse(readFileSync(fixture, 'utf8')), { pathname: url.pathname, params: url.searchParams,
          given: Array.isArray(header) ? header[0] : header, logo, swiftbar: /[?&]swiftbar\b/.test(request.headers.referer ?? ''), live });
        response.writeHead(answer.status, { 'Content-Type': answer.type, 'Cache-Control': 'no-store' });
        response.end(answer.body);
      });
    },
  };
}
