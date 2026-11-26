// The panel in English: every page the fixtures reach, read for any Chinese left over. The text on screen, the labels a
// screen reader hears and the words drawn on the charts and share pictures all count. A failure lists what en.ts is
// missing.
import { expect, test, type Page } from '@playwright/test';
import { liveWork, PHONE_KEY, servePhone, sessionTitles } from './phone';

// Every page, one after another: longer than a test usually takes.
test.describe.configure({ timeout: 120_000 });

/** Chinese that is meant to stay: the languages' own names in the language row and the tour's first card. */
const KEPT = new Set(['中文', '简体中文', '繁體中文']);
const CJK = /[㐀-鿿（-？、-。]/;

async function english(page: Page, query = '', shown = '.dashboard') {
  await page.addInitScript(() => {
    localStorage.setItem('starmeter.language', 'en');
    const drawn = new Set<string>();
    (window as unknown as { drawn: Set<string> }).drawn = drawn;
    const fill = CanvasRenderingContext2D.prototype.fillText;
    CanvasRenderingContext2D.prototype.fillText = function (text: string, ...rest: [number, number, number?]) {
      drawn.add(String(text));
      return fill.call(this, text, ...rest);
    };
  });
  await page.goto('/' + query);
  await expect(page.locator(shown)).toBeVisible();
  await page.waitForTimeout(300);
}

/** What the page shows and says now: its text, its labels and what has been drawn on its canvases. */
async function read(page: Page): Promise<string[]> {
  return page.evaluate(() => {
    const lines = document.body.innerText.split('\n');
    for (const element of document.querySelectorAll('[aria-label], [alt], [title]')) {
      for (const name of ['aria-label', 'alt', 'title']) { const value = element.getAttribute(name); if (value) lines.push(value); }
    }
    lines.push(...(window as unknown as { drawn: Set<string> }).drawn);
    return lines.map((line) => line.trim()).filter(Boolean);
  });
}

class Leftovers {
  found = new Map<string, string>();
  async take(page: Page, where: string) {
    for (const line of await read(page)) if (CJK.test(line) && !KEPT.has(line) && !this.found.has(line)) this.found.set(line, where);
  }
  get list() { return [...this.found].map(([line, where]) => `${where}: ${line}`); }
}

/** Opens every row on the page that unfolds, again and again until none is left closed. */
async function unfold(page: Page) {
  for (let round = 0; round < 6; round++) {
    const closed = page.locator('.detail-row[aria-expanded="false"]');
    const count = await closed.count();
    if (!count) return;
    for (let i = 0; i < count; i++) await closed.first().click({ timeout: 2000 }).catch(() => {});
    await page.waitForTimeout(150);
  }
}

async function back(page: Page) {
  await page.keyboard.press('Escape');
  await page.waitForTimeout(450);
}

/** Every homepage row that opens a page (it has an arrow; others run an action): open it, unfold it, read it, come
 *  back. */
async function rows(page: Page, left: Leftovers, where: string) {
  const pages = page.locator('.home .row.tall', { has: page.locator('.arrow') });
  const titles = await pages.locator('.title').allTextContents();
  for (const title of titles) {
    const row = pages.filter({ has: page.locator('.title', { hasText: title }) }).first();
    if (!(await row.count())) continue;
    if (!(await row.click({ timeout: 2000 }).then(() => true, () => false))) continue;
    await page.waitForTimeout(450);
    await unfold(page);
    await left.take(page, `${where} › ${title}`);
    await back(page);
  }
}

