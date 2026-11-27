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
/** After a reload: the homepage drawn and the panel grown to it (it opens 400 high and fits its page a frame later),
 *  still for two frames, so a click aimed low on the page lands where it was aimed (FLAKE-1). */
async function settled(page: Page) {
  await expect(page.locator('.dashboard')).toBeVisible();
  await expect.poll(() => page.locator('.window').evaluate(async (box) => {
    const first = box.getBoundingClientRect().height;
    await new Promise(requestAnimationFrame);
    await new Promise(requestAnimationFrame);
    return first > 400 && box.getBoundingClientRect().height === first;
  })).toBe(true);
}
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
  await expect(page.locator('.heading')).toContainText('Starmeter');
  await expect(page.locator('.figure-label').first()).toHaveText('已知 API 估算');
  await expect(page.locator('.legend')).toHaveCount(8);
  await expect(page.locator('.chart path')).not.toHaveCount(0);
  await expect(page.locator('.squares .cell')).toHaveCount(24);
  // The tools that used the most first (as the models and projects are): the costliest, then tokens, then Credits.
  await expect(page.locator('.card .name')).toHaveText(['Claude', 'ChatGPT', 'Magpie', 'Cursor', 'Gemini', 'OpenCode', 'Qoder']);
  await expect(page.getByRole('button', { name: /^其余 \d+ 个/ })).toHaveCount(0);
  await expect(page.locator('.card-note').filter({ hasText: '次已由 Magpie 计入' })).toBeVisible();
  await expect(page.locator('.row .title')).toContainText(['ChatGPT 订阅', 'Claude 订阅', 'Cursor 订阅']);
  // Usage rows carry their 3pt bar, coloured by level (Gemini 订阅's from Antigravity among them); a plan with more
  // than one limit has a bar per window, side by side, named under it: five hours on the left and the week on the
  // right, Gemini 订阅's two groups a line each, ended by their logos; the row's figure the week's (本周已用).
  await expect(page.locator('.gauge')).toHaveCount(10);
  const plan = (name: string) => page.locator('button.row', { hasText: name });
  const names = (name: string) => plan(name).locator('.gauge-name');
  await expect(plan('ChatGPT 订阅').locator('.value')).toHaveText('本周已用 38%');
  await expect(plan('Claude 订阅').locator('.value')).toHaveText('本周已用 76%');
  await expect(plan('Gemini 订阅').locator('.value')).toHaveText('本周已用 9%');
  await expect(names('ChatGPT 订阅')).toHaveText(['5 小时 12%', '本周 38%']);
  await expect(names('Claude 订阅')).toHaveText(['5 小时 22%', '本周 76%']);
  await expect(names('Gemini 订阅')).toHaveText(['5 小时 24%', '本周 9%', '5 小时 6%', '本周 18%']);
  const marks = plan('Gemini 订阅').locator('.marks');
  expect(await marks.evaluateAll((all) => all.map((one) => [one.getAttribute('aria-label'), one.children.length]))).toEqual([['Gemini', 1], ['Claude 和 GPT', 2]]);
  await expect(plan('ChatGPT 订阅').locator('.marks')).toHaveCount(0);
  await expect(names('Cursor 订阅')).toHaveCount(0);
  // Each bar its own share and colour: the week's orange at 76%, five hours' the accent.
  const claude = await plan('Claude 订阅').locator('.gauge').evaluateAll((bars) => bars.map((bar) => {
    const fill = bar.firstElementChild as HTMLElement;
    return { share: Math.round((fill.getBoundingClientRect().width / bar.getBoundingClientRect().width) * 100), colour: getComputedStyle(fill).backgroundColor };
  }));
  expect(claude.map((bar) => bar.share)).toEqual([22, 76]);
  expect(claude[0].colour).not.toBe(claude[1].colour);
  // Side by side and the same width; every plan's five hours in one column and its week in another, Gemini 订阅's
  // second line too; each line's logos at the right end of its names, under the end of its week's bar.
  const box = (name: string, selector: string) => plan(name).locator(selector).evaluateAll((all) => all.map((one) => one.getBoundingClientRect().toJSON()));
  const columns = async (name: string) => (await box(name, '.gauge')).map((bar) => [Math.round(bar.left), Math.round(bar.right)]);
  const chatgpt = await columns('ChatGPT 订阅');
  expect(chatgpt[0][1] - chatgpt[0][0]).toBe(chatgpt[1][1] - chatgpt[1][0]);
  expect(chatgpt[0][1]).toBeLessThan(chatgpt[1][0]);
  expect(await columns('Claude 订阅')).toEqual(chatgpt);
  expect(await columns('Gemini 订阅')).toEqual([...chatgpt, ...chatgpt]);
  const gemini = await box('Gemini 订阅', '.gauge');
  expect(gemini[2].top).toBeGreaterThan(gemini[0].bottom + 10);
  const ends = await box('Gemini 订阅', '.marks');
  ends.forEach((end, line) => {
    expect(Math.abs(end.right - gemini[line * 2 + 1].right)).toBeLessThan(1);
    expect(end.top).toBeGreaterThan(gemini[line * 2 + 1].bottom);
    expect(end.bottom).toBeLessThan(line ? Infinity : gemini[2].top);
  });
});

test('a homepage row held for a moment drags to another place in its group, a click still opens it, ⌥↑ ⌥↓ move it too', async ({ page }) => {
  await open(page);
  const order = () => page.locator('[data-module]').evaluateAll((all) => all.map((one) => (one as HTMLElement).dataset.module));
  const row = (key: string) => page.locator(`[data-module="${key}"]`);
  const plans = ['chatgpt', 'claude-subscription', 'cursor-subscription', 'gemini-subscription', 'qoder-subscription'];
  const apis = ['chatgpt-api', 'claude-api', 'deepseek', 'kimi-api'];
  expect(await order()).toEqual([...plans, ...apis, 'komari', 'updates']);
  /** Holds a row (or not long enough), then takes it to `y` in steps and lets go. */
  const drag = async (key: string, y: number, hold = 450) => {
    await row(key).scrollIntoViewIfNeeded();
    const box = (await row(key).boundingBox())!;
    await page.mouse.move(box.x + 80, box.y + 10);
    await page.mouse.down();
    await page.waitForTimeout(hold);
    await page.mouse.move(box.x + 80, y, { steps: 12 });
    await page.mouse.up();
  };
  // Held, it lifts; dragged above ChatGPT 订阅 it takes the first place, the others make way, and the release opens nothing.
  const top = (await row('chatgpt').boundingBox())!;
  await row('qoder-subscription').scrollIntoViewIfNeeded();
  const qoder = (await row('qoder-subscription').boundingBox())!;
  await page.mouse.move(qoder.x + 80, qoder.y + 10);
  await page.mouse.down();
  await page.waitForTimeout(450);
  await expect(row('qoder-subscription')).toHaveClass(/lifted/);
  await page.mouse.move(qoder.x + 80, top.y + 4, { steps: 12 });
  await expect(row('chatgpt')).toHaveAttribute('style', /translateY\([1-9]/);
  await page.mouse.up();
  await expect.poll(order).toEqual(['qoder-subscription', 'chatgpt', 'claude-subscription', 'cursor-subscription', 'gemini-subscription', ...apis, 'komari', 'updates']);
  await expect(page.locator('.dashboard')).toBeVisible();
  // A row stays in its group: Kimi API dragged far up stops at the head of the APIs.
  await drag('kimi-api', top.y - 40);
  await expect.poll(order).toEqual(['qoder-subscription', 'chatgpt', 'claude-subscription', 'cursor-subscription', 'gemini-subscription',
    'kimi-api', 'chatgpt-api', 'claude-api', 'deepseek', 'komari', 'updates']);
  // Moved without the hold, it was a scroll or a click: nothing moves.
  await drag('deepseek', top.y, 60);
  await expect.poll(order).toEqual(['qoder-subscription', 'chatgpt', 'claude-subscription', 'cursor-subscription', 'gemini-subscription',
    'kimi-api', 'chatgpt-api', 'claude-api', 'deepseek', 'komari', 'updates']);
  // A plain click still opens the row's page.
  await row('claude-subscription').locator('button').click();
  await expect(page.getByRole('heading', { name: 'Claude 订阅' })).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.locator('.dashboard')).toBeVisible();
  // ⌥↓ on the row in focus moves it one down, and it keeps the focus.
  await row('chatgpt').locator('button').focus();
  await page.keyboard.press('Alt+ArrowDown');
  await expect.poll(order).toEqual(['qoder-subscription', 'claude-subscription', 'chatgpt', 'cursor-subscription', 'gemini-subscription',
    'kimi-api', 'chatgpt-api', 'claude-api', 'deepseek', 'komari', 'updates']);
  await expect(row('chatgpt').locator('button')).toBeFocused();
  await page.keyboard.press('Alt+ArrowUp');
  await expect.poll(order).toEqual(['qoder-subscription', 'chatgpt', 'claude-subscription', 'cursor-subscription', 'gemini-subscription',
    'kimi-api', 'chatgpt-api', 'claude-api', 'deepseek', 'komari', 'updates']);
  // 通用 → 主页顺序 says it is custom now, and puts the default back.
  await button(page, '主页模块与设置').click();
  await page.getByText('通用', { exact: true }).click();
  await expect(page.locator('.detail-row', { hasText: '主页顺序' }).first()).toContainText('自定义');
  await page.getByRole('button', { name: /^主页顺序/ }).click();
  await expect(page.getByText('在主页按住一行，拖到想要的位置', { exact: false })).toBeVisible();
  await page.getByRole('button', { name: '恢复默认顺序' }).click();
  await page.keyboard.press('Escape');
  await page.keyboard.press('Escape');
  await page.keyboard.press('Escape');
  await expect(page.locator('.dashboard')).toBeVisible();
  await expect.poll(order).toEqual([...plans, ...apis, 'komari', 'updates']);
});

test('恢复默认顺序 puts the default order back at once, before the next snapshot comes (ORDER-3)', async ({ page }) => {
  await open(page);
  const order = () => page.locator('[data-module]').evaluateAll((all) => all.map((one) => (one as HTMLElement).dataset.module));
  const defaults = await order();
  await page.locator('[data-module="chatgpt"] button').focus();
  await page.keyboard.press('Alt+ArrowDown');
  await expect.poll(order).not.toEqual(defaults);
  // From here no new snapshot arrives: only what the reset itself answers.
  await page.evaluate(async () => {
    const loaded = performance.getEntriesByType('resource').map((entry) => entry.name).filter((name) => name.includes('/src/lib/backend.ts'));
    const { backend } = await import(/* @vite-ignore */ loaded.at(-1) ?? '/src/lib/backend.ts');
    backend.snapshot = () => new Promise(() => {});
  });
  await button(page, '主页模块与设置').click();
  await page.getByText('通用', { exact: true }).click();
  await page.getByRole('button', { name: /^主页顺序/ }).click();
  await page.getByRole('button', { name: '恢复默认顺序' }).click();
  for (let back = 0; back < 3; back++) await page.keyboard.press('Escape');
  await expect(page.locator('.dashboard')).toBeVisible();
  await expect.poll(order).toEqual(defaults);
});

test('允许折叠 in 外观 stays as switched before the next snapshot comes, and switches back as asked', async ({ page }) => {
  await open(page);
  // From here no new snapshot arrives: only what the switch itself answers.
  await page.evaluate(async () => {
    const loaded = performance.getEntriesByType('resource').map((entry) => entry.name).filter((name) => name.includes('/src/lib/backend.ts'));
    const { backend } = await import(/* @vite-ignore */ loaded.at(-1) ?? '/src/lib/backend.ts');
    backend.snapshot = () => new Promise(() => {});
  });
  await button(page, '主页模块与设置').click();
  await page.getByText('外观', { exact: true }).click();
  const fold = page.getByRole('switch', { name: /^允许折叠/ });
  const before = await fold.getAttribute('aria-checked');
  const after = before === 'true' ? 'false' : 'true';
  await fold.click();
  await expect(fold).toHaveAttribute('aria-checked', after);
  await page.waitForTimeout(700);
  await expect(fold).toHaveAttribute('aria-checked', after);
  await fold.click();
  await expect(fold).toHaveAttribute('aria-checked', before ?? 'false');
  await page.waitForTimeout(700);
  await expect(fold).toHaveAttribute('aria-checked', before ?? 'false');
});

test('a press let go off its row before the hold ends never lifts it, and the next click opens the row (ORDER-1)', async ({ page }) => {
  await open(page);
  const row = page.locator('[data-module="chatgpt"]');
  for (let round = 0; round < 3; round++) {
    const box = (await row.boundingBox())!;
    // Pressed just inside its left edge, moved a little off it (less than a drag's start), let go out there.
    await page.mouse.move(box.x + 1, box.y + box.height / 2);
    await page.mouse.down();
    await page.mouse.move(box.x - 3, box.y + box.height / 2);
    await page.mouse.up();
    await page.waitForTimeout(500);
    await expect(page.locator('.slot.lifted')).toHaveCount(0);
    await expect(page.locator('.rows.dragging')).toHaveCount(0);
  }
  // The very next click opens it.
  await row.locator('button').click();
  await expect(page.getByRole('heading', { name: 'ChatGPT 订阅' })).toBeVisible();
});

