import { test, expect, type Page, type Route } from '@playwright/test';
import { readFileSync } from 'node:fs';
import { dirname, extname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

// The website (docs/, served by GitHub Pages) from its own files at a made-up address, so its script runs as it does
// online: with motion on (the system doesn't ask for less), in each of its three languages.
const DOCS = join(dirname(fileURLToPath(import.meta.url)), '..', '..', 'docs');
const SITE = 'http://starmeter.test/';
const TYPES: Record<string, string> = {
  '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.png': 'image/png', '.jpg': 'image/jpeg',
  '.svg': 'image/svg+xml', '.webp': 'image/webp', '.woff2': 'font/woff2',
};

function serve(route: Route) {
  const path = new URL(route.request().url()).pathname.replace(/^\/$/, '/index.html');
  try {
    return route.fulfill({ body: readFileSync(join(DOCS, decodeURIComponent(path))), contentType: TYPES[extname(path)] ?? 'application/octet-stream' });
  } catch {
    return route.fulfill({ status: 404, body: '' });
  }
}

async function site(page: Page, query = '', hant?: (route: Route) => unknown) {
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  // Pictures from elsewhere (badges, the star chart) aren't what these tests look at.
  await page.route((url) => !url.href.startsWith(SITE), (route) => route.fulfill({ status: 204, body: '' }));
  await page.route(`${SITE}**`, (route) => (hant && route.request().url().endsWith('/hant.js') ? hant(route) : serve(route)));
  await page.goto(SITE + query);
  return errors;
}

test.use({ reducedMotion: 'no-preference', viewport: { width: 1280, height: 900 } });

for (const [name, query] of [['简体中文', '?lang=zh'], ['English', '?lang=en'], ['繁體中文', '?lang=zh-Hant']]) {
  test(`website (${name}): the download cards show and can be clicked once scrolled to, with motion on (SITE-3)`, async ({ page }) => {
    const errors = await site(page, query);
    await expect(page.locator('html')).toHaveClass(/motion/);
    await expect(page.locator('html')).toHaveClass(/ready/);
    const cards = page.locator('.platform');
    expect(await cards.count()).toBeGreaterThanOrEqual(9);
    await cards.first().scrollIntoViewIfNeeded();
    await cards.last().scrollIntoViewIfNeeded();
    // Each card rises in, then stays: fully drawn and the top element where it is clicked.
    for (const card of await cards.all()) {
      await card.scrollIntoViewIfNeeded();
      await expect.poll(() => card.evaluate((e) => Number(getComputedStyle(e).opacity)), { timeout: 4000 }).toBe(1);
      const hit = await card.evaluate((e) => {
        const box = e.getBoundingClientRect();
        const top = document.elementFromPoint(box.left + box.width / 2, box.top + box.height / 2);
        return !!top && e.contains(top);
      });
      expect(hit).toBe(true);
    }
    expect(errors).toEqual([]);
  });
}

test('website: 繁體中文 that failed to load once loads when chosen again, and the buttons say what the page shows (SITE-2)', async ({ page }) => {
  let asked = 0;
  await site(page, '?lang=zh', (route) => (++asked === 1 ? route.abort() : serve(route)));
  const html = page.locator('html');
  const button = (label: string) => page.getByRole('button', { name: label, exact: true });
  await button('繁體中文').click();
  await expect.poll(() => asked).toBe(1);
  await expect(html).toHaveAttribute('lang', 'zh-CN');
  await expect(button('简体中文')).toHaveAttribute('aria-pressed', 'true');
  await expect(button('繁體中文')).toHaveAttribute('aria-pressed', 'false');

  await button('繁體中文').click();
  await expect(html).toHaveAttribute('lang', 'zh-Hant');
  expect(asked).toBe(2);
  await expect(button('繁體中文')).toHaveAttribute('aria-pressed', 'true');
  await expect(page.locator('h1 .zh')).toContainText('編程工具');
});

test('website: a language chosen while 繁體中文 still loads is the one shown (SITE-1)', async ({ page }) => {
  let release: () => void = () => {};
  const held = new Promise<void>((resolve) => { release = resolve; });
  await site(page, '?lang=zh', async (route) => { await held; await serve(route); });
  await page.getByRole('button', { name: '繁體中文', exact: true }).click();
  await page.getByRole('button', { name: 'English', exact: true }).click();
  release();
  await expect.poll(() => page.evaluate(() => typeof (window as unknown as { toHant?: unknown }).toHant)).toBe('function');
  await expect(page.locator('html')).toHaveAttribute('lang', 'en');
  expect(await page.evaluate(() => localStorage.getItem('starmeter-lang'))).toBe('en');
});

for (const [name, first] of [['hant.js before site.js', 'hant'], ['site.js before hant.js', 'site']] as const) {
  test(`website: opened in 繁體中文 (${name}), the privacy promise follows each switch, converted as one sentence (SITE-4)`, async ({ page }) => {
    // Whichever of the two scripts the page gets first, the other waits for it.
    let served: () => void = () => {};
    const ready = new Promise<void>((resolve) => { served = resolve; });
    const errors: string[] = [];
    page.on('pageerror', (error) => errors.push(error.message));
    await page.route((url) => !url.href.startsWith(SITE), (route) => route.fulfill({ status: 204, body: '' }));
    await page.route(`${SITE}**`, async (route) => {
      const file = new URL(route.request().url()).pathname;
      const waiting = first === 'hant' ? '/site.js' : '/hant.js';
      if (file === waiting) await ready;
      await serve(route);
      if (file === (first === 'hant' ? '/hant.js' : '/site.js')) served();
    });
    await page.goto(SITE + '?lang=zh-Hant');
    await expect(page.locator('html')).toHaveClass(/motion/);
    const promise = page.locator('[data-scrub] .zh');
    const button = (label: string) => page.getByRole('button', { name: label, exact: true });
    await expect(page.locator('html')).toHaveAttribute('lang', 'zh-Hant');
    // Split into words for the motion, and 註冊 as the whole sentence converts it (注 alone would stay 注).
    await expect(promise.locator('.w').first()).toBeAttached();
    await expect(promise).toHaveText(/^不用註冊賬號，也沒有中轉服務器/);
    await button('简体中文').click();
    await expect(page.locator('html')).toHaveAttribute('lang', 'zh-CN');
    await expect(promise).toHaveText(/^不用注册账号，也没有中转服务器/);
    await button('繁體中文').click();
    await expect(promise).toHaveText(/^不用註冊賬號，也沒有中轉服務器/);
    expect(errors).toEqual([]);
  });
}

test('website: opened in 简体中文 and switched to 繁體中文, the privacy promise converts as one sentence (SITE-4)', async ({ page }) => {
  const errors = await site(page, '?lang=zh');
  const promise = page.locator('[data-scrub] .zh');
  await expect(promise.locator('.w').first()).toBeAttached();
  await page.getByRole('button', { name: '繁體中文', exact: true }).click();
  await expect(promise).toHaveText(/^不用註冊賬號/);
  await page.getByRole('button', { name: '简体中文', exact: true }).click();
  await expect(promise).toHaveText(/^不用注册账号/);
  expect(errors).toEqual([]);
});
