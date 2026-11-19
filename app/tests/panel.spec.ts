// The panel against the fixtures: what each page shows and how its controls answer, so a regression in the port shows
// up before anyone opens the app.
import { expect, test, type Page } from '@playwright/test';

const errors: string[] = [];
test.beforeEach(({ page }) => {
  errors.length = 0;
  page.on('pageerror', (error) => errors.push(String(error)));
  page.on('console', (message) => { if (message.type() === 'error') errors.push(message.text()); });
});
test.afterEach(() => expect(errors).toEqual([]));

async function open(page: Page, query = '') {
  await page.goto('/' + query);
  await expect(page.locator('.dashboard')).toBeVisible();
  // The panel asks for a fresh reading as it opens; let that settle first.
  await page.waitForTimeout(300);
}
const button = (page: Page, label: string) => page.getByRole('button', { name: label, exact: true });
/** The panel's own dropdown: opens under its button, then the option is clicked. */
async function choose(page: Page, label: RegExp, option: string) {
  await page.getByRole('button', { name: label }).click();
  await page.getByRole('option', { name: option, exact: true }).click();
}

test('the homepage shows Summary, the trend, the heatmap, the cards and the rows', async ({ page }) => {
  await open(page);
  await expect(page.locator('.heading')).toHaveText('Starmeter');
  await expect(page.locator('.figure-label').first()).toHaveText('已知 API 估算');
  await expect(page.locator('.legend')).toHaveCount(8);
  await expect(page.locator('.chart path')).not.toHaveCount(0);
  await expect(page.locator('.squares rect.cell')).toHaveCount(24);
  // Three cards, those that cost most, each in its place; the rest one click away.
  await expect(page.locator('.card .name')).toHaveText(['Magpie', 'Claude', 'ChatGPT']);
  await page.getByRole('button', { name: '其余 4 个工具' }).click();
  await expect(page.locator('.card .name')).toHaveText(['Magpie', 'Claude', 'ChatGPT', 'OpenCode', 'Qoder', 'Gemini CLI', 'Cursor']);
  await page.getByRole('button', { name: '只看前 3 个' }).click();
  await expect(page.locator('.card .name')).toHaveCount(3);
  await expect(page.locator('.card-note').filter({ hasText: '次已由 Magpie 计入' })).toBeVisible();
  await expect(page.locator('.row .title')).toContainText(['ChatGPT 订阅', 'Claude 订阅', 'Cursor 订阅']);
  // Usage rows carry their 3pt bar, coloured by level.
  await expect(page.locator('.gauge')).toHaveCount(4);
});

test('header buttons take clicks, and pinning turns the trend live', async ({ page }) => {
  await open(page);
  // No refresh or auto-detect in the header: opening reads what is due, pinning reads every few seconds, and
  // auto-detect lives in 外观与设置.
  await expect(button(page, '刷新数据')).toHaveCount(0);
  await expect(button(page, '自动检测')).toHaveCount(0);
  await button(page, '钉住面板').click();
  await expect(page.getByLabel('实时用量图')).toBeVisible();
  await expect(button(page, '取消钉住面板')).toBeVisible();
  // Pinned, the grabber and the corner say it moves and resizes; no line of text spells it out.
  await expect(page.locator('.grabber')).toBeVisible();
  await expect(page.getByRole('separator', { name: '拖动调整大小' })).toBeVisible();
  await expect(page.getByText('拖动顶部移动面板')).toHaveCount(0);
  // Pinned, the panel turns to 会话 and fits the dashboard, down to just above the subscriptions.
  await expect(page.getByRole('radio', { name: '会话' })).toHaveAttribute('aria-checked', 'true');
  await expect(page.locator('.session')).toHaveCount(3);
  const gap = async () => {
    const window = (await page.locator('.window').boundingBox())!, dashboard = (await page.locator('.dashboard').boundingBox())!;
    return window.y + window.height - (dashboard.y + dashboard.height);
  };
  await expect.poll(gap).toBeLessThan(24);
  expect(await gap()).toBeGreaterThanOrEqual(0);
  // Opening a session, or turning to the cards, refits it (within the screen).
  const tall = async () => (await page.locator('.window').boundingBox())!.height;
  const compact = await tall();
  await page.locator('.session .head').first().click();
  await expect.poll(tall).toBeGreaterThan(compact + 40);
  await expect.poll(gap).toBeLessThan(24);
  await page.locator('.session .head').first().click();
  await expect.poll(tall).toBeLessThan(compact + 4);
  // The live tick reads again and the figures roll up.
  const before = await page.locator('.figure').nth(1).textContent();
  await expect.poll(async () => page.locator('.figure').nth(1).textContent(), { timeout: 8000 }).not.toBe(before);
  // Token / Token/s: the line becomes the last minute's output rate, and the choice is kept.
  await page.getByRole('radio', { name: 'Token/s' }).click();
  await expect(page.getByRole('radio', { name: 'Token/s' })).toHaveAttribute('aria-checked', 'true');
  expect(await page.evaluate(() => localStorage.getItem('starmeter.liveMode'))).toBe('rate');
  await page.getByRole('radio', { name: 'Token', exact: true }).click();
  await button(page, '取消钉住面板').click();
  await expect(page.getByLabel('实时用量图')).toHaveCount(0);
  await expect(page.locator('.grabber')).toHaveCount(0);
  // Unpinned, the cards it showed come back.
  await expect(page.getByRole('radio', { name: '工具' })).toHaveAttribute('aria-checked', 'true');
});

