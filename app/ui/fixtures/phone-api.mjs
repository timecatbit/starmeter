// The phone server (app/src-tauri/src/phone.rs) answered from the fixtures: the homepage's figures for the key in the
// address, nothing without it, and the snapshot cut down as the app sends it to a phone. The Playwright tests
// (tests/phone.ts) and the dev server (vite.config.ts, for previews and screenshots of 在手机上看) both answer with it.

import { readFileSync } from 'node:fs';

export const PHONE_KEY = '0123456789abcdef0123456789abcdef';

/**
 * The answer to a phone's request: `pathname` /api/snapshot or /api/dashboard, `params` its query, `given` the key it
 * sent. `logo` turns a fixture's logo:<file> into what the app sends (the file, base64); the snapshot the server would
 * send for `?swiftbar` keeps the computer's range and grouping only, as SwiftBar's panel has them.
 * @param {any} data the parsed fixture (default.json)
 * @param {{ pathname: string, params: URLSearchParams, given: string | undefined, key?: string, logo: (file: string) => string, swiftbar?: boolean }} request
 * @returns {{ status: number, type: string, body: string }}
 */
export function phoneAnswer(data, { pathname, params, given, key = PHONE_KEY, logo, swiftbar = false }) {
  if (given !== key) return { status: 403, type: 'text/plain; charset=utf-8', body: 'key needed' };
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

/** 在手机上看 while developing: a Vite plugin answering /api/ from the fixtures as the app's phone server would, so the
 *  phone page opens at /#k=0123456789abcdef0123456789abcdef (a page address with ?swiftbar answers as SwiftBar's panel
 *  does). The dev server only: a build has no /api/. */
export function phoneFixtures() {
  const fixture = new URL('./default.json', import.meta.url);
  const logo = (file) => { try { return readFileSync(new URL('../../../assets/' + file, import.meta.url)).toString('base64'); } catch { return ''; } };
  return {
    name: 'phone-fixtures',
    configureServer(server) {
      server.middlewares.use('/api', (request, response) => {
        const url = new URL(request.url ?? '/', 'http://localhost');
        const header = request.headers['x-starmeter-key'];
        const answer = phoneAnswer(JSON.parse(readFileSync(fixture, 'utf8')), { pathname: url.pathname, params: url.searchParams,
          given: Array.isArray(header) ? header[0] : header, logo, swiftbar: /[?&]swiftbar\b/.test(request.headers.referer ?? '') });
        response.writeHead(answer.status, { 'Content-Type': answer.type, 'Cache-Control': 'no-store' });
        response.end(answer.body);
      });
    },
  };
}
