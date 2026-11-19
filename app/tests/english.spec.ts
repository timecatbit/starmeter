// The panel in English: every page the fixtures reach, read for any Chinese left over. The text on screen, the labels a
// screen reader hears and the words drawn on the charts and share pictures all count. A failure lists what en.ts is
// missing.
import { expect, test, type Page } from '@playwright/test';

// Every page, one after another: longer than a test usually takes.
test.describe.configure({ timeout: 120_000 });

/** Chinese that is meant to stay: the language row's own name for Chinese. */
const KEPT = new Set(['中文']);
const CJK = /[㐀-鿿（-？、-。]/;

async function english(page: Page, query = '') {
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
  await expect(page.locator('.dashboard')).toBeVisible();
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

test('English: the homepage, its pages and the card groupings', async ({ page }) => {
  const left = new Leftovers();
  await english(page);
  await left.take(page, 'home');
  // Tokens are counted in the plural, a plan's windows read mid-sentence, and the heatmap is made of dots.
  await expect(page.locator('.card').first()).toContainText('7.24M tokens');
  await expect(page.locator('.row', { hasText: 'Claude plan' })).toContainText('Used: this week 76%');
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

test('语言 in 外观与设置 turns the panel English at once, on the same page, and the choice is kept', async ({ page }) => {
  await page.goto('/');
  await expect(page.locator('.dashboard')).toBeVisible();
  await page.getByRole('button', { name: '主页模块与设置', exact: true }).click();
  await page.getByRole('button', { name: '外观与设置', exact: true }).click();
  await page.locator('.detail-row', { hasText: '语言 Language' }).click();
  await page.getByRole('radio', { name: 'English', exact: true }).click();
  // The page stays open and reads in English, its rows measured for it.
  await expect(page.locator('.page .title').first()).toHaveText('Settings');
  await expect(page.locator('.detail-row', { hasText: 'Language' })).toContainText('English');
  expect(await page.evaluate(() => localStorage.getItem('starmeter.language'))).toBe('en');
  await page.reload();
  await expect(page.locator('.figure-label').first()).toHaveText('Known API estimate');
});