test('会话 sits beside the groupings: running or idle, this turn\'s tokens, three shown and the rest one click away', async ({ page }) => {
  await open(page);
  // Its row leaves the other rows for the tab.
  await expect(page.locator('.row .title', { hasText: /^会话$/ })).toHaveCount(0);
  await page.getByRole('radio', { name: '会话' }).click();
  const cards = page.locator('.session');
  await expect(cards).toHaveCount(3);
  await expect(cards.first().locator('.title')).toHaveText('重做设置页的分段控件');
  await expect(cards.first().locator('.value')).toHaveText('2.41M Token');
  await expect(cards.first()).toContainText('运行中 4 分钟');
  await expect(cards.first()).toContainText('本轮 $0.92 · 9 次请求');
  await expect(page.locator('.session-list')).not.toContainText('等待输入');
  await page.locator('.session-list .more').click();
  await expect(cards).toHaveCount(4);
  await expect(cards.last()).toContainText('空闲 6 分钟');
  await expect(cards.last()).toContainText('上一轮 $0.12');
  // A card opens its last turns.
  await cards.first().locator('.head').click();
  await expect(cards.first().locator('.turn')).toHaveCount(3);
  await expect(page.getByRole('switch', { name: /这一轮完成时提醒/ })).toBeVisible();
  // The choice is remembered.
  await page.reload();
  await expect(page.getByRole('radio', { name: '会话' })).toHaveAttribute('aria-checked', 'true');
});

test('a pinned panel resized at its edges lays its pages out at that width, and unpinning gives the size back', async ({ page }) => {
  await open(page);
  const resize = (width: number, height: number) =>
    page.evaluate(([w, h]) => window.dispatchEvent(new CustomEvent('panel-resized', { detail: { width: w, height: h } })), [width, height]);
  // Not pinned: the user can't have resized it, and the page keeps 360.
  await resize(560, 640);
  await expect(page.locator('.window')).toHaveCSS('width', '360px');
  await button(page, '钉住面板').click();
  await resize(560, 640);
  await expect(page.locator('.window')).toHaveCSS('width', '560px');
  await expect(page.locator('.window')).toHaveCSS('height', '640px');
  await expect(page.locator('.dashboard')).toHaveCSS('width', '544px');
  // The figures, the live chart and the cards reach the new right edge.
  const right = async (selector: string) => page.locator(selector).last().evaluate((e) => e.getBoundingClientRect().right);
  const edge = await right('.dashboard');
  expect(edge - (await right('.figure'))).toBeLessThan(16);
  expect(edge - (await right('.live'))).toBeLessThan(16);
  expect(edge - (await right('.session .value'))).toBeLessThan(16);
  // The corner handle follows the pointer, no smaller than 360 × 220.
  const corner = (await page.getByRole('separator', { name: '拖动调整大小' }).boundingBox())!;
  await page.mouse.move(corner.x + 15, corner.y + 15);
  await page.mouse.down();
  await page.mouse.move(corner.x + 15 - 100, corner.y + 15 - 60, { steps: 6 });
  await page.mouse.up();
  await expect(page.locator('.window')).toHaveCSS('width', '460px');
  await expect(page.locator('.window')).toHaveCSS('height', '580px');
  await resize(560, 640);
  // A row's page takes the width too.
  await page.locator('.row').filter({ hasText: 'Claude 订阅' }).click();
  await expect(page.locator('.page .rule')).toHaveCSS('width', '528px');
  await page.keyboard.press('Escape');
  await button(page, '取消钉住面板').click();
  await expect(page.locator('.window')).toHaveCSS('width', '360px');
  await expect(page.locator('.dashboard')).toHaveCSS('width', '344px');
  await expect(page.locator('.card .cost').first()).toBeVisible();
});

