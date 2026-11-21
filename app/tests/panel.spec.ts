// The panel against the fixtures: what each page shows and how its controls answer, so a regression in the port shows
// up before anyone opens the app.
import { readFileSync } from 'node:fs';
import { expect, test, type Page } from '@playwright/test';
import { PHONE_KEY, servePhone } from './phone';

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

test('the range last picked is the one kept, however quickly the picks follow each other', async ({ page }) => {
  await open(page);
  /** The range the core reads now, as the next snapshot will carry it. */
  const reading = () => page.evaluate(async () => { const { backend } = await import('/src/lib/backend.ts'); return (await backend.snapshot()).rangeOptions?.find((o) => o.checked)?.action?.[1]; });
  const picker = page.locator('.picker .current');
  // Back to the range on screen before the new one's figures arrive: still a change.
  await choose(page, /统计范围/, '近 7 天');
  await choose(page, /统计范围/, '今天');
  await expect(picker).toHaveText('今天');
  await page.waitForTimeout(600);
  await expect(picker).toHaveText('今天');
  expect(await reading()).toBe('today');
  // Away and back while the first is still on its way: the last pick wins.
  for (const gap of [0, 60, 150, 320]) {
    await choose(page, /统计范围/, '近 30 天');
    await page.waitForTimeout(gap);
    await choose(page, /统计范围/, '昨天');
    await page.waitForTimeout(gap);
    await choose(page, /统计范围/, '近 30 天');
    await expect(picker).toHaveText('近 30 天');
    await page.waitForTimeout(600);
    await expect(picker).toHaveText('近 30 天');
    expect(await reading()).toBe('30d');
    await choose(page, /统计范围/, '今天');
    await page.waitForTimeout(600);
  }
});

test('the homepage shows Summary, the trend, the heatmap, the cards and the rows', async ({ page }) => {
  await open(page);
  await expect(page.locator('.heading')).toHaveText('Starmeter');
  await expect(page.locator('.figure-label').first()).toHaveText('已知 API 估算');
  await expect(page.locator('.legend')).toHaveCount(8);
  await expect(page.locator('.chart path')).not.toHaveCount(0);
  await expect(page.locator('.squares .cell')).toHaveCount(24);
  // The tools that used the most first (as the models and projects are): the costliest, then tokens, then Credits.
  await expect(page.locator('.card .name')).toHaveText(['Claude', 'ChatGPT', 'Magpie', 'Cursor', 'Gemini CLI', 'OpenCode', 'Qoder']);
  await expect(page.getByRole('button', { name: /^其余 \d+ 个/ })).toHaveCount(0);
  await expect(page.locator('.card-note').filter({ hasText: '次已由 Magpie 计入' })).toBeVisible();
  await expect(page.locator('.row .title')).toContainText(['ChatGPT 订阅', 'Claude 订阅', 'Cursor 订阅']);
  // Usage rows carry their 3pt bar, coloured by level.
  await expect(page.locator('.gauge')).toHaveCount(4);
});

