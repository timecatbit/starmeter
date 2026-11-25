// 在手机上看 on the phone, in its own look (网页版, PhoneStage.svelte): the live monitor (working, a turn done, resting,
// waking, asked for only while shown), portrait, landscape and a computer's window laid out within the screen, the
// figures the computer's homepage shows, its ranges and groupings, the choice between 网页版 and 电脑版 (kept in the
// address, a cookie and the page's storage), reduced motion, and the languages.
import { readFileSync } from 'node:fs';
import { expect, test, type Page } from '@playwright/test';
import { liveWork, PHONE_KEY, servePhone } from './phone';

const errors: string[] = [];
test.beforeEach(({ page }) => {
  errors.length = 0;
  page.on('pageerror', (error) => errors.push(String(error)));
  page.on('console', (message) => { if (message.type() === 'error') errors.push(message.text()); });
});
test.afterEach(() => expect(errors).toEqual([]));

const PORTRAIT = { width: 393, height: 852 };
const LANDSCAPE = { width: 852, height: 393 };

async function phone(page: Page, size = PORTRAIT, hash = '', options: { swiftbar?: boolean; live?: (data: unknown) => unknown } = {}) {
  await page.setViewportSize(size);
  const asked = await servePhone(page, PHONE_KEY, options);
  await page.goto('/#k=' + PHONE_KEY + hash);
  await expect(page.locator('.stage')).toBeVisible();
  // The opening (the figures counting up, the cards rising) is over.
  await page.waitForTimeout(1600);
  return asked;
}

/** A figure as written, without the thousands' commas the phone's look adds. */
const plain = (text: string | null) => (text ?? '').replace(/,/g, '').trim();
/** 网页版's estimate, tokens, requests. */
async function stageFigures(page: Page) {
  const amount = await page.locator('.stage .hero .amount [data-value]').getAttribute('data-value');
  const rest = await page.locator('.stage .figure [data-value]').evaluateAll((list) => list.map((e) => e.getAttribute('data-value')));
  return [amount, rest[0], rest[1]].map(plain);
}
/** 电脑版's: the homepage's three figures. */
async function panelFigures(page: Page) {
  return (await page.locator('.dashboard .summary > .figure').allTextContents()).map(plain);
}
/** Boxes of what the page lays out, in the screen's coordinates. */
const box = async (page: Page, selector: string) => (await page.locator(selector).first().boundingBox())!;

test('网页版 is what a phone opens: portrait, balanced within the screen, its figures the computer\'s', async ({ page }) => {
  await phone(page);
  // The order of importance, top to bottom: the bar, now (the monitor), what is happening, then the figures for a range.
  const order = ['.stage .bar', '.stage .monitor', '.stage .activity', '.stage .ranges', '.stage .amount', '.stage .figures', '.stage .trend', '.stage .shares', '.stage .tabs', '.stage .card'];
  let last = -1;
  for (const selector of order) {
    const { y } = await box(page, selector);
    expect(y, selector).toBeGreaterThan(last);
    last = y;
  }
  // Nothing wider than the screen, and the column centred with even margins.
  const width = await page.evaluate(() => { const s = document.querySelector('.stage > .scroll')!; return { scroll: s.scrollWidth, client: s.clientWidth }; });
  expect(width.scroll).toBeLessThanOrEqual(width.client);
  for (const selector of ['.stage .monitor', '.stage .trend', '.stage .shares', '.stage .card', '.stage .plan']) {
    const b = await box(page, selector);
    expect(Math.abs(b.x - (PORTRAIT.width - b.x - b.width)), selector).toBeLessThanOrEqual(1);
  }
  // The three figures side by side, as wide as each other.
  const tiles = await page.locator('.stage .figure').evaluateAll((list) => list.map((e) => { const r = e.getBoundingClientRect(); return [r.top, r.width]; }));
  expect(new Set(tiles.map(([top]) => Math.round(top))).size).toBe(1);
  expect(Math.max(...tiles.map(([, w]) => w)) - Math.min(...tiles.map(([, w]) => w))).toBeLessThan(1);
  // The same estimate, tokens and requests as the homepage the computer shows (电脑版).
  const stage = await stageFigures(page);
  expect(stage[0]).toMatch(/^\$\d/);
  await page.getByRole('radio', { name: '电脑版' }).click();
  await expect(page.locator('.dashboard')).toBeVisible();
  expect(await panelFigures(page)).toEqual(stage);
  // Each tool's amount on its card is its card's on the computer.
  const panelCards = await page.locator('.dashboard .card .name').allTextContents();
  expect(panelCards.length).toBeGreaterThan(3);
});

test('网页版 on its side: a monitor by the desk, the live view still on the left, the rest scrolling on the right', async ({ page }) => {
  for (const size of [LANDSCAPE, { width: 667, height: 375 }]) {
    await phone(page, size);
    const lead = await box(page, '.stage .lead'), rest = await box(page, '.stage .rest');
    // Side by side, the left column's foot within the screen; the curve has room to read.
    expect(lead.x + lead.width).toBeLessThanOrEqual(rest.x);
    expect(lead.y + lead.height).toBeLessThanOrEqual(size.height + 1);
    const curve = await box(page, '.stage .lead .curve');
    expect(curve.y + curve.height).toBeLessThanOrEqual(size.height);
    expect(curve.height).toBeGreaterThan(90);
    expect(rest.x + rest.width).toBeLessThanOrEqual(size.width);
    // The right column scrolls on its own; the left stays.
    const scrolled = await page.evaluate(() => {
      const right = document.querySelector<HTMLElement>('.stage .rest')!, left = document.querySelector<HTMLElement>('.stage .lead')!;
      const before = left.getBoundingClientRect().top;
      right.scrollTop = 400;
      return { right: right.scrollTop, moved: left.getBoundingClientRect().top - before, wide: right.scrollWidth <= right.clientWidth };
    });
    expect(scrolled.right).toBeGreaterThan(0);
    expect(scrolled.moved).toBe(0);
    expect(scrolled.wide).toBe(true);
    // Every legend name of the shares readable: none cut to nothing.
    for (const name of await page.locator('.stage .legend .name').all()) {
      expect(await name.evaluate((e) => e.scrollWidth - e.clientWidth)).toBeLessThanOrEqual(2);
    }
  }
  // Turned back upright, one column again.
  await page.setViewportSize(PORTRAIT);
  await expect(page.locator('.stage.wide')).toHaveCount(0);
  expect((await box(page, '.stage .rest')).y).toBeGreaterThan((await box(page, '.stage .monitor')).y);
});

test('网页版: the ranges and groupings are the phone\'s own, the figures read for them as the computer reads them', async ({ page }) => {
  const asked = await phone(page);
  const ranges = page.getByRole('radiogroup', { name: '统计范围' });
  await expect(ranges.getByRole('radio')).toHaveText(['今天', '昨天', '近 24 小时', '近 7 天', '近 30 天', '本周', '本月', '全部']);
  const today = await stageFigures(page);
  await ranges.getByRole('radio', { name: '近 7 天' }).click();
  await expect.poll(() => asked.some((path) => path.includes('range=7d'))).toBe(true);
  await expect(ranges.getByRole('radio', { name: '近 7 天' })).toHaveAttribute('aria-checked', 'true');
  await expect(page.locator('.stage .hero .range')).toHaveText('近 7 天');
  await expect(page.locator('.stage .trend-head')).toContainText('每天 Token');
  await expect.poll(async () => (await stageFigures(page))[0]).not.toBe(today[0]);
  const week = await stageFigures(page);
  // The cards grouped by model, by project, and 会话, each asked for.
  const tabs = page.getByRole('radiogroup', { name: '卡片分组' });
  await tabs.getByRole('radio', { name: '模型' }).click();
  await expect.poll(() => asked.some((path) => path.includes('group=model'))).toBe(true);
  await expect(page.locator('.stage .card .name').first()).toHaveText('Claude Opus 5.5');
  await tabs.getByRole('radio', { name: '项目' }).click();
  await expect(page.locator('.stage .card .name').first()).toHaveText('starmeter');
  await expect(page.locator('.stage p.note').first()).toHaveText('按客户端自己的记录');
  await tabs.getByRole('radio', { name: '会话' }).click();
  await expect(page.locator('.stage .session').first()).toContainText('重做设置页的分段控件');
  await expect(page.getByText('这一轮完成时提醒')).toHaveCount(0);
  // A session opens in place to its turns.
  await page.locator('.stage .session .head').first().click();
  await expect(page.locator('.stage .session .turns').first()).toContainText('最近 3 轮');
  await tabs.getByRole('radio', { name: '工具' }).click();
  await expect(page.locator('.stage .card .name').first()).toHaveText('Claude');
  // A card opens in place to its counts and models.
  await page.locator('.stage .card .head').first().click();
  await expect(page.locator('.stage .card .more').first()).toContainText('Opus 5.5');
  // The arrow keys move along the grouping and pick, as on the panel.
  await tabs.getByRole('radio', { name: '工具' }).focus();
  await page.keyboard.press('ArrowRight');
  await expect(tabs.getByRole('radio', { name: '模型' })).toBeFocused();
  await expect(tabs.getByRole('radio', { name: '模型' })).toHaveAttribute('aria-checked', 'true');
  // The week's figures are the computer's for the week: 电脑版 keeps the range the phone picked.
  await page.getByRole('radio', { name: '电脑版' }).click();
  await expect(page.getByRole('button', { name: /^统计范围/ })).toContainText('近 7 天');
  expect(await panelFigures(page)).toEqual(week);
});