test('questions are answered in place, under the row that asked, not in a dialog', async ({ page }) => {
  await open(page);
  await page.locator('.row').filter({ hasText: 'ChatGPT 订阅' }).click();
  await page.getByText('订阅月费…').click();
  const field = page.locator('.ask .field');
  await expect(field).toBeFocused();
  await expect(field).toHaveValue('20.00');
  await field.fill('30');
  await page.keyboard.press('Enter');
  await expect(page.locator('.ask')).toHaveCount(0);
  await expect(page.getByRole('button', { name: /订阅月费/ })).toContainText('$30.00');
  // Esc cancels the question, not the page.
  await page.getByText('订阅月费…').click();
  await expect(field).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.locator('.ask')).toHaveCount(0);
  await expect(page.getByText('订阅月费…')).toBeVisible();
  // A wrong amount says so in place.
  await page.getByText('订阅月费…').click();
  await field.fill('abc');
  await page.keyboard.press('Enter');
  await expect(page.locator('.ask.notice')).toContainText('金额无效');
  await page.getByRole('button', { name: '好', exact: true }).click();
  // Deleting a Key confirms under its row, in red.
  await page.keyboard.press('Escape');
  await page.locator('.row').filter({ hasText: 'Claude API' }).click();
  await page.getByText('删除…').first().click();
  await expect(page.locator('.ask .title')).toHaveText('删除这把 Key？');
  await expect(page.locator('.ask .button.danger')).toHaveText('删除');
  await page.getByRole('button', { name: '取消', exact: true }).click();
  await expect(page.locator('.ask')).toHaveCount(0);
});

test('a header hint appears after the pointer rests', async ({ page }) => {
  await open(page);
  await button(page, '分享用量图').hover();
  await expect(page.getByRole('tooltip')).toHaveText('分享用量图');
  await page.mouse.move(10, 900);
  await expect(page.getByRole('tooltip')).toHaveCount(0);
});

test('pointing at a heatmap square reads it out on the line below', async ({ page }) => {
  await open(page);
  await expect(page.locator('.caption')).toHaveCount(0);
  const square = page.locator('.squares rect.cell').nth(10);
  await square.hover();
  await expect(page.locator('.caption')).toContainText('11月19日 Thu 10:00');
  await expect(page.locator('.caption')).toContainText('Token');
});

test('a new range brings its own dashboard', async ({ page }) => {
  await open(page);
  await choose(page, /统计范围/, '近 7 天');
  await expect(page.locator('.picker .current')).toHaveText('近 7 天');
  await expect(page.locator('.squares rect.cell')).toHaveCount(168);
  await expect(page.locator('.caption')).toContainText('连续');
  await expect(page.locator('.date').last()).toHaveText('11/19');
});

test('grouping by model or project swaps the cards, with the pill on the choice', async ({ page }) => {
  await open(page);
  await page.getByRole('radio', { name: '模型' }).click();
  await expect(page.getByRole('radio', { name: '模型' })).toHaveAttribute('aria-checked', 'true');
  await expect(page.locator('.card .name').first()).toHaveText('Claude Opus 5.5');
  await page.getByRole('button', { name: /^其余 \d+ 个模型$/ }).click();
  await expect(page.locator('.card .name')).toContainText(['Claude Sonnet 5.5']);
  await page.getByRole('radio', { name: '项目' }).click();
  await expect(page.locator('.tabs .note')).toHaveText('按客户端自己的记录');
  await page.getByRole('button', { name: /^其余 \d+ 个项目$/ }).click();
  await expect(page.locator('.card .name')).toContainText(['starmeter', '未标注项目']);
});

test('a row opens its page, with usage bars and pace marks, and Esc comes back', async ({ page }) => {
  await open(page);
  await page.locator('.row', { hasText: 'Claude 订阅' }).click();
  await expect(page.locator('.page h2.title')).toHaveText('Claude 订阅');
  await expect(page.locator('.bar')).toHaveCount(3);
  await expect(page.locator('.pace')).toHaveCount(3);
  await expect(page.locator('.detail-row', { hasText: '打开订阅用量' }).locator('.out')).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.locator('.dashboard')).toBeVisible();
});

