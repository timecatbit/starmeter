// The phone server (phone.rs) as the tests see it: the homepage's figures for the key in the address, nothing without
// it, and the snapshot cut down as the app sends it to a phone.
import { readFileSync } from 'node:fs';
import type { Page } from '@playwright/test';

export const PHONE_KEY = '0123456789abcdef0123456789abcdef';

type Row = { label: string; children: { label: string; action?: string[]; separator?: boolean }[]; onOpenAction?: string[] };

const fixture = () => JSON.parse(readFileSync(new URL('../ui/fixtures/default.json', import.meta.url), 'utf8'));

/** The fixtures' session titles: the user's own words, which stay as they are in English. */
export const sessionTitles = (): string[] =>
  (fixture().snapshot.entries as Row[]).find((e) => e.onOpenAction?.[1] === 'sessions')?.children.filter((c) => !c.separator && !c.action).map((c) => c.label) ?? [];

/** Serves /api/ for the phone page; what it was asked, as path and query, goes into the list returned. */
export async function servePhone(page: Page, key = PHONE_KEY): Promise<string[]> {
  const data = fixture();
  const asked: string[] = [];
  await page.route('**/api/**', async (route) => {
    const request = route.request();
    if (request.headers()['x-starmeter-key'] !== key) return route.fulfill({ status: 403, body: 'key needed' });
    const url = new URL(request.url());
    asked.push(url.pathname + url.search);
    const range = url.searchParams.get('range') ?? 'today';
    // The phone's own grouping: the cards grouped as asked, the computer's (by tool) otherwise.
    const group = url.searchParams.get('group') ?? 'tool';
    const grouped = { ...data.dashboards[range], grouping: group, cards: group === 'tool' ? null : data.cards[range]?.[group] ?? [],
      groupNote: group === 'project' ? '按客户端自己的记录' : null };
    const snapshot = { ...data.snapshot, dashboard: grouped, moduleOptions: [],
      // As phone.rs sends them: 会话 keeps its sessions and its mark, the rest their rows only.
      entries: data.snapshot.entries.map((e: Row) =>
        e.onOpenAction?.[1] === 'sessions'
          ? { ...e, children: e.children.filter((c) => !c.action).filter((c, i, all) => !(c.separator && i === all.length - 1)) }
          : { ...e, children: [], onOpenAction: undefined }),
      rangeOptions: data.snapshot.rangeOptions.map((o: { action: string[] }) => ({ ...o, checked: o.action[1] === range })) };
    // The fixtures name their logos; the app sends the files themselves: a blank one stands in.
    const body = JSON.stringify(url.pathname.endsWith('dashboard') ? grouped : snapshot)
      .replace(/"logo:[^"]*"/g, JSON.stringify(Buffer.from('<svg xmlns="http://www.w3.org/2000/svg"/>').toString('base64')));
    return route.fulfill({ contentType: 'application/json', body });
  });
  return asked;
}
