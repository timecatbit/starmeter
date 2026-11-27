// 实时, the homepage's fifth tab (PanelLive.svelte): what the AI is doing now, 正在运行 and 动态, from the app's live_now
// as 网页版 has it. Pinning turns to it and unpinning turns back (unless another was picked meanwhile); the five tabs
// on one line in every language and look; the keyboard walks all five; unpinned it is never blank.
import { expect, test, type Page } from '@playwright/test';
import { toHant } from '../ui/src/lib/hant';
import { cutWords } from './textFits';

const errors: string[] = [];
test.beforeEach(({ page }) => {
  errors.length = 0;
  page.on('pageerror', (error) => errors.push(String(error)));
});
test.afterEach(() => expect(errors).toEqual([]));

async function open(page: Page, init: { lang?: string; style?: string } = {}) {
  await page.addInitScript(({ lang, style }) => {
    if (lang) localStorage.setItem('starmeter.language', lang);
    if (style) localStorage.setItem('starmeter.style', style);
  }, init);
  await page.goto('/');
  await expect(page.locator('.dashboard')).toBeVisible();
  await page.waitForTimeout(300);
}
const radio = (page: Page, name: string) => page.getByRole('radiogroup', { name: /卡片分组|Card grouping|卡片分組/ }).getByRole('radio', { name, exact: true });

test('pinned, the homepage turns to 实时: the sessions at work and what happened; unpinned, back to the tab it showed', async ({ page }) => {
  await open(page);
  await expect(radio(page, '工具')).toHaveAttribute('aria-checked', 'true');
  await page.getByRole('button', { name: '钉住面板', exact: true }).click();
  await expect(radio(page, '实时')).toHaveAttribute('aria-checked', 'true');
  const live = page.locator('.panel-live');
  await expect(live.locator('.live-session')).toHaveCount(3);
  await expect(live.locator('.live-session .title').first()).toHaveText('重做设置页的分段控件');
  await expect(live.locator('.live-session .clock').first()).toHaveText(/^\d+:\d\d$/);
  await expect(live.locator('.live-session .spent').first()).toHaveText(/^本轮 .+ Token · \$/);
  // Each pinned tick adds to today: what happened comes in at the top.
  await expect.poll(() => live.locator('.event').count(), { timeout: 10_000 }).toBeGreaterThan(1);
  await expect(live.locator('.event .what').first()).toHaveText(/^\+[\d.]+K Token/);
  // The strip of the fourth round is gone: 实时 is where it is.
  await expect(page.locator('.pinned-now')).toHaveCount(0);
  await page.getByRole('button', { name: '取消钉住面板', exact: true }).click();
  await expect(radio(page, '工具')).toHaveAttribute('aria-checked', 'true');
});

test('the five tabs spread across the row, the first at its left and the last at its right, the same room between, in every language and look, pinned and at any width', async ({ page }) => {
  const spread = async (tab: Page, what: string) => {
    const seen = await tab.evaluate(() => {
      const row = document.querySelector('.tabs')!.getBoundingClientRect();
      return { row: { left: row.left, right: row.right }, tabs: [...document.querySelectorAll('.tabs .tab')].map((b) => {
        const r = b.getBoundingClientRect(); return { top: r.top, left: r.left, right: r.right, wraps: b.scrollWidth > b.clientWidth + 1 }; }) };
    });
    expect(seen.tabs.length, what).toBe(5);
    for (const t of seen.tabs) { expect(t.top, what).toBe(seen.tabs[0].top); expect(t.wraps, what).toBe(false); }
    expect(seen.tabs[0].left - seen.row.left, what).toBeCloseTo(8, 0);
    expect(seen.row.right - seen.tabs[4].right, what).toBeCloseTo(8, 0);
    const gaps = seen.tabs.slice(1).map((t, i) => t.left - seen.tabs[i].right);
    for (const g of gaps) expect(Math.abs(g - gaps[0]), what).toBeLessThan(0.5);
    expect(gaps[0], what).toBeGreaterThan(2);
    // The pill under the chosen tab, as wide as it.
    const pill = await tab.locator('.tabs .pill').boundingBox(), chosen = await tab.locator('.tabs .tab.chosen').boundingBox();
    expect(Math.abs(pill!.x - chosen!.x), what).toBeLessThan(0.5);
    expect(Math.abs(pill!.width - chosen!.width), what).toBeLessThan(0.5);
  };
  for (const init of [{}, { lang: 'en' }, { lang: 'zh-Hant' }, { style: 'sketch' }, { lang: 'en', style: 'sketch' }]) {
    const tab = Object.keys(init).length ? await page.context().newPage() : page;
    await open(tab, init);
    await spread(tab, JSON.stringify(init));
    if (tab !== page) await tab.close();
  }
  // Pinned (实时 chosen, the pill slid to it), then made wider at the corner: still spread, end to end.
  await page.getByRole('button', { name: '钉住面板', exact: true }).click();
  await expect(radio(page, '实时')).toHaveAttribute('aria-checked', 'true');
  await page.waitForTimeout(400);
  await spread(page, 'pinned');
  await page.evaluate(() => window.dispatchEvent(new CustomEvent('panel-resized', { detail: { width: 440, height: 700 } })));
  await expect(page.locator('.window')).toHaveCSS('width', '440px');
  await page.waitForTimeout(400);
  await spread(page, 'pinned at 440');
});