test('English: a plan on its reserve names its single bar with its share first, as the headline says it', async ({ page }) => {
  await english(page);
  await page.evaluate(async () => {
    const loaded = performance.getEntriesByType('resource').map((entry) => entry.name).filter((name) => name.includes('/src/lib/backend.ts'));
    const { backend } = await import(/* @vite-ignore */ loaded.at(-1) ?? '/src/lib/backend.ts');
    const real = backend.snapshot;
    backend.snapshot = async () => {
      const shot = await real();
      for (const entry of shot.entries as { label?: string; module?: string }[]) {
        if (entry.module === 'chatgpt' && entry.label === 'ChatGPT 订阅') {
          Object.assign(entry, { value: '余 45,934 Credits', usageLevel: 'warning', share: 100, shareLevel: 'critical', detail: '本周已用 100%' });
          delete (entry as { gauges?: unknown }).gauges;
        }
      }
      return shot;
    };
    await backend.action(['set-group', 'tool']);
  });
  const row = page.locator('.row', { hasText: 'ChatGPT plan' });
  await expect(row.locator('.gauge-name')).toHaveText('100% used this week');
  await expect(row.locator('.gauge-label')).toHaveText('used this week');
});

test('English: a plan on its reserve with one window of another name says it in English, its share first (R6-2)', async ({ page }) => {
  for (const [detail, said, label] of [['5 小时已用 100%', '100% used in 5 hours', 'used in 5 hours'],
    ['Codex Spark已用 42.5%', '42.5% used on Codex Spark', 'used on Codex Spark']]) {
    await english(page);
    await page.evaluate(async (detail) => {
      const loaded = performance.getEntriesByType('resource').map((entry) => entry.name).filter((name) => name.includes('/src/lib/backend.ts'));
      const { backend } = await import(/* @vite-ignore */ loaded.at(-1) ?? '/src/lib/backend.ts');
      const real = backend.snapshot;
      backend.snapshot = async () => {
        const shot = await real();
        for (const entry of shot.entries as { label?: string; module?: string }[]) {
          if (entry.module === 'chatgpt' && entry.label === 'ChatGPT 订阅') {
            Object.assign(entry, { value: '余 45,934 Credits', usageLevel: 'warning', share: 100, shareLevel: 'critical', detail });
            delete (entry as { gauges?: unknown }).gauges;
          }
        }
        return shot;
      };
      await backend.action(['set-group', 'tool']);
    }, detail);
    const row = page.locator('.row', { hasText: 'ChatGPT plan' });
    await expect(row.locator('.gauge-name')).toHaveText(said);
    await expect(row.locator('.gauge-label')).toHaveText(label);
  }
});

test('English: the homepage, its pages and the card groupings', async ({ page }) => {
  const left = new Leftovers();
  await english(page);
  await left.take(page, 'home');
  // Tokens are counted in the plural, a plan's limits are named under their bars, and the heatmap is made of dots.
  await expect(page.locator('.card', { has: page.locator('.name', { hasText: /^Magpie$/ }) })).toContainText('7.24M tokens');
  await expect(page.locator('.row', { hasText: 'Claude plan' }).locator('.value')).toHaveText('76% used this week');
  await expect(page.locator('.row', { hasText: 'Claude plan' }).locator('.gauge-name')).toHaveText(['5 hours 22%', 'This week 76%']);
  await expect(page.locator('.row', { hasText: 'Gemini plan' }).locator('.gauge-name')).toHaveText(['5 hours 24%', 'This week 9%', '5 hours 6%', 'This week 18%']);
  expect(await page.locator('.row', { hasText: 'Gemini plan' }).locator('.marks').evaluateAll((all) => all.map((one) => one.getAttribute('aria-label')))).toEqual(['Gemini', 'Claude and GPT']);
  for (const name of await page.locator('.row', { hasText: 'Gemini plan' }).locator('.gauge-label').all()) {
    expect(await name.evaluate((label) => label.scrollWidth <= label.clientWidth)).toBe(true);
  }
  await expect(page.locator('.heatmap')).toContainText('1 hour per dot');
  const tabs = await page.getByRole('radiogroup', { name: 'Card grouping' }).getByRole('radio').all();
  for (const tab of tabs) {
    await tab.click();
    await page.waitForTimeout(400);
    await left.take(page, `home › ${await tab.textContent()}`);
  }
  await tabs[0].click();
  await rows(page, left, 'home');
  // A tool's own page.
  await page.locator('.card').first().click({ timeout: 5000 });
  await page.waitForTimeout(450);
  await left.take(page, 'tool page');
  expect(left.list).toEqual([]);
});

