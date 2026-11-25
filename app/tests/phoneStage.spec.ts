// 在手机上看 on the phone, in its own look (手机版, PhoneStage.svelte): the live monitor (working, a turn done, resting,
// waking, asked for only while shown), portrait, landscape and a computer's window laid out within the screen, the
// figures the computer's homepage shows, its ranges and groupings, the choice between 手机版 and 电脑版 (kept in the
// address, a cookie and the page's storage), reduced motion, and the languages.
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
/** 手机版's estimate, tokens, requests. */
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

test('手机版 is what a phone opens: portrait, balanced within the screen, its figures the computer\'s', async ({ page }) => {
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

test('手机版 on its side: a monitor by the desk, the live view still on the left, the rest scrolling on the right', async ({ page }) => {
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

test('手机版: the ranges and groupings are the phone\'s own, the figures read for them as the computer reads them', async ({ page }) => {
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

test('手机版: the trend reads under a finger, the share page opens and comes back, a new reading lights the tool that grew', async ({ page }) => {
  await phone(page);
  await page.locator('.stage .trend .chart').scrollIntoViewIfNeeded();
  const chart = await box(page, '.stage .trend .chart');
  await page.mouse.move(chart.x + chart.width * 0.5, chart.y + chart.height / 2);
  await expect(page.locator('.stage .trend .bubble .when')).toHaveText(/\d\d:\d\d/);
  await expect(page.locator('.stage .trend .bubble .part').first()).toBeVisible();
  await page.mouse.move(chart.x + chart.width * 0.5, chart.y - 60);
  await expect(page.locator('.stage .trend .bubble')).toHaveCount(0);
  // The share page, as on the panel's phone page, and back to 手机版 where it was.
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

test('手机版 and 电脑版: picked on the phone, at either page\'s foot, and kept through a reload and a new address', async ({ page }) => {
  await phone(page);
  await expect(page.getByRole('radiogroup', { name: '手机上的样式' }).getByRole('radio', { name: '手机版' })).toHaveAttribute('aria-checked', 'true');
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
  // Back at 电脑版's foot: 手机版, its opening again.
  const back = page.locator('.look-foot').getByRole('radio', { name: '手机版' });
  await back.scrollIntoViewIfNeeded();
  await back.click();
  await expect(page.locator('.stage')).toBeVisible();
  expect(new URL(page.url()).hash).toContain('v=stage');
  expect(await page.evaluate(() => document.cookie)).toContain('starmeter-phone-look=stage');
});

test('手机版 with reduced motion: everything simply there, nothing drifting or counting', async ({ page }) => {
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

test('手机版 for SwiftBar\'s panel: the computer\'s range and grouping only, with 会话', async ({ page }) => {
  await phone(page, PORTRAIT, '', { swiftbar: true });
  await expect(page.getByRole('radiogroup', { name: '统计范围' })).toHaveCount(0);
  await expect(page.locator('.stage .hero .range')).toHaveText('今天');
  await expect(page.getByRole('radiogroup', { name: '卡片分组' }).getByRole('radio')).toHaveText(['工具', '会话']);
});

test('手机版 in English and in 繁體中文: laid out within the screen, both ways up', async ({ context }) => {
  for (const [hash, words] of [['&l=en', ['Share by tool', 'Plans and services', 'Phone', 'Desktop', 'Hourly tokens']], ['&l=zh-Hant', ['工具佔比', '訂閱與服務', '手機版', '電腦版']]] as const) {
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

test('手机版 without Summary on the homepage: the figures for a range are the cards and plans only, as the panel shows it', async ({ page }) => {
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
  await expect(page.locator('.stage .monitor .unit')).toHaveText('输出 Token/s');
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

test('手机版 in a computer\'s window: the monitor large beside what is happening, the figures under both, a page that scrolls', async ({ page }) => {
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

test('手机版 follows the computer\'s look: its theme, its background as the panes\' material, 标准 or 手绘', async ({ page }) => {
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

test('手机版\'s figures and words keep the panel\'s rules: figures rounded at 600, words in the panel\'s own face', async ({ page }) => {
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

test('on a phone the browser\'s bars take the page\'s colours: 手机版\'s sky edges, 电脑版\'s background, as the theme changes', async ({ page }) => {
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
  // (Nothing seen written in the last minute: the speed eases to nothing, never what the locked minutes added over 60 s.)
  expect(Number(await rate.getAttribute('data-value'))).toBeLessThanOrEqual(Number(before));
  expect(await page.locator('.stage .feed .event.usage').count()).toBeLessThanOrEqual(events);
  // Locked for three minutes while both turns ended: resting, no turn said done now with the locked minutes as its time.
  await away(page, work, 180, 200);
  await expect(page.locator('.stage .monitor .state')).toHaveText(/休息中/, { timeout: 5000 });
  await page.waitForTimeout(2500);
  await expect(page.locator('.stage .turn-done')).toHaveCount(0);
  await expect(page.locator('.stage .feed .event.done')).toHaveCount(0);
});