test('a row dragged while the homepage scrolls drops where it is let go (ORDER-2)', async ({ page }) => {
  await page.setViewportSize({ width: 376, height: 600 });
  await open(page);
  const order = () => page.locator('[data-module]').evaluateAll((all) => all.map((one) => (one as HTMLElement).dataset.module));
  const row = (key: string) => page.locator(`[data-module="${key}"]`);
  const scroll = () => page.evaluate(() => document.querySelector('.home .viewport')!.scrollTop);
  await row('chatgpt').scrollIntoViewIfNeeded();
  const before = await scroll();
  const chatgpt = (await row('chatgpt').boundingBox())!;
  const gemini = (await row('gemini-subscription').boundingBox())!;
  await page.mouse.move(chatgpt.x + 80, chatgpt.y + 10);
  await page.mouse.down();
  await page.waitForTimeout(450);
  await expect(row('chatgpt')).toHaveClass(/lifted/);
  // The wheel scrolls the homepage 100 px under the held row; the row stays under the pointer.
  await page.mouse.wheel(0, 100);
  await expect.poll(scroll).toBeGreaterThan(before + 90);
  const scrolled = (await scroll()) - before;
  const held = (await row('chatgpt').boundingBox())!;
  expect(Math.abs(held.y - chatgpt.y)).toBeLessThan(2);
  // Taken past where Gemini 订阅's middle is on screen now, and let go: it lands after Gemini 订阅.
  const past = gemini.y - scrolled + gemini.height / 2 + (chatgpt.height / 2 - 10) + 12;
  await page.mouse.move(chatgpt.x + 80, past, { steps: 12 });
  await page.mouse.up();
  await expect.poll(order).toEqual(['claude-subscription', 'cursor-subscription', 'gemini-subscription', 'chatgpt', 'qoder-subscription',
    'chatgpt-api', 'claude-api', 'deepseek', 'kimi-api', 'komari', 'updates']);
  // Drawn again from the saved settings (into a page and back), the order is the same.
  await row('claude-subscription').locator('button').click();
  await expect(page.getByRole('heading', { name: 'Claude 订阅' })).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.locator('.dashboard')).toBeVisible();
  expect((await order()).slice(0, 5)).toEqual(['claude-subscription', 'cursor-subscription', 'gemini-subscription', 'chatgpt', 'qoder-subscription']);
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
  // Pinned, the panel turns to 实时: what the AI is doing now.
  await expect(page.getByRole('radio', { name: '实时' })).toHaveAttribute('aria-checked', 'true');
  await expect(page.locator('.panel-live')).toBeVisible();
  // 会话 picked while pinned stays, and fits down to the end of the third session; the rest is below, to scroll to or
  // to make the panel taller for.
  await page.getByRole('radio', { name: '会话' }).click();
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
  // Unpinned, 会话 stays: picked while pinned, it is the user's.
  await expect(page.getByRole('radio', { name: '会话' })).toHaveAttribute('aria-checked', 'true');
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

test('pinned, the live chart draws a line for each tool, in Token and in Token/s', async ({ page }) => {
  test.setTimeout(60_000);
  await open(page);
  await button(page, '钉住面板').click();
  const canvas = page.locator('canvas.live');
  const read = () => canvas.evaluate((c) => ({ ...(c as HTMLElement).dataset }));
  // The fixtures add to several tools at each pinned reading: each gets its own line beside the total's.
  await expect.poll(async () => ((await read()).lines ?? '').split(',').filter(Boolean).length, { timeout: 20_000 }).toBeGreaterThan(1);
  await page.getByRole('radio', { name: 'Token/s' }).click();
  // Each tool's rate line (the Token lines above say nothing of them).
  await expect.poll(async () => Object.keys(JSON.parse((await read()).toolRates ?? '{}')).length, { timeout: 20_000 }).toBeGreaterThan(1);
  // The tools' rates make up the total's (once the top right has written its rate: after the pinning's zoom).
  await expect.poll(async () => (await read()).rate, { timeout: 10_000 }).toBeDefined();
  const seen = await read();
  const rates = Object.values(JSON.parse(seen.toolRates ?? '{}') as Record<string, number>);
  const sum = rates.reduce((a, b) => a + b, 0);
  expect(Math.abs(sum - Number(seen.rate))).toBeLessThanOrEqual(1 + rates.length * 0.1);
  await page.getByRole('radio', { name: 'Token', exact: true }).click();
});

test('pinned, tools far apart in size put the live chart on a log scale, every line clear of the bottom', async ({ page }) => {
  test.setTimeout(60_000);
  // Claude adds 400 times what Magpie does at each reading.
  await open(page, '?live-grow=claude:400000,codex:30000,magpie:1000');
  await button(page, '钉住面板').click();
  const canvas = page.locator('canvas.live');
  const read = () => canvas.evaluate((c) => ({ ...(c as HTMLElement).dataset }));
  await expect.poll(async () => (await read()).scale, { timeout: 20_000 }).toBe('log');
  await expect.poll(async () => { const lines = ((await read()).lines ?? '').split(','); return ['claude', 'codex', 'magpie'].every((id) => lines.includes(id)); }, { timeout: 20_000 }).toBe(true);
  await page.getByRole('radio', { name: 'Token/s' }).click();
  await expect.poll(async () => Object.values(JSON.parse((await read()).toolRates ?? '{}') as Record<string, number>).filter((r) => r > 0).length, { timeout: 20_000 }).toBe(3);
  expect((await read()).scale).toBe('log');
  await page.getByRole('radio', { name: 'Token', exact: true }).click();
});

test('pinned, every tool on the homepage has its line, one not used in the last minute barely there along the bottom with no dot', async ({ page }) => {
  test.setTimeout(60_000);
  // Only Claude adds anything; the homepage's other tools sit idle.
  await open(page, '?live-grow=claude:20000');
  await button(page, '钉住面板').click();
  const canvas = page.locator('canvas.live');
  const read = () => canvas.evaluate((c) => ({ ...(c as HTMLElement).dataset }));
  // In Token (Token/s is what a pinned panel shows first, as 网页版 does).
  await page.getByRole('radio', { name: 'Token', exact: true }).click();
  await expect.poll(async () => Number((await read()).added ?? 0), { timeout: 20_000 }).toBeGreaterThan(0);
  const lines = ((await read()).lines ?? '').split(',').filter(Boolean);
  expect(lines).toContain('claude');
  expect(lines.length).toBeGreaterThan(2);
  // Claude, adding all of it, is the total: its line and its dot would lie on the total's, so it draws neither (it read
  // as a dot trailing behind). The idle tools have no dot: dots only at the ends of lines that are moving. (Its line
  // eases up over a couple of seconds after a reading: waited for.)
  await expect.poll(async () => (await read()).under ?? '', { timeout: 10_000 }).toBe('claude');
  const ends = Object.values(JSON.parse((await read()).ends ?? '{}') as Record<string, [number, number]>);
  expect(ends.length).toBe(0);
  // In Token/s too: a line for each, the idle ones at zero.
  await page.getByRole('radio', { name: 'Token/s' }).click();
  await expect.poll(async () => Object.keys(JSON.parse((await read()).toolRates ?? '{}')).length, { timeout: 20_000 }).toBe(lines.length);
  // The rate eases in over 15 seconds (liveLines.ts): Claude's comes up from 0 in the moments after a reading lands, so
  // it is waited for, not read once (read just after a reading it was still 0.0 on a busy machine); the others stay 0.
  const writing = async () => Object.entries(JSON.parse((await read()).toolRates ?? '{}') as Record<string, number>).filter(([, r]) => r > 0).map(([id]) => id);
  await expect.poll(writing, { timeout: 20_000 }).toEqual(['claude']);
  await page.getByRole('radio', { name: 'Token', exact: true }).click();
});

test('pinned, tools of a like size keep the live chart on a straight scale', async ({ page }) => {
  test.setTimeout(60_000);
  await open(page);
  await button(page, '钉住面板').click();
  const canvas = page.locator('canvas.live');
  await expect.poll(async () => (await canvas.evaluate((c) => (c as HTMLElement).dataset.lines ?? '')).split(',').filter(Boolean).length, { timeout: 20_000 }).toBeGreaterThan(1);
  expect(await canvas.evaluate((c) => (c as HTMLElement).dataset.scale)).toBe('linear');
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
  expect(seen.rate).toBeCloseTo(10 / 10, 5);
});

test('Token/s uses exponential decay over RATE_TAU instead of dividing by pin time or holding a flat 60s plateau', async ({ page }) => {
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
    // Peaks immediately at 4000 / 10s = 400 Token/s (not 4000 / 2s = 2000), then smoothly decays once idle.
    return [feed.rateAt(t + 2000) ?? 0, feed.rateAt(t + 12_000) ?? 0, feed.rateAt(t + 32_000) ?? 0, feed.rateAt(t + 100_000) ?? 0];
  });
  expect(Math.round(rates[0])).toBe(400);
  expect(Math.round(rates[1])).toBe(Math.round(400 * Math.exp(-1)));
  expect(Math.round(rates[2])).toBe(Math.round(400 * Math.exp(-3)));
  expect(rates[3]).toBe(0);
});

test('会话 sits beside the groupings: running first, what each cost in the range, five shown and the rest behind 查看更多', async ({ page }) => {
  await open(page);
  // Its row leaves the other rows for the tab.
  await expect(page.locator('.row .title', { hasText: /^会话$/ })).toHaveCount(0);
  await page.getByRole('radio', { name: '会话' }).click();
  const cards = page.locator('.session');
  await expect(cards).toHaveCount(5);
  await expect(cards.first().locator('.title')).toHaveText('重做设置页的分段控件');
  // On the right, what the session cost in the range shown (today); this turn's tokens and cost under its state.
  await expect(cards.first().locator('.value')).toHaveText('$6.90');
  // Written as the tool cards write their figures: the theme's rounded digits, the $ smaller.
  const figure = cards.first().locator('.value .figure');
  expect(await figure.evaluate((e) => getComputedStyle(e).fontFamily)).toContain('ui-rounded');
  expect(await figure.locator('.sign').evaluate((e) => parseFloat(getComputedStyle(e).fontSize))).toBeLessThan(
    await figure.evaluate((e) => parseFloat(getComputedStyle(e).fontSize)),
  );
  await expect(cards.first()).toContainText('运行中 4 分钟');
  await expect(cards.first()).toContainText('本轮 2.41M Token · $0.92 · 9 次请求');
  await expect(page.locator('.session-list')).not.toContainText('等待输入');
  await expect(cards.nth(3)).toContainText('空闲 6 分钟');
  await expect(cards.nth(3)).toContainText('上一轮 640.2K Token · $0.12');
  // The rest are one click away, and then all there, to scroll to.
  await page.getByRole('button', { name: '查看更多 2 个会话' }).click();
  await expect(cards).toHaveCount(7);
  await expect(page.getByRole('button', { name: /查看更多/ })).toHaveCount(0);
  await expect(cards.last()).toContainText('空闲 3 小时');
  // Changed today, nothing spent: the figure is there, drawn quieter than one that has spent.
  await expect(cards.last().locator('.value')).toHaveText('$0.00');
  const weight = (card: typeof cards) => card.locator('.value .figure').evaluate((e) => Number(getComputedStyle(e).fontWeight));
  expect(await weight(cards.last())).toBeLessThan(await weight(cards.first()));
  // Two sessions of one name are told apart by when each began, and are still two: opening one opens only it.
  await expect(cards.nth(5).locator('.title')).toHaveText('修好 CI 的缓存 · 13:02');
  await expect(cards.nth(6).locator('.title')).toHaveText('修好 CI 的缓存 · 11-20 21:10');
  await cards.nth(6).locator('.head').click();
  await expect(cards.nth(6).locator('.turn')).toHaveCount(1);
  await expect(cards.nth(5).locator('.turn')).toHaveCount(0);
  // Opened, a card's turns are headed by what the whole session has cost.
  await expect(cards.nth(6).locator('.turn-heading')).toHaveText('harbor-api · 最近 1 轮 · 累计 $3.12');
  await cards.nth(6).locator('.head').click();
  // Under the list, which sessions are listed (those active in the range) and what the figures count.
  await expect(page.locator('.session-list .empty')).toHaveText('今天活动过的会话 · 金额只算今天');
  // A card opens its last turns.
  await cards.first().locator('.head').click();
  await expect(cards.first().locator('.turn')).toHaveCount(3);
  await expect(cards.first().locator('.turn-heading')).toHaveText('starmeter · 最近 3 轮 · 累计 $6.90');
  await expect(page.getByRole('switch', { name: /这一轮完成时提醒/ })).toBeVisible();
  // Not kept for the next opening: the panel opens on the homepage's 工具.
  await page.reload();
  await expect(page.getByRole('radio', { name: '工具' })).toHaveAttribute('aria-checked', 'true');
});

test('opened again, the panel is on the homepage with its cards by tool, wherever it was left; a tab picked holds while it is open', async ({ page }) => {
  await open(page);
  const tab = (name: string) => page.getByRole('radiogroup', { name: '卡片分组' }).getByRole('radio', { name, exact: true });
  /** Closed (a click elsewhere, Esc, the star) and opened again from the star. */
  const reopen = async () => {
    await page.evaluate(() => window.dispatchEvent(new CustomEvent('panel-closed')));
    await page.waitForTimeout(150);
    await page.evaluate(() => window.dispatchEvent(new CustomEvent('panel-opened', { detail: { modules: false, maxHeight: 900 } })));
  };
  const onTools = async (where: string) => {
    await expect(page.locator('.dashboard'), where).toBeVisible();
    await expect(page.locator('.page'), where).toHaveCount(0);
    await expect(tab('工具'), where).toHaveAttribute('aria-checked', 'true');
    await expect(page.locator('.list .card .name').first(), where).toHaveText('Claude');
  };
  // Left on 会话.
  await tab('会话').click();
  await expect(tab('会话')).toHaveAttribute('aria-checked', 'true');
  await reopen();
  await onTools('会话');
  // Left on 项目 (the computer's grouping, in its preferences): set back to 工具.
  await tab('项目').click();
  await expect(tab('项目')).toHaveAttribute('aria-checked', 'true');
  await expect(page.locator('.list .card .name').first()).not.toHaveText('Claude');
  await reopen();
  await onTools('项目');
  // Left on a row's page, and in the settings.
  await page.locator('.row', { hasText: 'ChatGPT 订阅' }).click();
  await expect(page.locator('.page h2.title')).toHaveText('ChatGPT 订阅');
  await reopen();
  await onTools('a row\'s page');
  await button(page, '主页模块与设置').click();
  await expect(page.locator('.modules')).toBeVisible();
  await reopen();
  await expect(page.locator('.modules')).toHaveCount(0);
  await onTools('the settings');
  // Picked while it is open, a tab holds: 模型 stays 模型 for as long as the panel is open.
  await tab('模型').click();
  await page.waitForTimeout(2500);
  await expect(tab('模型')).toHaveAttribute('aria-checked', 'true');
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
  expect(edge - (await right('.panel-live .clock'))).toBeLessThan(16);
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
  // Nothing folded into “其他”: every model has its card, and the list ends with 收起 under the last one.
  await expect(page.locator('.card .name', { hasText: '其他' })).toHaveCount(0);
  const last = (await page.locator('.card').last().boundingBox())!;
  const fold = (await page.getByRole('button', { name: '收起' }).boundingBox())!;
  const list = (await page.locator('.dashboard').boundingBox())!;
  expect(Math.abs(fold.y - (last.y + last.height))).toBeLessThan(2);
  expect(Math.abs(list.y + list.height - (fold.y + fold.height + 6 + 4))).toBeLessThan(2);
  await page.getByRole('radio', { name: '项目' }).click();
  await expect(page.locator('.tabs .note')).toHaveText('按客户端自己的记录');
  await expect(page.locator('.card')).toHaveCount(5);
  await expect(page.locator('.card .name').first()).toHaveText('starmeter');
  await page.getByRole('button', { name: '查看更多 1 个项目' }).click();
  await expect(page.locator('.card .name').last()).toHaveText('未标注项目');
  // Each grouping keeps what was opened in it: 模型 shown whole stays whole when it comes back.
  await page.getByRole('radio', { name: '模型' }).click();
  await expect(page.locator('.card')).toHaveCount(15);
});

/** The grouping's answers slowed (190 ms, as Codex measured them; 0 for their usual pace; 'held' until letGroups, so
 *  clicks made meanwhile all come before any answer however busy the machine: FLAKE-1), every action noted in
 *  window.asked. */
async function slowGroups(page: Page, slow: number | 'held' = 190) {
  await page.evaluate(async (slow) => {
    const loaded = performance.getEntriesByType('resource').map((entry) => entry.name).filter((name) => name.includes('/src/lib/backend.ts'));
    const { backend } = await import(/* @vite-ignore */ loaded.at(-1) ?? '/src/lib/backend.ts');
    const real = backend.action;
    const w = window as unknown as { asked: string[][]; letGroups: () => void; holdGroups: () => void };
    w.asked = [];
    let gate = Promise.resolve(), release = () => {};
    w.holdGroups = () => { gate = new Promise<void>((resolve) => (release = resolve)); };
    w.letGroups = () => release();
    if (slow === 'held') w.holdGroups();
    backend.action = async (action: string[]) => {
      w.asked.push(action);
      if (action[0] === 'set-group' && slow === 'held') await gate;
      else if (action[0] === 'set-group' && slow) await new Promise((resolve) => setTimeout(resolve, slow));
      return real(action);
    };
  }, slow);
}
/** The grouping's answers kept by slowGroups(page, 'held') let go; holdGroups keeps the next ones again. */
const letGroups = (page: Page) => page.evaluate(() => (window as unknown as { letGroups: () => void }).letGroups());
const holdGroups = (page: Page) => page.evaluate(() => (window as unknown as { holdGroups: () => void }).holdGroups());
/** Every frame for the time given: which grouping tab is lit, and the first card's name. */
const frames = (page: Page, ms: number) => page.evaluate(async (ms) => {
  const seen: { lit: string; card: string }[] = [];
  const until = performance.now() + ms;
  while (performance.now() < until) {
    seen.push({ lit: document.querySelector('.tabs [aria-checked="true"]')?.textContent ?? '',
      card: document.querySelector('.card .name')?.textContent?.trim() ?? '' });
    await new Promise(requestAnimationFrame);
  }
  return seen;
}, ms);

test('the grouping clicked last is the one kept, a click back to the one on screen among them (GROUP-PICK)', async ({ page }) => {
  await open(page);
  await slowGroups(page, 'held');
  await choose(page, /统计范围/, '近 7 天');
  // 模型, then back to 工具 before 模型's answer comes, while 近 7 天 is being read.
  await page.getByRole('radio', { name: '模型' }).click();
  await page.getByRole('radio', { name: '工具' }).click();
  await expect(page.getByRole('radio', { name: '工具' })).toHaveAttribute('aria-checked', 'true');
  await letGroups(page);
  await page.waitForTimeout(1200);
  await expect(page.getByRole('radio', { name: '工具' })).toHaveAttribute('aria-checked', 'true');
  // The cards are the tools', and the last grouping asked is 工具.
  await expect(page.getByRole('button', { name: 'Claude 详情' })).toBeVisible();
  const asked = await page.evaluate(() => (window as unknown as { asked: string[][] }).asked.filter((a) => a[0] === 'set-group').map((a) => a[1]));
  expect(asked.at(-1)).toBe('tool');
});

test('the grouping lit is the one clicked last, from the click on, and the cards before it are out of reach (GROUP-FLASH)', async ({ page }) => {
  await open(page, '?slow-ranges');
  await slowGroups(page, 'held');
  // Every frame from the click on 项目, the answers let go: which tab is lit.
  await page.getByRole('radio', { name: '模型' }).click();
  await page.getByRole('radio', { name: '项目' }).click();
  const lit = await page.evaluate(async () => {
    (window as unknown as { letGroups: () => void }).letGroups();
    const seen: string[] = [];
    const until = performance.now() + 1200;
    while (performance.now() < until) {
      seen.push(document.querySelector('.tabs [aria-checked="true"]')?.textContent ?? '');
      await new Promise(requestAnimationFrame);
    }
    return seen;
  });
  expect(new Set(lit)).toEqual(new Set(['项目']));
  await expect(page.locator('.card .name').first()).toHaveText('starmeter');
  // 近 7 天, 模型, then at once the first card: the tool card still on screen is held, its page doesn't open.
  await page.getByRole('radio', { name: '工具' }).click();
  await expect(page.getByRole('button', { name: 'Claude 详情' })).toBeVisible();
  // 近 7 天 read slowly (?slow-ranges): read through before the click, its taller heatmap moved the cards down under a
  // click aimed at the old place, which landed on 会话 instead (FLAKE-1).
  await choose(page, /统计范围/, '近 7 天');
  await page.getByRole('radio', { name: '模型' }).click();
  await expect(page.locator('.list')).toHaveAttribute('inert', '');
  await page.locator('.list .card').first().click({ force: true });
  await page.waitForTimeout(600);
  await expect(page.getByRole('heading', { name: 'Claude', exact: true })).toHaveCount(0);
  await expect(page.getByRole('radio', { name: '模型' })).toHaveAttribute('aria-checked', 'true');
  await expect(page.locator('.card .name').first()).toHaveText('Claude Opus 5.5');
});

