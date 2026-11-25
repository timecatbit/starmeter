// 实时, the homepage's fifth tab (PanelLive.svelte): what the AI is doing now, 正在运行 and 动态, from the app's live_now
// as 网页版 has it. Pinning turns to it and unpinning turns back (unless another was picked meanwhile); the five tabs
// on one line in every language and look; the keyboard walks all five; unpinned it is never blank.
import { expect, test, type Page } from '@playwright/test';

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
  // Remembered as the homepage's tab.
  await page.reload();
  await expect(radio(page, '实时')).toHaveAttribute('aria-checked', 'true');
});