test('a Key opens in place on its guide line', async ({ page }) => {
  await open(page);
  await page.locator('.row', { hasText: 'DeepSeek API' }).click();
  await expect(page.locator('.detail-row', { hasText: '重命名…' })).toHaveCount(0);
  const key = page.locator('.detail-row', { hasText: 'lab' });
  await key.click();
  await expect(key).toHaveAttribute('aria-expanded', 'true');
  await expect(page.locator('.detail-row', { hasText: '删除…' })).toBeVisible();
  await expect(page.locator('.guide').first()).toBeVisible();
  await key.click();
  await expect(page.locator('.detail-row', { hasText: '删除…' })).toHaveCount(0);
});

test('settings: theme segments, switches and the module grid answer at once', async ({ page }) => {
  await open(page);
  await button(page, '主页模块与设置').click();
  await expect(page.locator('.modules .heading')).toHaveText('主页模块');
  await page.getByText('外观与设置', { exact: true }).click();
  await page.getByRole('radio', { name: '深色' }).click();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  const status = page.getByRole('switch', { name: /^服务状态/ });
  await expect(status).toHaveAttribute('aria-checked', 'false');
  await status.click();
  await expect(status).toHaveAttribute('aria-checked', 'true');
  await page.keyboard.press('Escape');
  // Back on the grid: turning Claude off takes its card off the homepage.
  await page.getByRole('checkbox', { name: '在主页显示 Claude', exact: true }).click();
  await page.keyboard.press('Escape');
  await expect(page.locator('.card .name')).not.toContainText(['Claude']);
});

test('a folded card unfolds where it is, and its arrow opens the tool', async ({ page }) => {
  await open(page);
  await button(page, '主页模块与设置').click();
  await page.getByText('允许折叠', { exact: true }).click();
  await page.keyboard.press('Escape');
  await expect(page.locator('.model-name', { hasText: 'Opus 5.5' })).toHaveCount(0);
  // A click on the card unfolds it, the same click again folds it: never a second meaning.
  const fold = page.getByRole('button', { name: '展开 Claude 明细' });
  await fold.click();
  await expect(page.locator('.model-name', { hasText: 'Opus 5.5' })).toBeVisible();
  await page.getByRole('button', { name: '收起 Claude 明细' }).click();
  await expect(page.locator('.model-name', { hasText: 'Opus 5.5' })).toHaveCount(0);
  await expect(page.locator('.dashboard')).toBeVisible();
  // The arrow is the page's own button.
  await button(page, 'Claude 详情').click();
  await expect(page.getByRole('heading', { name: 'Claude', exact: true })).toBeVisible();
});

test('a tool card opens its page: figures, its trend, every model, then its settings', async ({ page }) => {
  await open(page);
  await button(page, 'Claude 详情').click();
  await expect(page.getByRole('heading', { name: 'Claude', exact: true })).toBeVisible();
  await expect(page.locator('.figure-name')).toHaveText(['Token', '请求', '缓存', '输出', '会话']);
  await expect(page.locator('.page .trend path')).toHaveCount(2);
  await expect(page.locator('.page .model-name')).toHaveText(['Opus 5.5', 'Fable 5.2', 'Haiku 5.5']);
  await expect(page.locator('.page .section', { hasText: '设置' })).toBeVisible();
  await expect(page.getByText('选择 Claude 数据目录…')).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.locator('.dashboard')).toBeVisible();
  // A gateway's page names its clients; a card grouped by model has no page.
  await button(page, 'Magpie 详情').click();
  await expect(page.locator('.page .clients')).toContainText('客户端');
  await page.keyboard.press('Escape');
  await page.getByRole('radio', { name: '模型' }).click();
  await expect(page.locator('.card .name').first()).toHaveText('Claude Opus 5.5');
  await expect(page.locator('.card .open')).toHaveCount(0);
});