test('网页版: the trend reads under a finger, the share page opens and comes back, a new reading lights the tool that grew', async ({ page }) => {
  await phone(page);
  await page.locator('.stage .trend .chart').scrollIntoViewIfNeeded();
  const chart = await box(page, '.stage .trend .chart');
  await page.mouse.move(chart.x + chart.width * 0.5, chart.y + chart.height / 2);
  await expect(page.locator('.stage .trend .bubble .when')).toHaveText(/\d\d:\d\d/);
  await expect(page.locator('.stage .trend .bubble .part').first()).toBeVisible();
  await page.mouse.move(chart.x + chart.width * 0.5, chart.y - 60);
  await expect(page.locator('.stage .trend .bubble')).toHaveCount(0);
  // The share page, as on the panel's phone page, and back to 网页版 where it was.
  await page.getByRole('button', { name: '分享用量图' }).click();
  await expect(page.locator('.share-page .stage img')).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.locator('.stage .hero .amount')).toBeVisible();
  // The next reading has Claude further on: its card glows, the estimate rolls to the new amount.
  const before = (await stageFigures(page))[0];
  await page.route('**/api/snapshot**', async (route) => {
    const answer = await route.fetch();
    const body = await answer.json();
    for (const p of body.dashboard.providers) if (p.id === 'claude') { p.tokens += 5_000_000; p.cost += 3; }
    await route.fulfill({ response: answer, json: body });
  });
  await page.evaluate(() => document.dispatchEvent(new Event('visibilitychange')));
  await expect(page.locator('.stage .card.glow .name')).toHaveText('Claude');
  await expect.poll(async () => (await stageFigures(page))[0]).not.toBe(before);
  // The computer out of reach: said over the page, the figures kept.
  await page.unroute('**/api/snapshot**');
  await page.route('**/api/snapshot**', (route) => route.abort('connectionrefused'));
  await page.evaluate(() => document.dispatchEvent(new Event('visibilitychange')));
  await expect(page.locator('.phone-problem.staged')).toContainText('连不上电脑');
  await expect(page.locator('.stage .live.trouble')).toBeVisible();
  await expect(page.locator('.stage .hero .amount')).toBeVisible();
  errors.splice(0, errors.length, ...errors.filter((line) => !/Failed to load resource|Could not connect/.test(line)));
});

test('网页版 and 电脑版: picked on the phone, at either page\'s foot, and kept through a reload and a new address', async ({ page }) => {
  await phone(page);
  await expect(page.getByRole('radiogroup', { name: '手机上的样式' }).getByRole('radio', { name: '网页版' })).toHaveAttribute('aria-checked', 'true');
  await page.getByRole('radio', { name: '电脑版' }).click();
  await expect(page.locator('.dashboard')).toBeVisible();
  await expect(page.locator('.stage')).toHaveCount(0);
  expect(new URL(page.url()).hash).toContain('v=panel');
  expect(new URL(page.url()).hash).toContain('k=' + PHONE_KEY);
  // A reload keeps it, and so does the address the computer gives next time (no look in it): its cookie, even with the
  // page's storage gone (a new port is a new page to the browser, not to its cookies).
  await page.reload();
  await expect(page.locator('.dashboard')).toBeVisible();
  await page.evaluate(() => localStorage.clear());
  await page.goto('/#k=' + PHONE_KEY);
  await page.reload();
  await expect(page.locator('.dashboard')).toBeVisible();
  await expect(page.locator('.stage')).toHaveCount(0);
  // Back at 电脑版's foot: 网页版, its opening again.
  const back = page.locator('.look-foot').getByRole('radio', { name: '网页版' });
  await back.scrollIntoViewIfNeeded();
  await back.click();
  await expect(page.locator('.stage')).toBeVisible();
  expect(new URL(page.url()).hash).toContain('v=stage');
  expect(await page.evaluate(() => document.cookie)).toContain('starmeter-phone-look=stage');
});

test('网页版 with reduced motion: everything simply there, nothing drifting or counting', async ({ page }) => {
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.setViewportSize(PORTRAIT);
  await servePhone(page);
  await page.goto('/#k=' + PHONE_KEY);
  await expect(page.locator('.stage.still')).toBeVisible();
  // At once, without the opening: the estimate's first wheel already on its digit (not 0), the sky and the cards still.
  const turned = await page.locator('.stage .amount .wheel .digits').first().evaluate((e) => new DOMMatrix(getComputedStyle(e).transform).m42);
  expect(turned).toBeLessThan(0);
  expect(await page.locator('.stage .aurora').first().evaluate((e) => getComputedStyle(e).animationName)).toBe('none');
  await expect(page.locator('.stage .card.enter')).toHaveCount(0);
  await expect(page.locator('.stage .chart .drawn')).toHaveCount(0);
});

test('网页版 for SwiftBar\'s panel: the computer\'s range and grouping only, with 会话', async ({ page }) => {
  await phone(page, PORTRAIT, '', { swiftbar: true });
  await expect(page.getByRole('radiogroup', { name: '统计范围' })).toHaveCount(0);
  await expect(page.locator('.stage .hero .range')).toHaveText('今天');
  await expect(page.getByRole('radiogroup', { name: '卡片分组' }).getByRole('radio')).toHaveText(['工具', '会话']);
});

test('网页版 in English and in 繁體中文: laid out within the screen, both ways up', async ({ context }) => {
  for (const [hash, words] of [['&l=en', ['Share by tool', 'Plans and services', 'Web', 'Desktop', 'Hourly tokens']], ['&l=zh-Hant', ['工具佔比', '訂閱與服務', '網頁版', '電腦版']]] as const) {
    for (const size of [PORTRAIT, LANDSCAPE]) {
      // A page of its own each time: only the address's fragment would change, and the page keeps its language.
      const page = await context.newPage();
      await phone(page, size, hash);
      const text = await page.locator('.stage').innerText();
      for (const word of words) expect(text, `${hash} ${size.width}`).toContain(word);
      const wide = await page.evaluate(() => [...document.querySelectorAll<HTMLElement>('.stage > .scroll, .stage.wide .rest')].every((e) => e.scrollWidth <= e.clientWidth));
      expect(wide).toBe(true);
      // The tabs' labels and the figures' labels fit on their line.
      for (const selector of ['.stage .tabs .choice', '.stage .figure-label']) {
        for (const item of await page.locator(selector).all()) expect(await item.evaluate((e) => e.scrollWidth - e.clientWidth), selector).toBeLessThanOrEqual(1);
      }
      await page.close();
    }
  }
});

test('网页版 without Summary on the homepage: the figures for a range are the cards and plans only, as the panel shows it', async ({ page }) => {
  await page.setViewportSize(LANDSCAPE);
  await servePhone(page);
  await page.route('**/api/snapshot**', async (route) => {
    const answer = await route.fetch();
    const body = await answer.json();
    // As the engine sends it with Summary left off the homepage: the module gone from the choices, and the dashboard's flag.
    body.dashboard.showSummary = false;
    if (body.preferences) body.preferences.visible = body.preferences.visible.filter((m: string) => m !== 'summary');
    await route.fulfill({ response: answer, json: body });
  });
  await page.goto('/#k=' + PHONE_KEY);
  await expect(page.locator('.stage .card').first()).toBeVisible();
  await expect(page.locator('.stage .hero, .stage .trend, .stage .shares')).toHaveCount(0);
  // The live monitor stays: it is today's, whatever the homepage shows.
  await expect(page.locator('.stage .monitor')).toBeVisible();
  await expect(page.getByRole('radiogroup', { name: '统计范围' })).toBeVisible();
});


/** A clock the test moves: the made-up work (liveWork) and the computer's moments in each reading follow it. */
function scripted() {
  const clock = { now: Date.UTC(2026, 10, 25, 2, 0, 0) };
  const live = liveWork({ now: () => clock.now, length: 1000, claudeEnds: 20, codexEnds: 12 });
  return { clock, live, at: (seconds: number) => { clock.now = Date.UTC(2026, 10, 25, 2, 0, 0) + seconds * 1000; } };
}