test('the arrows walk all five tabs, 实时 among them, and Tab leaves the group', async ({ page }) => {
  await open(page);
  await radio(page, '会话').click();
  await radio(page, '会话').focus();
  await page.keyboard.press('ArrowRight');
  await expect(radio(page, '实时')).toHaveAttribute('aria-checked', 'true');
  await expect(radio(page, '实时')).toBeFocused();
  await expect(page.locator('.panel-live')).toBeVisible();
  await page.keyboard.press('ArrowRight');
  await expect(radio(page, '工具')).toBeFocused();
  await page.keyboard.press('End');
  await expect(radio(page, '实时')).toBeFocused();
  await expect(radio(page, '工具')).toHaveAttribute('tabindex', '-1');
  await page.keyboard.press('Tab');
  await expect(page.getByRole('radiogroup', { name: '卡片分组' }).locator(':focus')).toHaveCount(0);
});

test('unpinned, 实时 is never blank: the sessions running, and what happened or a line saying none yet', async ({ page }) => {
  await open(page);
  await radio(page, '实时').click();
  const live = page.locator('.panel-live');
  await expect(live.locator('.head').first()).toHaveText(/正在运行/);
  await expect(live.locator('.live-session')).toHaveCount(3);
  await expect(live.locator('.quiet')).toHaveText('等下一笔请求…');
  // Not kept for the next opening: the panel opens on the homepage's 工具.
  await page.reload();
  await expect(radio(page, '工具')).toHaveAttribute('aria-checked', 'true');
});

test('pinned, 实时 cuts no words: titles far too long for a line, an hour and more into their turns, in every language and look, narrow and wide', async ({ page }) => {
  test.setTimeout(180_000);
  const long = { zh: '重新设计设置页里的分段控件，补齐键盘导航、读屏标签以及深浅两种主题下的对比度检查', en: 'Rework the settings page segmented control with keyboard navigation, screen reader labels and contrast checks' };
  const found: string[] = [];
  for (const lang of ['zh', 'en', 'zh-Hant']) for (const style of ['standard', 'sketch']) for (const width of [360, 480]) {
    const tab = await page.context().newPage();
    tab.on('pageerror', (error) => errors.push(String(error)));
    // Two sessions with long titles (one in Chinese, one in English), every running turn 83 minutes in.
    await tab.route('**/fixtures/default.json*', async (route) => {
      const response = await route.fetch();
      const text = (await response.text()).replaceAll('重做设置页的分段控件', long.zh).replaceAll('修订论文的评估章节', long.en)
        .replace(/运行中( \d+ 分钟)?/g, '运行中 83 分钟');
      await route.fulfill({ response, body: text });
    });
    await open(tab, { lang, style });
    await tab.getByRole('button', { name: /^(钉住面板|Pin the panel|釘住面板)$/ }).click();
    await expect(tab.locator('.panel-live .live-session')).toHaveCount(3);
    await tab.evaluate((w) => window.dispatchEvent(new CustomEvent('panel-resized', { detail: { width: w, height: 760 } })), width);
    await tab.waitForTimeout(600);
    const where = `${lang} ${style} ${width}`;
    await expect(tab.locator('.panel-live .live-session .title').first(), where).toHaveText(lang === 'zh-Hant' ? toHant(long.zh) : long.zh);
    await expect(tab.locator('.panel-live .live-session .clock').first(), where).toHaveText(/^1:23:\d\d$/);
    // Each clock whole, right of its title (cut short before it), never under it.
    const clocks = await tab.locator('.panel-live .live-session').evaluateAll((rows) => rows.map((row) => {
      const box = row.getBoundingClientRect(), title = row.querySelector('.title')!.getBoundingClientRect(), clock = row.querySelector('.clock')!;
      const time = clock.getBoundingClientRect();
      return { inside: time.left >= box.left && time.right <= box.right + 0.5 && time.width > 0, apart: time.left >= title.right - 0.5, whole: clock.scrollWidth <= clock.clientWidth + 0.5 };
    }));
    expect(clocks, where).toEqual(clocks.map(() => ({ inside: true, apart: true, whole: true })));
    found.push(...(await cutWords(tab, '.panel-live')).map((line) => `${where} · ${line}`));
    await tab.close();
  }
  expect(found).toEqual([]);
});