for (const [name, query] of [['with usage', '?guide'], ['while reading', '?fixture=pending&guide'], ['with nothing found', '?fixture=empty&guide']]) {
  test(`English: the first-run tour ${name}`, async ({ page }) => {
    const left = new Leftovers();
    await english(page, query);
    await expect(page.locator('.guide h2')).toHaveText('Welcome to Starmeter');
    for (let stop = 0; stop < 8 && (await page.locator('.guide').count()); stop++) {
      await left.take(page, `tour ${stop}`);
      await page.locator('.guide .step.primary').click();
      await page.waitForTimeout(350);
    }
    await expect(page.locator('.guide')).toHaveCount(0);
    expect(left.list).toEqual([]);
  });
}

test('English: the phone view and its public link', async ({ page }) => {
  const left = new Leftovers();
  await english(page, '?no-cloudflared');
  await page.getByRole('button', { name: 'Phone view', exact: true }).click();
  await left.take(page, 'phone, not started');
  await page.getByRole('button', { name: 'Start', exact: true }).click();
  await expect(page.locator('.phone-page svg[role="img"]')).toBeVisible();
  await left.take(page, 'phone');
  await page.getByRole('radio', { name: 'Any network' }).click();
  await expect(page.locator('.phone-page').getByRole('status')).toContainText('Not started');
  await left.take(page, 'phone › public');
  await page.getByRole('button', { name: 'Download and start' }).click();
  await expect(page.locator('.phone-page .address')).toContainText('trycloudflare');
  await left.take(page, 'phone › public on');
  expect(left.list).toEqual([]);
});