test('pinned, the share page takes its own height and gives the panel back its size', async ({ page }) => {
  await open(page);
  await button(page, '钉住面板').click();
  await page.evaluate(() => window.dispatchEvent(new CustomEvent('panel-resized', { detail: { width: 400, height: 300 } })));
  await expect(page.locator('.window')).toHaveCSS('height', '300px');
  await button(page, '分享用量图').click();
  await page.locator('.share-page .frame img').waitFor();
  // As tall as the picture beside its controls, not the 300 the panel was sized to.
  await expect.poll(async () => (await page.locator('.window').boundingBox())!.height).toBeGreaterThan(330);
  await page.keyboard.press('Escape');
  await expect(page.locator('.window')).toHaveCSS('height', '300px');
  await expect(page.locator('.window')).toHaveCSS('width', '400px');
});

test('sharing previews the picture in each style, then copies or saves it', async ({ page }) => {
  await open(page);
  await button(page, '分享用量图').click();
  await expect(page.locator('.share-page .title')).toHaveText('分享用量图今天');
  const preview = page.locator('.frame img');
  // Every style draws, each with its own picture.
  const sources = new Set<string>();
  for (const name of ['卡片', '报刊', '夜空', '地铁', '织物', '终端']) {
    if (name !== (await page.locator('.share-page .trigger .current').textContent())) await choose(page, /^风格/, name);
    await expect(page.locator('.share-page .trigger .current')).toHaveText(name);
    await expect(preview).toHaveAttribute('alt', new RegExp('^' + name));
    sources.add((await preview.getAttribute('src'))!);
  }
  expect(sources.size).toBe(6);
  // The picture is the poster itself, shown small (enough to see its look) and never cropped; light or dark is chosen
  // beside it.
  const size = await preview.evaluate((img: HTMLImageElement) => [img.naturalWidth, img.naturalHeight, img.width, img.height]);
  expect(size[0]).toBe(1260);
  expect(size[2]).toBeLessThanOrEqual(130);
  const light = await preview.getAttribute('src');
  await page.getByRole('radio', { name: '深色' }).click();
  await expect(preview).not.toHaveAttribute('src', light!);
  expect(Math.abs(size[2] / size[3] - size[0] / size[1])).toBeLessThan(0.01);
  await button(page, '复制图片').click();
  await expect(page.locator('.action.primary')).toHaveText('已复制');
  await expect(page.locator('.action.primary')).toHaveText('复制图片', { timeout: 4000 });
  await button(page, '保存…').click();
  await expect(page.locator('.action', { hasText: '已保存' })).toBeVisible();
  // The chosen style is remembered, and Esc goes back home.
  await page.keyboard.press('Escape');
  await expect(page.locator('.dashboard')).toBeVisible();
  await button(page, '分享用量图').click();
  await expect(page.locator('.share-page .trigger .current')).toHaveText('终端');
  await expect(page.getByRole('radio', { name: '深色' })).toHaveAttribute('aria-checked', 'true');
});

test('failures and first-run states say so', async ({ page }) => {
  await open(page, '?fixture=failed');
  await expect(page.locator('.card-note', { hasText: '刷新失败 · 保留原数据' })).toBeVisible();
  await expect(page.locator('.note')).toContainText('部分图表数据不可用');
  await expect(page.locator('.row', { hasText: 'Claude 订阅' })).toContainText('刷新失败');
  await open(page, '?fixture=pending');
  await expect(page.locator('.waiting').first()).toHaveText('等待可用数据');
  await expect(page.locator('.caption')).toHaveText('待同步');
  // Nothing read yet: the trend's grid alone, no scale or flat line that would read as no usage.
  await expect(page.locator('.axis')).toHaveCount(0);
  await expect(page.locator('.chart path')).toHaveCount(0);
});

test('with reduced motion, pages still change and nothing animates forever', async ({ page }) => {
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await open(page);
  await page.locator('.row', { hasText: 'Kimi API' }).click();
  await expect(page.locator('.page h2.title')).toHaveText('Kimi API');
  await page.keyboard.press('Escape');
  await expect(page.locator('.dashboard')).toBeVisible();
});