test('at their usual pace, a grouping picked while a range is read is kept to the end, the last click its own (GROUP-PICK)', async ({ page }) => {
  await open(page);
  await slowGroups(page, 0);
  // 模型 answers at once, while 近 7 天 is still read (the cards on screen still today's tools).
  await choose(page, /统计范围/, '近 7 天');
  await page.getByRole('radio', { name: '模型' }).click();
  const before = await frames(page, 120);
  expect(new Set(before.map((frame) => frame.lit))).toEqual(new Set(['模型']));
  await page.getByRole('radio', { name: '工具' }).click();
  const after = await frames(page, 1200);
  expect(new Set(after.map((frame) => frame.lit))).toEqual(new Set(['工具']));
  await expect(page.getByRole('button', { name: 'Claude 详情' })).toBeVisible();
  const asked = await page.evaluate(() => (window as unknown as { asked: string[][] }).asked.filter((a) => a[0] === 'set-group').map((a) => a[1]));
  expect(asked).toEqual(['model', 'tool']);
});

test('at their usual pace, the tool cards of a range still read are out of reach once 模型 is picked (GROUP-FLASH)', async ({ page }) => {
  // 近 7 天 read slowly, so the cards stay where they are under the click (FLAKE-1); 模型 answers at once.
  await open(page, '?slow-ranges');
  await slowGroups(page, 0);
  await choose(page, /统计范围/, '近 7 天');
  await page.getByRole('radio', { name: '模型' }).click();
  await page.waitForTimeout(60);
  await expect(page.locator('.list')).toHaveAttribute('inert', '');
  await page.locator('.list .card').first().click({ force: true });
  await page.waitForTimeout(800);
  await expect(page.getByRole('heading', { name: 'Claude', exact: true })).toHaveCount(0);
  await expect(page.getByRole('radio', { name: '模型' })).toHaveAttribute('aria-checked', 'true');
  await expect(page.locator('.card .name').first()).toHaveText('Claude Opus 5.5');
  await expect(page.locator('.list')).not.toHaveAttribute('inert', '');
});

test('工具, 模型, 项目: the grouping between is never drawn, the tool cards held until the projects\' come (GROUP-FLASH)', async ({ page }) => {
  await open(page);
  await slowGroups(page, 'held');
  const tools = (await page.locator('.card .name').first().textContent())!.trim();
  await page.getByRole('radio', { name: '模型' }).click();
  await page.getByRole('radio', { name: '项目' }).click();
  await letGroups(page);
  const seen = await frames(page, 1200);
  // Every frame the tools' cards or the projects', never the models' between.
  expect([...new Set(seen.map((frame) => frame.card))].filter((card) => card !== tools && card !== 'starmeter')).toEqual([]);
  expect(seen.at(-1)!.card).toBe('starmeter');
  // 工具, 模型, back to 工具: the models' never drawn either.
  await page.getByRole('radio', { name: '工具' }).click();
  await expect(page.locator('.card .name').first()).toHaveText(tools);
  await holdGroups(page);
  await page.getByRole('radio', { name: '模型' }).click();
  await page.getByRole('radio', { name: '工具' }).click();
  await letGroups(page);
  const back = await frames(page, 1000);
  expect(new Set(back.map((frame) => frame.card))).toEqual(new Set([tools]));
});

test('工具, 模型, 工具 with the readings slow to come: the models\' cards are never drawn in between (R7-1)', async ({ page }) => {
  await open(page);
  // 工具 is clicked while 模型's action is on its way (held until both clicks are made); each reading of the whole
  // panel takes 250 ms, as the app's can over IPC. The reading asked once 模型's action is done (after 工具 was clicked,
  // so it counts) answers 模型's cards; 工具's action is then done before the reading after it comes, and its receipt
  // (carrying the preferences, as a module's or a theme's does) has the page look again at once.
  await page.evaluate(async () => {
    const loaded = performance.getEntriesByType('resource').map((entry) => entry.name).filter((name) => name.includes('/src/lib/backend.ts'));
    const { backend } = await import(/* @vite-ignore */ loaded.at(-1) ?? '/src/lib/backend.ts');
    const action = backend.action, snapshot = backend.snapshot, w = window as unknown as { letGroups: () => void };
    const gate = new Promise<void>((resolve) => (w.letGroups = resolve));
    backend.action = async (asked: string[]) => {
      if (asked[0] !== 'set-group') return action(asked);
      await gate;
      const receipt = await action(asked);
      return { ...receipt, preferences: (await snapshot()).preferences };
    };
    backend.snapshot = async () => { const shot = await snapshot(); await new Promise((resolve) => setTimeout(resolve, 250)); return shot; };
  });
  const tools = (await page.locator('.card .name').first().textContent())!.trim();
  await page.getByRole('radio', { name: '模型' }).click();
  await page.getByRole('radio', { name: '工具' }).click();
  await letGroups(page);
  const seen = await frames(page, 2000);
  expect(new Set(seen.map((frame) => frame.card))).toEqual(new Set([tools]));
  expect(new Set(seen.map((frame) => frame.lit))).toEqual(new Set(['工具']));
});

test('the grouping tabs move with the arrow keys, and Tab leaves them (GROUP-KEY)', async ({ page }) => {
  await open(page);
  const tool = page.getByRole('radio', { name: '工具' });
  const model = page.getByRole('radio', { name: '模型' });
  await tool.focus();
  await page.keyboard.press('ArrowRight');
  await expect(model).toHaveAttribute('aria-checked', 'true');
  await expect(model).toBeFocused();
  await expect(page.locator('.card .name').first()).toHaveText('Claude Opus 5.5');
  await page.keyboard.press('ArrowLeft');
  await expect(tool).toHaveAttribute('aria-checked', 'true');
  await expect(tool).toBeFocused();
  await expect(page.getByRole('button', { name: 'Claude 详情' })).toBeVisible();
  // One Tab stop for the group: the next Tab is past it.
  await expect(model).toHaveAttribute('tabindex', '-1');
  await page.keyboard.press('Tab');
  await expect(page.locator('.tabs [role="radio"]:focus')).toHaveCount(0);
});

/** ChatGPT 订阅 on its reserve: the week used up, what is left in Credits as its value, the single bar named under it. */
async function chatgptOnReserve(page: Page) {
  await page.evaluate(async () => {
    const loaded = performance.getEntriesByType('resource').map((entry) => entry.name).filter((name) => name.includes('/src/lib/backend.ts'));
    const { backend } = await import(/* @vite-ignore */ loaded.at(-1) ?? '/src/lib/backend.ts');
    const real = backend.snapshot;
    backend.snapshot = async () => {
      const shot = await real();
      const walk = (entries: { label?: string; module?: string; children?: unknown[] }[]) => {
        for (const entry of entries) {
          if (entry.module === 'chatgpt' && entry.label === 'ChatGPT 订阅') {
            Object.assign(entry, { value: '余 45,934 Credits', usageLevel: 'warning', share: 100, shareLevel: 'critical', detail: '本周已用 100%' });
            delete (entry as { gauges?: unknown }).gauges;
          }
          walk((entry.children ?? []) as typeof entries);
        }
      };
      walk(shot.entries);
      return shot;
    };
    // Something changed, as the engine says it: the panel reads the snapshot again.
    await backend.action(['set-group', 'tool']);
  });
}

test('a plan on its reserve names its single bar 本周已用, as its headline would', async ({ page }) => {
  await open(page);
  await chatgptOnReserve(page);
  const row = page.locator('.row', { hasText: 'ChatGPT 订阅' });
  await expect(row.locator('.value')).toHaveText('余 45,934 Credits');
  await expect(row.locator('.gauge-name')).toHaveText('本周已用 100%');
  await expect(row.locator('.gauge-label')).toHaveText('本周已用');
});

test('a range picked shows once read through, never the old range or a source at a time; the menu reads ahead', async ({ page }) => {
  const asked: string[] = [];
  page.on('console', (message) => { if (message.text().startsWith('[fixtures] action')) asked.push(message.text()); });
  await open(page);
  const tokens = page.locator('.summary .figure').nth(1);
  const today = (await tokens.textContent())!.trim();
  // Opening the range menu has its ranges read ahead.
  await page.getByRole('button', { name: /统计范围/ }).click();
  await expect.poll(() => asked.some((line) => line.includes('prefetch-ranges'))).toBe(true);
  // Watch every frame of the switch: what shows is today's figure, dimmed, until the week's is in whole.
  await page.evaluate(() => {
    const seen: { text: string; reading: boolean; magpie: string }[] = [];
    (window as unknown as { seen: typeof seen }).seen = seen;
    const look = () => {
      const figure = document.querySelectorAll('.summary .figure')[1]?.textContent?.trim() ?? '';
      const reading = document.querySelector('.dashboard')?.classList.contains('reading') ?? false;
      const magpie = [...document.querySelectorAll('.card')].find((card) => card.textContent?.includes('Magpie'))?.textContent ?? '';
      seen.push({ text: figure, reading, magpie });
      requestAnimationFrame(look);
    };
    look();
  });
  await page.getByRole('option', { name: '近 7 天' }).click();
  await expect(page.locator('.dashboard')).toHaveClass(/reading/);
  await expect(page.locator('.note', { hasText: '正在读取…' })).toBeVisible();
  await expect(page.locator('.dashboard')).not.toHaveClass(/reading/, { timeout: 5000 });
  await page.waitForTimeout(900);
  const week = (await tokens.textContent())!.trim();
  expect(week).not.toBe(today);
  const seen = await page.evaluate(() => (window as unknown as { seen: { text: string; reading: boolean; magpie: string }[] }).seen);
  // Before the week's figures: today's (dimmed while reading); after: the week's, rolled in, never one missing Magpie.
  for (const frame of seen) expect(frame.magpie).not.toContain('待同步');
  const first = seen.findIndex((frame) => frame.text !== today);
  expect(seen.slice(0, first).every((frame) => frame.text === today)).toBe(true);
  expect(seen.slice(first).every((frame) => !frame.reading)).toBe(true);
});

test('a newer version shows beside Starmeter, a pill until looked at and then a dot; 提醒 can keep the homepage quiet', async ({ page }) => {
  await open(page);
  const mark = page.locator('.heading .update-mark');
  // A ring with a down arrow in it, no words; on hover and to assistive technology, Starmeter's new version and the
  // tools' together. It opens 工具更新, Starmeter's own row first in it (用户选的方案 A), its page one level down.
  await expect(mark).toHaveClass(/fresh/);
  await expect(mark.locator('svg.ring circle')).toHaveCount(1);
  await expect(mark).toHaveText('');
  await expect(mark).toHaveAttribute('aria-label', 'Starmeter 有新版本 6.0.9 · 3 个工具可更新');
  await mark.click();
  await expect(page.locator('.dashboard')).toHaveCount(0);
  const first = page.locator('.detail-row').first();
  await expect(first).toContainText('Starmeter · 有新版本');
  await expect(first).toContainText('6.0.8 → 6.0.9');
  await first.click();
  await expect(page.getByText('更新到 6.0.9…')).toBeVisible();
  await page.keyboard.press('Escape');
  await page.keyboard.press('Escape');
  // Looked at: a dot, also after the panel is opened again; nothing moves or blinks.
  await expect(mark).not.toHaveClass(/fresh/);
  await expect(mark.locator('.dot')).toHaveCount(1);
  await page.reload();
  await settled(page);
  await expect(page.locator('.heading .update-mark')).not.toHaveClass(/fresh/);
  // In 工具更新, each row with a newer version has its 更新 pill to click, the pills in one column at the right edge
  // whether the row opens an app (no ↗ beside the pill) or runs a command; a command is never broken between words.
  await page.locator('.row', { hasText: '工具更新' }).click();
  const pills = page.locator('.value.pill', { hasText: '更新' });
  await expect(pills).toHaveCount(4);
  const edges = await pills.evaluateAll((all) => all.map((pill) => Math.round(pill.getBoundingClientRect().right)));
  expect(new Set(edges).size).toBe(1);
  const claude = page.locator('.detail-row', { hasText: 'Claude · 有新版本' });
  await expect(claude.locator('svg.out')).toHaveCount(0);
  const run = await page.locator('.detail-row', { hasText: 'Codex · 有新版本' }).locator('.detail').textContent();
  expect(run).toContain('brew\u00a0upgrade\u00a0-\u2060-\u2060cask\u00a0codex');
  await page.keyboard.press('Escape');
  // 提醒 → 主页提示工具更新 off: no mark, the 工具更新 row not highlighted (the notifications are the engine's, untouched).
  await button(page, '主页模块与设置').click();
  await page.getByText('提醒', { exact: true }).click();
  const hints = page.getByRole('switch', { name: /主页提示工具更新/ });
  await expect(hints).toHaveAttribute('aria-checked', 'true');
  await hints.click();
  await expect(hints).toHaveAttribute('aria-checked', 'false');
  await page.keyboard.press('Escape');
  await page.keyboard.press('Escape');
  await expect(page.locator('.dashboard')).toBeVisible();
  await expect(page.locator('.heading .update-mark')).toHaveCount(0);
  await expect(page.locator('.row', { hasText: '工具更新' }).locator('.value.pill')).toHaveCount(0);
  await page.reload();
  await expect(page.locator('.dashboard')).toBeVisible();
  await expect(page.locator('.heading .update-mark')).toHaveCount(0);
});

test('what was opened stays open: 查看更多 and an opened session, across tabs, a page and a reload', async ({ page }) => {
  await open(page);
  await page.getByRole('radio', { name: '会话' }).click();
  await page.getByRole('button', { name: /查看更多 \d+ 个会话/ }).click();
  const sessions = await page.locator('.session').count();
  const head = page.locator('.session .head').first();
  await head.click();
  await expect(head).toHaveAttribute('aria-expanded', 'true');
  await page.getByRole('radio', { name: '项目' }).click();
  await page.getByRole('button', { name: /查看更多 \d+ 个项目/ }).click();
  const projects = await page.locator('.card').count();
  // Another tab and back, a page and back: nothing folded up.
  await page.getByRole('radio', { name: '会话' }).click();
  await expect(page.locator('.session')).toHaveCount(sessions);
  await expect(page.locator('.session .head').first()).toHaveAttribute('aria-expanded', 'true');
  await page.locator('.row', { hasText: 'Claude 订阅' }).click();
  await expect(page.locator('.dashboard')).toHaveCount(0);
  await page.keyboard.press('Escape');
  await expect(page.locator('.dashboard')).toBeVisible();
  await expect(page.locator('.session')).toHaveCount(sessions);
  await expect(page.getByRole('button', { name: /查看更多/ })).toHaveCount(0);
  await expect(page.getByRole('button', { name: '收起' })).toHaveAttribute('aria-expanded', 'true');
  // A reopened panel (the page loaded afresh) has them as they were.
  await page.reload();
  await settled(page);
  await page.getByRole('radio', { name: '项目' }).click();
  await expect(page.locator('.card')).toHaveCount(projects);
  await page.getByRole('radio', { name: '会话' }).click();
  await expect(page.locator('.session')).toHaveCount(sessions);
  await expect(page.locator('.session .head').first()).toHaveAttribute('aria-expanded', 'true');
  // Closed again, it stays closed.
  await page.locator('.session .head').first().click();
  await page.reload();
  await settled(page);
  await page.getByRole('radio', { name: '会话' }).click();
  await expect(page.locator('.session .head').first()).toHaveAttribute('aria-expanded', 'false');
});

test('a list opened with 查看更多 folds back with 收起 at its end, the button staying under the pointer', async ({ page }) => {
  // 用户：展开之后就是不能再折叠回去了. Sessions, models and projects alike; folded, it stays folded.
  await open(page);
  for (const [tab, item, more] of [['会话', '.session', /查看更多 \d+ 个会话/], ['模型', '.card', /查看更多 \d+ 个模型/], ['项目', '.card', /查看更多 \d+ 个项目/]] as const) {
    await page.getByRole('radio', { name: tab }).click();
    await expect(page.locator(item)).toHaveCount(5);
    await page.getByRole('button', { name: more }).click();
    const whole = await page.locator(item).count();
    expect(whole, tab).toBeGreaterThan(5);
    const fold = page.getByRole('button', { name: '收起' });
    await expect(fold).toHaveAttribute('aria-expanded', 'true');
    await fold.click();
    await expect(page.locator(item), tab).toHaveCount(5);
    const again = page.getByRole('button', { name: more });
    await expect(again).toHaveAttribute('aria-expanded', 'false');
    // Still in sight, not off with the rows that went.
    await expect(again, tab).toBeInViewport();
  }
  await page.reload();
  await settled(page);
  await page.getByRole('radio', { name: '会话' }).click();
  await expect(page.locator('.session')).toHaveCount(5);
  await expect(page.getByRole('button', { name: /查看更多 \d+ 个会话/ })).toBeVisible();
});