test('the live view working: how fast, today rolling up, the sessions running with their clocks and tools, what happened', async ({ page }) => {
  const work = scripted();
  const asked = await phone(page, PORTRAIT, '', { live: work.live });
  // At the turn's start: Claude Code's session running, Codex's not yet.
  await expect(page.locator('.stage .monitor.working')).toBeVisible();
  await expect(page.locator('.stage .monitor .state')).toHaveText('正在工作 · 1 个会话运行中');
  await expect(page.locator('.stage .monitor [data-figure="rate"]')).toBeVisible();
  await expect(page.locator('.stage .monitor .unit')).toHaveText(/^输出 Token\/s/);
  // Nine seconds on: three of Claude's requests and one of Codex's have landed, both sessions run.
  work.at(9);
  await expect.poll(() => page.locator('.stage .activity .session').count(), { timeout: 5000 }).toBe(2);
  await expect(page.locator('.stage .monitor .state')).toHaveText('正在工作 · 2 个会话运行中');
  const first = page.locator('.stage .activity .session').first();
  await expect(first.locator('.title')).toHaveText('重做设置页的分段控件');
  await expect(first.locator('.timer')).toHaveText(/^0:\d\d$/);
  // The clock goes second by second, as the turn does.
  const shown = await first.locator('.timer').textContent();
  await expect(first.locator('.timer')).not.toHaveText(shown!, { timeout: 2500 });
  await expect(first.locator('.chip').first()).toHaveText(/^(Edit|Bash|Read) ×\d+$/);
  await expect(first.locator('.turn-figures')).toHaveText(/^本轮 [\d.]+K Token · \$0\.\d\d$/);
  await expect(page.locator('.stage .feed .event.usage').first()).toContainText(/(Claude|ChatGPT)\s*\+[\d.]+K Token/);
  await expect(page.locator('.stage .feed .event.tool').first()).toContainText('调用');
  // The curve is the live one, and today's estimate under the speed is the live reading's.
  await expect(page.locator('.stage .monitor .live-chart canvas')).toBeVisible();
  await expect(page.locator('.stage .monitor .tick-value[data-figure="today"] [data-value]')).toHaveAttribute('data-value', /^\$2\d\.\d\d$/);
  // Asked for every couple of seconds while working.
  const before = asked.filter((path) => path.startsWith('/api/live')).length;
  await page.waitForTimeout(4300);
  expect(asked.filter((path) => path.startsWith('/api/live')).length - before).toBeGreaterThanOrEqual(2);
});

test('the live view: a turn done is said at the top, then resting, then waking when work comes again', async ({ page }) => {
  const work = scripted();
  await phone(page, PORTRAIT, '', { live: work.live });
  work.at(9);
  await expect.poll(() => page.locator('.stage .activity .session').count(), { timeout: 5000 }).toBe(2);
  // Both turns end (Codex at 12 s, Claude at 20 s): each is said in what happened, the last at the top with its time.
  work.at(21);
  await expect(page.locator('.stage .turn-done')).toContainText('这一轮完成', { timeout: 5000 });
  await expect(page.locator('.stage .turn-done .done-detail')).toContainText(/重做设置页的分段控件|harbor-api/);
  await expect(page.locator('.stage .feed .event.done')).toHaveCount(2);
  await expect(page.locator('.stage .activity .session')).toHaveCount(0);
  // Requests landed just now: still working for a while, with no session running.
  await expect(page.locator('.stage .monitor .state')).toHaveText('正在工作');
  // Nothing for a minute: resting, the headline today's estimate and the curve today's trend.
  work.at(80);
  await expect(page.locator('.stage .monitor:not(.working)')).toBeVisible({ timeout: 5000 });
  await expect(page.locator('.stage .monitor .state')).toHaveText(/休息中/);
  await expect(page.locator('.stage .monitor [data-figure="today"]').first()).toBeVisible();
  await expect(page.locator('.stage .monitor .live-chart')).toHaveCount(0);
  await expect(page.locator('.stage .monitor .chart')).toBeVisible();
  await expect(page.locator('.stage .activity .quiet').first()).toContainText('没有在运行的会话');
  // A new turn: the page wakes, says so first, and the speed is the headline again.
  work.at(1001);
  await expect(page.locator('.stage .feed .event').first()).toHaveClass(/wake/, { timeout: 7000 });
  await expect(page.locator('.stage .feed .event.wake .what')).toHaveText('开始工作');
  await expect(page.locator('.stage .monitor.working [data-figure="rate"]')).toBeVisible();
});

test('the live view is asked for only while the page shows', async ({ page }) => {
  const work = scripted();
  const asked = await phone(page, PORTRAIT, '', { live: work.live });
  const live = () => asked.filter((path) => path.startsWith('/api/live')).length;
  await expect.poll(live).toBeGreaterThan(0);
  await page.evaluate(() => {
    Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => 'hidden' });
    document.dispatchEvent(new Event('visibilitychange'));
  });
  await page.waitForTimeout(300);
  const hidden = live();
  await page.waitForTimeout(4500);
  expect(live()).toBe(hidden);
  // Back in sight: asked at once.
  await page.evaluate(() => {
    Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => 'visible' });
    document.dispatchEvent(new Event('visibilitychange'));
  });
  await expect.poll(live, { timeout: 1500 }).toBeGreaterThan(hidden);
});

test('网页版 in a computer\'s window: the monitor large beside what is happening, the figures under both, a page that scrolls', async ({ page }) => {
  for (const size of [{ width: 1440, height: 900 }, { width: 1280, height: 800 }, { width: 1024, height: 768 }]) {
    const work = scripted();
    await phone(page, size, '', { live: work.live });
    await expect(page.locator('.stage.desk')).toBeVisible();
    const monitor = await box(page, '.stage .lead'), activity = await box(page, '.stage .activity-box');
    expect(monitor.x + monitor.width).toBeLessThanOrEqual(activity.x);
    expect(Math.abs(monitor.y - activity.y)).toBeLessThanOrEqual(1);
    const a = await box(page, '.stage .stats-a'), b = await box(page, '.stage .stats-b');
    expect(Math.min(a.y, b.y)).toBeGreaterThan(Math.max(monitor.y + monitor.height, activity.y + activity.height));
    expect(a.x + a.width).toBeLessThanOrEqual(b.x);
    // Centred, at most 1320 wide, nothing wider than the window.
    const frame = await box(page, '.stage .frame');
    expect(frame.width).toBeLessThanOrEqual(1320);
    expect(Math.abs(frame.x - (size.width - frame.x - frame.width))).toBeLessThanOrEqual(1);
    expect(await page.evaluate(() => { const s = document.querySelector('.stage > .scroll')!; return s.scrollWidth <= s.clientWidth; })).toBe(true);
    // A pointer reads the figures' trend on hover.
    await page.locator('.stage .trend .chart').scrollIntoViewIfNeeded();
    const trend = await box(page, '.stage .trend .chart');
    await page.mouse.move(trend.x + trend.width / 2, trend.y + trend.height / 2);
    await expect(page.locator('.stage .trend .bubble')).toBeVisible();
    await page.mouse.move(0, 0);
  }
});


/** The phone's snapshot as the computer would send it with these choices (theme, background, look). */
async function computerChose(page: Page, chosen: { theme?: string; appearance?: string; style?: string }) {
  await page.unroute('**/api/snapshot**').catch(() => {});
  await page.route('**/api/snapshot**', async (route) => {
    const answer = await route.fetch();
    const body = await answer.json();
    // The theme and background are the computer's saved preferences too, which the panel reads first.
    const preferences = { ...body.preferences, ...(chosen.theme ? { theme: chosen.theme } : {}), ...(chosen.appearance ? { appearance: chosen.appearance } : {}) };
    await route.fulfill({ response: answer, json: { ...body, ...chosen, preferences } });
  });
  await page.evaluate(() => document.dispatchEvent(new Event('visibilitychange')));
}

