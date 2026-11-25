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

test('the five tabs sit on one line, evenly, in Chinese, English and 繁體, and in 手绘', async ({ page }) => {
  for (const init of [{}, { lang: 'en' }, { lang: 'zh-Hant' }, { style: 'sketch' }, { lang: 'en', style: 'sketch' }]) {
    const tab = Object.keys(init).length ? await page.context().newPage() : page;
    await open(tab, init);
    const tabs = await tab.locator('.tabs .tab').evaluateAll((list) => list.map((b) => { const r = b.getBoundingClientRect(); return { top: r.top, left: r.left, right: r.right, text: b.textContent, wraps: b.scrollWidth > b.clientWidth + 1 }; }));
    const room = await tab.locator('.dashboard').evaluate((d) => d.getBoundingClientRect().right);
    expect(tabs.length, JSON.stringify(init)).toBe(5);
    for (const t of tabs) { expect(t.top).toBe(tabs[0].top); expect(t.wraps, t.text!).toBe(false); }
    for (let i = 1; i < tabs.length; i++) expect(tabs[i].left - tabs[i - 1].right).toBeCloseTo(2, 0);
    expect(tabs.at(-1)!.right).toBeLessThanOrEqual(room - 8);
    if (tab !== page) await tab.close();
  }
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