/** The last minutes the app would have kept by now (fixtureBackend's seedRecent): today's figures, each tool's output
 *  `rise(id, secondsAgo)` lower the further back, a reading every 2 seconds over `span` seconds (none in `gap`). */
async function seed(page: Page, span: number, rise: { id: string; tokens: number; output: number }, gap: [number, number] | null = null, jump = 0) {
  await page.evaluate(async ({ span, rise, gap, jump }) => {
    const f = (window as unknown as { starmeterFixtures: { today: () => Promise<import('../ui/src/lib/types').Dashboard>; seedRecent: (r: unknown[]) => void } }).starmeterFixtures;
    const base = await f.today();
    const now = Date.now() / 1000, readings = [];
    for (let ago = span; ago >= 2; ago -= 2) {
      if (gap && ago < gap[0] && ago > gap[1]) continue;
      // Before the gap the work stood `jump` lower too: what the gap added, at moments unknown.
      const back = ago / 2 + (gap && ago >= gap[0] ? jump : 0);
      readings.push({ at: now - ago, date: base.date, rangeKey: base.rangeKey,
        providers: base.providers.map((p) => ({ id: p.id, available: p.available, summaryIncluded: p.summaryIncluded, cost: p.cost,
          tokens: p.tokens - (p.id === rise.id ? back * rise.tokens : 0), output: p.output - (p.id === rise.id ? back * rise.output : 0) })) });
    }
    f.seedRecent(readings);
  }, { span, rise, gap, jump });
}
const asked = (page: Page) => page.evaluate(() => (window as unknown as { starmeterFixtures: { liveAsked: () => number } }).starmeterFixtures.liveAsked());

test('实时 opens with what happened in the last minutes, and reads only while the panel is on screen: closed, it is back on 工具 (STAGE-11)', async ({ page }) => {
  await open(page);
  await seed(page, 60, { id: 'claude', tokens: 5000, output: 50 });
  await radio(page, '实时').click();
  const live = page.locator('.panel-live');
  // At once, before a tick of its own: 动态 from the minutes the app kept while 实时 wasn't open (its newest twelve,
  // none folded away).
  await expect(live.locator('.event')).toHaveCount(12, { timeout: 1500 });
  await expect(live.getByRole('button', { name: /^查看更多/ })).toHaveCount(0);
  await expect(live.locator('.event .what').first()).toHaveText(/^\+5(\.0)?K Token/);
  // Closed: back on the homepage's 工具 while hidden (the next opening shows it at once, with no jump from 实时), 实时
  // gone with it and nothing more asked.
  await page.evaluate(() => window.dispatchEvent(new CustomEvent('panel-closed')));
  await expect(radio(page, '工具')).toHaveAttribute('aria-checked', 'true');
  await expect(live).toHaveCount(0);
  const closed = await asked(page);
  await page.waitForTimeout(4500);
  expect(await asked(page)).toBe(closed);
  // Open again: on 工具 (nothing of 实时 read); 实时 picked, asked at once, the clock going.
  await page.evaluate(() => window.dispatchEvent(new CustomEvent('panel-opened', { detail: { modules: false, maxHeight: 900 } })));
  await expect(radio(page, '工具')).toHaveAttribute('aria-checked', 'true');
  await page.waitForTimeout(500);
  expect(await asked(page)).toBe(closed);
  await radio(page, '实时').click();
  await expect.poll(() => asked(page), { timeout: 1500 }).toBeGreaterThan(closed);
  const clock = live.locator('.live-session .clock').first();
  await expect(clock).toBeVisible();
  const shown = await clock.textContent();
  await expect(clock).not.toHaveText(shown!, { timeout: 2500 });
});

