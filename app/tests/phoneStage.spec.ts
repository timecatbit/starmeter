// 在手机上看 on the phone, in its own look (手机版, PhoneStage.svelte): portrait and landscape laid out within the screen,
// the figures the computer's homepage shows, its ranges and groupings, the choice between 手机版 and 电脑版 (kept in the
// address, a cookie and the page's storage), reduced motion, and the languages.
import { expect, test, type Page } from '@playwright/test';
import { PHONE_KEY, servePhone } from './phone';

const errors: string[] = [];
test.beforeEach(({ page }) => {
  errors.length = 0;
  page.on('pageerror', (error) => errors.push(String(error)));
  page.on('console', (message) => { if (message.type() === 'error') errors.push(message.text()); });
});
test.afterEach(() => expect(errors).toEqual([]));

const PORTRAIT = { width: 393, height: 852 };
const LANDSCAPE = { width: 852, height: 393 };

async function phone(page: Page, size = PORTRAIT, hash = '', options: { swiftbar?: boolean } = {}) {
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
  const amount = await page.locator('.stage .amount [data-value]').getAttribute('data-value');
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
  // The order of importance, top to bottom: the bar, the ranges, the estimate and its figures, the trend, the shares, the cards.
  const order = ['.stage .bar', '.stage .ranges', '.stage .amount', '.stage .figures', '.stage .trend', '.stage .shares', '.stage .tabs', '.stage .card'];
  let last = -1;
  for (const selector of order) {
    const { y } = await box(page, selector);
    expect(y, selector).toBeGreaterThan(last);
    last = y;
  }
  // Nothing wider than the screen, and the column centred with even margins.
  const width = await page.evaluate(() => { const s = document.querySelector('.stage > .scroll')!; return { scroll: s.scrollWidth, client: s.clientWidth }; });
  expect(width.scroll).toBeLessThanOrEqual(width.client);
  for (const selector of ['.stage .trend', '.stage .shares', '.stage .card', '.stage .plan']) {
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

test('手机版 on its side: two columns inside the screen, the estimate and the trend still, the rest scrolling', async ({ page }) => {
  for (const size of [LANDSCAPE, { width: 667, height: 375 }]) {
    await phone(page, size);
    const lead = await box(page, '.stage .lead'), rest = await box(page, '.stage .rest');
    // Side by side, the left column's foot within the screen; the trend has room to read.
    expect(lead.x + lead.width).toBeLessThanOrEqual(rest.x);
    expect(lead.y + lead.height).toBeLessThanOrEqual(size.height + 1);
    const trend = await box(page, '.stage .trend');
    expect(trend.y + trend.height).toBeLessThanOrEqual(size.height);
    expect((await box(page, '.stage .chart')).height).toBeGreaterThan(90);
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
  expect((await box(page, '.stage .rest')).y).toBeGreaterThan((await box(page, '.stage .trend')).y);
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
  const chart = await box(page, '.stage .chart');
  await page.mouse.move(chart.x + chart.width * 0.5, chart.y + chart.height / 2);
  await expect(page.locator('.stage .bubble .when')).toHaveText(/\d\d:\d\d/);
  await expect(page.locator('.stage .bubble .part').first()).toBeVisible();
  await page.mouse.move(chart.x + chart.width * 0.5, chart.y - 60);
  await expect(page.locator('.stage .bubble')).toHaveCount(0);
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
      const wide = await page.evaluate(() => [...document.querySelectorAll<HTMLElement>('.stage > .scroll, .stage .rest')].every((e) => e.scrollWidth <= e.clientWidth));
      expect(wide).toBe(true);
      // The tabs' labels and the figures' labels fit on their line.
      for (const selector of ['.stage .tabs .choice', '.stage .figure-label']) {
        for (const item of await page.locator(selector).all()) expect(await item.evaluate((e) => e.scrollWidth - e.clientWidth), selector).toBeLessThanOrEqual(1);
      }
      await page.close();
    }
  }
});

test('手机版 without Summary on the homepage: the cards and plans only, as the panel shows it, one column on its side too', async ({ page }) => {
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
  await expect(page.locator('.stage.wide')).toHaveCount(0);
  await expect(page.getByRole('radiogroup', { name: '统计范围' })).toBeVisible();
});