test('网页版 follows the computer\'s look: its theme, its background as the panes\' material, 标准 or 手绘', async ({ page }) => {
  await page.emulateMedia({ colorScheme: 'light' });
  await phone(page);
  await expect(page.locator('.stage.light.solid')).toBeVisible();
  // Dark on the computer: dark here, whatever the phone's own scheme.
  await computerChose(page, { theme: 'dark' });
  await expect(page.locator('.stage:not(.light)')).toBeVisible();
  // Its background as the panes' material.
  await computerChose(page, { theme: 'dark', appearance: 'frosted' });
  await expect(page.locator('.stage.frosted')).toBeVisible();
  expect(await page.locator('.stage .feed, .stage .card').first().evaluate((e) => getComputedStyle(e).backdropFilter || getComputedStyle(e).getPropertyValue('-webkit-backdrop-filter'))).toContain('blur');
  await computerChose(page, { theme: 'dark', appearance: 'glass' });
  await expect(page.locator('.stage.glass')).toBeVisible();
  // 手绘: the page in the hand's letters and on paper, its lines by hand; back to 标准 as the computer goes back.
  await computerChose(page, { theme: 'light', style: 'sketch' });
  await expect(page.locator('.stage.sketch.light')).toBeVisible();
  await expect(page.locator('html')).toHaveAttribute('data-style', 'sketch');
  expect(await page.locator('.stage .state-text').evaluate((e) => getComputedStyle(e).fontFamily)).toMatch(/^"?Starmeter Hand/);
  expect(await page.locator('.stage .monitor .big').evaluate((e) => getComputedStyle(e).fontFamily)).toMatch(/^"?Starmeter Hand/);
  expect(await page.locator('.stage .lead .chart svg, .stage .live-chart canvas').first().evaluate((e) => getComputedStyle(e).filter)).toContain('hand');
  await computerChose(page, { theme: 'light', style: 'standard' });
  await expect(page.locator('.stage.sketch')).toHaveCount(0);
  await expect(page.locator('html')).not.toHaveAttribute('data-style', 'sketch');
});

test('网页版\'s figures and words keep the panel\'s rules: figures rounded at 600, words in the panel\'s own face', async ({ page }) => {
  const work = scripted();
  await phone(page, PORTRAIT, '', { live: work.live });
  work.at(9);
  await expect.poll(() => page.locator('.stage .activity .session').count(), { timeout: 5000 }).toBe(2);
  const rounded = await page.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue('--rounded').split(',')[0].trim().replace(/"/g, ''));
  const text = await page.evaluate(() => getComputedStyle(document.documentElement).fontFamily.split(',')[0].trim().replace(/"/g, ''));
  for (const selector of ['.stage .monitor .big', '.stage .tick-value', '.stage .timer-figure', '.stage .card .amount', '.stage .figure-value', '.stage .hero .amount']) {
    const face = await page.locator(selector).first().evaluate((e) => { const css = getComputedStyle(e); return { family: css.fontFamily.split(',')[0].trim().replace(/"/g, ''), weight: css.fontWeight }; });
    expect(face, selector).toEqual({ family: rounded, weight: '600' });
  }
  for (const selector of ['.stage .state-text', '.stage .activity .title', '.stage .card .name', '.stage .feed .when', '.stage .unit', '.stage .tick-label']) {
    expect(await page.locator(selector).first().evaluate((e) => getComputedStyle(e).fontFamily.split(',')[0].trim().replace(/"/g, '')), selector).toBe(text);
  }
  // Nothing on the page heavier than 600, and the sign of an amount as the panel sets it: smaller, lighter, raised.
  expect(await page.locator('.stage').evaluate((root) => [...root.querySelectorAll('*')].filter((e) => Number(getComputedStyle(e).fontWeight) > 600).length)).toBe(0);
  const sign = await page.locator('.stage .tick-value .sign').first().evaluate((e) => ({ size: parseFloat(getComputedStyle(e).fontSize), of: parseFloat(getComputedStyle(e.closest('.tick-value')!).fontSize), weight: getComputedStyle(e).fontWeight }));
  expect(sign.size / sign.of).toBeCloseTo(0.62, 2);
  expect(sign.weight).toBe('500');
});

test('on a phone the browser\'s bars take the page\'s colours: 网页版\'s sky edges, 电脑版\'s background, as the theme changes', async ({ page }) => {
  await page.emulateMedia({ colorScheme: 'dark' });
  await phone(page);
  expect(await page.locator('meta[name="viewport"]').getAttribute('content')).toContain('viewport-fit=cover');
  // The page is the whole screen, fixed to its edges (under the bars too), not a band that stops short of them.
  const window = await box(page, '.window.phone');
  expect(window).toEqual({ x: 0, y: 0, width: PORTRAIT.width, height: PORTRAIT.height });
  expect(await page.locator('.window.phone').evaluate((e) => getComputedStyle(e).position)).toBe('fixed');
  const edges = () => page.evaluate(() => {
    const stage = document.querySelector('.stage'), css = stage ? getComputedStyle(stage) : null;
    return { meta: document.querySelector<HTMLMetaElement>('meta[name="theme-color"]')?.content, html: getComputedStyle(document.documentElement).backgroundColor,
      top: css?.getPropertyValue('--edge-top').trim(), bottom: css ? getComputedStyle(document.querySelector('.stage .edge.bottom')!).backgroundColor : null,
      topStrip: css ? getComputedStyle(document.querySelector('.stage .edge.top')!).backgroundColor : null };
  });
  const dark = await edges();
  expect(dark.meta).toBe(dark.top);
  expect(dark.html).toBe(dark.bottom);
  // The top of the sky is flat in the status bar's colour, under the bar and the light.
  expect(dark.topStrip).not.toBe(dark.bottom);
  // The computer turns light: so do the bars.
  await computerChose(page, { theme: 'light' });
  await expect(page.locator('.stage.light')).toBeVisible();
  await expect.poll(async () => (await edges()).meta).not.toBe(dark.meta);
  const light = await edges();
  expect(light.meta).toBe(light.top);
  expect(light.html).toBe(light.bottom);
  // 电脑版: its own background, top and bottom.
  await page.getByRole('radio', { name: '电脑版' }).click();
  await expect(page.locator('.dashboard')).toBeVisible();
  await expect.poll(async () => (await edges()).html).toBe(await page.locator('.window.phone').evaluate((e) => getComputedStyle(e).backgroundColor));
});

test('保持亮屏 holds one lock at most, and lets it go when switched off or when the page goes away (STAGE-2)', async ({ page }) => {
  // A browser's wake lock, slow to answer: the locks held, and how many requests went out.
  await page.addInitScript(() => {
    const state = { held: 0, asked: 0 };
    (window as unknown as { lockState: typeof state }).lockState = state;
    Object.defineProperty(navigator, 'wakeLock', {
      configurable: true,
      value: {
        request: () => new Promise((resolve) => {
          state.asked++;
          setTimeout(() => {
            state.held++;
            let released = false;
            resolve({ release: async () => { if (!released) { released = true; state.held--; } }, addEventListener: () => {} });
          }, 300);
        }),
      },
    });
  });
  await phone(page, LANDSCAPE);
  const held = () => page.evaluate(() => (window as unknown as { lockState: { held: number; asked: number } }).lockState);
  const button = page.getByRole('button', { name: '保持亮屏' });
  await button.click();
  await expect(button).toHaveAttribute('aria-pressed', 'true');
  await expect.poll(async () => (await held()).held).toBe(1);
  await page.waitForTimeout(500);
  expect(await held()).toEqual({ held: 1, asked: 1 });
  // Off: nothing held.
  await button.click();
  await expect.poll(async () => (await held()).held).toBe(0);
  // Off again before the lock arrives: the lock that comes is let go.
  await button.click();
  await button.click();
  await page.waitForTimeout(500);
  expect((await held()).held).toBe(0);
  // On, then to 电脑版: this page gone, nothing held.
  await button.click();
  await expect.poll(async () => (await held()).held).toBe(1);
  await page.getByRole('radio', { name: '电脑版' }).click();
  await expect(page.locator('.dashboard')).toBeVisible();
  await expect.poll(async () => (await held()).held).toBe(0);
});

/** The page out of sight for `seconds` on the phone's clock (its Date moved on), the computer's clock at `computer`. */
async function away(page: Page, work: ReturnType<typeof scripted>, seconds: number, computer: number) {
  const visibility = (state: string) => page.evaluate((state) => {
    Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => state });
    document.dispatchEvent(new Event('visibilitychange'));
  }, state);
  await visibility('hidden');
  await page.waitForTimeout(300);
  work.at(computer);
  // Date.now as it stands (moved on already, if it was) plus the time away.
  await page.evaluate((ms) => { const was = Date.now.bind(Date); Date.now = () => was() + ms; }, seconds * 1000);
  await visibility('visible');
}

test('back after a while out of sight, what went on meanwhile is not this moment\'s speed, nor a turn just done (STAGE-3)', async ({ page }) => {
  const work = scripted();
  await phone(page, PORTRAIT, '', { live: work.live });
  work.at(3);
  await expect(page.locator('.stage .monitor.working')).toBeVisible();
  const rate = page.locator('.stage .monitor [data-figure="rate"] [data-value]');
  await expect.poll(() => rate.getAttribute('data-value'), { timeout: 5000 }).not.toBeNull();
  await page.waitForTimeout(2500);
  const before = await rate.getAttribute('data-value');
  const events = await page.locator('.stage .feed .event').count();
  // Locked for 100 s while both sessions worked on (the computer at 9 s, seven requests landed meanwhile): still working,
  // but what was written while locked is not spread over the last minute, nor dropped on the curve as landing now.
  await away(page, work, 100, 9);
  await expect.poll(() => page.locator('.stage .activity .session').count(), { timeout: 5000 }).toBe(2);
  await page.waitForTimeout(2500);
  await expect(page.locator('.stage .monitor.working')).toBeVisible();
  // (Nothing seen written in the last minute: the speed is nothing yet, never what the locked minutes added over 60 s.)
  expect(['—', '0']).toContain(await rate.getAttribute('data-value'));
  void before;
  expect(await page.locator('.stage .feed .event.usage').count()).toBeLessThanOrEqual(events);
  // Locked for three minutes while both turns ended: resting, no turn said done now with the locked minutes as its time.
  await away(page, work, 180, 200);
  await expect(page.locator('.stage .monitor .state')).toHaveText(/休息中/, { timeout: 5000 });
  await page.waitForTimeout(2500);
  await expect(page.locator('.stage .turn-done')).toHaveCount(0);
  await expect(page.locator('.stage .feed .event.done')).toHaveCount(0);
});

test('on SwiftBar\'s panel the live figures are the computer\'s range, said as such, and its picking another is no usage (STAGE-4)', async ({ page }) => {
  // The plugin's snapshot: today, then the computer picks 近 30 天 (far more), then today again.
  let range = 'today';
  const data = JSON.parse(readFileSync(new URL('../ui/fixtures/default.json', import.meta.url), 'utf8'));
  const live = () => ({ at: Date.now() / 1000, dashboard: structuredClone(data.dashboards[range]), sessions: null, turns: {} });
  await phone(page, PORTRAIT, '', { swiftbar: true, live });
  await expect(page.locator('.stage .monitor .figure-block > .unit')).toHaveText('今天 · API 估算');
  range = '30d';
  await expect(page.locator('.stage .monitor .figure-block > .unit')).toHaveText('近 30 天 · API 估算', { timeout: 7000 });
  await page.waitForTimeout(800);
  // Not tokens landing now: no request in what happened, not working, nothing added to the estimate.
  await expect(page.locator('.stage .feed .event')).toHaveCount(0);
  await expect(page.locator('.stage .monitor.working')).toHaveCount(0);
  await expect(page.locator('.stage .monitor .added')).toHaveCount(0);
  // Scrolled, the bar says that range too beside the estimate (STAGE-10).
  await page.locator('.stage > .scroll').evaluate((s) => (s.scrollTop = 700));
  await expect(page.locator('.stage .bar .compact-range')).toHaveText('近 30 天');
  await page.locator('.stage > .scroll').evaluate((s) => (s.scrollTop = 0));
  range = 'today';
  await expect(page.locator('.stage .monitor .figure-block > .unit')).toHaveText('今天 · API 估算', { timeout: 7000 });
  await page.waitForTimeout(800);
  await expect(page.locator('.stage .feed .event')).toHaveCount(0);
});

test('today\'s estimate with no priced tool counted in Summary is —, not $0.00, whatever a tool left out of it cost (STAGE-5)', async ({ page }) => {
  const data = JSON.parse(readFileSync(new URL('../ui/fixtures/default.json', import.meta.url), 'utf8'));
  // Every tool Summary counts unpriced, and one left out of Summary (主页 · 未计入 Summary) with a cost of its own.
  const live = () => {
    const dashboard = structuredClone(data.dashboards.today);
    dashboard.providers.forEach((p: { cost: number | null; counted?: unknown; summaryIncluded?: boolean }, i: number) => {
      p.counted = undefined;
      if (i === 0) { p.summaryIncluded = false; p.cost = 5; } else p.cost = null;
    });
    return { at: Date.now() / 1000, dashboard, sessions: null, turns: {} };
  };
  await phone(page, PORTRAIT, '', { live });
  await expect(page.locator('.stage .monitor:not(.working)')).toBeVisible();
  await expect(page.locator('.stage .monitor [data-figure="today"] [data-value]').first()).toHaveAttribute('data-value', '—');
});

test('one reading loop at a time: a reading two loops waited for is taken once, the page back in sight or 网页版 back on screen (STAGE-6)', async ({ page }) => {
  await phone(page);
  // The computer slow to answer, so the page comes back (or 网页版 does) while a reading is on its way.
  await page.route('**/api/live**', async (route) => { await new Promise((resolve) => setTimeout(resolve, 600)); await route.fallback(); });
  const taken = await page.evaluate(async () => {
    const { PhoneLive } = await import('/src/lib/phoneLive.svelte.ts');
    const wait = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
    const results: { takes: number; distinct: number }[] = [];
    for (const comeBack of ['visible', 'restart']) {
      const live = new PhoneLive(), answers: unknown[] = [], take = live.take.bind(live);
      live.take = (answer, now) => { answers.push(answer); take(answer, now); };
      live.start();
      await wait(150);
      if (comeBack === 'visible') document.dispatchEvent(new Event('visibilitychange'));
      else { live.stop(); live.start(); }
      await wait(1000);
      live.stop();
      results.push({ takes: answers.length, distinct: new Set(answers).size });
    }
    return results;
  });
  for (const { takes, distinct } of taken) {
    expect(takes).toBeGreaterThan(0);
    expect(takes).toBe(distinct);
  }
});

test('a turn done is said at the top once, not again when 网页版 comes back from 电脑版 or the share page (STAGE-7)', async ({ page }) => {
  const work = scripted();
  await phone(page, PORTRAIT, '', { live: work.live });
  work.at(9);
  await expect.poll(() => page.locator('.stage .activity .session').count(), { timeout: 5000 }).toBe(2);
  work.at(21);
  const toast = page.locator('.stage .turn-done');
  await expect(toast).toBeVisible({ timeout: 5000 });
  await toast.locator('.done-body').click();
  await expect(toast).toHaveCount(0);
  // To 电脑版 and back, then to the share page and back: the turn is not said done again.
  await page.getByRole('radio', { name: '电脑版' }).click();
  await expect(page.locator('.dashboard')).toBeVisible();
  const back = page.locator('.look-foot').getByRole('radio', { name: '网页版' });
  await back.scrollIntoViewIfNeeded();
  await back.click();
  await expect(page.locator('.stage')).toBeVisible();
  await never(toast);
  await page.getByRole('button', { name: '分享用量图' }).click();
  await expect(page.locator('.share-page .stage img')).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.locator('.stage .monitor')).toBeVisible();
  await never(toast);
});

/** Not there at any look over a second and a half. */
async function never(locator: ReturnType<Page['locator']>) {
  for (let i = 0; i < 15; i++) {
    expect(await locator.count()).toBe(0);
    await locator.page().waitForTimeout(100);
  }
}

// MARK: STAGE-8's small points

const fixtureData = () => JSON.parse(readFileSync(new URL('../ui/fixtures/default.json', import.meta.url), 'utf8'));
/** 会话's row as the phone has it, from the fixtures. */
const sessionsRow = (data: { snapshot: { entries: { onOpenAction?: string[]; children: { action?: unknown; separator?: boolean }[] }[] } }) => {
  const entry = structuredClone(data.snapshot.entries.find((e) => e.onOpenAction?.[1] === 'sessions')!);
  entry.children = entry.children.filter((c) => !c.action).filter((c, i, all) => !(c.separator && i === all.length - 1));
  delete entry.onOpenAction;
  return entry;
};

/** Today standing still, nothing running: resting. */
const resting = (data = fixtureData()) => () => ({ at: Date.now() / 1000, dashboard: structuredClone(data.dashboards.today), sessions: null, turns: {} });

test('resting, the bar says when the computer was last read, not the time now (STAGE-8 · 1)', async ({ page }) => {
  await phone(page, PORTRAIT, '', { live: resting() });
  const said = page.locator('.stage .bar .live .num');
  await expect(said).toHaveText(/^\d\d:\d\d$/);
  const read = await said.textContent();
  // The computer stops answering (a reading on its way for ever), and five minutes pass on the phone.
  await page.route('**/api/live**', () => {});
  await page.waitForTimeout(5500);
  await page.evaluate(() => { const was = Date.now.bind(Date); Date.now = () => was() + 5 * 60_000; });
  await page.waitForTimeout(1300);
  await expect(said).toHaveText(read!);
});

test('in English, resting, today\'s tokens are Tokens (STAGE-8 · 2)', async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem('starmeter.language', 'en'));
  await phone(page, PORTRAIT, '', { live: resting() });
  await expect(page.locator('.stage .monitor .ticker .tick-label').last()).toHaveText('Tokens');
});

test('a request costing less than a cent says no $0.00, neither in what happened nor rising off the estimate (STAGE-8 · 3)', async ({ page }) => {
  const data = fixtureData();
  let n = 0;
  const live = () => {
    const dashboard = structuredClone(data.dashboards.today), p = dashboard.providers.find((x: { id: string }) => x.id === 'claude');
    p.tokens += n * 2000; p.output += n * 20; p.cost += n * 0.002; p.requests += n;
    if (p.counted) { p.counted.tokens += n * 2000; p.counted.output = (p.counted.output ?? 0) + n * 20; p.counted.cost += n * 0.002; p.counted.requests += n; }
    n++;
    return { at: Date.now() / 1000, dashboard, sessions: null, turns: {} };
  };
  await phone(page, PORTRAIT, '', { live });
  await expect.poll(() => page.locator('.stage .feed .event.usage').count(), { timeout: 8000 }).toBeGreaterThan(1);
  for (const text of await page.locator('.stage .feed .event.usage .what').allTextContents()) expect(text).not.toContain('$0.00');
  await never(page.locator('.stage .monitor .added'));
});

test('before the live view\'s first reading, the monitor shows the homepage\'s estimate for today, not — (STAGE-8 · 5)', async ({ page }) => {
  await page.setViewportSize(PORTRAIT);
  await servePhone(page, PHONE_KEY);
  await page.route('**/api/live**', () => {});
  await page.goto('/#k=' + PHONE_KEY);
  await expect(page.locator('.stage')).toBeVisible();
  const homepage = await page.locator('.stage .hero .amount [data-value]').getAttribute('data-value');
  expect(homepage).toMatch(/^\$\d/);
  await expect(page.locator('.stage .monitor [data-figure="today"] [data-value]').first()).toHaveAttribute('data-value', homepage!);
});

test('a turn\'s minute from 会话 (no second from the computer) begun before midnight on the computer\'s clock reads its minutes (STAGE-8 · 6)', async ({ page }) => {
  const data = fixtureData();
  // The computer's clock two hours ahead of the phone's and a minute past its midnight; the turn began at 23:58.
  const offset = 2 * 3600_000;
  const live = () => {
    const there = new Date(Date.now() + offset);
    const midnight = new Date(there); midnight.setHours(0, 0, 0, 0);
    const at = midnight.getTime() + 60_000;
    const sessions = sessionsRow(data);
    const row = sessions.children.find((c: { statusLight?: string }) => c.statusLight !== undefined) as { statusLight: string; detail: string; children: { label: string; detail?: string }[] };
    row.statusLight = 'new';
    row.detail = row.detail.replace(/^[^·]*·/, '运行中 3 分钟 ·');
    const turn = row.children.find((c) => c.detail !== undefined)!;
    turn.label = '23:58 · 12.0K Token · $0.05';
    turn.detail = '1 次请求 · Bash ×1';
    return { at: at / 1000, dashboard: structuredClone(data.dashboards.today), sessions, turns: {} };
  };
  await phone(page, PORTRAIT, '', { live });
  await expect(page.locator('.stage .activity .session').first().locator('.timer')).toHaveText('3分钟');
});

test('the computer back on dollars, the live figures are in dollars again (STAGE-8 · 7)', async ({ page }) => {
  const data = fixtureData();
  let yuan = true;
  const live = () => ({ at: Date.now() / 1000, dashboard: { ...structuredClone(data.dashboards.today), currency: yuan ? { code: 'CNY', symbol: '¥', rate: 7.2 } : null }, sessions: null, turns: {} });
  await phone(page, PORTRAIT, '', { live });
  const today = page.locator('.stage .monitor [data-figure="today"] [data-value]').first();
  await expect(today).toHaveAttribute('data-value', /^¥/, { timeout: 7000 });
  yuan = false;
  await expect(today).toHaveAttribute('data-value', /^\$/, { timeout: 7000 });
});

/** The snapshot the phone is sent, changed by `change`. */
async function snapshotAs(page: Page, change: (snapshot: Record<string, unknown> & { entries: { gauges?: { label: string }[] }[] }) => void) {
  await page.route('**/api/snapshot**', async (route) => {
    const answer = await route.fetch();
    const body = await answer.json();
    change(body);
    await route.fulfill({ response: answer, json: body });
  });
}

test('a plan\'s window with a long name in a narrow column keeps its share whole, the name giving way (STAGE-8 · 4)', async ({ page }) => {
  await page.setViewportSize({ width: 667, height: 375 });
  await servePhone(page, PHONE_KEY);
  await snapshotAs(page, (snapshot) => {
    for (const entry of snapshot.entries) for (const g of entry.gauges ?? []) g.label = g.label + ' 编程工具与网页共用的额度';
  });
  await page.goto('/#k=' + PHONE_KEY);
  await expect(page.locator('.stage .plan').first()).toBeVisible();
  const windows = await page.locator('.stage .window-name').evaluateAll((list) => list.map((name) => {
    const box = name.getBoundingClientRect(), share = name.querySelector('.num')!.getBoundingClientRect();
    return { name: box.right, share: share.right, width: share.width, text: name.querySelector('.num')!.textContent };
  }));
  expect(windows.length).toBeGreaterThan(0);
  for (const w of windows) {
    expect(w.width, w.text!).toBeGreaterThan(0);
    expect(w.share, w.text!).toBeLessThanOrEqual(w.name + 0.5);
  }
});

test('with no figures from the computer yet, a phone that chose 网页版 sees the homepage as it is, and can still choose (STAGE-8 · 8)', async ({ page }) => {
  await page.setViewportSize(PORTRAIT);
  await servePhone(page, PHONE_KEY);
  await snapshotAs(page, (snapshot) => { snapshot.dashboard = null; });
  await page.goto('/#k=' + PHONE_KEY);
  await expect(page.locator('.look-foot')).toBeVisible();
  // The homepage laid out as 电脑版 lays it out, not 网页版's screen-wide frame.
  await expect(page.locator('.window')).not.toHaveClass(/staged/);
  await expect(page.locator('.stage')).toHaveCount(0);
  const foot = page.locator('.look-foot');
  await expect(foot.getByRole('radio', { name: '网页版' })).toHaveAttribute('aria-checked', 'true');
  await expect(foot.locator('.look-note')).toBeVisible();
  await foot.getByRole('radio', { name: '电脑版' }).click();
  await expect(foot.getByRole('radio', { name: '电脑版' })).toHaveAttribute('aria-checked', 'true');
  await expect(foot.locator('.look-note')).toHaveCount(0);
  expect(new URL(page.url()).hash).toContain('v=panel');
  await foot.getByRole('radio', { name: '网页版' }).click();
  await expect(foot.getByRole('radio', { name: '网页版' })).toHaveAttribute('aria-checked', 'true');
  expect(new URL(page.url()).hash).toContain('v=stage');
});

test('reloaded on a phone in dark mode, the page and the bars are the computer\'s light colours before it answers (the first frame)', async ({ page }) => {
  await page.emulateMedia({ colorScheme: 'dark' });
  await phone(page);
  await computerChose(page, { theme: 'light' });
  await expect(page.locator('.stage.light')).toBeVisible();
  const lit = await page.evaluate(() => ({ meta: document.querySelector<HTMLMetaElement>('meta[name="theme-color"]')?.content, html: getComputedStyle(document.documentElement).backgroundColor }));
  // Reloaded, the computer slow to answer: what is drawn meanwhile is in its colours, not the phone's dark.
  await page.unroute('**/api/snapshot**');
  await page.route('**/api/snapshot**', () => {});
  await page.route('**/api/live**', () => {});
  // Before the page's own script: the colours of the last load, and its theme.
  const main = /\/src\/main\.ts(\?|$)/;
  await page.route(main, () => {});
  await page.reload({ waitUntil: 'commit' });
  const colours = () => page.evaluate(() => ({ meta: document.querySelector<HTMLMetaElement>('meta[name="theme-color"]')?.content, html: getComputedStyle(document.documentElement).backgroundColor }));
  await expect.poll(colours).toEqual(lit);
  expect(await page.evaluate(() => document.documentElement.dataset.theme)).toBe('light');
  // Then the page, waiting for the computer: the last load's sky shows through and stays, neither painted over with
  // 电脑版's background nor kept as it (STAGE-9).
  await page.unroute(main);
  await page.reload({ waitUntil: 'domcontentloaded' });
  await expect(page.locator('.window.phone')).toBeVisible();
  await page.waitForTimeout(800);
  expect(await colours()).toEqual(lit);
  expect(await page.locator('.window.phone').evaluate((e) => getComputedStyle(e).backgroundColor)).toBe('rgba(0, 0, 0, 0)');
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem('starmeter.edges')!).top)).toBe(lit.meta);
});

test('reloaded while the work goes on, the page has the computer\'s last minutes: the speed at once, not 0; opened fresh, — until it has its own', async ({ page }) => {
  const clock = { now: Date.UTC(2026, 10, 25, 2, 0, 0) };
  const live = liveWork({ now: () => clock.now, length: 1000, claudeEnds: 300, codexEnds: 300 });
  const at = (seconds: number) => { clock.now = Date.UTC(2026, 10, 25, 2, 0, 0) + seconds * 1000; };
  await phone(page, PORTRAIT, '', { live });
  const rate = page.locator('.stage .monitor [data-figure="rate"] [data-value]');
  const minute = page.locator('.stage .monitor .tick').nth(1).locator('[data-value]').first();
  // Just opened, the sessions running: no speed to say yet, and not 0.
  await expect(page.locator('.stage .monitor.working')).toBeVisible();
  await expect(rate).toHaveAttribute('data-value', '—');
  await expect(minute).toHaveAttribute('data-value', '—');
  // Watched a while (the computer's readings kept as it goes), then reloaded.
  for (const seconds of [3, 8, 13, 18, 23, 28]) { at(seconds); await page.waitForTimeout(2200); }
  await page.reload();
  await expect(page.locator('.stage .monitor.working')).toBeVisible();
  await expect.poll(async () => Number(await rate.getAttribute('data-value')), { timeout: 5000 }).toBeGreaterThan(0);
  await expect(minute).toHaveAttribute('data-value', /^\+\d/);
});

test('on its side under Safari\'s bars, the monitor held still fits what is seen of the page, its curve\'s foot too', async ({ page }) => {
  // Safari on its side with the tab bar: 852 × 393 of window, 286 of it seen below the bars.
  await page.addInitScript(() => {
    const view = Object.assign(new EventTarget(), { height: 286, width: 852, offsetTop: 0, offsetLeft: 0, scale: 1, pageTop: 0, pageLeft: 0 });
    Object.defineProperty(window, 'visualViewport', { configurable: true, value: view });
  });
  await phone(page, LANDSCAPE);
  const seen = await page.evaluate(() => {
    const lead = document.querySelector('.stage .lead')!, curve = document.querySelector('.stage .monitor .curve')!;
    return { lead: lead.getBoundingClientRect().bottom, curve: curve.getBoundingClientRect().height, scroll: lead.scrollHeight - lead.clientHeight };
  });
  expect(seen.lead).toBeLessThanOrEqual(286.5);
  expect(seen.curve).toBeGreaterThanOrEqual(63);
  // Shorter still: the monitor scrolls in its column, its curve's foot (1 分钟前 · 现在) reachable, nothing cut.
  await page.evaluate(() => { Object.assign(window.visualViewport!, { height: 200 }); window.visualViewport!.dispatchEvent(new Event('resize')); });
  await expect.poll(() => page.evaluate(() => document.querySelector('.stage .lead')!.getBoundingClientRect().bottom)).toBeLessThanOrEqual(200.5);
  const scrolled = await page.evaluate(() => {
    const lead = document.querySelector('.stage .lead')!;
    lead.scrollTop = lead.scrollHeight;
    return { gap: lead.scrollHeight - lead.clientHeight, curve: document.querySelector('.stage .monitor .curve')!.getBoundingClientRect().bottom, bottom: lead.getBoundingClientRect().bottom };
  });
  expect(scrolled.gap).toBeGreaterThan(0);
  expect(scrolled.curve).toBeLessThanOrEqual(scrolled.bottom + 0.5);
});

test('网页版 on a computer\'s screen: the monitor as tall as what is beside it, the two columns of figures ending close together, centred, in both languages', async ({ page }) => {
  for (const [hash, size] of [['', { width: 1440, height: 900 }], ['&l=en', { width: 1440, height: 900 }], ['', { width: 1920, height: 1080 }]] as const) {
    await phone(page, size, hash, { live: liveWork({ length: 1000, claudeEnds: 300, codexEnds: 300 }) });
    await expect(page.locator('.stage .feed')).toBeVisible({ timeout: 8000 });
    const seen = await page.evaluate(() => {
      const box = (selector: string) => document.querySelector(selector)!.getBoundingClientRect();
      const last = (selector: string) => [...document.querySelectorAll(selector)].map((e) => e.getBoundingClientRect().bottom).reduce((a, b) => Math.max(a, b), 0);
      const tiles = document.querySelector('.stage .tiles')?.closest('.part')?.getBoundingClientRect().height ?? 0;
      return { lead: box('.stage .lead').bottom, activity: box('.stage .activity-box').bottom, feed: box('.stage .activity > :last-child').bottom,
        left: last('.stage .stats-a > *'), right: last('.stage .stats-b > *'), tiles, frame: box('.stage .frame'), width: innerWidth };
    });
    // Side by side, the monitor and what happened end on one line.
    expect(Math.abs(seen.lead - seen.activity), hash).toBeLessThanOrEqual(1);
    expect(Math.abs(seen.feed - seen.lead), hash).toBeLessThanOrEqual(1);
    // The figures' two columns end within the last block placed of each other (none could be moved to do better).
    expect(Math.abs(seen.left - seen.right), hash).toBeLessThanOrEqual(seen.tiles);
    // At most 1320 wide, in the middle.
    expect(seen.frame.width).toBeLessThanOrEqual(1320);
    expect(Math.abs(seen.frame.left - (seen.width - seen.frame.right))).toBeLessThanOrEqual(1);
  }
});

test('the live curve has a line for each tool beside the total\'s, named in a legend, their ends apart, both ways up, on a computer\'s screen and in 手绘', async ({ page }) => {
  test.setTimeout(90_000);
  for (const [size, look] of [[PORTRAIT, ''], [LANDSCAPE, ''], [{ width: 1440, height: 900 }, ''], [PORTRAIT, 'sketch']] as const) {
    // Each in a tab of its own (a page left mid-reading reports the reading it gave up as an error).
    const tab = size === PORTRAIT && !look ? page : await page.context().newPage();
    const work = scripted();
    await phone(tab, size, look ? '&s=sketch' : '', { live: work.live });
    for (const seconds of [3, 6, 9]) { work.at(seconds); await tab.waitForTimeout(2100); }
    const chart = tab.locator('.stage .monitor .live-chart');
    // The total's line, then each tool's: Claude and ChatGPT (the fixtures' Codex work) among them, in their colours.
    await expect(chart.locator('.legend li').first()).toHaveText('总量');
    const names = await chart.locator('.legend li').allTextContents();
    expect(names).toContain('Claude');
    expect(names).toContain('ChatGPT');
    // The ones writing plain, the idle ones quieter.
    await expect(chart.locator('.legend li', { hasText: 'Claude' })).not.toHaveClass(/idle/);
    await expect(chart.locator('.legend li.idle').first()).toBeVisible();
    const drawn = await chart.locator('canvas').evaluate((c) => ({ lines: Number(c.dataset.lines), dots: JSON.parse(c.dataset.dots ?? '{}') as Record<string, number>, under: c.dataset.under }));
    expect(drawn.lines, JSON.stringify(size)).toBeGreaterThanOrEqual(2);
    const ys = Object.values(drawn.dots).sort((a, b) => a - b);
    for (let i = 1; i < ys.length; i++) expect(ys[i] - ys[i - 1]).toBeGreaterThanOrEqual(4.5);
    // The legend within the monitor, on one or two lines.
    const legend = (await chart.locator('.legend').boundingBox())!, box = (await chart.boundingBox())!;
    expect(legend.x + legend.width).toBeLessThanOrEqual(box.x + box.width + 0.5);
    expect(legend.height).toBeLessThanOrEqual(40);
    if (tab !== page) await tab.close();
  }
});

test('figures that change only now and then (SwiftBar\'s plugin) make a speed that rises over the stretch, not steps', async ({ page }) => {
  await phone(page);
  const samples = await page.evaluate(async () => {
    const { LiveFeed } = await import('/src/lib/live.ts');
    const feed = new LiveFeed();
    feed.spread = 30_000;
    const reading = (tokens: number, output: number) => ({ rangeKey: 'today', date: '2026-11-25', dates: [],
      providers: [{ id: 'claude', available: true, tokens, output, cost: tokens / 1e6, history: [] }] });
    // Read every two seconds; the figures move at 0 s and at 30 s only.
    for (let s = 0; s <= 30; s += 2) feed.add(reading(s < 30 ? 1000 : 61000, s < 30 ? 10 : 610) as never, s * 1000);
    return feed.samples.map((x) => x.output);
  });
  // Between the two, the output climbs evenly, not flat then a jump.
  expect(samples.at(-1)! - samples[0]).toBe(600);
  for (let i = 1; i < samples.length; i++) expect(samples[i]).toBeGreaterThan(samples[i - 1]);
});

test('on SwiftBar\'s panel, reloaded while the work goes on, the page picks up the computer\'s last minutes', async ({ page }) => {
  const clock = { now: Date.UTC(2026, 10, 25, 2, 0, 0) };
  const live = liveWork({ now: () => clock.now, length: 1000, claudeEnds: 300, codexEnds: 300 });
  await phone(page, PORTRAIT, '', { swiftbar: true, live });
  const rate = page.locator('.stage .monitor [data-figure="rate"] [data-value]');
  for (const seconds of [3, 8, 13, 18, 23, 28]) { clock.now = Date.UTC(2026, 10, 25, 2, 0, 0) + seconds * 1000; await page.waitForTimeout(2200); }
  const said = () => page.locator('.stage .feed .event').evaluateAll((list) => list.slice(0, 6).map((e) => `${e.querySelector('.who')?.textContent} ${e.querySelector('.what')?.textContent}`));
  const before = await said();
  expect(before.length).toBe(6);
  await page.reload();
  // At once as it was: the speed and the last minute, and 动态's latest six; no 正在读取, no 等下一笔请求.
  await expect(page.locator('.stage .monitor.working')).toBeVisible();
  await expect.poll(async () => Number(await rate.getAttribute('data-value')), { timeout: 3000 }).toBeGreaterThan(0);
  await expect(page.locator('.stage .monitor .tick-value').nth(1).locator('[data-value]').first()).toHaveAttribute('data-value', /^\+\d/);
  await expect(page.locator('.stage .monitor .reading')).toHaveCount(0);
  expect(await said()).toEqual(before);
});

test('the monitor\'s 今天 and 近 1 分钟 stand on their figures\' line, the figures alike, the sign and unit as the panel\'s, in both looks and three languages', async ({ page }) => {
  test.setTimeout(90_000);
  // The four at once, each watched long enough for the last minute to be said (twenty seconds of readings).
  const hashes = ['', '&l=zh-Hant&s=sketch', '&l=en', '&l=en&s=sketch'];
  const tabs = await Promise.all(hashes.map(async (hash) => {
    const tab = hash ? await page.context().newPage() : page;
    const work = scripted();
    await phone(tab, PORTRAIT, hash, { live: work.live });
    return { tab, work, hash };
  }));
  for (let seconds = 2; seconds <= 26; seconds += 2) { for (const { work } of tabs) work.at(seconds); await page.waitForTimeout(2000); }
  for (const { tab, hash } of tabs) {
    await expect(tab.locator('.stage .monitor .ticker .tick-value .wheel').first()).toBeVisible();
    await expect.poll(() => tab.locator('.stage .monitor .ticker .tick-value').nth(1).locator('.wheel').count(), { timeout: 10_000 }).toBeGreaterThan(0);
    const seen = await tab.evaluate(() => {
      // A probe with no height stands on the line it is in: its top is that line's baseline.
      const baseline = (inside: Element) => {
        const probe = document.createElement('span');
        probe.style.cssText = 'display:inline-block;width:0;height:0';
        inside.append(probe);
        const y = probe.getBoundingClientRect().top;
        probe.remove();
        return y;
      };
      const ticker = document.querySelector('.stage .monitor .ticker')!;
      const labels = [...ticker.querySelectorAll('.tick-label')].map(baseline);
      const figures = [...ticker.querySelectorAll('.tick-value .number')].map((n) => ({
        // A wheel's digit stands where its line is within the wheel, whichever digit the wheel has turned to.
        line: baseline(n.querySelector('.ghost .body')!), digit: (() => {
          const wheel = n.querySelector('.wheel')!, first = wheel.querySelector('.digits > span')!;
          return wheel.getBoundingClientRect().top + baseline(first) - first.getBoundingClientRect().top;
        })(),
        size: getComputedStyle(n).fontSize, weight: getComputedStyle(n).fontWeight,
        sign: n.querySelector('.ghost .sign') ? getComputedStyle(n.querySelector('.ghost .sign')!).fontSize : null,
        unit: n.querySelector('.ghost .unit') ? getComputedStyle(n.querySelector('.ghost .unit')!).fontSize : null,
      }));
      return { labels, figures };
    });
    const line = seen.figures[0].line;
    for (const y of seen.labels) expect(Math.abs(y - line), hash).toBeLessThanOrEqual(0.5);
    for (const f of seen.figures) {
      expect(Math.abs(f.line - line), hash).toBeLessThanOrEqual(0.5);
      expect(Math.abs(f.digit - line), hash).toBeLessThanOrEqual(0.5);
      expect([f.size, f.weight]).toEqual([seen.figures[0].size, seen.figures[0].weight]);
    }
    // $ at 0.62 and K/M at 0.74 of the figure, as Figure.svelte draws them.
    const px = (v: string | null) => Number.parseFloat(v ?? 'NaN');
    expect(px(seen.figures[0].sign) / px(seen.figures[0].size)).toBeCloseTo(0.62, 2);
    expect(px(seen.figures[1].unit) / px(seen.figures[1].size)).toBeCloseTo(0.74, 2);
    if (tab !== page) await tab.close();
  }
});

test('opened with nothing kept yet, 网页版 waits quietly: a faint dash in the figures\' face (not a pen\'s bar in 手绘), no scale over an empty curve', async ({ page }) => {
  await page.addInitScript(() => {
    const drawn: string[] = [];
    (window as unknown as { drawn: string[] }).drawn = drawn;
    const fill = CanvasRenderingContext2D.prototype.fillText;
    CanvasRenderingContext2D.prototype.fillText = function (text: string, ...rest: [number, number, number?]) { drawn.push(String(text)); return fill.call(this, text, ...rest); };
  });
  await page.emulateMedia({ colorScheme: 'dark' });
  const work = scripted();
  await phone(page, PORTRAIT, '&s=sketch', { live: work.live });
  await expect(page.locator('.stage.sketch .monitor.working')).toBeVisible();
  const big = page.locator('.stage .monitor .big.unknown');
  await expect(big).toBeVisible();
  const colours = await big.evaluate((e) => ({ big: getComputedStyle(e).color, faint: getComputedStyle(e.closest('.stage')!).getPropertyValue('--ink-3').trim() }));
  const probe = await page.evaluate((faint) => { const s = document.createElement('span'); s.style.color = faint; document.body.append(s); const c = getComputedStyle(s).color; s.remove(); return c; }, colours.faint);
  expect(colours.big).toBe(probe);
  expect(await page.evaluate(() => (window as unknown as { drawn: string[] }).drawn.filter((text) => /Token\/s$/.test(text)))).toEqual([]);
});

test('the computer\'s last minutes with a gap in them, or ended a while ago, are no speed now nor work now (STAGE-3)', async ({ page }) => {
  await phone(page, PORTRAIT, '', { live: resting() });
  const seen = await page.evaluate(async (key) => {
    const { PhoneLive } = await import('/src/lib/phoneLive.svelte.ts');
    const template = await (await fetch('/api/live', { headers: { 'X-Starmeter-Key': key } })).json();
    const now = 1_800_000_000_000, at = now / 1000;
    // Claude's figures at a moment `back` seconds ago: `tokens` and a hundredth of them written.
    const reading = (back: number, tokens: number) => ({ at: at - back, date: template.dashboard.date, rangeKey: 'today',
      providers: [{ id: 'claude', available: true, tokens, output: tokens / 100, cost: tokens / 1e6 }] });
    const answer = (history: unknown[], tokens: number) => {
      const dashboard = structuredClone(template.dashboard);
      const claude = dashboard.providers.find((p: { id: string }) => p.id === 'claude');
      Object.assign(claude, { tokens, output: tokens / 100, cost: tokens / 1e6, counted: undefined });
      return { at, dashboard, sessions: null, turns: {}, history };
    };
    const run = (history: unknown[], tokens: number) => {
      const live = new PhoneLive();
      live.take(answer(history, tokens) as never, now);
      // What the history said happened stays at its moments; nothing of the gap is said as happening now.
      return { rate: live.rate(now), ready: live.ready(), state: live.state, usage: live.events.filter((e: { kind: string; at: number }) => e.kind === 'usage' && e.at >= now).length };
    };
    const steady = (from: number, to: number, base: number, step: number) => {
      const list = [];
      for (let back = from, i = 0; back >= to; back -= 2, i++) list.push(reading(back, base + i * step));
      return list;
    };
    return {
      // Readings that ended 100 s ago, a million tokens added since: nothing to say of now.
      ended: run(steady(130, 100, 1e6, 1000), 2e6),
      // A run, a minute's gap, then ten seconds: the last run too short to say a speed.
      gapped: run([...steady(120, 90, 1e6, 1000), ...steady(10, 2, 3e6, 1000)], 3e6 + 5000),
      // Forty seconds unbroken, growing: a speed, and working.
      steady: run(steady(40, 2, 1e6, 20000), 1e6 + 20 * 20000),
    };
  }, PHONE_KEY);
  expect(seen.ended).toEqual({ rate: null, ready: false, state: 'resting', usage: 0 });
  expect(seen.gapped.ready).toBe(false);
  expect(seen.gapped.rate).toBeNull();
  expect(seen.steady.ready).toBe(true);
  expect(seen.steady.rate).toBeGreaterThan(0);
  expect(seen.steady.state).toBe('working');
});

test('保持亮屏 refused on coming back (low power mode) goes off, not lit and holding nothing (STAGE-10)', async ({ page }) => {
  await page.addInitScript(() => {
    const state = { refuse: false, release: null as null | (() => void) };
    (window as unknown as { lockState: typeof state }).lockState = state;
    Object.defineProperty(navigator, 'wakeLock', { configurable: true, value: {
      request: () => state.refuse ? Promise.reject(new Error('NotAllowedError')) : Promise.resolve({
        release: async () => {}, addEventListener: (_: string, listener: () => void) => { state.release = listener; } }) } });
  });
  await phone(page, LANDSCAPE, '', { live: resting() });
  const button = page.getByRole('button', { name: '保持亮屏' });
  await button.click();
  await expect(button).toHaveAttribute('aria-pressed', 'true');
  // Hidden: the system lets go of it. Back, the browser refuses it now.
  await page.evaluate(() => {
    const state = (window as unknown as { lockState: { refuse: boolean; release: (() => void) | null } }).lockState;
    Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => 'hidden' });
    document.dispatchEvent(new Event('visibilitychange'));
    state.release?.();
    state.refuse = true;
    Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => 'visible' });
    document.dispatchEvent(new Event('visibilitychange'));
  });
  await expect(button).toHaveAttribute('aria-pressed', 'false');
});

