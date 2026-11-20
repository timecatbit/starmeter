// Everything on the panel clicked, the way a user would go through it: every homepage row, tool page, grouping tab and
// 查看更多, every range, every settings page and module page, the share page's controls and the pin. Each opens what
// it should and comes back, and nothing goes wrong on the page. One round here; SWEEP_ROUNDS=3 goes round three times.
import { expect, test, type Page } from '@playwright/test';

const ROUNDS = Number(process.env.SWEEP_ROUNDS ?? 1);
const button = (page: Page, label: string) => page.getByRole('button', { name: label, exact: true });

/** Back to the homepage with Esc, and the homepage alone on screen (the page that left has finished sliding). */
async function home(page: Page) {
  await page.keyboard.press('Escape');
  await expect(page.locator('.view')).toHaveCount(1);
  await expect(page.locator('.dashboard')).toBeVisible();
}

test('every part of the panel opens what it should and comes back', async ({ page }) => {
  test.setTimeout(120_000 * ROUNDS);
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  page.on('console', (message) => { if (message.type() === 'error') errors.push(message.text()); });
  await page.goto('/?mac');
  await expect(page.locator('.dashboard')).toBeVisible();
  for (let round = 0; round < ROUNDS; round++) {
    // Every homepage row with a page: its page, then back.
    const rows = await page.locator('.home button.row:not([disabled]) .title').allTextContents();
    expect(rows.length).toBeGreaterThan(5);
    for (const title of rows) {
      await page.locator('.home button.row', { has: page.locator('.title', { hasText: title }) }).first().click();
      await expect(page.locator('.page h2.title')).toHaveText(title);
      await home(page);
    }
    // Every tool's page.
    await page.getByRole('radio', { name: '工具' }).click();
    const tools = await page.locator('.card .open').evaluateAll((all) => all.map((b) => b.getAttribute('aria-label') ?? ''));
    for (const label of tools) {
      await button(page, label).click();
      await expect(page.locator('.dashboard')).toHaveCount(0);
      await home(page);
    }
    // The groupings and 会话, each with its 查看更多.
    for (const [tab, selector] of [['模型', '.card'], ['项目', '.card'], ['会话', '.session'], ['工具', '.card']] as const) {
      await page.getByRole('radio', { name: tab }).click();
      await expect(page.getByRole('radio', { name: tab })).toHaveAttribute('aria-checked', 'true');
      const more = page.locator('.more');
      if (await more.count()) {
        const shown = await page.locator(selector).count();
        await more.click();
        await expect.poll(() => page.locator(selector).count()).toBeGreaterThan(shown);
        await expect(more).toHaveCount(0);
      }
    }
    // Every session opens and closes.
    await page.getByRole('radio', { name: '会话' }).click();
    const heads = page.locator('.session .head');
    for (let i = 0; i < await heads.count(); i++) {
      await heads.nth(i).click();
      await expect(page.locator('.session').nth(i)).toHaveClass(/open/);
      await heads.nth(i).click();
      await expect(page.locator('.session').nth(i)).not.toHaveClass(/open/);
    }
    await page.getByRole('radio', { name: '工具' }).click();
    // Every range, from the dropdown.
    for (const range of ['昨天', '近 24 小时', '近 7 天', '近 30 天', '本周', '本月', '全部', '今天']) {
      await page.getByRole('button', { name: /^统计范围/ }).click();
      await page.getByRole('option', { name: range, exact: true }).click();
      await expect(page.getByRole('button', { name: /^统计范围/ })).toContainText(range);
      await expect(page.locator('.summary .reading')).toHaveCount(0);
    }
    // Every cell of the module grid: a settings page, a module's page, or (a module with nothing to set) a switch
    // flipped and flipped back.
    await button(page, '主页模块与设置').click();
    await expect(page.locator('.modules')).toBeVisible();
    const cells = await page.locator('.modules .name').allTextContents();
    expect(cells.length).toBeGreaterThan(20);
    for (const name of cells) {
      const cell = page.locator('.modules .name', { hasText: name }).first();
      await cell.click();
      await page.waitForTimeout(80);
      if (await page.locator('.modules').count() && await page.locator('.view').count() === 1) {
        await cell.click();
      } else {
        await expect(page.locator('.view')).toHaveCount(1);
        await page.keyboard.press('Escape');
        await expect(page.locator('.view')).toHaveCount(1);
        await expect(page.locator('.modules')).toBeVisible();
      }
    }
    await home(page);
    // The share page: every style, both colours, fewer and more tools, then back.
    await button(page, '分享用量图').click();
    await expect(page.locator('.share-page .stage img')).toBeVisible();
    const styles = ['终端', '手绘', '报刊', '夜空', '地铁', '织物', '收据', '蓝图', '霓虹', '卡片'];
    for (const style of styles) {
      await page.getByRole('button', { name: /^风格/ }).click();
      await page.getByRole('option', { name: style, exact: true }).click();
      await expect(page.locator('.stage img')).toHaveAttribute('alt', new RegExp(style));
    }
    for (const mode of ['深色', '浅色']) await page.getByRole('radio', { name: mode }).click();
    await button(page, '少一个').click();
    await button(page, '多一个').click();
    await home(page);
    // The pin, on and off.
    await button(page, '钉住面板').click();
    await expect(page.locator('.grabber')).toHaveCount(1);
    await button(page, '取消钉住面板').click();
    await expect(page.locator('.grabber')).toHaveCount(0);
  }
  expect(errors).toEqual([]);
});
