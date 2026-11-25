// The phone server (phone.rs) as the tests see it: the homepage's figures for the key in the address, nothing without
// it, and the snapshot cut down as the app sends it to a phone (ui/fixtures/phone-api.mjs, which the dev server uses too).
import { readFileSync } from 'node:fs';
import type { Page } from '@playwright/test';
import { PHONE_KEY, liveWork, phoneAnswer } from '../ui/fixtures/phone-api.mjs';

export { PHONE_KEY, liveWork };

type Row = { label: string; children: { label: string; action?: string[]; separator?: boolean }[]; onOpenAction?: string[] };

const fixture = () => JSON.parse(readFileSync(new URL('../ui/fixtures/default.json', import.meta.url), 'utf8'));

/** The fixtures' session titles: the user's own words, which stay as they are in English. */
export const sessionTitles = (): string[] =>
  (fixture().snapshot.entries as Row[]).find((e) => e.onOpenAction?.[1] === 'sessions')?.children.filter((c) => !c.separator && !c.action).map((c) => c.label) ?? [];

// The fixtures name their logos; the app sends the files themselves: a blank one stands in.
const blank = Buffer.from('<svg xmlns="http://www.w3.org/2000/svg"/>').toString('base64');

/** Serves /api/ for the phone page; what it was asked, as path and query, goes into the list returned. `swiftbar`
 *  answers as SwiftBar's panel does: the computer's range and grouping only. `live` answers /api/live (liveWork on a
 *  clock the test moves); without it, today stands still. */
export async function servePhone(page: Page, key = PHONE_KEY, { swiftbar = false, live = null as ((data: unknown) => unknown) | null } = {}): Promise<string[]> {
  const data = fixture();
  const asked: string[] = [];
  await page.route('**/api/**', async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    const answer = phoneAnswer(data, { pathname: url.pathname, params: url.searchParams, given: request.headers()['x-starmeter-key'], key, logo: () => blank, swiftbar, live });
    if (answer.status === 200) asked.push(url.pathname + url.search);
    return route.fulfill({ status: answer.status, contentType: answer.type, body: answer.body });
  });
  return asked;
}