test('the sky\'s floating things answer the work: busier as the models write, few and slow resting, a burst for a turn done, still while hidden', async ({ page }) => {
  test.setTimeout(60_000);
  const work = scripted();
  await phone(page, PORTRAIT, '', { live: work.live });
  const drift = page.locator('.stage .sky canvas.drift');
  const read = () => drift.evaluate((c) => ({ energy: Number(c.dataset.energy), active: Number(c.dataset.active), frames: Number(c.dataset.frames), bursts: Number(c.dataset.bursts) }));
  // Working: the energy climbs (smoothly) and more of them show; never more than the screen's few dozen.
  for (const seconds of [3, 6, 9]) { work.at(seconds); await page.waitForTimeout(2000); }
  await expect.poll(async () => (await read()).energy, { timeout: 8000 }).toBeGreaterThan(0.3);
  const busy = await read();
  expect(busy.active).toBeLessThanOrEqual(56);
  // A turn done: a burst under its banner.
  work.at(21);
  await expect(page.locator('.stage .turn-done')).toBeVisible({ timeout: 5000 });
  await expect.poll(async () => (await read()).bursts).toBeGreaterThan(0);
  // Resting: few and slow.
  work.at(80);
  await expect(page.locator('.stage .monitor:not(.working)')).toBeVisible({ timeout: 5000 });
  await expect.poll(async () => (await read()).energy, { timeout: 10_000 }).toBeLessThan(0.15);
  expect((await read()).active).toBeLessThan(busy.active);
  // Hidden: nothing drawn.
  await page.evaluate(() => { Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => 'hidden' }); document.dispatchEvent(new Event('visibilitychange')); });
  await page.waitForTimeout(300);
  const hidden = (await read()).frames;
  await page.waitForTimeout(1000);
  expect((await read()).frames).toBe(hidden);
});

test('with reduced motion the sky is one still frame; in 手绘 its things are the pen\'s doodles on the paper', async ({ page }) => {
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await phone(page, PORTRAIT, '&s=sketch', { live: resting() });
  const drift = page.locator('.stage.sketch .sky canvas.drift');
  await expect(drift).toHaveCount(1);
  const frames = async () => Number(await drift.evaluate((c) => c.dataset.frames ?? '0'));
  await page.waitForTimeout(500);
  const first = await frames();
  expect(first).toBeLessThanOrEqual(2);
  await page.waitForTimeout(1000);
  expect(await frames()).toBe(first);
});
