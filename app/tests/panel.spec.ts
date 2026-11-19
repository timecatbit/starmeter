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
  await expect(page.locator('.squares .cell')).toHaveCount(24);
  await expect(page.locator('.card .name')).toHaveText(['Magpie', 'ChatGPT', 'Claude', 'Cursor', 'Gemini CLI', 'OpenCode', 'Qoder']);
  await expect(page.getByRole('button', { name: /^其余 \d+ 个/ })).toHaveCount(0);
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
  // Pinned, the panel turns to 会话 and fits down to the end of the third session; the rest is below, to scroll to or
  // to make the panel taller for.
  await expect(page.getByRole('radio', { name: '会话' })).toHaveAttribute('aria-checked', 'true');
  await expect(page.locator('.session')).toHaveCount(4);
  const gap = async () => {
    const window = (await page.locator('.window').boundingBox())!, third = (await page.locator('.session').nth(2).boundingBox())!;
    return window.y + window.height - (third.y + third.height);
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

test('a new range while pinned carries the live line on instead of starting it over', async ({ page }) => {
  await open(page);
  const result = await page.evaluate(async () => {
    const { backend } = await import('/src/lib/backend.ts');
    const { LiveFeed } = await import('/src/lib/live.ts');
    const today = (await backend.snapshot()).dashboard!;
    const more = (d: typeof today, by: number, key = d.rangeKey) => ({ ...d, rangeKey: key,
      providers: d.providers.map((p) => ({ ...p, tokens: p.tokens * by, output: p.output * by })) });
    const feed = new LiveFeed(), t = Date.now();
    feed.add(today, t);
    feed.add(more(today, 1.01), t + 2000);
    const before = feed.samples.at(-1)!;
    // 近 7 天 totals far more: its reading carries on from the last one, and what it adds after keeps adding up.
    feed.add(more(today, 30, '7d'), t + 4000);
    feed.add(more(today, 30.2, '7d'), t + 6000);
    const samples = feed.samples;
    const near = (a: number, b: number) => Math.abs(a - b) <= 1e-6 * Math.max(1, Math.abs(b));
    return { count: samples.length, carried: near(samples[2].total, before.total) && near(samples[2].output, before.output),
      grew: samples[3].total > samples[2].total, rate: feed.rateAt(t + 6000) ?? 0,
      ranges: near(feed.value(samples[2]), LiveFeed.latest(more(today, 30, '7d'))) };
  });
  expect(result.count).toBe(4);
  expect(result.carried).toBe(true);
  expect(result.grew).toBe(true);
  expect(result.rate).toBeGreaterThan(0);
  // The line's height on the axis is the new range's newest point.
  expect(result.ranges).toBe(true);
});

test('pinned, picking ranges back and forth adds nothing to the live line', async ({ page }) => {
  test.setTimeout(60_000);
  await open(page);
  await button(page, '钉住面板').click();
  await page.waitForTimeout(4500);
  const added = () => page.locator('.live').evaluate((c) => Number((c as HTMLElement).dataset.added ?? 0));
  // As a person picks: at the menu's own place, nothing scrolled to it first.
  const pick = async (label: string) => {
    const at = async (l: ReturnType<Page['locator']>) => { const b = (await l.boundingBox())!; await page.mouse.click(b.x + b.width / 2, b.y + b.height / 2); };
    await at(page.locator('.picker .trigger'));
    await at(page.getByRole('option', { name: label, exact: true }));
  };
  for (const label of ['昨天', '今天', '近 7 天', '昨天', '今天']) {
    const before = await added();
    await pick(label);
    await page.waitForTimeout(700);
    // A new range, its sources arriving one by one, a gateway taking over its clients' requests: none of it is usage.
    expect(await added() - before, label).toBeLessThan(150_000);
    if (label === '昨天') {
      const settled = await added();
      await page.waitForTimeout(2500);
      expect(await added(), 'yesterday does not grow').toBe(settled);
    }
    expect(await page.evaluate(() => document.querySelector('.home .viewport')?.scrollTop ?? 0)).toBe(0);
  }
});

test('Token/s is the last minute\'s output over 60 seconds, even just after pinning', async ({ page }) => {
  await open(page);
  const rates = await page.evaluate(async () => {
    const { backend } = await import('/src/lib/backend.ts');
    const { LiveFeed } = await import('/src/lib/live.ts');
    const today = (await backend.snapshot()).dashboard!;
    const out = (by: number) => ({ ...today, providers: today.providers.map((p) => (p.id === 'claude' ? { ...p, output: p.output + by, tokens: p.tokens + by * 50 } : p)) });
    const feed = new LiveFeed(), t = Date.now();
    feed.add(today, t);
    // Two seconds after pinning, a request that wrote 4,000 tokens over its whole run is logged at once.
    feed.add(out(4000), t + 2000);
    // Read the same way every few seconds; the 4,000 leaves the window 60 seconds after it landed.
    return [feed.rateAt(t + 2000) ?? 0, feed.rateAt(t + 30_000) ?? 0, feed.rateAt(t + 61_000) ?? 0, feed.rateAt(t + 63_000) ?? 0];
  });
  // 4,000 over 60 seconds, not over the 2 since pinning (which read as 2,000 a second); gone once the minute passes it.
  expect(Math.round(rates[0])).toBe(67);
  expect(Math.round(rates[1])).toBe(67);
  expect(Math.round(rates[2])).toBe(67);
  expect(Math.round(rates[3])).toBe(0);
});

test('会话 sits beside the groupings: running or idle, this turn\'s tokens, three shown and the rest one click away', async ({ page }) => {
  await open(page);
  // Its row leaves the other rows for the tab.
  await expect(page.locator('.row .title', { hasText: /^会话$/ })).toHaveCount(0);
  await page.getByRole('radio', { name: '会话' }).click();
  const cards = page.locator('.session');
  await expect(cards).toHaveCount(4);
  await expect(cards.first().locator('.title')).toHaveText('重做设置页的分段控件');
  await expect(cards.first().locator('.value')).toHaveText('2.41M Token');
  await expect(cards.first()).toContainText('运行中 4 分钟');
  await expect(cards.first()).toContainText('本轮 $0.92 · 9 次请求');
  await expect(page.locator('.session-list')).not.toContainText('等待输入');
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
  await resize(480, 640);
  await expect(page.locator('.window')).toHaveCSS('width', '360px');
  await button(page, '钉住面板').click();
  await resize(480, 640);
  await expect(page.locator('.window')).toHaveCSS('width', '480px');
  await expect(page.locator('.window')).toHaveCSS('height', '640px');
  await expect(page.locator('.dashboard')).toHaveCSS('width', '464px');
  // The figures, the live chart and the cards reach the new right edge.
  const right = async (selector: string) => page.locator(selector).last().evaluate((e) => e.getBoundingClientRect().right);
  const edge = await right('.dashboard');
  expect(edge - (await right('.figure'))).toBeLessThan(16);
  expect(edge - (await right('.live'))).toBeLessThan(16);
  expect(edge - (await right('.session .value'))).toBeLessThan(16);
  // The corner handle follows the pointer: no wider than 480 (wider only adds air), no narrower than 360.
  const corner = async () => (await page.getByRole('separator', { name: '拖动调整大小' }).boundingBox())!;
  const drag = async (dx: number, dy: number) => {
    const c = await corner();
    await page.mouse.move(c.x + 15, c.y + 15);
    await page.mouse.down();
    await page.mouse.move(c.x + 15 + dx, c.y + 15 + dy, { steps: 6 });
    await page.mouse.up();
  };
  await drag(-60, -60);
  await expect(page.locator('.window')).toHaveCSS('width', '420px');
  await expect(page.locator('.window')).toHaveCSS('height', '580px');
  await drag(200, 0);
  await expect(page.locator('.window')).toHaveCSS('width', '480px');
  // Never shorter than the homepage's top down to its heatmap.
  await drag(0, -250);
  await expect.poll(async () => (await page.locator('.window').boundingBox())!.height).toBeLessThan(420);
  const floor = await page.locator('.heatmap').evaluate((e) => e.getBoundingClientRect().bottom);
  const panel = (await page.locator('.window').boundingBox())!;
  expect(panel.y + panel.height).toBeGreaterThanOrEqual(floor);
  await resize(480, 640);
  // A row's page takes the width too.
  await page.locator('.row').filter({ hasText: 'Claude 订阅' }).click();
  await expect(page.locator('.page .rule')).toHaveCSS('width', '448px');
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
  const square = page.locator('.squares .cell').nth(10);
  await square.hover();
  await expect(page.locator('.caption')).toContainText('11月19日 Thu 10:00');
  await expect(page.locator('.caption')).toContainText('Token');
});

test('a new range brings its own dashboard', async ({ page }) => {
  await open(page);
  await choose(page, /统计范围/, '近 7 天');
  await expect(page.locator('.picker .current')).toHaveText('近 7 天');
  await expect(page.locator('.squares .cell')).toHaveCount(168);
  await expect(page.locator('.caption')).toContainText('连续');
  await expect(page.locator('.date').last()).toHaveText('11/19');
});

test('grouping by model or project swaps the cards, with the pill on the choice', async ({ page }) => {
  await open(page);
  await page.getByRole('radio', { name: '模型' }).click();
  await expect(page.getByRole('radio', { name: '模型' })).toHaveAttribute('aria-checked', 'true');
  await expect(page.locator('.card .name').first()).toHaveText('Claude Opus 5.5');
  await expect(page.locator('.card .name')).toContainText(['Claude Sonnet 5.5']);
  await page.getByRole('radio', { name: '项目' }).click();
  await expect(page.locator('.tabs .note')).toHaveText('按客户端自己的记录');
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

test('pinned, a row\'s page fits its own rows, and the homepage keeps its size', async ({ page }) => {
  await open(page);
  const height = async () => (await page.locator('.window').boundingBox())!.height;
  await page.locator('.row', { hasText: 'Kimi API' }).first().click();
  await expect(page.locator('.page h2.title')).toHaveText('Kimi API');
  await page.waitForTimeout(400);
  const own = await height();
  await page.keyboard.press('Escape');
  await button(page, '钉住面板').click();
  await page.evaluate(() => window.dispatchEvent(new CustomEvent('panel-resized', { detail: { width: 400, height: 760 } })));
  await expect(page.locator('.window')).toHaveCSS('height', '760px');
  await page.locator('.row', { hasText: 'Kimi API' }).first().click();
  await expect(page.locator('.page h2.title')).toHaveText('Kimi API');
  // Its own height, not the 760 the homepage was sized to; the width stays the panel's.
  await expect.poll(height).toBeLessThan(own + 3);
  expect(await height()).toBeGreaterThan(own - 3);
  await expect(page.locator('.window')).toHaveCSS('width', '400px');
  await page.keyboard.press('Escape');
  await expect(page.locator('.window')).toHaveCSS('height', '760px');
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

test('the share page picks its range, the homepage\'s own, and draws that range', async ({ page }) => {
  await open(page);
  await button(page, '分享用量图').click();
  const preview = page.locator('.frame img');
  await expect(preview).toBeVisible();
  const today = await preview.getAttribute('src');
  await choose(page, /^时间范围/, '近 7 天');
  await expect(page.locator('.share-page .title')).toHaveText('分享用量图近 7 天');
  await expect(preview).not.toHaveAttribute('src', today!);
  await page.keyboard.press('Escape');
  await expect(page.locator('.picker .current')).toHaveText('近 7 天');
});

test('sharing previews the picture in each style, then copies or saves it', async ({ page }) => {
  await open(page);
  await button(page, '分享用量图').click();
  await expect(page.locator('.share-page .title')).toHaveText('分享用量图今天');
  const preview = page.locator('.frame img');
  // Every style draws, each with its own picture.
  const sources = new Set<string>();
  for (const name of ['卡片', '报刊', '夜空', '地铁', '织物', '终端']) {
    if (name !== (await page.getByRole('button', { name: /^风格 · / }).locator('.current').textContent())) await choose(page, /^风格/, name);
    await expect(page.getByRole('button', { name: /^风格 · / }).locator('.current')).toHaveText(name);
    await expect(preview).toHaveAttribute('alt', new RegExp('^' + name));
    sources.add((await preview.getAttribute('src'))!);
  }
  expect(sources.size).toBe(6);
  // The picture is the poster itself, shown small (enough to see its look) and never cropped; light or dark is chosen
  // beside it.
  const size = await preview.evaluate((img: HTMLImageElement) => [img.naturalWidth, img.naturalHeight, img.width, img.height]);
  expect(size[0]).toBe(1260);
  expect(size[2]).toBeLessThanOrEqual(172);
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
  await expect(page.getByRole('button', { name: /^风格 · / }).locator('.current')).toHaveText('终端');
  await expect(page.getByRole('radio', { name: '深色' })).toHaveAttribute('aria-checked', 'true');
});

test('the share picture gives rows to as many tools as asked, the ones that used the most, and folds the rest', async ({ page }) => {
  await open(page);
  // In the picture's own terms: the tools listed, in homepage order, and 其他工具 holding what Summary counts of the rest.
  const posters = await page.evaluate(async () => {
    const { backend } = await import('/src/lib/backend.ts');
    const { poster } = await import('/src/lib/sharePoster.ts');
    const { dashboard } = await backend.snapshot();
    const result: Record<number, { names: string[]; other: string | null; members: string[] }> = {};
    for (const n of [1, 3, 6]) {
      const p = await poster(dashboard, '#000', n);
      const other = p.tools.find((tool) => tool.id === 'other');
      result[n] = { names: p.tools.filter((tool) => tool.id !== 'other').map((tool) => tool.name), other: other?.cost ?? null, members: other?.members.map((m) => m.name) ?? [] };
    }
    return result;
  });
  expect(posters[1].names).toEqual(['Claude']);
  expect(posters[1].other).toBe('$9.13');
  expect(posters[3].names).toEqual(['Magpie', 'ChatGPT', 'Claude']);
  expect(posters[3].members).toEqual(['Cursor', 'Gemini CLI', 'OpenCode', 'Qoder']);
  expect(posters[6].names).toHaveLength(6);
  expect(posters[6].members).toEqual(['Qoder']);
  // On the page: 3 until another is picked, one click redraws, and the choice is remembered.
  await button(page, '分享用量图').click();
  const preview = page.locator('.frame img');
  await expect(page.getByRole('radio', { name: '3', exact: true })).toHaveAttribute('aria-checked', 'true');
  await expect(preview).toBeVisible();
  const tall = await preview.evaluate((img: HTMLImageElement) => img.naturalHeight);
  await page.getByRole('radio', { name: '1', exact: true }).click();
  await expect.poll(() => preview.evaluate((img: HTMLImageElement) => img.naturalHeight)).toBeLessThan(tall);
  await page.keyboard.press('Escape');
  await button(page, '分享用量图').click();
  await expect(page.getByRole('radio', { name: '1', exact: true })).toHaveAttribute('aria-checked', 'true');
});

/** Whether the tour's lit area lies over the first element the selector finds. */
async function lit(page: Page, selector: string) {
  return page.evaluate((selector) => {
    const hole = document.querySelector('.guide .hole')?.getBoundingClientRect(), part = document.querySelector(selector)?.getBoundingClientRect();
    if (!hole || !part) return false;
    return hole.left <= part.left + 1 && hole.top <= part.top + 1 && hole.right >= part.right - 1 && hole.bottom >= part.bottom - 1;
  }, selector);
}

test('新手指引 lights up each part of the homepage in turn, Esc skips it, and 外观与设置 shows it again', async ({ page }) => {
  // In a browser only when asked; the app shows it the first time the panel opens.
  await open(page);
  await expect(page.locator('.guide')).toHaveCount(0);
  await open(page, '?guide');
  const guide = page.locator('.guide');
  await expect(guide.locator('h2')).toHaveText('欢迎使用 Starmeter');
  await expect(guide.locator('.hole')).toHaveCount(0);
  await button(page, '开始').click();
  const stops: [string, string][] = [['用量总览', '.heatmap'], ['统计范围', '.summary .picker'], ['每个工具一张卡', '.cards .tabs'],
    ['订阅额度和 API 余额', '.rows .row'], ['三个按钮', '.summary .buttons .slot']];
  for (const [i, [title, part]] of stops.entries()) {
    await expect(guide.locator('h2')).toHaveText(title);
    await expect(guide.locator('.count')).toHaveText(`${i + 1} / ${stops.length}`);
    await expect.poll(() => lit(page, part)).toBe(true);
    // The parts under the dim take no clicks while the tour is on: the gear leaves the page as it is.
    if (i === stops.length - 1) {
      const gear = (await button(page, '主页模块与设置').boundingBox())!;
      await page.mouse.click(gear.x + gear.width / 2, gear.y + gear.height / 2);
      await expect(page.locator('.dashboard')).toBeVisible();
    }
    await page.keyboard.press(i === 0 ? 'ArrowRight' : 'Enter');
    if (i === 0) { await expect(guide.locator('h2')).toHaveText(stops[1][0]); await page.keyboard.press('ArrowLeft'); await expect(guide.locator('h2')).toHaveText(title); await button(page, '下一步').click(); }
  }
  await expect(guide).toHaveCount(0);
  expect(await page.evaluate(() => localStorage.getItem('starmeter.guide'))).toBe('1');
  // Again from 外观与设置 → 帮助: back on the homepage; Esc skips it there without leaving the homepage.
  await button(page, '主页模块与设置').click();
  await page.getByText('外观与设置').first().click();
  await page.getByRole('button', { name: '新手指引' }).click();
  await expect(guide.locator('h2')).toHaveText('欢迎使用 Starmeter');
  await expect(page.locator('.dashboard')).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(guide).toHaveCount(0);
  await expect(page.locator('.dashboard')).toBeVisible();
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
  // End goes to the list's last range and leaves the page where it is; the arrows walk the list round.
  await page.keyboard.press('End');
  await expect(page.getByRole('option', { name: '全部' })).toBeInViewport();
  expect(await viewport.evaluate((node) => node.scrollTop)).toBe(before);
  await page.keyboard.press('ArrowDown');
  await page.keyboard.press('ArrowUp');
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