test('实时\'s 动态 shows its newest twelve, none folded away; pinned, the panel fits down to the third, the rest a scroll away', async ({ page }) => {
  // 用户：你默认显示12个动作就很好，我只是想你pin住的时候整个画面高度，显示三个就好。实时这里不用折叠。
  await open(page);
  await seed(page, 60, { id: 'claude', tokens: 5000, output: 50 });
  // Pinned: 实时, and a reading every couple of seconds, each adding to 动态.
  await page.getByRole('button', { name: '钉住面板', exact: true }).click();
  await expect(radio(page, '实时')).toHaveAttribute('aria-checked', 'true');
  const live = page.locator('.panel-live');
  await expect(live.locator('.event')).toHaveCount(12, { timeout: 2000 });
  await expect(live.getByRole('button', { name: /^查看更多|^收起/ })).toHaveCount(0);
  // The pinned panel, fitted (as it is when pinned), shows down to the third happening's foot: the third whole, the
  // fourth below what is seen.
  // (Measured once it stands still: a reading every couple of seconds slides a new one in at the top.)
  const edges = () => page.evaluate(() => {
    const seen = document.querySelector('.view .scroller')!.getBoundingClientRect(), rows = document.querySelectorAll('.panel-live .event');
    return { seen: Math.round(seen.bottom), third: Math.round(rows[2].getBoundingClientRect().bottom), fourth: Math.round(rows[3].getBoundingClientRect().top) };
  });
  await expect(async () => {
    const first = await edges();
    await page.waitForTimeout(400);
    expect(await edges()).toEqual(first);
    expect(first.third).toBeLessThanOrEqual(first.seen + 1);
    expect(first.fourth).toBeGreaterThanOrEqual(first.seen - 1);
  }).toPass({ timeout: 8000 });
  // More come in at each reading: still twelve.
  const top = await live.locator('.event').first().textContent();
  await expect.poll(async () => live.locator('.event').first().textContent(), { timeout: 8000 }).not.toBe(top);
  await expect(live.locator('.event')).toHaveCount(12);
});

test('started hidden (the app at login), nothing is read until the panel is opened and 实时 picked (STAGE-11)', async ({ page }) => {
  // ?hidden: the browser's stand-in for the app's window not yet on screen (info().visible false).
  await page.goto('/?hidden');
  await expect(page.locator('.dashboard')).toBeAttached();
  await page.waitForTimeout(3500);
  expect(await asked(page)).toBe(0);
  // The star clicked: the panel opened on the homepage's 工具; 实时 picked, it reads at once and its clocks go.
  await page.evaluate(() => window.dispatchEvent(new CustomEvent('panel-opened', { detail: { modules: false, maxHeight: 900 } })));
  await expect(radio(page, '工具')).toHaveAttribute('aria-checked', 'true');
  expect(await asked(page)).toBe(0);
  await radio(page, '实时').click();
  await expect.poll(() => asked(page), { timeout: 1500 }).toBeGreaterThan(0);
  const live = page.locator('.panel-live');
  await expect(live.locator('.live-session')).toHaveCount(3);
  const clock = await live.locator('.live-session .clock').first().textContent();
  await expect(live.locator('.live-session .clock').first()).not.toHaveText(clock!, { timeout: 2500 });
});

test('pinned again while the live line still zooms out, it zooms back in and stays live (STAGE-16)', async ({ page }) => {
  await open(page);
  await seed(page, 50, { id: 'claude', tokens: 2000, output: 50 });
  const pin = page.getByRole('button', { name: '钉住面板', exact: true }), unpin = page.getByRole('button', { name: '取消钉住面板', exact: true });
  await pin.click();
  const canvas = page.locator('canvas.live');
  await expect.poll(async () => Number(await canvas.getAttribute('data-samples')), { timeout: 1500 }).toBeGreaterThan(20);
  await page.waitForTimeout(900);
  await unpin.click();
  await page.waitForTimeout(250);
  await pin.click();
  // Well past the 750 ms the zoom out takes: still the live line, its modes back, and readings still coming in.
  await page.waitForTimeout(1500);
  await expect(canvas).toBeVisible();
  await expect(page.getByRole('radiogroup', { name: '实时图显示' })).toBeVisible();
  const samples = Number(await canvas.getAttribute('data-samples'));
  await expect.poll(async () => Number(await canvas.getAttribute('data-samples')), { timeout: 6000 }).toBeGreaterThan(samples);
  await expect(canvas).toBeVisible();
});