test('header buttons take clicks, and pinning turns the trend live', async ({ page }) => {
  await open(page);
  // No refresh or auto-detect in the header: opening reads what is due, pinning reads every few seconds, and
  // auto-detect lives in 通用.
  await expect(button(page, '刷新数据')).toHaveCount(0);
  await expect(button(page, '自动检测')).toHaveCount(0);
  await button(page, '钉住面板').click();
  await expect(page.getByLabel('实时用量图')).toBeVisible();
  await expect(button(page, '取消钉住面板')).toBeVisible();
  // Pinned, the grabber and the corner say it moves and resizes; no line of text spells it out.
  await expect(page.locator('.grabber')).toBeVisible();
  await expect(page.getByRole('separator', { name: '拖动调整大小' })).toBeVisible();
  await expect(page.getByText('拖动顶部移动面板')).toHaveCount(0);
  // Not in front, the shell says when the pointer comes in and goes out (not each move): in, they stay; out, they fade.
  await page.evaluate(() => window.dispatchEvent(new CustomEvent('panel-hover', { detail: true })));
  await page.waitForTimeout(3000);
  await expect(page.locator('.grabber')).not.toHaveClass(/faded/);
  await page.evaluate(() => window.dispatchEvent(new CustomEvent('panel-hover', { detail: false })));
  await expect(page.locator('.grabber')).toHaveClass(/faded/);
  // Pinned, the panel turns to 会话 and fits down to the end of the third session; the rest is below, to scroll to or
  // to make the panel taller for.
  await expect(page.getByRole('radio', { name: '会话' })).toHaveAttribute('aria-checked', 'true');
  await expect(page.locator('.session')).toHaveCount(5);
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

test('a request a gateway counts adds to the live line, however long its prompt and short its answer', async ({ page }) => {
  await open(page);
  const seen = await page.evaluate(async () => {
    const { backend } = await import('/src/lib/backend.ts');
    const { LiveFeed } = await import('/src/lib/live.ts');
    const today = (await backend.snapshot()).dashboard!;
    // Logged by Magpie and by ChatGPT both: Magpie counts it, ChatGPT's counted part stays as it was.
    const later = { ...today, providers: today.providers.map((p) => (p.id === 'codex' || p.id === 'magpie' ? { ...p, tokens: p.tokens + 9000, output: p.output + 10 } : p)) };
    const feed = new LiveFeed(), t = Date.now();
    feed.add(today, t);
    feed.add(later, t + 2000);
    return { carried: today.providers.find((p) => p.id === 'codex')?.counted !== undefined, added: feed.samples.at(-1)!.total - feed.samples[0].total,
      rate: feed.rateAt(t + 2000) ?? 0 };
  });
  expect(seen.carried).toBe(true);
  expect(seen.added).toBe(9000);
  expect(seen.rate).toBeCloseTo(10 / 60, 5);
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

test('会话 sits beside the groupings: running first, what each has cost, five shown and the rest behind 查看更多', async ({ page }) => {
  await open(page);
  // Its row leaves the other rows for the tab.
  await expect(page.locator('.row .title', { hasText: /^会话$/ })).toHaveCount(0);
  await page.getByRole('radio', { name: '会话' }).click();
  const cards = page.locator('.session');
  await expect(cards).toHaveCount(5);
  await expect(cards.first().locator('.title')).toHaveText('重做设置页的分段控件');
  // On the right, what the whole session has cost; this turn's tokens and cost under its state.
  await expect(cards.first().locator('.value')).toHaveText('$6.90');
  await expect(cards.first()).toContainText('运行中 4 分钟');
  await expect(cards.first()).toContainText('本轮 2.41M Token · $0.92 · 9 次请求');
  await expect(page.locator('.session-list')).not.toContainText('等待输入');
  await expect(cards.nth(3)).toContainText('空闲 6 分钟');
  await expect(cards.nth(3)).toContainText('上一轮 640.2K Token · $0.12');
  // The rest are one click away, and then all there, to scroll to.
  await page.getByRole('button', { name: '查看更多 2 个会话' }).click();
  await expect(cards).toHaveCount(7);
  await expect(page.getByRole('button', { name: /查看更多/ })).toHaveCount(0);
  await expect(cards.last()).toContainText('空闲 5 小时');
  // Two sessions of the same name are still two: opening one opens only it.
  await expect(cards.nth(5).locator('.title')).toHaveText('修好 CI 的缓存');
  await expect(cards.nth(6).locator('.title')).toHaveText('修好 CI 的缓存');
  await cards.nth(6).locator('.head').click();
  await expect(cards.nth(6).locator('.turn')).toHaveCount(1);
  await expect(cards.nth(5).locator('.turn')).toHaveCount(0);
  await cards.nth(6).locator('.head').click();
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

test('a question under its row goes with its page: back is 取消, and nothing is left at the foot', async ({ page }) => {
  await open(page);
  await button(page, '主页模块与设置').click();
  await page.getByText('外观', { exact: true }).click();
  await page.getByText('汇率…').click();
  await expect(page.locator('.ask .field')).toBeFocused();
  // Back by the button: the question is gone, from its row and from the foot.
  await button(page, '返回').click();
  await expect(page.locator('.modules')).toBeVisible();
  await expect(page.locator('.ask')).toHaveCount(0);
  await expect(page.locator('.ask-sheet')).toHaveCount(0);
  // And the action it belonged to is over: the same row asks again, afresh.
  await page.getByText('外观', { exact: true }).click();
  await page.getByText('汇率…').click();
  await expect(page.locator('.ask .field')).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.locator('.ask')).toHaveCount(0);
  await expect(page.getByText('汇率…')).toBeVisible();
});

test('a click on ChatGPT 订阅 opens its page at once, also right after the panel opens (HOME-1)', async ({ page }) => {
  const heading = page.getByRole('heading', { level: 2, name: 'ChatGPT 订阅', exact: true });
  const row = page.locator('.home button.row').filter({ hasText: 'ChatGPT 订阅' });
  // The row's click as soon as the dashboard shows, the panel perhaps still 400pt tall and its first reading on the
  // way: the page opens and stays. Nothing about the panel's opening takes a chosen page back.
  await page.goto('/?mac');
  await expect(page.locator('.dashboard')).toBeVisible();
  await row.dispatchEvent('click');
  await expect(heading).toBeVisible();
  await page.waitForTimeout(1500);
  await expect(heading).toBeVisible();
  // A pointer's click, each time the panel opens afresh: one click on the row opens its page, without a retry. The
  // panel shows 400pt tall and grows to fit the homepage a frame later; the app's window is sized before it is shown,
  // so the click waits for that fit here, as a person's could only come after it.
  for (let time = 0; time < 5; time++) {
    await page.goto('/?mac');
    await expect(page.locator('.dashboard')).toBeVisible();
    await expect(page.locator('.window')).not.toHaveCSS('height', '400px');
    await row.click();
    await expect(heading).toBeVisible({ timeout: 1000 });
    await page.waitForTimeout(300);
    await expect(heading).toBeVisible();
  }
});

test('figures that come in while a row is pressed wait for its click, so the row stays under the pointer (HOME-1)', async ({ page }) => {
  await open(page);
  await expect(page.locator('.window')).not.toHaveCSS('height', '400px');
  const row = page.locator('.home button.row').filter({ hasText: 'ChatGPT 订阅' });
  const heading = page.getByRole('heading', { level: 2, name: 'ChatGPT 订阅', exact: true });
  const top = async () => (await row.boundingBox())!.y;
  const full = await top();
  // The homepage drawn short first (each tool's card with one model), as a reading still filling in draws it; the
  // next reading, the full one, is asked for and held back until let go.
  await page.evaluate(async () => {
    // The panel's own copy of the module (a dev server that has reloaded it serves it under a new address).
    const loaded = performance.getEntriesByType('resource').map((entry) => entry.name).filter((name) => name.includes('/src/lib/backend.ts'));
    const { backend } = await import(/* @vite-ignore */ loaded.at(-1) ?? '/src/lib/backend.ts');
    (window as unknown as { backend: typeof backend }).backend = backend;
    const real = backend.snapshot;
    const w = window as unknown as { short: boolean; gate: Promise<void> | null; letGo: () => void };
    w.short = true;
    w.gate = null;
    backend.snapshot = async () => {
      const answer = await real();
      if (w.short && answer.dashboard) for (const card of [...answer.dashboard.providers, ...(answer.dashboard.cards ?? [])]) card.models = card.models?.slice(0, 1);
      if (w.gate) await w.gate;
      return answer;
    };
    await backend.action(['set-group', 'tool']);
  });
  await expect.poll(top).toBeLessThan(full - 40);
  const short = await top();
  await page.evaluate(async () => {
    const w = window as unknown as { short: boolean; gate: Promise<void> | null; letGo: () => void };
    w.short = false;
    w.gate = new Promise((resolve) => (w.letGo = resolve));
    await (window as unknown as { backend: { action: (action: string[]) => Promise<unknown> } }).backend.action(['set-group', 'tool']);
  });
  // A person's click on the row: the full figures come in between the press and the release.
  const box = (await row.boundingBox())!;
  expect(box.y).toBe(short);
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.down();
  await page.evaluate(() => (window as unknown as { letGo: () => void }).letGo());
  await page.waitForTimeout(150);
  await page.mouse.up();
  await expect(heading).toBeVisible({ timeout: 1000 });
  // The click done, the figures it waited for are on the homepage.
  await page.keyboard.press('Escape');
  await expect.poll(top).toBe(full);
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

test('grouping by model or project swaps the cards, five of them and the rest behind 查看更多', async ({ page }) => {
  await open(page);
  await page.getByRole('radio', { name: '模型' }).click();
  await expect(page.getByRole('radio', { name: '模型' })).toHaveAttribute('aria-checked', 'true');
  await expect(page.locator('.card .name').first()).toHaveText('Claude Opus 5.5');
  await expect(page.locator('.card .name')).toContainText(['Claude Sonnet 5.5']);
  await expect(page.locator('.card')).toHaveCount(5);
  await page.getByRole('button', { name: '查看更多 10 个模型' }).click();
  await expect(page.locator('.card')).toHaveCount(15);
  await expect(page.getByRole('button', { name: /查看更多/ })).toHaveCount(0);
  // Nothing folded into “其他”: every model has its card, and the list ends where the last one does.
  await expect(page.locator('.card .name', { hasText: '其他' })).toHaveCount(0);
  const last = (await page.locator('.card').last().boundingBox())!;
  const list = (await page.locator('.dashboard').boundingBox())!;
  expect(Math.abs(list.y + list.height - (last.y + last.height + 4))).toBeLessThan(2);
  await page.getByRole('radio', { name: '项目' }).click();
  await expect(page.locator('.tabs .note')).toHaveText('按客户端自己的记录');
  await expect(page.locator('.card')).toHaveCount(5);
  await expect(page.locator('.card .name').first()).toHaveText('starmeter');
  await page.getByRole('button', { name: '查看更多 1 个项目' }).click();
  await expect(page.locator('.card .name').last()).toHaveText('未标注项目');
  // Another grouping starts from five again.
  await page.getByRole('radio', { name: '模型' }).click();
  await expect(page.locator('.card')).toHaveCount(5);
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
  const key = page.locator('.detail-row', { has: page.getByText('demo', { exact: true }) });
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
  // The settings are pages by what they are for, a group titled 设置 like the modules' below: 外观, 通用, 提醒, 数据,
  // then the phone; each cell an icon and a name, as tall as the others.
  await expect(page.locator('.modules .section').first()).toHaveText('设置');
  expect(await page.locator('.modules .cell').evaluateAll((cells) => cells.slice(0, 5).map((c) => c.getAttribute('aria-label'))))
    .toEqual(['外观', '通用', '提醒', '数据', '在手机上看']);
  expect(new Set(await page.locator('.modules .cell').evaluateAll((cells) => cells.slice(0, 5).map((c) => c.getBoundingClientRect().height)))).toEqual(new Set([32]));
  await page.getByText('外观', { exact: true }).click();
  await page.getByRole('radio', { name: '深色' }).click();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  await page.keyboard.press('Escape');
  await page.getByText('提醒', { exact: true }).click();
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

test('全局快捷键 in 通用: off at first, a choice is kept, one already taken says so and keeps the old', async ({ page }) => {
  // 通用, with the row unfolded to its choices.
  const settings = async () => {
    await button(page, '主页模块与设置').click();
    await page.getByText('通用', { exact: true }).click();
    await page.getByRole('button', { name: /^全局快捷键/ }).click();
  };
  await open(page);
  await settings();
  const row = page.locator('.detail-row', { hasText: '全局快捷键' }).first();
  await expect(row).toContainText('不使用');
  await page.getByRole('radio', { name: '⌃⌥S', exact: true }).click();
  await expect(page.getByRole('radio', { name: '⌃⌥S', exact: true })).toHaveAttribute('aria-checked', 'true');
  expect(await page.evaluate(() => localStorage.getItem('starmeter.shortcut'))).toBe('ctrl-alt-s');
  // Held elsewhere: it says so where it was asked, and the choice stays as it was.
  await open(page, '?shortcut-taken');
  await settings();
  await expect(page.getByRole('radio', { name: '⌃⌥S', exact: true })).toHaveAttribute('aria-checked', 'true');
  await page.getByRole('radio', { name: '⌃⌥Space', exact: true }).click();
  await expect(page.getByText('快捷键已被占用')).toBeVisible();
  await expect(page.getByRole('radio', { name: '⌃⌥S', exact: true })).toHaveAttribute('aria-checked', 'true');
  expect(await page.evaluate(() => localStorage.getItem('starmeter.shortcut'))).toBe('ctrl-alt-s');
  // Taken by the time the panel loads again: the choice is kept, and the row says it does nothing for now.
  await expect(row).toContainText('当前被占用');
  // Off is always to be had.
  await page.getByRole('radio', { name: '不使用', exact: true }).click();
  await expect(row).not.toContainText('当前被占用');
  await expect(page.getByRole('radio', { name: '不使用', exact: true })).toHaveAttribute('aria-checked', 'true');
});

test('在手机上看: a code for the same network, a public link only after a yes, and 停止 turns it off', async ({ page }) => {
  await open(page);
  await button(page, '主页模块与设置').click();
  // A cell of its own in the module grid, beside the settings.
  const row = page.getByRole('button', { name: /^在手机上看/ });
  await expect(row.locator('.dot')).toHaveCount(0);
  await row.click();
  const phone = page.locator('.phone-page');
  await expect(phone.locator('svg[role="img"]')).toBeVisible();
  // The address without its key, which stays in the code; on the same network no time limit, until 停止.
  await expect(phone.locator('.address')).toHaveText('http://192.168.1.20:51234/');
  await expect(phone.locator('.note').last()).toContainText('一直开着');
  await expect(phone.locator('.note')).not.toContainText(['分钟']);
  // Any network: asked first, nothing opened until the yes, and no note about a link not made yet.
  await page.getByRole('radio', { name: '任何网络' }).click();
  await expect(phone.locator('.ask')).toContainText('经 Cloudflare 生成外网链接？');
  // Said before the yes: the link doesn't open in parts of mainland China, and the same network is the steadier way.
  await expect(phone.locator('.ask')).toContainText('中国大陆部分地区打不开 Cloudflare 的链接');
  await expect(phone.locator('svg[role="img"]')).toHaveCount(0);
  await expect(phone.locator('.note')).toHaveCount(0);
  await button(page, '取消').click();
  await expect(page.getByRole('radio', { name: '同一个网络' })).toHaveAttribute('aria-checked', 'true');
  await page.getByRole('radio', { name: '任何网络' }).click();
  await button(page, '生成链接').click();
  await expect(phone.locator('.address')).toHaveText('https://quiet-river-1234.trycloudflare.com/');
  // The public link has its hour.
  await expect(phone.locator('.note').first()).toContainText('还能用 59 分钟');
  await button(page, '关闭外网链接').click();
  await expect(phone.locator('.address')).toHaveText('http://192.168.1.20:51234/');
  // Left while on: the cell has a green dot, as a module set up has; 停止 turns it off.
  await page.keyboard.press('Escape');
  await expect(page.getByRole('button', { name: /^在手机上看/ }).locator('.dot')).toBeVisible();
  await page.getByRole('button', { name: /^在手机上看/ }).click();
  // The code in the logo's look: dots, the three rounded eyes and the app icon in the middle.
  const code = phone.locator('svg.qr');
  await expect(code.locator('image')).toHaveCount(1);
  expect(await code.locator('circle').count()).toBeGreaterThan(100);
  // 停止 stays on the page: the code's place says it has stopped, and 重新开始 is the way on.
  await button(page, '停止').click();
  await expect(phone.getByRole('status')).toContainText('已停止');
  await expect(phone.locator('svg.qr')).toHaveCount(0);
  await expect(phone.locator('.address')).toHaveCount(0);
  await expect(phone.locator('.note')).toHaveText(['原来的二维码和链接已失效。点“重新开始”生成新的二维码，手机再扫一次就能看。']);
  await expect(page.getByRole('radio', { name: '任何网络' })).toBeDisabled();
  await expect(button(page, '停止')).toHaveCount(0);
  await button(page, '重新开始').click();
  await expect(phone.locator('svg.qr')).toBeVisible();
  await expect(phone.locator('.address')).toHaveText('http://192.168.1.20:51234/');
  await expect(button(page, '停止')).toBeVisible();
  // Stopped and left: the cell has no dot.
  await button(page, '停止').click();
  await page.keyboard.press('Escape');
  await expect(page.getByRole('button', { name: /^在手机上看/ }).locator('.dot')).toHaveCount(0);
});

/** The panel's own copy of the backend module (a dev server that has reloaded it serves it under a new address), with
 *  its shell: a test swaps one of the shell's calls on it. */
const panelShell = `(async () => {
  const loaded = performance.getEntriesByType('resource').map((entry) => entry.name).filter((name) => name.includes('/src/lib/backend.ts'));
  return (await import(/* @vite-ignore */ loaded.at(-1) ?? '/src/lib/backend.ts')).shell;
})()`;

test('在手机上看: a public link still being made when 停止 is pressed leaves the next start alone (PHONE-3)', async ({ page }) => {
  await open(page);
  await button(page, '主页模块与设置').click();
  await page.getByRole('button', { name: /^在手机上看/ }).click();
  const phone = page.locator('.phone-page');
  await expect(phone.locator('svg.qr')).toBeVisible();
  // The public link takes its time, and answers as the host does once the server it was for has stopped.
  await page.evaluate(async (source) => {
    const shell = await eval(source);
    const w = window as unknown as { letGo: () => void };
    shell.phonePublic = () => new Promise((_, refuse) => (w.letGo = () => refuse('手机查看已经关闭')));
  }, panelShell);
  await page.getByRole('radio', { name: '任何网络' }).click();
  await button(page, '生成链接').click();
  await expect(phone.locator('.wait')).toHaveText('正在向 Cloudflare 要链接…');
  await button(page, '停止').click();
  await button(page, '重新开始').click();
  // The new round's code, on the same network, without the old request's wait.
  await expect(phone.locator('svg.qr')).toBeVisible();
  await expect(phone.locator('.wait')).toHaveCount(0);
  await expect(page.getByRole('radio', { name: '同一个网络' })).toHaveAttribute('aria-checked', 'true');
  // The old request answers late: nothing of it shows beside the new code.
  await page.evaluate(() => (window as unknown as { letGo: () => void }).letGo());
  await page.waitForTimeout(200);
  await expect(phone.locator('.failed')).toHaveCount(0);
  await expect(phone.locator('svg.qr')).toBeVisible();
  await expect(phone.locator('.address')).toHaveText('http://192.168.1.20:51234/');
});

test('在手机上看: a start that fails once says why and offers 重试, which starts it (PHONE-4)', async ({ page }) => {
  await open(page);
  await button(page, '主页模块与设置').click();
  await page.getByRole('button', { name: /^在手机上看/ }).click();
  const phone = page.locator('.phone-page');
  await expect(phone.locator('svg.qr')).toBeVisible();
  // The next start fails once (its port taken for a moment), then works.
  await page.evaluate(async (source) => {
    const shell = await eval(source);
    const real = shell.phoneStart;
    let once = true;
    shell.phoneStart = () => (once ? ((once = false), Promise.reject('这台电脑暂时开不了端口，稍后再试')) : real());
  }, panelShell);
  await button(page, '停止').click();
  await button(page, '重新开始').click();
  await expect(phone.getByRole('status')).toContainText('没能开启');
  await expect(phone.locator('.failed')).toHaveText('这台电脑暂时开不了端口，稍后再试');
  await button(page, '重试').click();
  await expect(phone.locator('svg.qr')).toBeVisible();
  await expect(phone.locator('.failed')).toHaveCount(0);
  await expect(button(page, '停止')).toBeVisible();
});

test('在手机上看: a public link that fails goes back to the same network and says why, until the next try', async ({ page }) => {
  await open(page, '?tunnel-fails');
  await button(page, '主页模块与设置').click();
  await page.getByRole('button', { name: /^在手机上看/ }).click();
  const phone = page.locator('.phone-page');
  await page.getByRole('radio', { name: '任何网络' }).click();
  await button(page, '生成链接').click();
  await expect(phone.locator('.failed')).toContainText('Cloudflare 这次没有连上');
  await expect(page.getByRole('radio', { name: '同一个网络' })).toHaveAttribute('aria-checked', 'true');
  await expect(phone.locator('svg[role="img"]')).toBeVisible();
  // Trying again starts afresh: the question, without the last failure under it.
  await page.getByRole('radio', { name: '任何网络' }).click();
  await expect(phone.locator('.ask')).toBeVisible();
  await expect(phone.locator('.failed')).toHaveCount(0);
});

test('在手机上看: 已复制链接 only once the clipboard took it; refused, the whole link to copy by hand', async ({ page }) => {
  await open(page);
  // The clipboard as the test says: refusing first, then taking it.
  await page.evaluate(() => {
    const state = { refuse: true, text: '' };
    Object.assign(window, { clipboardState: state });
    Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText: (text: string) => (state.refuse ? Promise.reject(new Error('denied')) : Promise.resolve(void (state.text = text))) } });
  });
  await button(page, '主页模块与设置').click();
  await page.getByRole('button', { name: /^在手机上看/ }).click();
  const phone = page.locator('.phone-page'), address = phone.locator('.address');
  await expect(address).toHaveText('http://192.168.1.20:51234/');
  await address.click();
  await expect(address).not.toHaveText('已复制链接');
  const manual = phone.getByRole('textbox', { name: '完整链接' });
  await expect(manual).toHaveValue(/^http:\/\/192\.168\.1\.20:51234\/#k=/);
  await expect(manual).toBeFocused();
  expect(await manual.evaluate((input: HTMLInputElement) => input.selectionEnd! - input.selectionStart!)).toBe(await manual.evaluate((input: HTMLInputElement) => input.value.length));
  await page.evaluate(() => { (window as unknown as { clipboardState: { refuse: boolean } }).clipboardState.refuse = false; });
  await address.click();
  await expect(address).toHaveText('已复制链接');
  await expect(manual).toHaveCount(0);
  expect(await page.evaluate(() => (window as unknown as { clipboardState: { text: string } }).clipboardState.text)).toMatch(/#k=/);
});

test('在手机上看 the first time says cloudflared is downloaded first', async ({ page }) => {
  await open(page, '?no-cloudflared');
  await button(page, '主页模块与设置').click();
  await page.getByRole('button', { name: /^在手机上看/ }).click();
  await page.getByRole('radio', { name: '任何网络' }).click();
  await expect(page.locator('.phone-page .ask')).toContainText('cloudflared');
  await expect(button(page, '下载并生成')).toBeVisible();
});

test('on the phone: the homepage read-only, its figures asked for with the key, a range of its own', async ({ page }) => {
  const asked = await servePhone(page);
  await page.goto('/#k=' + PHONE_KEY);
  await expect(page.locator('.dashboard')).toBeVisible();
  await expect(page.locator('.window.phone')).toBeVisible();
  // No pin or settings on the phone; the range and share are there.
  await expect(button(page, '钉住面板')).toHaveCount(0);
  await expect(button(page, '主页模块与设置')).toHaveCount(0);
  await expect(button(page, '分享用量图')).toBeVisible();
  await expect(page.locator('.card').first()).toBeVisible();
  // The rows read as they do on the computer, their usage bars too, and don't open: their insides stay there.
  const plan = page.locator('button.row', { hasText: 'Claude 订阅' });
  await expect(plan).toBeDisabled();
  await expect(plan).toHaveClass(/tall/);
  await expect(plan.locator('.title')).toHaveAttribute('style', /var\(--label\)/);
  await expect(plan.locator('.gauge')).toBeVisible();
  await expect(page.locator('button.row', { hasText: 'Komari 监控' }).locator('.value')).toHaveAttribute('style', /var\(--label\)/);
  // 会话 is the fourth tab, as on the computer, without the computer's switch.
  await page.getByRole('radio', { name: '会话' }).click();
  await expect(page.getByText('重做设置页的分段控件')).toBeVisible();
  await expect(page.getByText('这一轮完成时提醒')).toHaveCount(0);
  // The cards grouped by model or project on the phone itself: asked for, drawn, and the computer's choice untouched.
  await page.getByRole('radio', { name: '模型' }).click();
  await expect.poll(() => asked.some((path) => path.includes('group=model'))).toBe(true);
  await expect(page.locator('.card .name').first()).toHaveText('Claude Opus 5.5');
  await expect(page.getByRole('radio', { name: '模型' })).toHaveAttribute('aria-checked', 'true');
  await page.getByRole('radio', { name: '项目' }).click();
  await expect(page.locator('.card .name').first()).toHaveText('starmeter');
  await page.getByRole('radio', { name: '工具' }).click();
  await expect(page.locator('.card .name').first()).not.toHaveText('starmeter');
  await page.getByRole('button', { name: /^统计范围/ }).click();
  await page.getByRole('option', { name: '近 7 天' }).click();
  await expect.poll(() => asked.some((path) => path.includes('range=7d'))).toBe(true);
  await expect(page.getByRole('button', { name: /^统计范围/ })).toContainText('近 7 天');
  // The share picture on the phone: drawn from the phone's own range, saved as a download (the browser's way).
  await button(page, '分享用量图').click();
  await expect(page.locator('.share-page .title')).toHaveText('分享用量图近 7 天');
  await expect(page.locator('.share-page .stage img')).toBeVisible();
  await expect(page.getByText('长按图片也能存到相册')).toBeVisible();
  const download = page.waitForEvent('download');
  await page.getByRole('button', { name: '保存图片' }).click();
  // WebKit on Linux hands the name over with its spaces as underscores.
  expect((await download).suggestedFilename()).toMatch(/^Starmeter[ _].+[ _]卡片\.png$/);
  await page.keyboard.press('Escape');
  await expect(page.locator('.dashboard')).toBeVisible();
});

test('on the phone: a slow reading of the range before never takes the page back', async ({ page }) => {
  await servePhone(page);
  await page.goto('/#k=' + PHONE_KEY);
  await expect(page.locator('.dashboard')).toBeVisible();
  // Today's next reading is held back; the week is picked meanwhile and arrives at once.
  let release = () => {};
  const held = new Promise<void>((resolve) => { release = resolve; });
  await page.route('**/api/snapshot', async (route) => { await held; await route.fallback(); });
  await page.evaluate(() => document.dispatchEvent(new Event('visibilitychange')));
  await page.getByRole('button', { name: /^统计范围/ }).click();
  await page.getByRole('option', { name: '近 7 天' }).click();
  await expect(page.locator('.summary .reading')).toHaveCount(0);
  await expect(page.getByRole('button', { name: /^统计范围/ })).toContainText('近 7 天');
  const week = await page.locator('.figure').first().textContent();
  release();
  await page.waitForTimeout(600);
  await expect(page.getByRole('button', { name: /^统计范围/ })).toContainText('近 7 天');
  await expect(page.locator('.figure').first()).toHaveText(week!);
});

test('on the phone: one reading at a time, and a computer out of reach or a stale link said at the foot', async ({ page }) => {
  const asked = await servePhone(page);
  await page.goto('/#k=' + PHONE_KEY);
  await expect(page.locator('.dashboard')).toBeVisible();
  // A slow computer: asking again while a reading is on its way sends nothing more.
  let slow = 0;
  await page.route('**/api/snapshot**', async (route) => { slow++; await new Promise((r) => setTimeout(r, 1500)); await route.fallback(); });
  const before = asked.length;
  for (let i = 0; i < 4; i++) await page.evaluate(() => document.dispatchEvent(new Event('visibilitychange')));
  await expect.poll(() => asked.length).toBe(before + 1);
  expect(slow).toBe(1);
  await page.unroute('**/api/snapshot**');
  // Out of reach: said at the foot, the figures on screen kept.
  await page.route('**/api/snapshot**', (route) => route.abort('connectionrefused'));
  await page.evaluate(() => document.dispatchEvent(new Event('visibilitychange')));
  await expect(page.locator('.phone-problem')).toContainText('连不上电脑');
  await expect(page.locator('.card').first()).toBeVisible();
  await page.unroute('**/api/snapshot**');
  // A key the computer no longer has: scan again.
  await page.route('**/api/snapshot**', (route) => route.fulfill({ status: 403, body: 'key needed' }));
  await page.evaluate(() => document.dispatchEvent(new Event('visibilitychange')));
  await expect(page.locator('.phone-problem')).toContainText('链接已失效');
  await page.unroute('**/api/snapshot**');
  // Cloudflare's answer when the public link has no computer behind it, and any other failure, each in its words.
  await page.route('**/api/snapshot**', (route) => route.fulfill({ status: 530, body: 'error code: 1033' }));
  await page.evaluate(() => document.dispatchEvent(new Event('visibilitychange')));
  await expect(page.locator('.phone-problem')).toHaveText('外网链接连不上电脑：电脑可能睡着了、断了网，或者外网链接已关闭');
  await page.unroute('**/api/snapshot**');
  await page.route('**/api/snapshot**', (route) => route.fulfill({ status: 500, body: '' }));
  await page.evaluate(() => document.dispatchEvent(new Event('visibilitychange')));
  await expect(page.locator('.phone-problem')).toHaveText('电脑暂时没有返回数据（HTTP 500），稍后会自动重试');
  await page.unroute('**/api/snapshot**');
  // Back in reach: the line goes.
  await page.evaluate(() => document.dispatchEvent(new Event('visibilitychange')));
  await expect(page.locator('.phone-problem')).toHaveCount(0);
  // The browser logs the refused requests itself; nothing else may have gone wrong.
  const expected = (line: string) => /Failed to load resource|Could not connect/.test(line);
  errors.splice(0, errors.length, ...errors.filter((line) => !expected(line)));
});

test('the share page: the picture on the left, the settings and actions in a column beside it, level at both ends', async ({ page }) => {
  await open(page, '?mac');
  await button(page, '分享用量图').click();
  await expect(page.locator('.stage img')).toBeVisible();
  const box = async (selector: string) => (await page.locator(selector).first().boundingBox())!;
  const layout = async () => {
    const stage = await box('.stage'), picture = await box('.stage img'), rule = await box('.share-page .rule');
    const settings = await page.locator('.setting').evaluateAll((all) => all.map((e) => { const r = e.getBoundingClientRect(); return [r.x, r.y, r.width]; }));
    const actions = await page.locator('.action').evaluateAll((all) => all.map((e) => { const r = e.getBoundingClientRect(); return [r.x, r.y, r.width, r.bottom]; }));
    return { stage, picture, rule, settings, actions };
  };
  // Measured once the page has slid in and stopped moving.
  let before = await layout();
  await expect(async () => {
    const now = await layout();
    const still = JSON.stringify(now) === JSON.stringify(before);
    before = now;
    expect(still).toBe(true);
  }).toPass({ intervals: [150] });
  const { stage, picture, rule, settings, actions } = before;
  // The picture whole and centred on its stage.
  expect(Math.abs(picture.x + picture.width / 2 - (stage.x + stage.width / 2))).toBeLessThanOrEqual(1);
  expect(Math.abs(picture.y + picture.height / 2 - (stage.y + stage.height / 2))).toBeLessThanOrEqual(1);
  expect(picture.height).toBeLessThanOrEqual(stage.height);
  expect(picture.width).toBeLessThanOrEqual(stage.width);
  // The stage from the rule's left end, the column to its right end; level at the top with the first setting and at
  // the foot with the last action.
  expect(Math.abs(stage.x - rule.x)).toBeLessThanOrEqual(0.5);
  expect(settings).toHaveLength(4);
  const [column, top, width] = settings[0];
  expect(Math.abs(column + width - (rule.x + rule.width))).toBeLessThanOrEqual(0.5);
  expect(Math.abs(top - stage.y)).toBeLessThanOrEqual(0.5);
  expect(Math.abs(actions[actions.length - 1][3] - (stage.y + stage.height))).toBeLessThanOrEqual(0.5);
  // One under another, all of them on the column's edges: copy first and strongest, then save and share.
  for (const [x, , w] of [...settings, ...actions]) {
    expect(Math.abs(x - column)).toBeLessThanOrEqual(0.5);
    expect(Math.abs(w - width)).toBeLessThanOrEqual(0.5);
  }
  expect(actions).toHaveLength(3);
  for (let i = 1; i < actions.length; i++) expect(actions[i][1]).toBeGreaterThan(actions[i - 1][3]);
  await expect(page.locator('.action').first()).toHaveClass(/primary/);
  await expect(page.locator('.action').first()).toContainText('复制图片');
  // Fewer tools change the picture and nothing around it.
  await button(page, '少一个').click();
  await button(page, '少一个').click();
  await expect(async () => {
    const after = await layout();
    expect(after.stage).toEqual(stage);
    expect(after.settings).toEqual(settings);
    expect(after.actions).toEqual(actions);
    expect(after.picture.width / after.picture.height).not.toBeCloseTo(picture.width / picture.height, 2);
  }).toPass();
});

test('a folded card unfolds where it is, and its arrow opens the tool', async ({ page }) => {
  await open(page);
  await button(page, '主页模块与设置').click();
  // 允许折叠 is a switch in 外观.
  await page.getByText('外观', { exact: true }).click();
  await page.getByRole('switch', { name: /^允许折叠/ }).click();
  await page.keyboard.press('Escape');
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
  await page.locator('.share-page .stage img').waitFor();
  // As tall as the picture beside its controls, not the 300 the panel was sized to.
  await expect.poll(async () => (await page.locator('.window').boundingBox())!.height).toBeGreaterThan(330);
  await page.keyboard.press('Escape');
  await expect(page.locator('.window')).toHaveCSS('height', '300px');
  await expect(page.locator('.window')).toHaveCSS('width', '400px');
});

test('the share picture\'s cache share is the tools\' cache reads over their cache-eligible input', async ({ page }) => {
  await open(page);
  const cache = await page.evaluate(async () => {
    const { backend } = await import('/src/lib/backend.ts');
    const { poster } = await import('/src/lib/sharePoster.ts');
    const d = (await backend.snapshot()).dashboard!;
    const [a, b] = d.providers;
    // Each 100 tokens of cache-eligible input, one all read from the cache, one none; the first also wrote a lot.
    const one = { ...a, counted: undefined, summaryIncluded: true, available: true, tokens: 10_000, output: 9900, cache: 1, cacheInputs: 100 };
    const two = { ...b, counted: undefined, summaryIncluded: true, available: true, tokens: 100, output: 0, cache: 0, cacheInputs: 100 };
    return (await poster({ ...d, providers: [one, two] }, '#000')).cache;
  });
  expect(cache).toBe('50.0%');
});

test('leaving the share page before its picture is drawn leaves no picture in memory', async ({ page }) => {
  // Every object URL made and released, counted on the page.
  await page.addInitScript(() => {
    const live = new Set<string>();
    const make = URL.createObjectURL.bind(URL), drop = URL.revokeObjectURL.bind(URL);
    URL.createObjectURL = (object: Blob | MediaSource) => { const url = make(object); live.add(url); return url; };
    URL.revokeObjectURL = (url: string) => { live.delete(url); drop(url); };
    Object.assign(window, { liveUrls: live });
    // A slow encoder, so the page is left while the picture is still being made.
    const encode = HTMLCanvasElement.prototype.toBlob;
    HTMLCanvasElement.prototype.toBlob = function (this: HTMLCanvasElement, ...args: Parameters<HTMLCanvasElement['toBlob']>) {
      setTimeout(() => encode.apply(this, args), 800);
    };
  });
  await open(page);
  const held = () => page.evaluate(() => (window as unknown as { liveUrls: Set<string> }).liveUrls.size);
  const before = await held();
  for (let i = 0; i < 2; i++) {
    await button(page, '分享用量图').click();
    await page.keyboard.press('Escape');
    await expect(page.locator('.share-page')).toHaveCount(0);
  }
  await page.waitForTimeout(2500);
  await expect.poll(held).toBe(before);
  // Chosen again while the page is open, a picture drawn meanwhile is kept, and released when the page goes.
  await button(page, '分享用量图').click();
  await expect(page.locator('.share-page .stage img')).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.locator('.share-page')).toHaveCount(0);
  await expect.poll(held).toBe(before);
  // A style picked, another, and the first again before either is drawn: the two drawings of the first are one too
  // many, and the spare goes.
  await button(page, '分享用量图').click();
  await expect(page.locator('.share-page .stage img')).toBeVisible();
  for (const style of ['报刊', '卡片', '报刊']) {
    await page.getByRole('button', { name: /^风格/ }).click();
    await page.getByRole('option', { name: style, exact: true }).click();
  }
  await page.waitForTimeout(3000);
  await page.keyboard.press('Escape');
  await expect(page.locator('.share-page')).toHaveCount(0);
  await expect.poll(held).toBe(before);
});

test('back on a range seen before, the share page draws its figures as they are now', async ({ page }) => {
  await open(page);
  await button(page, '分享用量图').click();
  const preview = page.locator('.stage img');
  await expect(preview).toBeVisible();
  const first = await preview.getAttribute('src');
  await page.getByRole('button', { name: /^时间范围/ }).click();
  await page.getByRole('option', { name: '近 7 天', exact: true }).click();
  await expect(page.locator('.share-page .title')).toHaveText('分享用量图近 7 天');
  // Today grows meanwhile (pinned readings add to it in the fixtures), then today is picked again.
  await page.evaluate(async () => { const { backend } = await import('/src/lib/backend.ts'); await backend.action(['refresh-live']); });
  await page.getByRole('button', { name: /^时间范围/ }).click();
  await page.getByRole('option', { name: '今天', exact: true }).click();
  await expect(page.locator('.share-page .title')).toHaveText('分享用量图今天');
  await expect(page.getByRole('button', { name: '复制图片' })).toBeEnabled();
  await expect(preview).not.toHaveAttribute('src', first!);
  // What it would export is the very picture the page draws when opened afresh, byte for byte.
  const bytes = () => preview.evaluate(async (img: HTMLImageElement) => {
    const data = new Uint8Array(await (await fetch(img.src)).arrayBuffer());
    const sum = await crypto.subtle.digest('SHA-256', data);
    return Array.from(new Uint8Array(sum)).map((b) => b.toString(16).padStart(2, '0')).join('');
  });
  const exported = await bytes();
  await page.keyboard.press('Escape');
  await button(page, '分享用量图').click();
  await expect(page.getByRole('button', { name: '复制图片' })).toBeEnabled();
  expect(await bytes()).toBe(exported);
});

test('a source whose reading ends without figures is not waited for: the share page exports what came (SHARE-4)', async ({ page }) => {
  await open(page, '?pending-fails');
  await button(page, '分享用量图').click();
  const copy = page.getByRole('button', { name: '复制图片' });
  await expect(copy).toBeEnabled();
  // Magpie waits (待同步) as the week's figures come in, then its reading ends unavailable: the rest can go, on this
  // page, without opening it again.
  await page.getByRole('button', { name: /^时间范围/ }).click();
  await page.getByRole('option', { name: '近 7 天', exact: true }).click();
  await expect(page.locator('.share-page .title')).toHaveText('分享用量图近 7 天');
  await expect(copy).toBeEnabled();
});

test('the range menu closes when Tab or Shift+Tab leaves it, or the panel goes away, the choice as it was (MENU-1, MENU-2)', async ({ page }) => {
  await open(page);
  const trigger = page.getByRole('button', { name: /^统计范围/ });
  const menuOpen = () => page.evaluate(async () => (await import('/src/lib/ui.svelte.ts')).panel.menuOpen);
  for (const key of ['Tab', 'Shift+Tab']) {
    await trigger.focus();
    await page.keyboard.press('Enter');
    await expect(trigger).toHaveAttribute('aria-expanded', 'true');
    // The arrows still move through the choices while it is open.
    await page.keyboard.press('ArrowDown');
    await expect(page.locator('.option.lit')).toHaveText('昨天');
    await page.keyboard.press(key);
    await expect(trigger).toHaveAttribute('aria-expanded', 'false');
    await expect(page.getByRole('listbox')).toHaveCount(0);
    await expect(trigger).toContainText('今天');
    // No longer holding the refreshes back.
    expect(await menuOpen()).toBe(false);
  }
  // The panel going away while the menu is open (a click on the desktop hides it): opened again, the menu is folded,
  // the range as it was (MENU-2).
  await trigger.click();
  await expect(trigger).toHaveAttribute('aria-expanded', 'true');
  await page.evaluate(() => window.dispatchEvent(new Event('blur')));
  await expect(trigger).toHaveAttribute('aria-expanded', 'false');
  await expect(trigger).toContainText('今天');
  expect(await menuOpen()).toBe(false);
});

test('the range menu is gone at once when the panel goes away, so it does not flash as the panel opens again (MENU-4)', async ({ page }) => {
  await open(page);
  const trigger = page.getByRole('button', { name: /^统计范围/ });
  // The panel hidden (a click on the desktop): its window loses the focus. The menu is gone in that same moment, not
  // fading out: a hidden window stops the fade halfway, and the rest of it played as the panel opened again.
  await trigger.click();
  await expect(page.getByRole('listbox')).toBeVisible();
  const left = await page.evaluate(async () => {
    window.dispatchEvent(new Event('blur'));
    await Promise.resolve();
    await new Promise((resolve) => setTimeout(resolve, 0));
    return [...document.querySelectorAll('.dropdown .menu')].filter((menu) => getComputedStyle(menu).display !== 'none').length;
  });
  expect(left).toBe(0);
  await expect(trigger).toHaveAttribute('aria-expanded', 'false');
  // The host's closing (it lets go of the focus) does the same.
  await trigger.click();
  await expect(page.getByRole('listbox')).toBeVisible();
  const after = await page.evaluate(async () => {
    (document.activeElement as HTMLElement | null)?.blur();
    await Promise.resolve();
    await new Promise((resolve) => setTimeout(resolve, 0));
    return [...document.querySelectorAll('.dropdown .menu')].filter((menu) => getComputedStyle(menu).display !== 'none').length;
  });
  expect(after).toBe(0);
  // Opened again, it opens as ever; a choice made in the panel still folds it with its short fade.
  await trigger.click();
  await expect(page.getByRole('listbox')).toBeVisible();
  await page.getByRole('option', { name: '昨天' }).click();
  await expect(trigger).toContainText('昨天');
  await expect(page.getByRole('listbox')).toHaveCount(0);
});

test('moving through the range menu with the arrows tells assistive technology which choice is lit (MENU-5)', async ({ page }) => {
  await open(page);
  const trigger = page.getByRole('button', { name: /^统计范围/ });
  const list = page.getByRole('listbox');
  /** The option the list names as its active one. */
  const active = async () => {
    const id = await list.getAttribute('aria-activedescendant');
    return id ? page.locator(`[id="${id}"]`).textContent().then((text) => text?.trim()) : null;
  };
  await trigger.focus();
  await page.keyboard.press('Enter');
  await expect(list).toBeFocused();
  expect(await active()).toBe('今天');
  for (const choice of ['昨天', '近 24 小时', '近 7 天']) {
    await page.keyboard.press('ArrowDown');
    await expect(page.locator('.option.lit')).toHaveText(choice);
    expect(await active()).toBe(choice);
    await expect(list).toBeFocused();
  }
  // The current choice is still the one marked selected; Esc leaves it.
  await expect(page.getByRole('option', { name: '今天' })).toHaveAttribute('aria-selected', 'true');
  await page.keyboard.press('Escape');
  await expect(trigger).toContainText('今天');
  // Return picks the lit one.
  await page.keyboard.press('Enter');
  for (let i = 0; i < 3; i++) await page.keyboard.press('ArrowDown');
  expect(await active()).toBe('近 7 天');
  await page.keyboard.press('Enter');
  await expect(trigger).toContainText('近 7 天');
  // Two menus on one page name their options apart.
  await page.keyboard.press('Enter');
  const ids = await page.locator('.dropdown .option').evaluateAll((all) => all.map((option) => option.id));
  expect(new Set(ids).size).toBe(ids.length);
  await page.keyboard.press('Escape');
});

test('the range menu gives the focus back to its button when Esc or Return closes it, so Return opens it again (MENU-3)', async ({ page }) => {
  await open(page);
  const trigger = page.getByRole('button', { name: /^统计范围/ });
  await trigger.focus();
  await page.keyboard.press('Enter');
  await expect(trigger).toHaveAttribute('aria-expanded', 'true');
  await page.keyboard.press('Escape');
  await expect(trigger).toHaveAttribute('aria-expanded', 'false');
  await expect(trigger).toContainText('今天');
  await expect(trigger).toBeFocused();
  // Return opens it again straight away; the arrows and Return choose 昨天, and the focus is back on the button.
  await page.keyboard.press('Enter');
  await expect(trigger).toHaveAttribute('aria-expanded', 'true');
  await page.keyboard.press('ArrowDown');
  await page.keyboard.press('Enter');
  await expect(trigger).toHaveAttribute('aria-expanded', 'false');
  await expect(trigger).toContainText('昨天');
  await expect(trigger).toBeFocused();
  await page.keyboard.press('Enter');
  await expect(trigger).toHaveAttribute('aria-expanded', 'true');
  await page.keyboard.press('Escape');
  await expect(trigger).toBeFocused();
  // A click on the button closes the menu it opened, and it stays closed.
  await trigger.click();
  await expect(trigger).toHaveAttribute('aria-expanded', 'true');
  await trigger.click();
  await expect(trigger).toHaveAttribute('aria-expanded', 'false');
  await page.waitForTimeout(200);
  await expect(trigger).toHaveAttribute('aria-expanded', 'false');
});

test('the share page picks its range, the homepage\'s own, and draws that range', async ({ page }) => {
  await open(page);
  await button(page, '分享用量图').click();
  const preview = page.locator('.stage img');
  await expect(preview).toBeVisible();
  const today = await preview.getAttribute('src');
  const copy = page.getByRole('button', { name: '复制图片' });
  await expect(copy).toBeEnabled();
  await choose(page, /^时间范围/, '近 7 天');
  // Until the week's picture is drawn, nothing gives away today's under the week's name.
  await expect(copy).toBeDisabled();
  await expect(page.locator('.share-page .title')).toHaveText('分享用量图近 7 天');
  await expect(preview).not.toHaveAttribute('src', today!);
  await expect(copy).toBeEnabled();
  await page.keyboard.press('Escape');
  await expect(page.locator('.picker .current')).toHaveText('近 7 天');
});

test('sharing previews the picture in each style, then copies or saves it', async ({ page }) => {
  await open(page);
  await button(page, '分享用量图').click();
  await expect(page.locator('.share-page .title')).toHaveText('分享用量图今天');
  const preview = page.locator('.stage img');
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
  expect(posters[3].names).toEqual(['Claude', 'ChatGPT', 'Magpie']);
  expect(posters[3].members).toEqual(['Cursor', 'Gemini CLI', 'OpenCode', 'Qoder']);
  expect(posters[6].names).toHaveLength(6);
  expect(posters[6].members).toEqual(['Qoder']);
  // On the page: a stepper, 3 until changed, each click redraws, no further than the tools used (7 here, six at most),
  // and the choice is remembered.
  await button(page, '分享用量图').click();
  const preview = page.locator('.stage img'), count = page.locator('.stepper .count');
  const fewer = button(page, '少一个'), more = button(page, '多一个');
  await expect(count).toHaveText('3');
  await expect(preview).toBeVisible();
  const tall = await preview.evaluate((img: HTMLImageElement) => img.naturalHeight);
  await fewer.click();
  await fewer.click();
  await expect(count).toHaveText('1');
  await expect(fewer).toBeDisabled();
  await expect.poll(() => preview.evaluate((img: HTMLImageElement) => img.naturalHeight)).toBeLessThan(tall);
  for (let i = 0; i < 7; i++) if (await more.isEnabled()) await more.click();
  await expect(count).toHaveText('6');
  await expect(more).toBeDisabled();
  await fewer.click();
  await page.keyboard.press('Escape');
  await button(page, '分享用量图').click();
  await expect(count).toHaveText('5');
  // A range with a single tool: one, and nothing to step to.
  await choose(page, /^时间范围/, '昨天');
  await expect(async () => {
    const used = await page.evaluate(async () => { const { backend } = await import('/src/lib/backend.ts'); return (await backend.snapshot()).dashboard?.providers.filter((p) => p.tokens > 0 || p.requests > 0).length; });
    expect(await count.textContent()).toBe(String(Math.min(5, used ?? 0)));
  }).toPass();
});

/** Whether the tour's lit area lies over the first element the selector finds. */
async function lit(page: Page, selector: string) {
  return page.evaluate((selector) => {
    const hole = document.querySelector('.guide .hole')?.getBoundingClientRect(), part = document.querySelector(selector)?.getBoundingClientRect();
    if (!hole || !part) return false;
    return hole.left <= part.left + 1 && hole.top <= part.top + 1 && hole.right >= part.right - 1 && hole.bottom >= part.bottom - 1;
  }, selector);
}
/** The tour's stops in order, read by walking it with its primary button; the tour is left at its last stop. */
async function walk(page: Page): Promise<string[]> {
  const titles: string[] = [];
  const guide = page.locator('.guide');
  for (let i = 0; i < 10; i++) {
    const title = (await guide.locator('h2').textContent())!;
    titles.push(title);
    const primary = guide.locator('.step.primary');
    if (!(await guide.locator('.skip').count())) break;
    await primary.click();
    await expect(guide.locator('h2')).not.toHaveText(title);
  }
  return titles;
}
const READY_STOPS = ['欢迎使用 Starmeter', '用量总览', '统计范围', '每个工具一张卡', '订阅额度和 API 余额', '三个按钮'];
const seen = (page: Page) => page.evaluate(() => localStorage.getItem('starmeter.guide'));

test.describe('新手指引', () => {
  test('with usage: six stops, each lighting its part, Esc skips, and 通用 shows it again', async ({ page }) => {
    // In a browser only when asked; the app shows it the first time the panel opens.
    await open(page);
    await expect(page.locator('.guide')).toHaveCount(0);
    await open(page, '?guide');
    const guide = page.locator('.guide');
    await expect(guide.locator('h2')).toHaveText('欢迎使用 Starmeter');
    await expect(guide.locator('.hole')).toHaveCount(0);
    await button(page, '开始').click();
    const stops: [string, string][] = [['用量总览', '.heatmap'], ['统计范围', '.summary .picker'], ['每个工具一张卡', '.cards .tabs'],
      ['订阅额度和 API 余额', '.rows .row[data-key="chatgpt"]'], ['三个按钮', '.summary .buttons .slot']];
    for (const [i, [title, part]] of stops.entries()) {
      await expect(guide.locator('h2')).toHaveText(title);
      await expect(guide.locator('.count')).toHaveText(`${i + 1} / ${stops.length}`);
      await expect.poll(() => lit(page, part)).toBe(true);
      if (title === '订阅额度和 API 余额') {
        // The plan rows, not 工具更新 or Komari further down.
        await expect.poll(() => lit(page, '.rows .row[data-key="updates"]')).toBe(false);
        await expect.poll(() => lit(page, '.rows .row[data-key="komari"]')).toBe(false);
      }
      // The parts under the dim take no clicks while the tour is on: the gear leaves the page as it is.
      if (i === stops.length - 1) {
        const gear = (await button(page, '主页模块与设置').boundingBox())!;
        await page.mouse.click(gear.x + gear.width / 2, gear.y + gear.height / 2);
        await expect(page.locator('.dashboard')).toBeVisible();
        await expect(guide.locator('.later')).toContainText('齿轮 → 通用 → 新手指引');
      }
      await page.keyboard.press(i === 0 ? 'ArrowRight' : 'Enter');
      if (i === 0) { await expect(guide.locator('h2')).toHaveText(stops[1][0]); await page.keyboard.press('ArrowLeft'); await expect(guide.locator('h2')).toHaveText(title); await button(page, '下一步').click(); }
    }
    await expect(guide).toHaveCount(0);
    expect(await seen(page)).toBe('1');
    // Again from 通用 → 帮助: back on the homepage; Esc skips it there without leaving the homepage.
    await button(page, '主页模块与设置').click();
    await page.getByText('通用', { exact: true }).click();
    await page.getByRole('button', { name: '新手指引' }).click();
    await expect(guide.locator('h2')).toHaveText('欢迎使用 Starmeter');
    await expect(page.locator('.dashboard')).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(guide).toHaveCount(0);
    await expect(page.locator('.dashboard')).toBeVisible();
  });

  test('while the first readings come: the same stops, and the figures said to appear by themselves', async ({ page }) => {
    await open(page, '?fixture=pending&guide');
    expect(await walk(page)).toEqual(READY_STOPS);
    await button(page, '上一步').click();
    for (let i = 0; i < 3; i++) await button(page, '上一步').click();
    await expect(page.locator('.guide h2')).toHaveText('用量总览');
    await expect(page.locator('.guide .body')).toContainText('第一次读取要一点时间');
  });

  test('with nothing found: it says so, points to the gear, and opens 主页模块 from its last stop', async ({ page }) => {
    await open(page, '?fixture=empty&guide');
    const guide = page.locator('.guide');
    expect(await walk(page)).toEqual(['欢迎使用 Starmeter', '还没有用量', '选择要读的工具']);
    await expect(guide.locator('.count')).toHaveText('2 / 2');
    await expect.poll(() => lit(page, '.summary .buttons > .slot:nth-child(2)')).toBe(true);
    await expect(button(page, '下一步')).toHaveCount(0);
    // 稍后 ends it on the homepage, seen.
    await button(page, '稍后').click();
    await expect(guide).toHaveCount(0);
    await expect(page.locator('.dashboard')).toBeVisible();
    expect(await seen(page)).toBe('1');
    // Shown again, its last button opens the grid where the tools are chosen.
    await open(page, '?fixture=empty&guide');
    await walk(page);
    await button(page, '打开主页模块').click();
    await expect(guide).toHaveCount(0);
    await expect(page.locator('.modules')).toBeVisible();
  });

  /** Summary turned off, then the tour from 通用 (the browser's choices last until the page reloads). */
  async function withoutSummary(page: Page, query: string) {
    await open(page, query);
    await page.evaluate(async () => { const { backend } = await import('/src/lib/backend.ts'); await backend.action(['set-module', 'summary', 'false']); });
    await expect(page.locator('.summary .figure')).toHaveCount(0);
    await button(page, '主页模块与设置').click();
    await page.getByText('通用', { exact: true }).click();
    await page.getByRole('button', { name: '新手指引' }).click();
    await expect(page.locator('.guide h2')).toHaveText('欢迎使用 Starmeter');
  }

  test('with nothing found and Summary off, the words still say so, over the whole panel', async ({ page }) => {
    await withoutSummary(page, '?fixture=empty');
    await button(page, '开始').click();
    await expect(page.locator('.guide h2')).toHaveText('还没有用量');
    await expect(page.locator('.guide .hole')).toHaveCount(0);
  });

  test('with usage and Summary off, the summary stop is left out, the heatmap still there or not', async ({ page }) => {
    await withoutSummary(page, '');
    await expect(page.locator('.heatmap')).toBeVisible();
    expect(await walk(page)).toEqual(READY_STOPS.filter((t) => t !== '用量总览'));
  });

  test('tools going or coming during the tour turn it to the stops that fit', async ({ page }) => {
    await open(page, '?guide');
    await button(page, '开始').click();
    await expect(page.locator('.guide h2')).toHaveText('用量总览');
    const set = (keys: string[], on: boolean) => page.evaluate(async ({ keys, on }) => {
      const { backend } = await import('/src/lib/backend.ts');
      for (const key of keys) await backend.action(['set-module', key, on ? 'true' : 'false']);
    }, { keys, on });
    await set(['magpie', 'codex', 'claude', 'cursor', 'gemini', 'opencode', 'qoder'], false);
    await expect(page.locator('.guide h2')).toHaveText('还没有用量');
    await expect(page.locator('.guide .count')).toHaveText('1 / 2');
    await set(['claude'], true);
    await expect(page.locator('.guide h2')).toHaveText('用量总览');
    await expect(page.locator('.guide .count')).toHaveText('1 / 5');
  });

  test('as the app runs it: once, on the first opening; shown is seen, even closed halfway', async ({ page }) => {
    await open(page, '?firstrun');
    await expect(page.locator('.guide h2')).toHaveText('欢迎使用 Starmeter');
    // Noted as soon as it shows (the app keeps it where both panels read it).
    expect(await seen(page)).toBe('1');
    await button(page, '开始').click();
    // Closed halfway (a new page load is a new opening): it doesn't come back.
    await open(page, '?firstrun');
    await page.waitForTimeout(900);
    await expect(page.locator('.guide')).toHaveCount(0);
    // Asked for again, it shows again.
    await button(page, '主页模块与设置').click();
    await page.getByText('通用', { exact: true }).click();
    await page.getByRole('button', { name: '新手指引' }).click();
    await expect(page.locator('.guide h2')).toHaveText('欢迎使用 Starmeter');
    // Opened on the module grid (right-click 主页模块…), it waits for the homepage.
    await page.evaluate(() => localStorage.removeItem('starmeter.guide'));
    await open(page, '?firstrun&modules').catch(() => {});
    await expect(page.locator('.modules')).toBeVisible();
    await page.waitForTimeout(900);
    await expect(page.locator('.guide')).toHaveCount(0);
  });
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

test('a module\'s checkbox in the grid is a control of its own: Space switches it, Return on the cell opens the settings (GRID-1)', async ({ page }) => {
  await open(page);
  await button(page, '主页模块与设置').click();
  const box = page.getByRole('checkbox', { name: '在主页显示 Claude', exact: true });
  const cell = page.locator('.modules').getByRole('button', { name: 'Claude', exact: true });
  const heading = page.getByRole('heading', { level: 2, name: 'Claude', exact: true });
  await expect(box).toHaveAttribute('aria-checked', 'true');
  // Two controls in the cell's order on screen, both in the keyboard's way: the checkbox, then the cell. (Tab itself
  // goes by the system's keyboard navigation setting; the test's WebKit tabs through form fields only.)
  expect(await cell.locator('xpath=..').evaluate((slot) =>
    [...slot.querySelectorAll('button')].map((b) => `${b.getAttribute('aria-label')}:${b.tabIndex}`))).toEqual(['在主页显示 Claude:0', 'Claude:0']);
  // The keyboard: the checkbox takes the focus, Space switches it and leaves the focus there; Return on the cell opens
  // Claude's settings.
  await box.focus();
  await page.keyboard.press('Space');
  await expect(box).toHaveAttribute('aria-checked', 'false');
  await expect(box).toBeFocused();
  await page.keyboard.press('Space');
  await expect(box).toHaveAttribute('aria-checked', 'true');
  await cell.focus();
  await page.keyboard.press('Enter');
  await expect(heading).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.locator('.modules')).toBeVisible();
  // The mouse keeps both ways in: the box's spot switches, the name opens.
  await box.click();
  await expect(box).toHaveAttribute('aria-checked', 'false');
  await expect(page.locator('.modules')).toBeVisible();
  await cell.locator('.name').click();
  await expect(heading).toBeVisible();
  await page.keyboard.press('Escape');
  await box.click();
  await expect(box).toHaveAttribute('aria-checked', 'true');
  // A module with nothing to set stays one switch, the whole cell.
  const summary = page.locator('.modules').getByRole('button', { name: 'Summary', exact: true });
  const was = await summary.getByRole('checkbox').getAttribute('aria-checked');
  await summary.locator('.name').click();
  await expect(summary.getByRole('checkbox')).toHaveAttribute('aria-checked', was === 'true' ? 'false' : 'true');
  await summary.locator('.name').click();
  await expect(summary.getByRole('checkbox')).toHaveAttribute('aria-checked', was!);
});

test('in the module grid a checkbox, an icon and a name sit on one line, in the columns of the header', async ({ page }) => {
  await open(page);
  await button(page, '主页模块与设置').click();
  const cell = page.getByRole('button', { name: 'Claude', exact: true });
  const toggle = page.getByRole('checkbox', { name: '在主页显示 Claude', exact: true });
  const box = (await cell.boundingBox())!, check = (await toggle.locator('.box').boundingBox())!, name = (await cell.locator('.name').boundingBox())!;
  expect(Math.abs(check.y + check.height / 2 - (box.y + box.height / 2))).toBeLessThanOrEqual(0.5);
  expect(Math.abs(name.y + name.height / 2 - (box.y + box.height / 2))).toBeLessThanOrEqual(1);
  // The settings' names start in the modules' name column (measured once the page has slid in).
  const setting = page.getByRole('button', { name: '外观', exact: true }), module = page.getByRole('button', { name: 'Summary', exact: true });
  await expect(async () => {
    expect(Math.abs((await setting.locator('.name').boundingBox())!.x - (await module.locator('.name').boundingBox())!.x)).toBeLessThanOrEqual(0.5);
  }).toPass();
});

test('pinned, a press on the grabber or just around it moves the window on every page, scrolled or not (PIN-1)', async ({ page }) => {
  await open(page);
  await button(page, '钉住面板').click();
  const grabber = page.locator('.grabber');
  await expect(grabber).toBeVisible();
  await page.evaluate(async (source) => {
    const shell = await eval(source);
    const w = window as unknown as { drags: number };
    w.drags = 0;
    shell.drag = () => { w.drags++; return Promise.resolve(); };
  }, panelShell);
  const drags = () => page.evaluate(() => (window as unknown as { drags: number }).drags);
  const frame = (await page.locator('.window').boundingBox())!;
  let count = 0;
  const moves = async (x: number, y: number) => {
    await page.mouse.move(frame.x + x, frame.y + y);
    await page.mouse.down();
    await page.mouse.up();
    expect(await drags(), `a press ${y}pt down, ${x}pt in`).toBe(++count);
  };
  // On the grabber (4 to 8pt down), above and below it, at its ends and out at the sides; below, the header's strip.
  for (const y of [1, 4, 6, 8, 10]) for (const x of [12, 165, 180, 195, 348]) await moves(x, y);
  await moves(150, 24);
  // The header's buttons start below it and keep their whole place.
  for (const control of [button(page, '取消钉住面板'), button(page, '主页模块与设置'), button(page, '分享用量图'), page.getByRole('button', { name: /统计范围/ }).first()]) {
    const box = (await control.boundingBox())!;
    expect(await control.evaluate((element, [x, y]) => document.elementFromPoint(x, y)?.closest('button') === element, [box.x + box.width / 2, box.y + 0.5])).toBe(true);
  }
  // A press after one whose release the window drag kept comes without a pointerdown: it still moves the window.
  await page.mouse.move(frame.x + 180, frame.y + 6);
  await page.mouse.down();
  await page.mouse.down();
  await page.mouse.up();
  expect(await drags()).toBe(count += 2);
  await page.mouse.move(frame.x + 150, frame.y + 24);
  await page.mouse.down();
  await page.mouse.down();
  await page.mouse.up();
  expect(await drags()).toBe(count += 2);
  // The homepage scrolled: its header has gone up, and the grabber's place still moves the window.
  const viewport = page.locator('.home .viewport');
  await page.mouse.move(frame.x + 180, frame.y + 300);
  await page.mouse.wheel(0, 110);
  await expect.poll(() => viewport.evaluate((element) => element.scrollTop)).toBeGreaterThan(100);
  await moves(180, 6);
  await moves(60, 10);
  // Faded once the pointer rests, it moves the window all the same.
  await page.evaluate(() => window.dispatchEvent(new CustomEvent('panel-hover', { detail: false })));
  await expect(grabber).toHaveClass(/faded/);
  await moves(180, 6);
  // The share page, by the grabber and by its own strip.
  await button(page, '分享用量图').click();
  await expect(page.locator('.share-page, .share').first()).toBeVisible();
  await moves(180, 6);
  await moves(348, 30);
  await page.keyboard.press('Escape');
  // Unpinned, the top is the page's again and nothing moves the window.
  await button(page, '取消钉住面板').click();
  await expect(page.locator('.strip')).toHaveCount(0);
  await page.mouse.move(frame.x + 180, frame.y + 6);
  await page.mouse.down();
  await page.mouse.up();
  expect(await drags()).toBe(count);
});