test('English: on the phone, 网页版 with its sessions, and the share page', async ({ page }) => {
  const left = new Leftovers();
  await servePhone(page);
  await english(page, '#k=' + PHONE_KEY + '&l=en', '.stage');
  await expect(page.locator('.window.phone')).toBeVisible();
  await left.take(page, 'phone');
  await page.getByRole('radio', { name: 'Sessions' }).click();
  await left.take(page, 'phone › sessions');
  await page.getByRole('button', { name: 'Share usage picture', exact: true }).click();
  await expect(page.locator('.share-page .stage img')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Save picture' })).toBeVisible();
  await left.take(page, 'phone › share');
  // Session titles are the user's own words, never translated; the fixtures' English for them is the dev server's only.
  const titles = new Set(sessionTitles());
  expect(left.list.filter((line) => !titles.has(line.replace(/^.*?: /, '')))).toEqual([]);
});

test('English: on the phone, 网页版 with each grouping, a card and a session open, and both ways up', async ({ page }) => {
  const left = new Leftovers();
  // Work going on a minute-long cycle: requests landing, tools called, a turn done, resting, waking.
  const clock = { now: Date.now() };
  await servePhone(page, PHONE_KEY, { live: liveWork({ now: () => clock.now, length: 1000, claudeEnds: 20, codexEnds: 12 }) });
  await page.setViewportSize({ width: 393, height: 852 });
  await english(page, '#k=' + PHONE_KEY + '&l=en', '.stage');
  await page.waitForTimeout(1400);
  clock.now += 9000;
  await expect(page.locator('.stage .feed .event').first()).toBeVisible({ timeout: 5000 });
  await left.take(page, 'phone');
  clock.now += 12_000;
  await expect(page.locator('.stage .turn-done')).toBeVisible({ timeout: 5000 });
  await left.take(page, 'phone › a turn done');
  clock.now += 60_000;
  await expect(page.locator('.stage .monitor:not(.working)')).toBeVisible({ timeout: 7000 });
  await left.take(page, 'phone › resting');
  clock.now += 1000 * 1000;
  await expect(page.locator('.stage .feed .event.wake')).toBeVisible({ timeout: 7000 });
  await left.take(page, 'phone › waking');
  for (const name of ['Model', 'Project', 'Sessions', 'Tool']) {
    await page.getByRole('radiogroup', { name: 'Card grouping' }).getByRole('radio', { name }).click();
    await page.waitForTimeout(700);
    if (name === 'Sessions') await page.locator('.stage .session .head').first().click();
    if (name === 'Tool') await page.locator('.stage .card .head').first().click();
    await page.waitForTimeout(400);
    await left.take(page, 'phone › ' + name);
  }
  const chart = (await page.locator('.stage .chart').boundingBox())!;
  await page.mouse.move(chart.x + chart.width / 2, chart.y + chart.height / 2);
  await left.take(page, 'phone › the trend read');
  await page.setViewportSize({ width: 852, height: 393 });
  await page.waitForTimeout(400);
  await left.take(page, 'phone › landscape');
  const titles = new Set(sessionTitles());
  // Titles are the user's own words wherever they stand, a line of their own or within one (最近：<title>).
  const untitled = (line: string) => [...titles].reduce((rest, title) => rest.split(title).join(''), line.replace(/^.*?: /, ''));
  expect(left.list.filter((line) => CJK.test(untitled(line)))).toEqual([]);
});

test('English: the module grid and every module\'s settings', async ({ page }) => {
  const left = new Leftovers();
  await english(page);
  await page.getByRole('button', { name: 'Modules and settings', exact: true }).click();
  await page.waitForTimeout(450);
  await left.take(page, 'modules');
  const names = await page.locator('.modules .cell .name').allTextContents();
  for (const name of names) {
    const cell = page.locator('.modules .cell', { has: page.locator('.name', { hasText: name }) }).first();
    if (!(await cell.locator('.name').click({ timeout: 2000 }).then(() => true, () => false))) continue;
    await page.waitForTimeout(450);
    if (await page.locator('.modules').count()) continue;
    await unfold(page);
    await left.take(page, `modules › ${name}`);
    await back(page);
  }
  expect(left.list).toEqual([]);
});

test('English: pinned, the live chart and the share pictures', async ({ page }) => {
  const left = new Leftovers();
  await english(page);
  await page.getByRole('button', { name: 'Pin the panel', exact: true }).click();
  await page.waitForTimeout(5000);
  await left.take(page, 'pinned');
  await page.getByRole('button', { name: 'Unpin the panel', exact: true }).click();
  await page.getByRole('button', { name: 'Share usage picture', exact: true }).click();
  await page.waitForTimeout(800);
  const styles = page.getByRole('button', { name: /^Style · / });
  await styles.click();
  const options = await page.getByRole('option').allTextContents();
  await page.keyboard.press('Escape');
  for (const option of options) {
    await styles.click();
    await page.getByRole('option', { name: option, exact: true }).click();
    await page.waitForTimeout(700);
    await left.take(page, `share › ${option}`);
  }
  expect(left.list).toEqual([]);
});

for (const fixture of ['pending', 'failed']) {
  test(`English: the homepage and its pages, ${fixture}`, async ({ page }) => {
    const left = new Leftovers();
    await english(page, `?fixture=${fixture}`);
    await left.take(page, fixture);
    await rows(page, left, fixture);
    expect(left.list).toEqual([]);
  });
}

test('语言 in 通用 turns the panel English at once, on the same page, and the choice is kept', async ({ page }) => {
  await page.goto('/');
  await expect(page.locator('.dashboard')).toBeVisible();
  await page.getByRole('button', { name: '主页模块与设置', exact: true }).click();
  await page.getByRole('button', { name: '通用', exact: true }).click();
  await page.locator('.detail-row', { hasText: '语言 Language' }).click();
  await page.getByRole('radio', { name: 'English', exact: true }).click();
  // The page stays open and reads in English, its rows measured for it.
  await expect(page.locator('.page .title').first()).toHaveText('General');
  await expect(page.locator('.detail-row', { hasText: 'Language' })).toContainText('English');
  expect(await page.evaluate(() => localStorage.getItem('starmeter.language'))).toBe('en');
  await page.reload();
  await expect(page.locator('.figure-label').first()).toHaveText('Known API estimate');
});