test.describe('header icons', () => {
  test.use({ deviceScaleFactor: 2 });
  test('sit on one line by the centre the eye sees (shape and ink), within 0.25pt', async ({ page }) => {
    await open(page);
    // A plain white backdrop, so ink is everything darker than it.
    await page.addStyleTag({ content: '.window { background: #fff !important; backdrop-filter: none !important } :root { --label: #000 }' });
    await page.mouse.move(5, 1650);
    await page.waitForTimeout(400);
    const { decodePng } = await import('./png');
    for (const label of ['主页模块与设置', '钉住面板', '分享用量图']) {
      const shot = decodePng(await button(page, label).screenshot());
      let minX = shot.width, maxX = -1, minY = shot.height, maxY = -1, mass = 0, sumY = 0;
      for (let y = 0; y < shot.height; y++) {
        for (let x = 0; x < shot.width; x++) {
          const i = (y * shot.width + x) * 4, ink = 1 - (shot.pixels[i] + shot.pixels[i + 1] + shot.pixels[i + 2]) / 765;
          if (ink <= 0.02) continue;
          minX = Math.min(minX, x); maxX = Math.max(maxX, x); minY = Math.min(minY, y); maxY = Math.max(maxY, y);
          mass += ink; sumY += ink * (y + 0.5);
        }
      }
      const eyeX = (minX + maxX + 1) / 2 / 2, eyeY = ((minY + maxY + 1) / 2 + sumY / mass) / 2 / 2;
      expect.soft(Math.abs(eyeX - shot.width / 4), `${label} across`).toBeLessThanOrEqual(0.25);
      expect.soft(Math.abs(eyeY - shot.height / 4), `${label} down`).toBeLessThanOrEqual(0.25);
    }
  });
});

test('the range picker is bare, as wide as its range, and opens its own menu under itself', async ({ page }) => {
  await open(page);
  const picker = page.locator('.picker .trigger');
  await expect(picker).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
  const today = (await picker.boundingBox())!.width;
  await picker.click();
  const menu = page.getByRole('listbox', { name: '统计范围' });
  await expect(menu).toBeVisible();
  // Under its button, never over it; the current range checked.
  expect((await menu.boundingBox())!.y).toBeGreaterThan((await picker.boundingBox())!.y + 20);
  await expect(page.getByRole('option', { name: '今天' })).toHaveAttribute('aria-selected', 'true');
  // Arrow keys and Return pick; Esc and a click elsewhere close without picking.
  await page.keyboard.press('Escape');
  await expect(menu).toHaveCount(0);
  await picker.click();
  await page.mouse.click(5, 600);
  await expect(menu).toHaveCount(0);
  await choose(page, /统计范围/, '近 24 小时');
  await expect(page.locator('.picker .current')).toHaveText('近 24 小时');
  expect((await picker.boundingBox())!.width).toBeGreaterThan(today + 20);
});

test('the range menu opens without moving the page, and fits a small pinned panel', async ({ page }) => {
  await open(page);
  await button(page, '钉住面板').click();
  // The panel made small at its corner.
  await page.evaluate(() => window.dispatchEvent(new CustomEvent('panel-resized', { detail: { width: 360, height: 260 } })));
  await expect.poll(async () => (await page.locator('.window').boundingBox())!.height).toBeLessThan(300);
  const viewport = page.locator('.dashboard').locator('xpath=ancestor::div[contains(@class,"viewport")]').first();
  const before = await viewport.evaluate((node) => node.scrollTop);
  const heading = (await page.locator('.picker .trigger').boundingBox())!.y;
  await page.locator('.picker .trigger').click();
  const menu = page.getByRole('listbox', { name: '统计范围' });
  await expect(menu).toBeVisible();
  await page.waitForTimeout(250);
  // Nothing under the pointer moves, and the list scrolls inside a menu that ends inside the window.
  expect(await viewport.evaluate((node) => node.scrollTop)).toBe(before);
  expect((await page.locator('.picker .trigger').boundingBox())!.y).toBe(heading);
  const box = (await menu.boundingBox())!, window = (await page.locator('.window').boundingBox())!;
  expect(box.y + box.height).toBeLessThanOrEqual(window.y + window.height);
  await page.keyboard.press('End');
  for (let i = 0; i < 7; i++) await page.keyboard.press('ArrowDown');
  await expect(page.getByRole('option', { name: '全部' })).toBeInViewport();
  await page.keyboard.press('Enter');
  await expect(page.locator('.picker .current')).toHaveText('全部');
});

test('a module cell with a note keeps its checkbox and icon in the middle', async ({ page }) => {
  await open(page);
  await button(page, '主页模块与设置').click();
  const cell = page.getByRole('button', { name: '允许折叠', exact: true });
  const box = (await cell.boundingBox())!, check = (await cell.locator('.box').boundingBox())!;
  expect(Math.abs(check.y + check.height / 2 - (box.y + box.height / 2))).toBeLessThanOrEqual(0.5);
});