test('pinned on 昨天, the live line has none of the minutes kept from today: flat from the start, no speed (STAGE-17)', async ({ page }) => {
  await open(page);
  await page.getByRole('button', { name: /统计范围/ }).click();
  await page.getByRole('option', { name: '昨天', exact: true }).click();
  await expect(page.locator('.summary')).toContainText('昨天');
  await page.waitForTimeout(600);
  // Fifty seconds of Claude writing today: what the app kept, all of it today's.
  await seed(page, 50, { id: 'claude', tokens: 2000, output: 50 });
  await page.getByRole('button', { name: '钉住面板', exact: true }).click();
  const canvas = page.locator('canvas.live');
  await expect(canvas).toBeVisible();
  await page.waitForTimeout(800);
  expect(Number(await canvas.getAttribute('data-samples'))).toBeLessThan(5);
  expect(Number(await canvas.getAttribute('data-rate') ?? 0)).toBe(0);
});

test('pinned, the live line opens with the last minutes the app kept: its speed at once, and a gap in them no burst', async ({ page }) => {
  await open(page);
  // Fifty seconds of Claude writing 25 tokens a second.
  await seed(page, 50, { id: 'claude', tokens: 2000, output: 50 });
  await page.getByRole('button', { name: '钉住面板', exact: true }).click();
  const canvas = page.locator('canvas.live');
  // The first frames already have the minutes: the readings, and a speed (not 0, not waiting a minute for it).
  await expect.poll(async () => Number(await canvas.getAttribute('data-samples')), { timeout: 1200 }).toBeGreaterThan(20);
  // 25 a second written, read as an instant rate (PERF-10: each reading's output lands at once and eases off): about 25,
  // more just after a reading lands. Not a burst: the minutes replayed whole would read as tens of thousands.
  const steady = Number(await canvas.getAttribute('data-rate'));
  expect(steady).toBeGreaterThan(10);
  expect(steady).toBeLessThan(60);
  await page.getByRole('button', { name: '取消钉住面板', exact: true }).click();
  await page.waitForTimeout(800);
  // A gap of a minute and a half, after which 600,000 more output had been written at moments unknown: not a speed.
  await seed(page, 140, { id: 'claude', tokens: 2000, output: 50 }, [110, 20], 12_000);
  await page.getByRole('button', { name: '钉住面板', exact: true }).click();
  await expect.poll(async () => Number(await canvas.getAttribute('data-samples')), { timeout: 1200 }).toBeGreaterThan(10);
  await page.waitForTimeout(300);
  // The 600,000 taken as a speed would read 60,000 a second.
  expect(Number(await canvas.getAttribute('data-rate'))).toBeLessThan(60);
});

test('pinned, the live line\'s end keeps its halo and ring inside the canvas, narrow and wide', async ({ page }) => {
  await open(page);
  await seed(page, 50, { id: 'claude', tokens: 2000, output: 50 });
  await page.getByRole('button', { name: '钉住面板', exact: true }).click();
  const canvas = page.locator('canvas.live');
  for (const width of [360, 480]) {
    await page.evaluate((w) => window.dispatchEvent(new CustomEvent('panel-resized', { detail: { width: w, height: 760 } })), width);
    for (let i = 0; i < 12; i++) {
      await page.waitForTimeout(250);
      const tip = await canvas.getAttribute('data-tip');
      if (!tip) continue;
      const [x, y, w, h] = tip.split(',').map(Number);
      // The halo (5) and the ring a reading sends out (10).
      expect(y - 10, tip).toBeGreaterThanOrEqual(0);
      expect(y + 10, tip).toBeLessThanOrEqual(h);
      expect(x + 10, tip).toBeLessThanOrEqual(w);
    }
  }
});