test('a service in trouble: the note on its card opens the status page, the card elsewhere its page', async ({ page }) => {
  // 用户：降级那个提醒是否可以直接看这个网页 (https://status.claude.com), not the page the card opens.
  await page.addInitScript(() => { (window as unknown as { opened: string[] }).opened = []; window.open = ((url: string) => { (window as unknown as { opened: string[] }).opened.push(url); return null; }) as typeof window.open; });
  await open(page, '?fixture=failed');
  const note = page.getByRole('button', { name: '打开 Claude 状态页' });
  await expect(note).toHaveText('服务部分降级 ↗');
  await note.click();
  expect(await page.evaluate(() => (window as unknown as { opened: string[] }).opened)).toEqual(['https://status.claude.com']);
  await expect(page.locator('.dashboard')).toBeVisible();
  // The card elsewhere opens its page, where the same trouble is a row saying what is wrong, opening the same page.
  await page.getByRole('button', { name: 'Claude 详情' }).click();
  const row = page.locator('.detail-row', { hasText: 'Claude 服务状态' });
  await expect(row).toContainText('服务部分降级');
  await expect(row).toContainText('Elevated errors on Claude Opus 5.5 · 正在调查');
  await row.click();
  expect(await page.evaluate(() => (window as unknown as { opened: string[] }).opened)).toEqual(['https://status.claude.com', 'https://status.claude.com']);
});

