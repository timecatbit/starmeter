// The panel in 繁體中文: every page the fixtures reach, read for any Simplified Chinese left over, that is any text the
// converter (hant.ts) would still change. The text on screen, the labels a screen reader hears and the words drawn on
// the charts and share pictures all count; the languages' own names are meant to stay.
import { expect, test, type Page } from '@playwright/test';
import { toHant } from '../ui/src/lib/hant';
import { liveWork, PHONE_KEY, servePhone } from './phone';

test.describe.configure({ timeout: 120_000 });

const KEPT = new Set(['简体中文']);

async function traditional(page: Page, query = '', shown = '.dashboard') {
  await page.addInitScript(() => {
    localStorage.setItem('starmeter.language', 'zh-Hant');
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
    for (const line of await read(page)) if (!KEPT.has(line) && toHant(line) !== line && !this.found.has(line)) this.found.set(line, where);
  }
  get list() { return [...this.found].map(([line, where]) => `${where}: ${line} → ${toHant(line)}`); }
}

async function back(page: Page) {
  await page.keyboard.press('Escape');
  await page.waitForTimeout(450);
}

test('繁體中文: the homepage, its pages, the groupings and 會話', async ({ page }) => {
  const left = new Leftovers();
  await traditional(page);
  await expect(page.locator('html')).toHaveAttribute('lang', 'zh-Hant');
  await left.take(page, 'home');
  // The cards' groupings, each character converted.
  const tabs = page.getByRole('radiogroup', { name: '卡片分組' }).getByRole('radio');
  await expect(tabs).toHaveText(['工具', '模型', '項目', '會話']);
  for (const tab of await tabs.all()) {
    await tab.click();
    await page.waitForTimeout(400);
    await left.take(page, `home › ${await tab.textContent()}`);
  }
  await expect(page.locator('.session-list .empty')).toHaveText('今天活動過的會話 · 金額只算今天');
  await (await tabs.all())[0].click();
  const pages = page.locator('.home .row.tall', { has: page.locator('.arrow') });
  for (const title of await pages.locator('.title').allTextContents()) {
    const row = pages.filter({ has: page.locator('.title', { hasText: title }) }).first();
    if (!(await row.click({ timeout: 2000 }).then(() => true, () => false))) continue;
    await page.waitForTimeout(450);
    await left.take(page, `home › ${title}`);
    await back(page);
  }
  // The modules and settings, and the language row offering each language by its own name.
  await page.getByRole('button', { name: '主頁模塊與設置', exact: true }).click();
  await page.waitForTimeout(450);
  await left.take(page, 'modules');
  await page.getByRole('button', { name: '通用', exact: true }).click();
  await page.waitForTimeout(450);
  await expect(page.locator('.page')).toContainText('語言 Language');
  await left.take(page, 'general');
  expect(left.list).toEqual([]);
});

test('繁體中文 on the phone: 手機版 with each grouping, a card and a session open, both ways up', async ({ page }) => {
  const left = new Leftovers();
  const clock = { now: Date.now() };
  await servePhone(page, PHONE_KEY, { live: liveWork({ now: () => clock.now, length: 1000, claudeEnds: 20, codexEnds: 12 }) });
  await page.setViewportSize({ width: 393, height: 852 });
  await traditional(page, '#k=' + PHONE_KEY + '&l=zh-Hant', '.stage');
  await page.waitForTimeout(1400);
  clock.now += 9000;
  await expect(page.locator('.stage .feed .event').first()).toBeVisible({ timeout: 5000 });
  await left.take(page, 'phone');
  clock.now += 12_000;
  await expect(page.locator('.stage .turn-done')).toBeVisible({ timeout: 5000 });
  await left.take(page, 'phone › 這一輪完成');
  clock.now += 60_000;
  await expect(page.locator('.stage .monitor:not(.working)')).toBeVisible({ timeout: 7000 });
  await left.take(page, 'phone › 休息中');
  for (const name of ['模型', '項目', '會話', '工具']) {
    await page.getByRole('radiogroup', { name: '卡片分組' }).getByRole('radio', { name }).click();
    await page.waitForTimeout(700);
    if (name === '會話') await page.locator('.stage .session .head').first().click();
    if (name === '工具') await page.locator('.stage .card .head').first().click();
    await page.waitForTimeout(400);
    await left.take(page, 'phone › ' + name);
  }
  await page.setViewportSize({ width: 852, height: 393 });
  await page.waitForTimeout(400);
  await left.take(page, 'phone › landscape');
  // 電腦版 at the foot, and back.
  await page.getByRole('radio', { name: '電腦版' }).click();
  await expect(page.locator('.dashboard')).toBeVisible();
  await left.take(page, 'phone › 電腦版');
  expect(left.list).toEqual([]);
});

test('繁體中文: the first-run tour, and its three languages', async ({ page }) => {
  const left = new Leftovers();
  await traditional(page, '?guide');
  await expect(page.locator('.guide h2')).toHaveText('歡迎使用 Starmeter');
  await expect(page.locator('.guide .choice')).toHaveText(['简体中文', '繁體中文', 'English']);
  await expect(page.locator('.guide .choice.chosen')).toHaveText('繁體中文');
  for (let stop = 0; stop < 8 && (await page.locator('.guide').count()); stop++) {
    await left.take(page, `tour ${stop}`);
    await page.locator('.guide .step.primary').click();
    await page.waitForTimeout(350);
  }
  expect(left.list).toEqual([]);
});

test('the tour switches between the three languages on its first card', async ({ page }) => {
  await page.goto('/?guide');
  await expect(page.locator('.guide h2')).toHaveText('欢迎使用 Starmeter');
  await page.locator('.guide .choice', { hasText: '繁體中文' }).click();
  await expect(page.locator('.guide h2')).toHaveText('歡迎使用 Starmeter');
  await page.locator('.guide .choice', { hasText: 'English' }).click();
  await expect(page.locator('.guide h2')).toHaveText('Welcome to Starmeter');
  await page.locator('.guide .choice', { hasText: '简体中文' }).click();
  await expect(page.locator('.guide h2')).toHaveText('欢迎使用 Starmeter');
});

test('繁體中文 leaves a folder on disk as it is named, converting only the words around it (LANG-1)', async ({ page }) => {
  // A folder named in both scripts at once: on screen it must read as the file system names it.
  const folder = '/Users/demo/软件開發/项目记录';
  // The fixtures come to the dev page as a module: the folder is put in as it is served.
  await page.route('**/fixtures/default.json*', async (route) => {
    const response = await route.fetch();
    await route.fulfill({ response, body: (await response.text()).replace('/Users/demo/.claude/projects', folder) });
  });
  await traditional(page);
  await page.getByRole('button', { name: '主頁模塊與設置', exact: true }).click();
  await page.getByRole('button', { name: 'Claude', exact: true }).click();
  await expect(page.locator('.page')).toContainText('讀取目錄');
  await expect(page.locator('.page')).toContainText(folder);
  expect(await page.locator('.page').innerText()).not.toContain('/Users/demo/軟件開發/項目記錄');
});

test('繁體中文 shows a folder with spaces and punctuation in its name exactly as it is on disk (LANG-1)', async ({ page }) => {
  // After a space, a comma or a bracket the converter can't tell a path goes on; the row says it is one instead.
  const folder = '/Users/demo/Audio 工作/录音（备份）, 旧/发布 记录';
  await page.route('**/fixtures/default.json*', async (route) => {
    const response = await route.fetch();
    await route.fulfill({ response, body: (await response.text()).replace('/Users/demo/.claude/projects', folder) });
  });
  await traditional(page);
  await page.getByRole('button', { name: '主頁模塊與設置', exact: true }).click();
  await page.getByRole('button', { name: 'Claude', exact: true }).click();
  await expect(page.locator('.page')).toContainText('讀取目錄');
  const lines = (await page.locator('.page .detail').allInnerTexts()).flatMap((text) => text.split('\n'));
  expect(lines).toContain(folder);
});