test('立即刷新 turns while its source is read again, and stops once it is in', async ({ page }) => {
  // The row is pressed for an action that redraws nothing by itself: it still shows it is working.
  await open(page);
  await page.locator('.row', { hasText: 'Cursor 订阅' }).click();
  const again = page.locator('.detail-row', { hasText: '立即刷新' });
  await again.click();
  await expect(again.locator('.spinner')).toBeVisible();
  await expect(again.locator('.spinner')).toHaveCount(0, { timeout: 3000 });
  await expect(again.locator('.value')).toBeVisible();
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
  // The settings are pages by what they are for, a group titled 设置 like the modules' below: 外观, 通用, 提醒, 数据
  // (在手机上看 is the homepage's, beside 分享用量图); each cell an icon and a name, as tall as the others.
  await expect(page.locator('.modules .section').first()).toHaveText('设置');
  expect(await page.locator('.modules .cell').evaluateAll((cells) => cells.slice(0, 4).map((c) => c.getAttribute('aria-label'))))
    .toEqual(['外观', '通用', '提醒', '数据']);
  await expect(page.locator('.modules').getByRole('button', { name: '在手机上看' })).toHaveCount(0);
  expect(new Set(await page.locator('.modules .cell').evaluateAll((cells) => cells.slice(0, 4).map((c) => c.getBoundingClientRect().height)))).toEqual(new Set([32]));
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

test('by night the solid panel lies on 网页版\'s sky, what floats on it in a blue of that sky; by day and in 手绘 as before', async ({ page }) => {
  await open(page);
  const look = () => page.locator('.window').evaluate((w) => {
    const css = getComputedStyle(w);
    return { colour: css.backgroundColor, image: css.backgroundImage, solid: css.getPropertyValue('--solid').trim() };
  });
  await button(page, '主页模块与设置').click();
  await button(page, '外观').click();
  await page.getByRole('radio', { name: '深色' }).click();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
  // The phone's night: #1F3A7A at the top, #0E1D4C, #050A24 at the foot (the user: 电脑版的深色主页背景色弄成和手机版的一致).
  let seen = await look();
  expect(seen.image).toContain('rgb(31, 58, 122)');
  expect(seen.image).toContain('rgb(14, 29, 76)');
  expect(seen.image).toContain('rgb(5, 10, 36)');
  expect(seen.solid).toBe('#182D68');
  // The homepage too, and its range menu a blue of the sky, not the grey of before.
  await page.keyboard.press('Escape');
  await page.keyboard.press('Escape');
  await expect(page.locator('.dashboard')).toBeVisible();
  expect((await look()).image).toContain('rgb(31, 58, 122)');
  await page.locator('button[aria-haspopup=listbox]').first().click();
  await expect(page.locator('[role=listbox]')).toBeVisible();
  expect(await page.locator('[role=listbox]').evaluate((m) => getComputedStyle(m).backgroundColor)).toBe('rgb(24, 45, 104)');
  await page.keyboard.press('Escape');
  // 磨砂 and 玻璃 by night: the same sky laid over the material, thick and thin, and what floats in the same blue (用户：
  // 深色模式的磨砂和玻璃颜色需要对齐实色模式). The sky's colours as WebKit gives a mix: #1F3A7A is 0.121569 0.227451 0.478431.
  await button(page, '主页模块与设置').click();
  await button(page, '外观').click();
  for (const [name, material, veil] of [['磨砂', 'frosted', '0.8'], ['玻璃', 'glass', '0.56']]) {
    await page.getByRole('radio', { name }).click();
    await expect(page.locator('.window')).toHaveAttribute('data-material', material);
    seen = await look();
    expect(seen.image).toContain(`0.121569 0.227451 0.478431 / ${veil})`);
    expect(seen.image).toContain(`0.054902 0.113725 0.298039 / ${veil})`);
    expect(seen.image).toContain(`0.019608 0.039216 0.141176 / ${veil})`);
    expect(seen.solid).toBe('#182D68');
  }
  // By day: the white of before, no sky; 磨砂 the material alone.
  await page.getByRole('radio', { name: '浅色' }).click();
  expect((await look()).image).toBe('none');
  await page.getByRole('radio', { name: '实色' }).click();
  seen = await look();
  expect(seen.image).toBe('none');
  expect(seen.colour).toBe('rgb(253, 253, 253)');
  // 手绘 by night keeps its blackboard.
  await page.getByRole('radio', { name: '深色' }).click();
  await page.getByRole('radio', { name: '手绘' }).click();
  await expect(page.locator('html')).toHaveAttribute('data-style', 'sketch');
  expect((await look()).image).not.toContain('rgb(31, 58, 122)');
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

test('today\'s hour still under way is drawn at its pace, in the same line as the rest; yesterday\'s hours are whole', async ({ page }) => {
  // 用户：这个图对吗？最后怎么这么奇怪 (23 minutes' tokens on an axis of whole hours read as a fall); 不要虚线.
  await open(page);
  const pace = await page.evaluate(async () => {
    const loaded = performance.getEntriesByType('resource').map((entry) => entry.name).filter((name) => name.includes('/src/lib/chart.ts'));
    const chart = await import(/* @vite-ignore */ loaded.at(-1) ?? '/src/lib/chart.ts');
    const day = (chartPartial: number | null) => ({ dates: ['15:00', '15:23'], chartPartial }) as never;
    return { at23: chart.paceOf(day(23 * 60)), at2: chart.paceOf(day(120)), whole: chart.paceOf(day(null)), full: chart.paceOf(day(3600)),
      values: chart.paced([10, 20, 46], { dates: ['a', 'b', 'c'], chartPartial: 23 * 60 } as never) };
  });
  // 23 minutes in: scaled to the hour; two minutes in, counted as ten, so it doesn't leap; a whole hour as it is.
  expect(pace.at23).toBeCloseTo(3600 / 1380, 6);
  expect(pace.at2).toBe(6);
  expect([pace.whole, pace.full]).toEqual([1, 1]);
  expect(pace.values.slice(0, 2)).toEqual([10, 20]);
  expect(pace.values[2]).toBeCloseTo(46 * 3600 / 1380, 6);
  // No dashes anywhere: the fixtures' today ends 40 minutes into its hour, drawn in one solid line.
  const chart = page.locator('.summary svg.chart');
  await expect(chart.locator('path').first()).toBeVisible();
  await expect(chart.locator('path[stroke-dasharray]')).toHaveCount(0);
});

test('the card picture by night lies on the panel\'s night sky, lighter at the top than the foot; by day on white', async ({ page }) => {
  // 用户：分享图里，卡片深色和当前不一致 (it was a near black grey; the dark panel is the night sky now).
  await open(page);
  await button(page, '分享用量图').click();
  const ground = async () => {
    const src = await page.locator('.share-page .stage img').getAttribute('src');
    return page.evaluate(async (url) => {
      const image = new Image(); image.src = url!; await image.decode();
      const canvas = Object.assign(document.createElement('canvas'), { width: image.naturalWidth, height: image.naturalHeight });
      const ctx = canvas.getContext('2d')!; ctx.drawImage(image, 0, 0);
      // Down the middle of the left edge, clear of the figures: the ground and the tools' glows over it.
      const at = (f: number) => [...ctx.getImageData(6, Math.round(canvas.height * f), 1, 1).data.slice(0, 3)];
      return { top: at(0.12), middle: at(0.5), foot: at(0.9) };
    }, src);
  };
  await page.getByRole('radio', { name: '深色' }).click();
  await expect.poll(async () => (await ground()).top[2]).toBeGreaterThan(90);
  const night = await ground();
  for (const [r, g, b] of [night.top, night.middle]) expect(b).toBeGreaterThan(Math.max(r, g) + 25);
  expect(night.foot.reduce((a, c) => a + c)).toBeLessThan(night.top.reduce((a, c) => a + c));
  await page.getByRole('radio', { name: '浅色' }).click();
  await expect.poll(async () => Math.min(...(await ground()).middle)).toBeGreaterThan(200);
});

test('the header has the pin, settings, 在手机上看 and share in a row, Starmeter whole beside them in every range, Chinese and English', async ({ page }) => {
  for (const english of [false, true]) {
    await open(page);
    if (english) {
      await page.evaluate(() => localStorage.setItem('starmeter.language', 'en'));
      await page.reload();
      await settled(page);
    }
    const names = english ? ['Pin the panel', 'Modules and settings', 'Phone view', 'Share usage picture'] : ['钉住面板', '主页模块与设置', '在手机上看', '分享用量图'];
    const slots = page.locator('.summary .buttons .slot');
    await expect(slots).toHaveCount(4);
    const labels = await slots.locator('button.hint-button').evaluateAll((buttons) => buttons.map((b) => b.getAttribute('aria-label')));
    expect(labels).toEqual(names);
    for (let i = 0; i < 8; i++) {
      await page.locator('.summary .picker button').click();
      await page.getByRole('option').nth(i).click();
      await expect(page.getByRole('option')).toHaveCount(0);
      const room = await page.evaluate(() => {
        const heading = document.querySelector('.summary .heading')!, words = document.createRange();
        words.selectNodeContents(heading);
        const boxes = [...document.querySelectorAll('.summary .buttons .slot, .summary .picker')].map((e) => e.getBoundingClientRect());
        return { spare: heading.getBoundingClientRect().width - words.getBoundingClientRect().width,
          apart: boxes.slice(1).every((box, j) => box.left >= boxes[j].right - 0.5), range: document.querySelector('.summary .picker')!.textContent!.trim() };
      });
      expect(room.spare, room.range).toBeGreaterThanOrEqual(0);
      expect(room.apart, room.range).toBe(true);
    }
  }
});

/** The green dot on the homepage's 在手机上看 button while a way of it is on. */
const phoneDot = (page: Page) => page.locator('.summary .slot').filter({ has: button(page, '在手机上看') }).locator('.phone-on');

test('在手机上看: both ways off until their own 开始, a code for the same network, a public link through Cloudflare, and 停止 turns each off', async ({ page }) => {
  await open(page);
  // A button of its own on the homepage, beside 分享用量图; off, no dot on it.
  await expect(phoneDot(page)).toHaveCount(0);
  await button(page, '在手机上看').click();
  const phone = page.locator('.phone-page');
  // Off as it opens (用户：默认，同一网络，任何网络都是停止状态，用户点击开始才开始): what 开始 gives, and no code.
  await expect(phone.getByRole('status')).toContainText('未开始');
  await expect(phone.locator('svg.qr')).toHaveCount(0);
  await expect(phone.locator('.on')).toHaveCount(0);
  await button(page, '开始').click();
  await expect(phone.locator('svg[role="img"]')).toBeVisible();
  // The address without its key, which stays in the code; on the same network no time limit, until 停止.
  await expect(phone.locator('.address')).toHaveText('http://192.168.1.20:51234/');
  await expect(phone.locator('.note', { hasText: '只读' })).toContainText('一直开着');
  await expect(phone.locator('.note')).not.toContainText(['分钟']);
  // What the sun on the phone's page is for, said before the scan: it needs the https link (any network).
  await expect(phone.locator('.note', { hasText: '太阳' })).toHaveText('网页右上角的太阳可让手机屏幕保持常亮，适合放在桌边一直看；要用“任何网络”的 https 链接打开才能用');
  // Any network: off too, saying before its 开始 that the figures go through Cloudflare, and that it doesn't open in
  // parts of mainland China; nothing opened by looking.
  await page.getByRole('radio', { name: '任何网络' }).click();
  await expect(phone.getByRole('status')).toContainText('未开始');
  await expect(phone.locator('.note', { hasText: '主页的数字会经过 Cloudflare' })).toBeVisible();
  await expect(phone.locator('.note', { hasText: '中国大陆部分地区打不开 Cloudflare 的链接' })).toBeVisible();
  await expect(phone.locator('svg[role="img"]')).toHaveCount(0);
  await button(page, '开始').click();
  await expect(phone.locator('.address')).toHaveText('https://quiet-river-1234.trycloudflare.com/');
  // The public link has its hour.
  await expect(phone.locator('.note').first()).toContainText('还能用 59 分钟');
  // Its own 停止: the page stays 任何网络's, saying the link is closed, with 开始; the same network's code on all along.
  await button(page, '停止').click();
  await expect(page.getByRole('radio', { name: '任何网络' })).toHaveAttribute('aria-checked', 'true');
  await expect(phone.getByRole('status')).toContainText('外网链接已关闭');
  await expect(button(page, '开始')).toBeVisible();
  await page.getByRole('radio', { name: '同一个网络' }).click();
  await expect(phone.locator('.address')).toHaveText('http://192.168.1.20:51234/');
  // Left while on: back on the homepage, its button has a green dot; opened again, the code still there.
  await page.keyboard.press('Escape');
  await expect(page.locator('.phone-page')).toHaveCount(0);
  await expect(phoneDot(page)).toBeVisible();
  await button(page, '在手机上看').click();
  // The code in the logo's look: dots, the three rounded eyes and the app icon in the middle.
  const code = phone.locator('svg.qr');
  await expect(code.locator('image')).toHaveCount(1);
  expect(await code.locator('circle').count()).toBeGreaterThan(100);
  // 停止 stays on the page: the code's place says it has stopped, and 开始 is the way on.
  await button(page, '停止').click();
  await expect(phone.getByRole('status')).toContainText('已停止');
  await expect(phone.locator('svg.qr')).toHaveCount(0);
  await expect(phone.locator('.address')).toHaveCount(0);
  await expect(phone.locator('.note')).toHaveText(['原来的二维码和链接已失效。点“开始”生成新的二维码，手机再扫一次就能看。']);
  // The choices stay, still to pick between (用户：停止之后三个页面全没了).
  await expect(page.getByRole('radio', { name: '任何网络' })).toBeEnabled();
  await expect(button(page, '停止')).toHaveCount(0);
  await button(page, '开始').click();
  await expect(phone.locator('svg.qr')).toBeVisible();
  await expect(phone.locator('.address')).toHaveText('http://192.168.1.20:51234/');
  await expect(button(page, '停止')).toBeVisible();
  // Stopped and left: no dot.
  await button(page, '停止').click();
  await page.keyboard.press('Escape');
  await expect(phoneDot(page)).toHaveCount(0);
});

/** The panel's own copy of the backend module (a dev server that has reloaded it serves it under a new address), with
 *  its shell: a test swaps one of the shell's calls on it. */
const panelShell = `(async () => {
  const loaded = performance.getEntriesByType('resource').map((entry) => entry.name).filter((name) => name.includes('/src/lib/backend.ts'));
  return (await import(/* @vite-ignore */ loaded.at(-1) ?? '/src/lib/backend.ts')).shell;
})()`;

test('在手机上看: nothing starts by opening the page or looking at a page, only by 开始; 停止 stays on its page; the app started again, each way as it was left', async ({ page }) => {
  // 用户：默认，同一网络，任何网络都是停止状态，用户点击开始才开始。……你要记住用户的状态。就算重新安装新版本，也要记住之前用户开没开。
  const counted = () => page.evaluate(async (source) => {
    const shell = await eval(source);
    const w = window as unknown as { starts: string[] };
    w.starts = [];
    for (const name of ['phoneStart', 'phonePublic']) {
      const real = shell[name];
      shell[name] = () => { w.starts.push(name); return real(); };
    }
  }, panelShell);
  const starts = () => page.evaluate(() => (window as unknown as { starts: string[] }).starts);
  await open(page, '?tailnet');
  await counted();
  await button(page, '在手机上看').click();
  const phone = page.locator('.phone-page');
  await expect(phone.getByRole('radio')).toHaveText(['同一个网络', '任何网络']);
  await expect(phone.getByRole('status')).toContainText('未开始');
  // Looking at 任何网络 and back starts nothing.
  await page.getByRole('radio', { name: '任何网络' }).click();
  await expect(phone.getByRole('status')).toContainText('未开始');
  await page.getByRole('radio', { name: '同一个网络' }).click();
  await expect(phone.getByRole('status')).toContainText('未开始');
  expect(await starts()).toEqual([]);
  // 开始: the same network's code and the Tailscale one, on one page; then 停止, on the page it was pressed on.
  await button(page, '开始').click();
  await expect(phone.locator('svg.qr')).toHaveCount(2);
  expect(await starts()).toEqual(['phoneStart']);
  await button(page, '停止').click();
  await expect(page.getByRole('radio', { name: '同一个网络' })).toHaveAttribute('aria-checked', 'true');
  await expect(phone.getByRole('status')).toContainText('已停止');
  await expect(phone.locator('svg.qr')).toHaveCount(0);
  // Left and opened again: still stopped, nothing started.
  await page.keyboard.press('Escape');
  await button(page, '在手机上看').click();
  await expect(phone.getByRole('status')).toContainText('已停止');
  expect(await starts()).toEqual(['phoneStart']);
  // Stopped, then the app started again (the page loaded afresh, the server with it): still off.
  await page.reload();
  await settled(page);
  await counted();
  await expect(phoneDot(page)).toHaveCount(0);
  await button(page, '在手机上看').click();
  await expect(phone.getByRole('status')).toContainText('未开始');
  await expect(phone.locator('svg.qr')).toHaveCount(0);
  // On (and the public link), then the app started again: both on again by themselves, the dot on the button, nothing
  // started by the page.
  await button(page, '开始').click();
  await expect(phone.locator('.address')).toHaveText(['192.168.1.20:51234', '100.76.55.73:51234']);
  await page.getByRole('radio', { name: '任何网络' }).click();
  await button(page, '开始').click();
  await expect(phone.locator('.address')).toContainText('trycloudflare.com');
  await page.reload();
  await settled(page);
  await counted();
  await expect(phoneDot(page)).toBeVisible();
  await button(page, '在手机上看').click();
  await expect(phone.locator('svg.qr')).toHaveCount(2);
  await expect(phone.locator('.address')).toHaveText(['192.168.1.20:51234', '100.76.55.73:51234']);
  await page.getByRole('radio', { name: '任何网络' }).click();
  await expect(phone.locator('.address')).toContainText('trycloudflare.com');
  expect(await starts()).toEqual([]);
  // Opened again after leaving it: as it was left.
  await page.keyboard.press('Escape');
  await button(page, '在手机上看').click();
  await expect(phone.locator('svg.qr')).toBeVisible();
});

test('在手机上看: each way stops on its own, the other\'s code staying as it was', async ({ page }) => {
  // 用户：为什么一按停止所有都停止了？同一网络和任何网络为什么是一切开始和结束？互不影响啊。
  const calls = () => page.evaluate(() => (window as unknown as { calls: string[] }).calls);
  await open(page, '?tailnet');
  await page.evaluate(async (source) => {
    const shell = await eval(source);
    const w = window as unknown as { calls: string[] };
    w.calls = [];
    for (const name of ['phoneStart', 'phoneStop', 'phonePublic', 'phonePrivate']) {
      const real = shell[name];
      shell[name] = () => { w.calls.push(name); return real(); };
    }
  }, panelShell);
  await button(page, '在手机上看').click();
  const phone = page.locator('.phone-page');
  await button(page, '开始').click();
  const local = page.getByRole('radio', { name: '同一个网络' }), anywhere = page.getByRole('radio', { name: '任何网络' });
  await expect(phone.locator('svg.qr')).toHaveCount(2);
  await anywhere.click();
  await button(page, '开始').click();
  await expect(phone.locator('.address')).toHaveText('https://quiet-river-1234.trycloudflare.com/');
  // Both on: a green dot after each.
  await expect(local.locator('.on')).toHaveCount(1);
  await expect(anywhere.locator('.on')).toHaveCount(1);
  // The same network's 停止: its page says so; the public link is still up, its code as it was.
  await local.click();
  await button(page, '停止').click();
  await expect(phone.getByRole('status')).toContainText('已停止');
  await expect(local.locator('.on')).toHaveCount(0);
  await expect(anywhere.locator('.on')).toHaveCount(1);
  await anywhere.click();
  await expect(phone.locator('svg.qr')).toBeVisible();
  await expect(phone.locator('.address')).toHaveText('https://quiet-river-1234.trycloudflare.com/');
  expect(await calls()).toEqual(['phoneStart', 'phonePublic', 'phoneStop']);
  // The same network started again, the public link's code untouched; then the public link's 停止, the same network's
  // codes untouched.
  await local.click();
  await button(page, '开始').click();
  await expect(phone.locator('svg.qr')).toHaveCount(2);
  await anywhere.click();
  await expect(phone.locator('.address')).toHaveText('https://quiet-river-1234.trycloudflare.com/');
  await button(page, '停止').click();
  await expect(anywhere).toHaveAttribute('aria-checked', 'true');
  await expect(phone.getByRole('status')).toContainText('外网链接已关闭');
  await expect(phone.locator('svg.qr')).toHaveCount(0);
  await expect(anywhere.locator('.on')).toHaveCount(0);
  await expect(local.locator('.on')).toHaveCount(1);
  await local.click();
  await expect(phone.locator('svg.qr')).toHaveCount(2);
  await expect(phone.locator('.address')).toHaveText(['192.168.1.20:51234', '100.76.55.73:51234']);
  expect(await calls()).toEqual(['phoneStart', 'phonePublic', 'phoneStop', 'phoneStart', 'phonePrivate']);
  // 开始 again on 任何网络: a new link.
  await anywhere.click();
  await button(page, '开始').click();
  await expect(phone.locator('.address')).toHaveText('https://quiet-river-1234.trycloudflare.com/');
  // Left with only the public link on, the same network stopped: opened again, still stopped, the link still up.
  await local.click();
  await button(page, '停止').click();
  await page.keyboard.press('Escape');
  await expect(phoneDot(page)).toBeVisible();
  await button(page, '在手机上看').click();
  await expect(phone.getByRole('status')).toContainText('已停止');
  await anywhere.click();
  await expect(phone.locator('.address')).toHaveText('https://quiet-river-1234.trycloudflare.com/');
});

test('在手机上看: the 停止 button sits symmetrically clear of the panel\'s bottom edge, centered at half width', async ({ page }) => {
  await open(page, '?tailnet');
  await button(page, '在手机上看').click();
  const phone = page.locator('.phone-page');
  await button(page, '开始').click();
  const metrics = async () => {
    const stop = (await button(page, '停止').boundingBox())!, panel = (await page.locator('.window').boundingBox())!;
    const reach = (await phone.locator('.reach').boundingBox())!;
    return {
      bottom: panel.y + panel.height - (stop.y + stop.height),
      widthRatio: stop.width / reach.width,
      centerDiff: Math.abs((stop.x + stop.width / 2) - (panel.x + panel.width / 2)),
    };
  };
  await expect(phone.locator('svg.qr')).toHaveCount(2);
  await expect.poll(async () => (await metrics()).bottom).toBeGreaterThanOrEqual(15);
  await expect.poll(async () => (await metrics()).bottom).toBeLessThanOrEqual(18);
  expect((await metrics()).widthRatio).toBeCloseTo(0.5, 1);
  expect((await metrics()).centerDiff).toBeLessThanOrEqual(1.5);
  await page.getByRole('radio', { name: '任何网络' }).click();
  await button(page, '开始').click();
  await expect(phone.locator('svg.qr')).toBeVisible();
  await expect.poll(async () => (await metrics()).bottom).toBeGreaterThanOrEqual(15);
  await expect.poll(async () => (await metrics()).bottom).toBeLessThanOrEqual(18);
});

test('在手机上看: a public link still being made when 停止 is pressed leaves the next start alone (PHONE-3)', async ({ page }) => {
  await open(page);
  await button(page, '在手机上看').click();
  const phone = page.locator('.phone-page');
  await button(page, '开始').click();
  await expect(phone.locator('svg.qr')).toBeVisible();
  // The public link takes its time, and answers as the host does once the server it was for has stopped.
  await page.evaluate(async (source) => {
    const shell = await eval(source);
    const w = window as unknown as { letGo: () => void };
    shell.phonePublic = () => new Promise((_, refuse) => (w.letGo = () => refuse('手机查看已经关闭')));
  }, panelShell);
  await page.getByRole('radio', { name: '任何网络' }).click();
  await button(page, '开始').click();
  await expect(phone.locator('.wait')).toHaveText('正在向 Cloudflare 要链接…');
  await button(page, '停止').click();
  // Stopped at once, without the old request's wait; the same network's code a choice away.
  await expect(phone.getByRole('status')).toContainText('外网链接已关闭');
  await expect(phone.locator('.wait')).toHaveCount(0);
  await page.getByRole('radio', { name: '同一个网络' }).click();
  await expect(phone.locator('svg.qr')).toBeVisible();
  // The old request answers late: nothing of it shows beside the new code.
  await page.evaluate(() => (window as unknown as { letGo: () => void }).letGo());
  await page.waitForTimeout(200);
  await expect(phone.locator('.failed')).toHaveCount(0);
  await expect(phone.locator('svg.qr')).toBeVisible();
  await expect(phone.locator('.address')).toHaveText('http://192.168.1.20:51234/');
  // Nor on 任何网络's page: still stopped, as it was left.
  await page.getByRole('radio', { name: '任何网络' }).click();
  await expect(phone.getByRole('status')).toContainText('外网链接已关闭');
  await expect(phone.locator('.failed')).toHaveCount(0);
});

test('在手机上看: 任何网络 chosen again while its link is made waits for that link, asking once (TUNNEL-1)', async ({ page }) => {
  await open(page);
  await button(page, '在手机上看').click();
  const phone = page.locator('.phone-page');
  await button(page, '开始').click();
  await expect(phone.locator('svg.qr')).toBeVisible();
  await page.evaluate(async (source) => {
    const shell = await eval(source);
    const real = shell.phonePublic;
    const w = window as unknown as { asked: number; letGo: () => void };
    w.asked = 0;
    shell.phonePublic = () => {
      w.asked++;
      return new Promise((resolve) => (w.letGo = () => resolve(real())));
    };
  }, panelShell);
  const asked = () => page.evaluate(() => (window as unknown as { asked: number }).asked);
  await page.getByRole('radio', { name: '任何网络' }).click();
  await button(page, '开始').click();
  await expect(phone.locator('.wait')).toHaveText('正在向 Cloudflare 要链接…');
  // Back on 同一个网络 meanwhile: its code, its address and what to do with them, not the public link's wait.
  await page.getByRole('radio', { name: '同一个网络' }).click();
  await expect(phone.locator('svg.qr')).toBeVisible();
  await expect(phone.locator('.wait')).toHaveCount(0);
  await expect(phone.locator('.note').first()).toContainText('手机和电脑连同一个网络（Wi-Fi）后扫码');
  await expect(phone.locator('.note').nth(1)).toContainText('只读');
  // Back and forth while it is made: no second question, no second request; the wait stays.
  await page.getByRole('radio', { name: '任何网络' }).click();
  await page.waitForTimeout(200);
  await expect(button(page, '开始')).toHaveCount(0);
  await expect(phone.locator('.wait')).toHaveText('正在向 Cloudflare 要链接…');
  expect(await asked()).toBe(1);
  await page.evaluate(() => (window as unknown as { letGo: () => void }).letGo());
  await expect(phone.locator('.address')).toContainText('trycloudflare.com');
  expect(await asked()).toBe(1);
});

test('在手机上看: a start that fails once says why and offers 重试, which starts it (PHONE-4)', async ({ page }) => {
  await open(page);
  await button(page, '在手机上看').click();
  const phone = page.locator('.phone-page');
  await button(page, '开始').click();
  await expect(phone.locator('svg.qr')).toBeVisible();
  // The next start fails once (its port taken for a moment), then works.
  await page.evaluate(async (source) => {
    const shell = await eval(source);
    const real = shell.phoneStart;
    let once = true;
    shell.phoneStart = () => (once ? ((once = false), Promise.reject('这台电脑暂时开不了端口，稍后再试')) : real());
  }, panelShell);
  await button(page, '停止').click();
  await button(page, '开始').click();
  await expect(phone.getByRole('status')).toContainText('没能开启');
  await expect(phone.locator('.failed')).toHaveText('这台电脑暂时开不了端口，稍后再试');
  await button(page, '重试').click();
  await expect(phone.locator('svg.qr')).toBeVisible();
  await expect(phone.locator('.failed')).toHaveCount(0);
  await expect(button(page, '停止')).toBeVisible();
});

test('在手机上看: a public link that fails stays on 任何网络, says why in the code\'s place, with 重试 and 改用同一个网络', async ({ page }) => {
  await open(page);
  // Cloudflare not answering: twice (the first is asked again on its own), then the link as it comes.
  await page.evaluate(async (source) => {
    const shell = await eval(source);
    const real = shell.phonePublic;
    const w = window as unknown as { asked: number };
    w.asked = 0;
    shell.phonePublic = () => (++w.asked <= 2 ? Promise.reject('Cloudflare 这次没有连上，稍后再试') : real());
  }, panelShell);
  const asked = () => page.evaluate(() => (window as unknown as { asked: number }).asked);
  await button(page, '在手机上看').click();
  const phone = page.locator('.phone-page');
  await page.getByRole('radio', { name: '任何网络' }).click();
  await button(page, '开始').click();
  // 用户：在所有网络失败，应该停留在所有网络的页面，而不是返回到同一网络显示错误.
  await expect(phone.getByRole('status')).toContainText('没能生成外网链接');
  await expect(page.getByRole('radio', { name: '任何网络' })).toHaveAttribute('aria-checked', 'true');
  await expect(phone.locator('.failed')).toContainText('Cloudflare 这次没有连上');
  expect(await asked()).toBe(2);
  // No code, and nothing about the same network's: neither is what is chosen.
  await expect(phone.locator('svg.qr')).toHaveCount(0);
  await expect(phone.locator('.address')).toHaveCount(0);
  await expect(phone.locator('.note', { hasText: '同一个网络（Wi-Fi）' })).toHaveCount(0);
  // 重试: the link this time, on 任何网络 still.
  await button(page, '重试').click();
  await expect(phone.locator('.address')).toContainText('trycloudflare.com');
  await expect(phone.locator('svg.qr')).toBeVisible();
  await expect(phone.locator('.failed')).toHaveCount(0);
  await expect(page.getByRole('radio', { name: '任何网络' })).toHaveAttribute('aria-checked', 'true');
});

test('在手机上看: 改用同一个网络 from a failed public link shows the same network\'s code at once', async ({ page }) => {
  await open(page, '?tunnel-fails');
  await button(page, '在手机上看').click();
  const phone = page.locator('.phone-page');
  await button(page, '开始').click();
  await page.getByRole('radio', { name: '任何网络' }).click();
  await button(page, '开始').click();
  await expect(phone.getByRole('status')).toContainText('没能生成外网链接');
  await button(page, '改用同一个网络').click();
  await expect(page.getByRole('radio', { name: '同一个网络' })).toHaveAttribute('aria-checked', 'true');
  await expect(phone.locator('svg.qr')).toBeVisible();
  await expect(phone.locator('.failed')).toHaveCount(0);
  // Trying again starts afresh: its 开始, without the last failure under it.
  await page.getByRole('radio', { name: '任何网络' }).click();
  await expect(phone.getByRole('status')).toContainText('未开始');
  await expect(button(page, '开始')).toBeVisible();
  await expect(phone.locator('.failed')).toHaveCount(0);
});

test('在手机上看: a public link that drops stays on 任何网络, with 重新生成 in the code\'s place', async ({ page }) => {
  await open(page);
  await page.evaluate(async (source) => {
    const shell = await eval(source);
    const real = shell.phoneStatus;
    const w = window as unknown as { dropped: boolean };
    w.dropped = false;
    // Once dropped, the status says the server is up and its public link gone (its cloudflared ended).
    shell.phoneStatus = async () => {
      const view = await real();
      return w.dropped && view ? { ...view, public: null, secondsLeft: 0 } : view;
    };
  }, panelShell);
  await page.clock.install();
  await button(page, '在手机上看').click();
  const phone = page.locator('.phone-page');
  await page.getByRole('radio', { name: '任何网络' }).click();
  await button(page, '开始').click();
  await expect(phone.locator('.address')).toContainText('trycloudflare.com');
  await page.evaluate(() => { (window as unknown as { dropped: boolean }).dropped = true; });
  await page.clock.fastForward(10_500);
  await expect(phone.getByRole('status')).toContainText('外网链接断开了');
  await expect(page.getByRole('radio', { name: '任何网络' })).toHaveAttribute('aria-checked', 'true');
  await expect(phone.locator('svg.qr')).toHaveCount(0);
  await expect(button(page, '重新生成')).toBeVisible();
  await expect(button(page, '改用同一个网络')).toBeVisible();
});

test('在手机上看: a public link that fails after 同一个网络 was chosen leaves that code alone (PHONE-12)', async ({ page }) => {
  for (const away of ['同一个网络']) {
    await open(page);
    await button(page, '在手机上看').click();
    const phone = page.locator('.phone-page');
    await button(page, '开始').click();
    await expect(phone.locator('svg.qr')).toBeVisible();
    // The link takes its time (PHONE-11's wait), then fails for good.
    await page.evaluate(async (source) => {
      const shell = await eval(source);
      const w = window as unknown as { letGo: () => void };
      shell.phonePublic = () => new Promise((_, refuse) => (w.letGo = () => refuse('Cloudflare 这次没有给出链接，稍后再试')));
    }, panelShell);
    await page.getByRole('radio', { name: '任何网络' }).click();
    await button(page, '开始').click();
    await expect(phone.locator('.wait')).toHaveText('正在向 Cloudflare 要链接…');
    await page.getByRole('radio', { name: away }).click();
    const address = 'http://192.168.1.20:51234/';
    await expect(phone.locator('.address')).toHaveText(address);
    await page.evaluate(() => (window as unknown as { letGo: () => void }).letGo());
    await page.waitForTimeout(300);
    // The chosen code as it was, nothing of Cloudflare under it.
    await expect(page.getByRole('radio', { name: away })).toHaveAttribute('aria-checked', 'true');
    await expect(phone.locator('.address')).toHaveText(address);
    await expect(phone.locator('svg.qr')).toBeVisible();
    await expect(phone.locator('.failed')).toHaveCount(0);
    await expect(phone.locator('.stopped')).toHaveCount(0);
    // It is 任何网络's: said on its page, with 重试.
    await page.getByRole('radio', { name: '任何网络' }).click();
    await expect(phone.getByRole('status')).toContainText('没能生成外网链接');
    await expect(phone.locator('.failed')).toContainText('Cloudflare 这次没有给出链接');
    await expect(button(page, '重试')).toBeVisible();
  }
});

test('在手机上看: the public link shows once a phone can open it, its wait saying so, and a wait run out says to give it a minute (PHONE-11)', async ({ page }) => {
  for (const query of ['', '?unchecked']) {
    await open(page, query);
    await button(page, '在手机上看').click();
    const phone = page.locator('.phone-page');
    await button(page, '开始').click();
    await expect(phone.locator('svg.qr')).toBeVisible();
    // The host answers once the link is seen to open (or the wait for it is over): held here to look at the wait.
    await page.evaluate(async (source) => {
      const shell = await eval(source);
      const w = window as unknown as { letGo: () => void };
      const made = shell.phonePublic;
      shell.phonePublic = () => new Promise((done) => (w.letGo = () => done(made())));
    }, panelShell);
    await page.getByRole('radio', { name: '任何网络' }).click();
    await button(page, '开始').click();
    await expect(phone.locator('.wait')).toHaveText('正在向 Cloudflare 要链接…');
    await expect(phone.locator('.wait-detail')).toHaveText('确认手机能打开后再显示，通常十几秒');
    await expect(phone.locator('svg.qr')).toHaveCount(0);
    await page.evaluate(() => (window as unknown as { letGo: () => void }).letGo());
    await expect(phone.locator('.address')).toHaveText('https://quiet-river-1234.trycloudflare.com/');
    await expect(phone.locator('svg.qr')).toBeVisible();
    await expect(phone.locator('.wait-detail')).toHaveCount(0);
    // Seen to open: nothing more said. Not seen in time: shown all the same, with what to do.
    await expect(phone.locator('.note.caution')).toHaveText(query ? ['还没确认手机能打开：扫码打不开就等一分钟再试'] : []);
    // The next round from both ways off, not as this one left them.
    await page.evaluate(() => localStorage.removeItem('starmeter.test-phone'));
  }
});

test('在手机上看: on a tailnet, the same network\'s page has its own code and the Tailscale one side by side, the same server', async ({ page }) => {
  // 用户：本地网络和 tailscale 应该在同一页展示。
  await open(page, '?tailnet');
  await button(page, '在手机上看').click();
  const phone = page.locator('.phone-page');
  await button(page, '开始').click();
  await expect(phone.getByRole('radio')).toHaveText(['同一个网络', '任何网络']);
  await expect(page.getByRole('radio', { name: '同一个网络' })).toHaveAttribute('aria-checked', 'true');
  await expect(phone.getByRole('img', { name: '手机扫这个二维码', exact: true })).toBeVisible();
  await expect(phone.getByRole('img', { name: '连着 Tailscale 的手机扫这个二维码' })).toBeVisible();
  await expect(phone.locator('.address')).toHaveText(['192.168.1.20:51234', '100.76.55.73:51234']);
  await expect(phone.locator('.card-title')).toHaveText(['局域网', 'Tailscale']);
  await expect(phone.locator('.card-detail')).toHaveText(['手机连同一个 Wi-Fi', '手机开着 Tailscale']);
  // Side by side: one row, the same size, the codes level (once the page has slid in).
  await expect(async () => {
    const [left, right] = [(await phone.locator('.card').nth(0).boundingBox())!, (await phone.locator('.card').nth(1).boundingBox())!];
    expect(Math.abs(left.y - right.y)).toBeLessThan(1);
    expect(Math.abs(left.width - right.width)).toBeLessThan(1);
    expect(Math.abs(left.height - right.height)).toBeLessThan(1);
    expect(right.x).toBeGreaterThan(left.x + left.width);
  }).toPass();
  await expect(phone.locator('.note').first()).toContainText('Tailscale 的二维码在哪都能打开');
  // The pill under 同一个网络, half the bar.
  await expect(async () => {
    const pill = (await phone.locator('.reach .pill').boundingBox())!;
    const first = (await page.getByRole('radio', { name: '同一个网络' }).boundingBox())!;
    expect(Math.abs(pill.x - first.x)).toBeLessThan(2);
    expect(Math.abs(pill.width - first.width)).toBeLessThan(2);
  }).toPass();
  // Each link copied on its own.
  await phone.locator('.address').nth(1).click();
  await expect(phone.locator('.address')).toHaveText(['192.168.1.20:51234', '已复制链接']);
});

test('在手机上看: a WireGuard tunnel has its own code like Tailscale\'s: two side by side, a third under them in the middle', async ({ page }) => {
  // 用户：如果有wireguard也和tailscale一样处理哈。
  await open(page, '?wireguard');
  await button(page, '在手机上看').click();
  const phone = page.locator('.phone-page');
  await button(page, '开始').click();
  await expect(phone.locator('.card-title')).toHaveText(['局域网', 'WireGuard']);
  await expect(phone.locator('.card-detail')).toHaveText(['手机连同一个 Wi-Fi', '手机开着 WireGuard']);
  await expect(phone.getByRole('img', { name: '连着 WireGuard 的手机扫这个二维码' })).toBeVisible();
  await expect(phone.locator('.address')).toHaveText(['192.168.1.20:51234', '10.0.0.2:51234']);
  await expect(phone.locator('.note').first()).toContainText('WireGuard 的二维码在手机连着同一个 WireGuard 网络时打开');
  await page.evaluate(() => localStorage.removeItem('starmeter.test-phone'));

  // With Tailscale too: three codes, the same size; two in a row, the third under them in the middle.
  await open(page, '?tailnet&wireguard');
  await button(page, '在手机上看').click();
  await button(page, '开始').click();
  await expect(phone.locator('.card-title')).toHaveText(['局域网', 'Tailscale', 'WireGuard']);
  await expect(phone.locator('.address')).toHaveText(['192.168.1.20:51234', '100.76.55.73:51234', '10.0.0.2:51234']);
  await expect(phone.locator('.note').first()).toContainText('Tailscale 和 WireGuard 的二维码在手机连着对应的网络时打开');
  await expect(async () => {
    const [a, b, c] = await Promise.all([0, 1, 2].map(async (i) => (await phone.locator('.card').nth(i).boundingBox())!));
    const codes = (await phone.locator('.codes').boundingBox())!;
    expect(Math.abs(a.y - b.y)).toBeLessThan(1);
    for (const card of [b, c]) {
      expect(Math.abs(card.width - a.width)).toBeLessThan(1);
      expect(Math.abs(card.height - a.height)).toBeLessThan(1);
    }
    expect(c.y).toBeGreaterThan(a.y + a.height);
    expect(Math.abs(c.x + c.width / 2 - (codes.x + codes.width / 2))).toBeLessThan(1);
    expect(Math.abs(a.x - codes.x)).toBeLessThan(1);
    expect(Math.abs(b.x + b.width - (codes.x + codes.width))).toBeLessThan(1);
  }).toPass();
  await phone.locator('.address').nth(2).click();
  await expect(phone.locator('.address')).toHaveText(['192.168.1.20:51234', '100.76.55.73:51234', '已复制链接']);
  await page.evaluate(() => localStorage.removeItem('starmeter.test-phone'));
});

test('在手机上看: the Tailscale code comes and goes with the tailnet, the same network\'s code staying', async ({ page }) => {
  await open(page);
  // Off a tailnet at first; then on it, then off it again: the status asked every ten seconds says which.
  await page.evaluate(async (source) => {
    const shell = await eval(source);
    const real = shell.phoneStatus;
    const w = window as unknown as { on: boolean };
    w.on = false;
    const tailnet = (view: { local: string } | null) => view && { ...view, tailnet: w.on ? 'http://100.76.55.73:51234/#k=0123456789abcdef0123456789abcdef' : null };
    shell.phoneStatus = async () => tailnet(await real());
  }, panelShell);
  // The page's ten seconds on a clock the test moves (installed now: the module lookup above reads real timings).
  await page.clock.install();
  await button(page, '在手机上看').click();
  const phone = page.locator('.phone-page');
  await button(page, '开始').click();
  await expect(phone.locator('svg.qr')).toHaveCount(1);
  await expect(phone.locator('.card')).toHaveCount(0);
  await page.evaluate(() => { (window as unknown as { on: boolean }).on = true; });
  await page.clock.fastForward(10_500);
  await expect(phone.locator('svg.qr')).toHaveCount(2);
  await expect(phone.locator('.address')).toHaveText(['192.168.1.20:51234', '100.76.55.73:51234']);
  await page.evaluate(() => { (window as unknown as { on: boolean }).on = false; });
  await page.clock.fastForward(10_500);
  await expect(phone.locator('.card')).toHaveCount(0);
  await expect(phone.locator('.address')).toHaveText('http://192.168.1.20:51234/');
  await expect(phone.getByRole('radio')).toHaveText(['同一个网络', '任何网络']);
});

test('on the phone: a link without its key says so in the panel\'s place, not an empty panel', async ({ page }) => {
  await page.goto('/?keyless');
  await expect(page.getByRole('status')).toContainText('链接不完整');
  await expect(page.getByRole('status')).toContainText('# 后面的一次性口令');
  await expect(page.locator('.dashboard')).toHaveCount(0);
  // Nothing behind the page is asked for: a release build has nothing there to answer, and its refusal went unheard
  // into the console (PHONE-13).
  await page.waitForTimeout(600);
  const asked = await page.evaluate(() => performance.getEntriesByType('resource').map((entry) => entry.name)
    .filter((name) => /fixtureBackend|phoneBackend/.test(name)));
  expect(asked).toEqual([]);
});

test('on the phone: a key no longer the server\'s, at the first reading, says so in the panel\'s place', async ({ page }) => {
  await servePhone(page, 'ffffffffffffffffffffffffffffffff');
  await page.goto('/#k=' + PHONE_KEY);
  await expect(page.getByRole('status').first()).toContainText('链接打不开');
  await expect(page.getByRole('status').first()).toContainText('链接已失效');
  await expect(page.locator('.dashboard')).toHaveCount(0);
  await expect(page.locator('.phone-problem')).toHaveCount(0);
  // The browser logs the refused request itself; nothing else may have gone wrong.
  errors.splice(0, errors.length, ...errors.filter((line) => !/Failed to load resource/.test(line)));
});

test('在手机上看: 已复制链接 only once the clipboard took it; refused, the whole link to copy by hand', async ({ page }) => {
  await open(page);
  // The clipboard as the test says: refusing first, then taking it.
  await page.evaluate(() => {
    const state = { refuse: true, text: '' };
    Object.assign(window, { clipboardState: state });
    Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText: (text: string) => (state.refuse ? Promise.reject(new Error('denied')) : Promise.resolve(void (state.text = text))) } });
  });
  await button(page, '在手机上看').click();
  const phone = page.locator('.phone-page'), address = phone.locator('.address');
  await button(page, '开始').click();
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
  await button(page, '在手机上看').click();
  await page.getByRole('radio', { name: '任何网络' }).click();
  await expect(page.locator('.phone-page .note', { hasText: 'cloudflared' })).toBeVisible();
  await expect(button(page, '下载并开始')).toBeVisible();
});

test('on the phone: a slow reading of the range before never takes the page back', async ({ page }) => {
  await servePhone(page);
  await page.goto('/#k=' + PHONE_KEY);
  await expect(page.locator('.stage .monitor')).toBeVisible();
  // Today's next reading is held back; the week is picked meanwhile and arrives at once.
  let release = () => {};
  const held = new Promise<void>((resolve) => { release = resolve; });
  await page.route('**/api/snapshot', async (route) => { await held; await route.fallback(); });
  await page.evaluate(() => document.dispatchEvent(new Event('visibilitychange')));
  const ranges = page.getByRole('radiogroup', { name: '统计范围' });
  await ranges.getByRole('radio', { name: '近 7 天' }).click();
  await expect(page.locator('.stage .hero .range')).toHaveText('近 7 天');
  const amount = page.locator('.stage .hero .amount [data-value]');
  await page.waitForTimeout(600);
  const week = await amount.getAttribute('data-value');
  release();
  await page.waitForTimeout(600);
  await expect(ranges.getByRole('radio', { name: '近 7 天' })).toHaveAttribute('aria-checked', 'true');
  await expect(page.locator('.stage .hero .range')).toHaveText('近 7 天');
  expect(await amount.getAttribute('data-value')).toBe(week);
});

test('on the phone: one reading at a time, and a computer out of reach or a stale link said at the foot', async ({ page }) => {
  const asked = await servePhone(page);
  await page.goto('/#k=' + PHONE_KEY);
  await expect(page.locator('.stage .monitor')).toBeVisible();
  // A slow computer: asking again while a reading is on its way sends nothing more.
  let slow = 0;
  await page.route('**/api/snapshot**', async (route) => { slow++; await new Promise((r) => setTimeout(r, 1500)); await route.fallback(); });
  const snapshots = () => asked.filter((path) => path.startsWith('/api/snapshot')).length;
  const before = snapshots();
  for (let i = 0; i < 4; i++) await page.evaluate(() => document.dispatchEvent(new Event('visibilitychange')));
  await expect.poll(snapshots).toBe(before + 1);
  expect(slow).toBe(1);
  await page.unroute('**/api/snapshot**');
  // Out of reach: said at the foot, the figures on screen kept.
  await page.route('**/api/snapshot**', (route) => route.abort('connectionrefused'));
  await page.evaluate(() => document.dispatchEvent(new Event('visibilitychange')));
  await expect(page.locator('.phone-problem')).toContainText('连不上电脑');
  await expect(page.locator('.stage .card').first()).toBeVisible();
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
  // Pinned, 实时 comes in with its first reading.
  await expect(page.locator('.panel-live')).toBeVisible();
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

test('卡片: a long estimate (全部\'s thousands) keeps the dots beside it, never drawn over them', async ({ page }) => {
  await open(page);
  const laid = await page.evaluate(async () => {
    const { heroLayout, HERO_DOTS } = await import('/src/lib/shareCard.ts');
    const { WIDTH } = await import('/src/lib/shareCommon.ts');
    const ctx = document.createElement('canvas').getContext('2d')!;
    return { WIDTH, HERO_DOTS, sums: ['21.78', '9,100.46', '22,679.02', '283,487.69', '9,876,543.21'].map((figure) => {
      const { size, right, dots } = heroLayout(ctx, { symbol: '$', figure }, 100);
      return { figure, size, right, dots };
    }) };
  });
  for (const { figure, size, right, dots } of laid.sums) {
    // The dots start after the sum, a gap between, at most 170 wide from the right edge; a short sum keeps nearly all.
    if (dots) expect(dots.x, figure).toBeGreaterThanOrEqual(Math.max(laid.WIDTH - 170, right + 12));
    if (figure === '21.78') expect(dots!.w, figure).toBeGreaterThanOrEqual(150);
    // Up to hundreds of thousands, the sum is fitted to leave the dots their room, still large.
    if (figure !== '9,876,543.21') {
      expect(dots!.w, figure).toBeGreaterThanOrEqual(laid.HERO_DOTS);
      expect(size, figure).toBeGreaterThanOrEqual(44);
    }
  }
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
  expect(posters[3].members).toEqual(['Cursor', 'Gemini', 'OpenCode', 'Qoder']);
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
const READY_STOPS = ['欢迎使用 Starmeter', '用量总览', '统计范围', '每个工具一张卡', '订阅额度和 API 余额', '四个按钮'];
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
      ['订阅额度和 API 余额', '.rows .row[data-key="chatgpt"]'], ['四个按钮', '.summary .buttons .slot']];
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
  // The header's strip beside Starmeter, past the update mark (a button of its own, opening the updates).
  const mark = await page.locator('.update-mark').boundingBox();
  const strip = mark ? Math.round(mark.x - frame.x + mark.width + 8) : 150;
  await moves(strip, 24);
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
  await page.mouse.move(frame.x + strip, frame.y + 24);
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

test('在手机上看: a status answer from before 停止 and 重新开始 leaves the new address alone (PHONE-5)', async ({ page }) => {
  await open(page);
  // Each start a new port; the status asked every ten seconds is held back, and answers with the first address.
  await page.evaluate(async (source) => {
    const shell = await eval(source);
    const w = window as unknown as { port: number; answer: () => void };
    const view = (port: number) => ({ local: `http://192.168.1.20:${port}/#k=0123456789abcdef0123456789abcdef`, public: null, secondsLeft: 0 });
    w.port = 49000;
    shell.phoneStart = () => Promise.resolve(view(++w.port));
    shell.phoneStatus = () => { const first = view(49001); return new Promise((resolve) => (w.answer = () => resolve(first))); };
  }, panelShell);
  // The page's ten seconds on a clock the test moves (installed now: the module lookup above reads real timings).
  await page.clock.install();
  await button(page, '在手机上看').click();
  const phone = page.locator('.phone-page');
  await button(page, '开始').click();
  await expect(phone.locator('.address')).toHaveText('http://192.168.1.20:49001/');
  // The status is asked, and still out when 停止 and 重新开始 bring a new address.
  await page.clock.fastForward(10_500);
  await expect.poll(() => page.evaluate(() => typeof (window as unknown as { answer?: () => void }).answer)).toBe('function');
  await button(page, '停止').click();
  await button(page, '开始').click();
  await expect(phone.locator('.address')).toHaveText('http://192.168.1.20:49002/');
  await page.evaluate(() => (window as unknown as { answer: () => void }).answer());
  await page.waitForTimeout(200);
  await expect(phone.locator('.address')).toHaveText('http://192.168.1.20:49002/');
  await expect(phone.locator('svg.qr')).toBeVisible();
});

test('手绘 lays its paper on the computer\'s background: opaque for 实色, cold-pressed and translucent for 磨砂, tracing paper for 玻璃', async ({ page }) => {
  await open(page);
  await button(page, '主页模块与设置').click();
  await button(page, '外观').click();
  await page.getByRole('radio', { name: '手绘' }).click();
  await expect(page.locator('html')).toHaveAttribute('data-style', 'sketch');
  const paper = () => page.locator('.window').evaluate((w) => {
    const css = getComputedStyle(w), seen = css.backgroundColor.match(/\/\s*([\d.]+)\)|rgba\([^)]*,\s*([\d.]+)\)/);
    return { material: w.dataset.material, alpha: seen ? Number(seen[1] ?? seen[2]) : 1, image: css.backgroundImage };
  });
  // 实色: the drawing paper, opaque and finely dotted.
  let shown = await paper();
  expect(shown.material).toBe('solid');
  expect(shown.alpha).toBe(1);
  // 磨砂: heavy cold-pressed paper, its clouds (the turbulence at 0.016) and tooth, the desktop's frost faintly through.
  await page.getByRole('radio', { name: '磨砂' }).click();
  await expect.poll(async () => (await paper()).material).toBe('frosted');
  shown = await paper();
  expect(shown.alpha).toBeGreaterThan(0.75);
  expect(shown.alpha).toBeLessThan(0.9);
  expect(shown.image).toContain('0.016');
  // 玻璃: tracing paper, plainly translucent, its fibres (0.035 0.5) and a sheen across it.
  await page.getByRole('radio', { name: '玻璃' }).click();
  await expect.poll(async () => (await paper()).material).toBe('glass');
  shown = await paper();
  expect(shown.alpha).toBeGreaterThan(0.45);
  expect(shown.alpha).toBeLessThan(0.65);
  expect(shown.image).toContain('0.035');
  expect(shown.image).toContain('linear-gradient');
  // Back to 实色: opaque again.
  await page.getByRole('radio', { name: '实色' }).click();
  await expect.poll(async () => (await paper()).alpha).toBe(1);
});

test('外观 → 风格 draws the panel by hand, in faces scaled to the usual width, kept across a reload, and back', async ({ page }) => {
  await open(page);
  const standard = await page.locator('.window').boundingBox();
  await button(page, '主页模块与设置').click();
  await button(page, '外观').click();
  await expect(page.getByRole('radio', { name: '标准' })).toHaveAttribute('aria-checked', 'true');
  await page.getByRole('radio', { name: '手绘' }).click();
  await expect(page.locator('html')).toHaveAttribute('data-style', 'sketch');
  await expect(page.getByRole('radio', { name: '手绘' })).toHaveAttribute('aria-checked', 'true');
  // The hand's letters, scaled to the system faces' width: a line keeps its length.
  const widths = await page.evaluate(async () => {
    await document.fonts.load('13px "Starmeter Hand"', 'A');
    const width = (family: string) => {
      const span = Object.assign(document.createElement('span'), { textContent: 'ChatGPT 14.46M Token · 45 requests' });
      Object.assign(span.style, { position: 'absolute', fontSize: '13px', fontFamily: family, whiteSpace: 'nowrap' });
      document.body.append(span);
      const measured = span.getBoundingClientRect().width;
      span.remove();
      return measured;
    };
    return { hand: width('"Starmeter Hand"'), chalkboard: width('"Chalkboard SE"') };
  });
  // Chalkboard SE at 97%, no wider than San Francisco (this test's WebKit has no San Francisco to compare with).
  expect(widths.hand / widths.chalkboard).toBeCloseTo(0.97, 2);
  expect(await page.locator('.window').evaluate((e) => getComputedStyle(e).fontFamily)).toContain('Starmeter Hand');
  // Chinese in 霞鹜文楷, bundled with the app and loaded from its own files; only Chinese: letters stay in Chalkboard.
  expect(await page.locator('.window').evaluate((e) => getComputedStyle(e).fontFamily)).toContain('Starmeter Kai');
  expect(await page.evaluate(async () => (await document.fonts.load('16px "Starmeter Kai"', '用量')).length)).toBeGreaterThan(0);
  expect(await page.evaluate(async () => (await document.fonts.load('16px "Starmeter Kai"', 'Magpie')).length)).toBe(0);
  // Coloured strokes, as the 手绘 picture draws: a wavy line under the name, each tool's models hatched in its pen.
  await page.keyboard.press('Escape');
  await page.keyboard.press('Escape');
  await expect(page.locator('.card .fill').first()).toBeVisible();
  await expect(page.locator('h1.heading')).toHaveCSS('text-decoration-style', 'wavy');
  const bar = page.locator('.card .fill').first();
  expect(await bar.evaluate((e) => getComputedStyle(e).backgroundImage)).toContain('repeating-linear-gradient');
  expect(await page.locator('.card').first().evaluate((e) => getComputedStyle(e).getPropertyValue('--pen'))).toMatch(/^#/);
  // The three figures each in its own pen, and no box round them.
  const figurePens = await page.locator('.summary .figure.num').evaluateAll((all) => all.map((e) => getComputedStyle(e).color));
  expect(new Set(figurePens).size).toBe(3);
  // The picture's own strokes: each tool's mark ringed by hand, the trend on sketched axes with a ring round its latest
  // reading in place of the grid and the dot.
  await expect(page.locator('.card svg.ring path').first()).toHaveAttribute('stroke', /^#/);
  await expect(page.locator('svg.chart > line')).toHaveCount(0);
  await expect(page.locator('svg.chart circle')).toHaveCount(0);
  // Chalkboard's line box is taller than a row's: a name clips only across, so the tail of its g is never cut off.
  expect(await page.locator('.card .name').first().evaluate((e) => [getComputedStyle(e).overflowX, getComputedStyle(e).overflowY])).toEqual(['clip', 'visible']);
  // Kept: the homepage drawn by hand after a reload, laid out as before.
  await page.reload();
  await expect(page.locator('.dashboard')).toBeVisible();
  await expect(page.locator('html')).toHaveAttribute('data-style', 'sketch');
  expect((await page.locator('.window').boundingBox())!.width).toBe(standard!.width);
  // And back to the standard look.
  await button(page, '主页模块与设置').click();
  await button(page, '外观').click();
  await page.getByRole('radio', { name: '标准' }).click();
  await expect(page.locator('html')).not.toHaveAttribute('data-style', 'sketch');
});

test('the page carries no inline style: in the app, under its CSP, nothing stands over the panel', async ({ page }) => {
  // Tauri puts a nonce on every inline <style> of index.html and into style-src, which turns 'unsafe-inline' off: each
  // style attribute is then refused. The hand's filter, kept out of the flow by one, stood as a 16-point line over the
  // panel: a gap under the menu bar, the panel's foot cut off (the user: 为什么状态栏图标离页面这么远).
  const html = readFileSync(new URL('../ui/index.html', import.meta.url), 'utf8');
  const markup = html.replace(/<!--[\s\S]*?-->/g, '');
  expect(markup).not.toMatch(/<style[\s>]/i);
  expect(markup).not.toMatch(/\sstyle\s*=/i);
  // And under such a policy (a nonce, no 'unsafe-inline', as the app has whenever a <style> is there), the page's first
  // rules still hold: nothing above the panel.
  await page.route((url) => url.pathname === '/', (route) => route.fulfill({
    status: 200,
    headers: { 'Content-Type': 'text/html', 'Content-Security-Policy': "default-src 'self'; style-src 'self' 'nonce-1234'" },
    body: html.replace(/<script type="module"[^>]*><\/script>/, ''),
  }));
  await page.goto('/');
  // (The app's own styles aren't loaded here: the body keeps its 8 pixels; what counts is nothing between it and #app.)
  expect(await page.evaluate(() => ({
    above: document.getElementById('app')!.getBoundingClientRect().top - document.body.getBoundingClientRect().top,
    filters: getComputedStyle(document.querySelector('svg.filters')!).position,
  }))).toEqual({ above: 0, filters: 'absolute' });
});

test('FOLD-1: 允许折叠 folds subscriptions, API and other into one bar with quick-glance plan usage, unfolding in place', async ({ page }) => {
  await open(page);
  // Before enabling 允许折叠: lower rows are shown directly with no combined fold toggle.
  await expect(page.locator('.rows-fold')).toHaveCount(0);
  await expect(page.locator('.row', { hasText: 'Kimi API' })).toBeVisible();
  // Enable 允许折叠 in 外观.
  await button(page, '主页模块与设置').click();
  await page.getByText('外观', { exact: true }).click();
  await page.getByRole('switch', { name: /^允许折叠/ }).click();
  await page.keyboard.press('Escape');
  await page.keyboard.press('Escape');
  // Lower groups collapse into one combined bar showing subscription preview; individual rows are hidden.
  const toggle = page.locator('.rows-fold');
  await expect(toggle).toBeVisible();
  await expect(toggle).toHaveAttribute('aria-expanded', 'false');
  await expect(toggle.locator('.fold-preview')).toBeVisible();
  await expect(toggle).toContainText('ChatGPT');
  await expect(page.locator('.row', { hasText: 'Kimi API' })).toHaveCount(0);
  // Clicking the bar expands subscriptions, API, and other rows in place.
  await toggle.click();
  await expect(toggle).toHaveAttribute('aria-expanded', 'true');
  await expect(page.locator('.row', { hasText: 'Kimi API' })).toBeVisible();
  // Clicking again collapses them back into one bar.
  await toggle.click();
  await expect(toggle).toHaveAttribute('aria-expanded', 'false');
  await expect(page.locator('.row', { hasText: 'Kimi API' })).toHaveCount(0);
});

test('PERF-10: LiveFeed.smoothRates reacts immediately on landing without 10s delay and decays naturally when idle', async ({ page }) => {
  await open(page);
  const result = await page.evaluate(async () => {
    const { LiveFeed } = await import('/src/lib/live.ts');
    const feed = new LiveFeed();
    const base = {
      rangeKey: 'today', rangeLabel: '今天', chartUnit: '小时', chartSpan: '00:00–10:00',
      providers: [{ id: 'codex', name: 'ChatGPT', available: true, tokens: 1000, output: 1000, requests: 1, cost: 0.01, unpriced: 0, cache: null, models: [], history: [] }],
      dates: ['09:00', '10:00'],
    };
    const t0 = 1_700_000_000_000;
    // Steady baseline for 60s, then a sudden 1000-token output burst lands at t0 + 60_000 (= 100 Token/s instantaneous peak).
    for (let s = 0; s < 60; s += 2) feed.add(base as never, t0 + s * 1000);
    const burst = { ...base, providers: [{ ...base.providers[0], tokens: 2000, output: 2000, requests: 2 }] };
    feed.add(burst as never, t0 + 60_000);
    // Right at landing (t = t0 + 60_000): leading edge jumps immediately (>= 60% of raw peak).
    const timesAtLand = Array.from({ length: 61 }, (_, i) => t0 + i * 1000);
    const smoothedAtLand = feed.smoothRates(timesAtLand);
    const atLand = smoothedAtLand.at(-1)!;
    // Fractional millisecond check (t0 + 60_350): headline feed.rate(now) must exactly match curve endpoint smoothRates(times).at(-1).
    const fracNow = t0 + 60_350;
    const fracTimes = [...timesAtLand, fracNow];
    const fracCurveEnd = feed.smoothRates(fracTimes).at(-1)!;
    const fracHeadline = feed.rate(fracNow)!;
    // Two seconds later: crests smoothly; 20 seconds later with no more output: clearly decaying (no 60s flat plateau).
    feed.add(burst as never, t0 + 62_000);
    const times = Array.from({ length: 81 }, (_, i) => t0 + i * 1000);
    const smoothed = feed.smoothRates(times);
    const raw60 = feed.rateSeries([t0 + 60_000])[0] ?? 0;
    const at2s = smoothed[62];
    const at20s = smoothed[80];
    const solo = feed.rate(t0 + 80_000)!;
    return { raw60, atLand, fracCurveEnd, fracHeadline, at2s, at20s, solo, minVal: Math.min(...smoothed) };
  });
  expect(result.raw60).toBeCloseTo(100, 1);
  // Immediate onset: >= 60% of raw peak visible at landing (t=0) and >= 75% crest by t=+2s.
  expect(result.atLand / result.raw60).toBeGreaterThanOrEqual(0.6);
  expect(result.at2s / result.raw60).toBeGreaterThanOrEqual(0.75);
  // Headline rate and curve endpoint agree identically even at fractional milliseconds right after landing.
  expect(Math.abs(result.fracHeadline - result.fracCurveEnd)).toBeLessThan(0.01);
  // Natural decay once idle (after 20s, down below 25% of peak instead of staying flat for 60s).
  expect(result.at20s).toBeLessThan(result.at2s * 0.25);
  // Headline rate (solo) stays consistent with curve endpoint and curve is non-negative.
  expect(Math.abs(result.solo - result.at20s)).toBeLessThan(0.05);
  expect(result.minVal).toBeGreaterThanOrEqual(0);
});

test('closing the panel from 模型/项目/会话 resets to 工具 while hidden so reopening never visibly jumps', async ({ page }) => {
  await open(page);
  const tab = (name: string) => page.getByRole('radiogroup', { name: '卡片分组' }).getByRole('radio', { name, exact: true });
  await tab('模型').click();
  await expect(tab('模型')).toHaveAttribute('aria-checked', 'true');
  // Closing the panel immediately resets back to 工具 while hidden (before panel-opened even arrives).
  await page.evaluate(() => window.dispatchEvent(new CustomEvent('panel-closed')));
  await expect(tab('工具')).toHaveAttribute('aria-checked', 'true');
  await expect(page.locator('.list .card .name').first()).toHaveText('Claude');
  // Reopening finds 工具 already painted on the first frame.
  await page.evaluate(() => window.dispatchEvent(new CustomEvent('panel-opened', { detail: { modules: false, maxHeight: 900 } })));
  await expect(tab('工具')).toHaveAttribute('aria-checked', 'true');
  await expect(page.locator('.list .card .name').first()).toHaveText('Claude');
});

test('a running tool keeps its live line continuous along 0 to 现在 instead of breaking off mid-chart', async ({ page }) => {
  await open(page);
  const result = await page.evaluate(async () => {
    const { drawLines } = await import('/src/lib/liveLines.ts');
    const canvas = document.createElement('canvas');
    canvas.width = 300;
    canvas.height = 100;
    const ctx = canvas.getContext('2d')!;
    const points = (ys: number[]) => ys.map((y, i): [number, number] => [i * 50, y]);
    const base = 90;
    const total = points([40, 50, 60, 50, 60, 60]);
    const gemini = points([50, 55, 60, 50, 60, 60]);
    // Claude was active on the left, then rested at 0 (y = base) on the right while still running.
    const claude = points([80, 85, 90, 90, 90, 90]);
    const drawnRunning = drawLines(ctx, {
      total, base, plotTop: 10, totalColour: '#00ffaa', areaColour: '#00ffaa', dark: true,
      tools: [
        { id: 'gemini', points: gemini, colour: '#00aaff', running: true },
        { id: 'claude', points: claude, colour: '#ff8800', running: true },
      ],
    });
    // Continuous running line is clearly visible along 0 (1.2px stroke splits across y=89..90); stopped tool fades back.
    const bandAlpha = () => Math.max(ctx.getImageData(225, base - 1, 1, 1).data[3], ctx.getImageData(225, base, 1, 1).data[3]);
    const runningAlpha = bandAlpha();
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    const drawnStopped = drawLines(ctx, {
      total, base, plotTop: 10, totalColour: '#00ffaa', areaColour: '#00ffaa', dark: true,
      tools: [
        { id: 'gemini', points: gemini, colour: '#00aaff', running: true },
        { id: 'claude', points: claude, colour: '#ff8800', running: false },
      ],
    });
    const stoppedAlpha = bandAlpha();
    return {
      runningDots: drawnRunning.dots,
      stoppedDots: drawnStopped.dots,
      runningAlpha,
      stoppedAlpha,
    };
  });
  expect(result.runningDots).toContain('claude');
  expect(result.stoppedDots).not.toContain('claude');
  expect(result.runningAlpha).toBeGreaterThan(90);
  expect(result.stoppedAlpha).toBeLessThan(40);
  expect(result.runningAlpha).toBeGreaterThan(result.stoppedAlpha * 4);
});

test('在手机上看: 任何网络 through your own domain: its address, where the proxy runs, the address to forward to, its code checked, 停止 on its own, kept for next time', async ({ page }) => {
  // 用户：域名和固定端口那里……逻辑上感觉怪怪的不符合直觉. A domain is for a phone anywhere: one of 任何网络's two links,
  // beside Cloudflare's. 用户：反代转发到，应该是0.0.0.0吧，127.0.0.1感觉不太对？……这样不就把同一网络访问打开了？应该
  //怎么区分呢？ Where the proxy runs decides both the address to forward to and whether the network gets in.
  await open(page, '?tailnet');
  await button(page, '在手机上看').click();
  const phone = page.locator('.phone-page');
  const local = page.getByRole('radio', { name: '同一个网络' }), anywhere = page.getByRole('radio', { name: '任何网络' });
  const cloudflare = page.getByRole('radio', { name: 'Cloudflare 临时链接' }), own = page.getByRole('radio', { name: '自有域名' });
  const here = page.getByRole('radio', { name: '这台电脑上' }), nearby = page.getByRole('radio', { name: 'NAS、路由器上' });
  const target = phone.locator('.forward .address');
  await button(page, '开始').click();
  await expect(phone.locator('svg.qr')).toHaveCount(2);
  // The same network's page as it was: no domain, no port there.
  await expect(phone.getByRole('textbox')).toHaveCount(0);
  await anywhere.click();
  // Cloudflare's link until a domain is set, as before.
  await expect(cloudflare).toHaveAttribute('aria-checked', 'true');
  await expect(phone.getByRole('status')).toContainText('经 Cloudflare 的临时链接');
  await own.click();
  const domain = phone.getByRole('textbox', { name: '域名' });
  // The proxy on this computer to begin with: this computer's own address with the server's port, the network kept out.
  await expect(here).toHaveAttribute('aria-checked', 'true');
  await expect(target).toHaveText('127.0.0.1:51234');
  await expect(phone.getByText('只接受这台电脑转来的连接，局域网里的设备连不上')).toBeVisible();
  // On a NAS or the router: this computer's network address, the network let in with the domain's key only.
  await nearby.click();
  await expect(target).toHaveText('192.168.1.20:51234');
  await expect(phone.getByText('局域网里的设备要带这个域名的口令才能连上，同一个网络的二维码不受影响')).toBeVisible();
  // The arrow keys move the choice, as on a segmented control.
  await nearby.focus();
  await page.keyboard.press('ArrowLeft');
  await expect(here).toHaveAttribute('aria-checked', 'true');
  await expect(here).toBeFocused();
  await expect(target).toHaveText('127.0.0.1:51234');
  // The port is the server's (同一个网络's codes use it too): no field for it until 换端口.
  await expect(phone.getByRole('textbox', { name: '端口' })).toHaveCount(0);
  await expect(button(page, '开始')).toBeDisabled();
  await phone.getByRole('button', { name: '换端口' }).click();
  const port = phone.getByRole('textbox', { name: '端口' });
  await expect(port).toBeFocused();
  await expect(port).toHaveValue('51234');
  // Another port: the address to forward to with it at once, and what moving the server does.
  await domain.fill('meter.example.com/starmeter');
  await port.fill('8765');
  await expect(target).toHaveText('127.0.0.1:8765');
  await expect(phone.locator('.note.caution')).toHaveText('换了端口：同一个网络的二维码改用新端口，手机要重新扫；Cloudflare 链接会关闭');
  await button(page, '开始').click();
  await expect(phone.locator('.failed')).toHaveText('域名的格式不对：只填域名，比如 meter.example.com');
  await expect(phone.locator('svg.qr')).toHaveCount(0);
  await domain.fill('https://Meter.Example.com/');
  await domain.press('Enter');
  // On: its code, checked through the user's proxy, and where it forwards; a green dot after it and after 任何网络.
  await expect(phone.locator('svg.qr')).toHaveCount(1);
  await expect(phone.locator('.address')).toHaveText('https://meter.example.com/');
  await expect(phone.locator('.check.ok')).toHaveText('已确认能通过 meter.example.com 打开');
  await expect(phone.getByText('经这台电脑上的反代转发到 127.0.0.1:8765，局域网里的设备连不上')).toBeVisible();
  await expect(own.locator('.on')).toHaveCount(1);
  await expect(cloudflare.locator('.on')).toHaveCount(0);
  await expect(anywhere.locator('.on')).toHaveCount(1);
  // One server, one port: the same network's codes moved to it, still opening.
  await local.click();
  await expect(phone.locator('.address')).toHaveText(['192.168.1.20:8765', '100.76.55.73:8765']);
  // Its 停止: its own link only; its address, port and where the proxy runs stay for the next 开始.
  await anywhere.click();
  await button(page, '停止').click();
  await expect(phone.locator('svg.qr')).toHaveCount(0);
  await expect(domain).toHaveValue('meter.example.com');
  await expect(target).toHaveText('127.0.0.1:8765');
  await expect(phone.getByText('原来的二维码和链接已失效')).toBeVisible();
  await expect(anywhere.locator('.on')).toHaveCount(0);
  await expect(local.locator('.on')).toHaveCount(1);
  // On again with the proxy on a NAS, then the app started again: on as it was left, its page shown.
  await nearby.click();
  await button(page, '开始').click();
  await expect(phone.locator('.address')).toHaveText('https://meter.example.com/');
  await expect(phone.getByText('经 NAS 或路由器上的反代转发到这台电脑的 192.168.1.20:8765')).toBeVisible();
  await open(page, '?tailnet');
  await expect(phoneDot(page)).toBeVisible();
  await button(page, '在手机上看').click();
  await anywhere.click();
  await expect(own).toHaveAttribute('aria-checked', 'true');
  await expect(phone.locator('.address')).toHaveText('https://meter.example.com/');
  await expect(phone.getByText('经 NAS 或路由器上的反代转发到这台电脑的 192.168.1.20:8765')).toBeVisible();
});

test('在手机上看: Cloudflare\'s link and your own domain on at once, each stopping on its own', async ({ page }) => {
  // 用户：可以同时开cloudflare和自有域名.
  await open(page);
  await button(page, '在手机上看').click();
  const phone = page.locator('.phone-page');
  const anywhere = page.getByRole('radio', { name: '任何网络' });
  const cloudflare = page.getByRole('radio', { name: 'Cloudflare 临时链接' }), own = page.getByRole('radio', { name: '自有域名' });
  await anywhere.click();
  await own.click();
  await phone.getByRole('textbox', { name: '域名' }).fill('meter.example.com');
  await button(page, '开始').click();
  await expect(phone.locator('.address')).toHaveText('https://meter.example.com/');
  // Looking at Cloudflare's page starts nothing; its 开始 starts its link, the domain's staying on.
  await cloudflare.click();
  await expect(phone.getByRole('status')).toContainText('经 Cloudflare 的临时链接');
  await button(page, '开始').click();
  await expect(phone.locator('.address')).toHaveText('https://quiet-river-1234.trycloudflare.com/');
  await expect(cloudflare.locator('.on')).toHaveCount(1);
  await expect(own.locator('.on')).toHaveCount(1);
  await own.click();
  await expect(phone.locator('.address')).toHaveText('https://meter.example.com/');
  // The domain's 停止: Cloudflare's link as it was.
  await button(page, '停止').click();
  await expect(own.locator('.on')).toHaveCount(0);
  await expect(cloudflare.locator('.on')).toHaveCount(1);
  await cloudflare.click();
  await expect(phone.locator('.address')).toHaveText('https://quiet-river-1234.trycloudflare.com/');
  // Cloudflare's 停止 with the domain on again: the domain's link as it was.
  await own.click();
  await button(page, '开始').click();
  await cloudflare.click();
  await button(page, '停止').click();
  await expect(phone.getByRole('status')).toContainText('外网链接已关闭');
  await expect(cloudflare.locator('.on')).toHaveCount(0);
  await expect(own.locator('.on')).toHaveCount(1);
  await expect(anywhere.locator('.on')).toHaveCount(1);
});

test('在手机上看: your own domain says why it didn\'t start or doesn\'t open, and checks again', async ({ page }) => {
  const start = async (query: string, typed: string) => {
    await open(page, query);
    await button(page, '在手机上看').click();
    await page.getByRole('radio', { name: '任何网络' }).click();
    await page.getByRole('radio', { name: '自有域名' }).click();
    await page.getByRole('textbox', { name: '域名' }).fill(typed);
    await button(page, '开始').click();
  };
  const phone = page.locator('.phone-page');
  const answerCheck = () => page.evaluate(() => (window as unknown as { answerCheck: () => void }).answerCheck());
  // The port taken by another app: said under the fields, nothing on, the port's field open to pick another.
  await start('?port-taken', 'meter.example.com');
  await expect(phone.locator('.failed')).toHaveText('端口 51234 被别的程序占用');
  await expect(phone.locator('svg.qr')).toHaveCount(0);
  await expect(phone.getByRole('textbox', { name: '端口' })).toBeFocused();
  // The proxy answering with its own page: on, with what to look at, and 再检查 (the check held until answered, so
  // its wait is seen whatever the machine's pace).
  await start('?domain-502&check-held', 'meter.example.com');
  await expect(phone.locator('.check')).toHaveText('正在确认手机能打开…');
  await answerCheck();
  const caution = phone.locator('.check.caution');
  await expect(caution).toContainText('meter.example.com 有回应（HTTP 502），但不是 Starmeter：检查反代是否转发到 127.0.0.1:51234');
  await caution.getByRole('button', { name: '再检查' }).click();
  await expect(phone.locator('.check')).toHaveText('正在确认手机能打开…');
  await answerCheck();
  await expect(caution).toBeVisible();
  await button(page, '停止').click();
  // Nothing answering: why, in words to act on, as a caution (a phone outside may still get through).
  await start('?domain-unreachable', 'meter.example.com');
  await expect(caution).toContainText('这台电脑没能打开 meter.example.com（找不到这个域名）');
  await button(page, '停止').click();
  // The asking itself failing: said so, with 再检查, not left blank.
  await start('?check-fails', 'meter.example.com');
  await expect(caution).toContainText('没能检查 meter.example.com 能不能打开');
  await expect(caution.getByRole('button', { name: '再检查' })).toBeVisible();
  await button(page, '停止').click();
  // An http address sent on to https: to give it as https.
  await start('?domain-redirect', 'http://home.lan:8080');
  await expect(caution).toContainText('总是跳到 https 时，把域名改成 https://home.lan:8080');
  await expect(phone.getByText('域名用 https 时才能用')).toBeVisible();
});

test('在手机上看: your own domain starts once for Return pressed twice, and moving the port closes Cloudflare\'s link without calling it a failure', async ({ page }) => {
  // The host takes its time (a held port asked again): a second Return while it is on its way does nothing, and the
  // first start's answer is shown.
  await open(page, '?slow-domain');
  await button(page, '在手机上看').click();
  const phone = page.locator('.phone-page');
  const cloudflare = page.getByRole('radio', { name: 'Cloudflare 临时链接' }), own = page.getByRole('radio', { name: '自有域名' });
  await page.getByRole('radio', { name: '任何网络' }).click();
  // Cloudflare's link on first.
  await button(page, '开始').click();
  await expect(phone.locator('.address')).toHaveText('https://quiet-river-1234.trycloudflare.com/');
  await own.click();
  const domain = phone.getByRole('textbox', { name: '域名' });
  await domain.fill('meter.example.com');
  await phone.getByRole('button', { name: '换端口' }).click();
  await phone.getByRole('textbox', { name: '端口' }).fill('8765');
  await expect(phone.locator('.note.caution')).toContainText('Cloudflare 链接会关闭');
  await domain.press('Enter');
  // While it is on its way: the port emptied and Return again, which once dropped the first start's answer and left
  // 开始 greyed out for good.
  await phone.getByRole('textbox', { name: '端口' }).fill('');
  await domain.press('Enter');
  await expect(phone.locator('.failed')).toHaveCount(0);
  await expect(phone.locator('svg.qr')).toHaveCount(1);
  await expect(phone.locator('.address')).toHaveText('https://meter.example.com/');
  await expect(button(page, '停止')).toBeEnabled();
  // Cloudflare's page: closed by the move, said as stopped, not as a link that failed.
  await cloudflare.click();
  await expect(phone.getByRole('status')).toContainText('外网链接已关闭');
  await expect(phone.getByText('没能生成外网链接')).toHaveCount(0);
  await expect(cloudflare.locator('.on')).toHaveCount(0);
  await expect(own.locator('.on')).toHaveCount(1);
});

test('typing into a field lets the input method\'s candidate window show above the panel, and Return or Esc that end its composition stay with it', async ({ page }) => {
  // 用户：在域名那里打字，联想框会出现在下方被覆盖. 简体拼音's candidate window sits at level 20 to 24, under the unpinned
  // panel's pop-up level (101): while a field has the focus the page tells the shell, which floats the panel meanwhile.
  await open(page);
  await page.evaluate(async () => {
    const { shell } = await import('/src/lib/backend.ts');
    (window as unknown as { told: boolean[] }).told = [];
    shell.typing = (typing: boolean) => { (window as unknown as { told: boolean[] }).told.push(typing); return Promise.resolve(); };
  });
  const told = () => page.evaluate(() => (window as unknown as { told: boolean[] }).told);
  const typing = async () => (await told()).at(-1) ?? false;
  // A key as WebKit hands it over while 拼音 is composing: Return after the composition has ended, so only its key code
  // says so; Esc while it is still on.
  const imeKey = (field: ReturnType<Page['locator']>, key: string) => field.evaluate((element, key) => element.dispatchEvent(
    new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, keyCode: 229, isComposing: key === 'Escape' })), key);

  await button(page, '在手机上看').click();
  const phone = page.locator('.phone-page');
  await page.getByRole('radio', { name: '任何网络' }).click();
  await page.getByRole('radio', { name: '自有域名' }).click();
  await phone.getByRole('button', { name: '换端口' }).click();
  const domain = phone.getByRole('textbox', { name: '域名' }), port = phone.getByRole('textbox', { name: '端口' });
  await expect.poll(typing).toBe(true);
  // From the port to the domain is one stretch of typing: nothing goes back up in between.
  await domain.click();
  await page.waitForTimeout(100);
  expect(await told()).toEqual([true]);
  await port.click();
  await domain.click();
  await page.waitForTimeout(100);
  expect(await told()).toEqual([true]);
  await domain.click();
  await page.keyboard.type('meter.example.com');
  // Return that puts 拼音's letters in, and Esc that drops them: the page neither starts nor goes back.
  await imeKey(domain, 'Enter');
  await imeKey(domain, 'Escape');
  await page.waitForTimeout(300);
  await expect(phone).toBeVisible();
  await expect(phone.locator('svg.qr')).toHaveCount(0);
  await expect(button(page, '开始')).toBeVisible();
  // Return of its own starts it.
  await domain.press('Enter');
  await expect(phone.locator('svg.qr')).toHaveCount(1);
  // No field left: the panel is back at its level.
  await expect.poll(typing).toBe(false);
  expect(await told()).toEqual([true, false]);
  await page.keyboard.press('Escape');
  await expect(phone).toHaveCount(0);

  // A question under its row the same: typing into it, and the input method's Return and Esc left to it.
  await page.locator('.row').filter({ hasText: 'ChatGPT 订阅' }).click();
  await page.getByText('订阅月费…').click();
  const field = page.locator('.ask .field');
  await expect(field).toBeFocused();
  await expect.poll(typing).toBe(true);
  await field.fill('30');
  await imeKey(field, 'Enter');
  await imeKey(field, 'Escape');
  await page.waitForTimeout(300);
  await expect(field).toBeVisible();
  await page.keyboard.press('Enter');
  await expect(page.locator('.ask')).toHaveCount(0);
  await expect(page.getByRole('button', { name: /订阅月费/ })).toContainText('$30.00');
  await expect.poll(typing).toBe(false);
  // Told once as each field took the focus and once as it went, no more.
  expect(await told()).toEqual([true, false, true, false]);
});
