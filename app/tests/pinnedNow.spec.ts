// Pinned, under the live curve: what the AI is doing now (PinnedNow.svelte, from the app's live_now, as 网页版 has
// it): the sessions running, a line each with this turn's tokens, cost and clock; the latest of what happened, 动态
// opening the last six (and remembered); under the heatmap, the cards moved down to make room; gone when unpinned.
import { expect, test, type Page } from '@playwright/test';

const errors: string[] = [];
test.beforeEach(({ page }) => {
  errors.length = 0;
  page.on('pageerror', (error) => errors.push(String(error)));
});
test.afterEach(() => expect(errors).toEqual([]));

async function pinned(page: Page) {
  await page.goto('/');
  await expect(page.locator('.dashboard')).toBeVisible();
  await page.waitForTimeout(300);
  const cardsTop = async () => (await page.locator('.dashboard > .cards').boundingBox())?.y ?? 0;
  const before = await cardsTop();
  await page.getByRole('button', { name: '钉住面板', exact: true }).click();
  await expect(page.getByLabel('实时用量图')).toBeVisible();
  return { cardsTop, before };
}

test('pinned, the sessions running and what happened sit under the live curve and its heatmap, the cards moved down for them', async ({ page }) => {
  const { cardsTop, before } = await pinned(page);
  const now = page.locator('.pinned-now');
  await expect(now).toBeVisible();
  await expect(now.locator('.label')).toHaveText('正在运行');
  // The fixtures' three running sessions, each with this turn's tokens and cost and a clock to the second.
  await expect(now.locator('.pinned-session')).toHaveCount(3);
  await expect(now.locator('.pinned-session .title').first()).toHaveText('重做设置页的分段控件');
  await expect(now.locator('.pinned-session .spent').first()).toHaveText(/Token · \$\d/);
  await expect(now.locator('.pinned-session .clock').first()).toHaveText(/^\d+:\d\d$/);
  const clock = await now.locator('.pinned-session .clock').first().textContent();
  await expect(now.locator('.pinned-session .clock').first()).not.toHaveText(clock!, { timeout: 3000 });
  // Each pinned tick adds to today: a request landing is the latest of what happened, on one line.
  await expect(now.locator('.pinned-event')).toHaveCount(1, { timeout: 8000 });
  await expect(now.locator('.pinned-event .what')).toHaveText(/^\+[\d.]+K Token/);
  // Under the curve and its heatmap, over the cards, which moved down by the strip's height.
  const strip = (await now.boundingBox())!, chart = (await page.getByLabel('实时用量图').boundingBox())!;
  const heatmap = (await page.locator('.dashboard > .heatmap').boundingBox())!;
  expect(strip.y).toBeGreaterThanOrEqual(chart.y + chart.height - 1);
  expect(strip.y).toBeGreaterThanOrEqual(heatmap.y + heatmap.height - 1);
  expect(await cardsTop()).toBeGreaterThanOrEqual(strip.y + strip.height - 1);
  expect(await cardsTop()).toBeGreaterThan(before);
  // Unpinned: gone, and the cards back where they were.
  await page.getByRole('button', { name: '取消钉住面板', exact: true }).click();
  await expect(now).toHaveCount(0);
  await expect.poll(cardsTop).toBe(before);
});

test('pinned, 动态 opens the last six of what happened, and stays open the next time', async ({ page }) => {
  await pinned(page);
  const now = page.locator('.pinned-now'), toggle = now.getByRole('button', { name: '动态' });
  await expect(toggle).toBeVisible({ timeout: 10000 });
  await expect(toggle).toHaveAttribute('aria-expanded', 'false');
  await expect(now.locator('.pinned-event')).toHaveCount(1);
  await toggle.click();
  await expect(toggle).toHaveAttribute('aria-expanded', 'true');
  await expect.poll(() => now.locator('.pinned-event').count()).toBeGreaterThan(1);
  expect(await now.locator('.pinned-event').count()).toBeLessThanOrEqual(6);
  await page.reload();
  await expect(page.locator('.dashboard')).toBeVisible();
  const again = page.getByRole('button', { name: '钉住面板', exact: true });
  if (await again.count()) await again.click();
  await expect(page.locator('.pinned-now').getByRole('button', { name: '动态' })).toHaveAttribute('aria-expanded', 'true', { timeout: 10000 });
});

test('pinned in English, 正在运行 and 动态 in English and within the panel', async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem('starmeter.language', 'en'));
  await page.goto('/');
  await expect(page.locator('.dashboard')).toBeVisible();
  await page.getByRole('button', { name: 'Pin the panel', exact: true }).click();
  const now = page.locator('.pinned-now');
  await expect(now.locator('.label')).toHaveText('Running');
  await expect(now.locator('.pinned-event .what')).toHaveText(/^\+[\d.]+K tokens/, { timeout: 8000 });
  const width = await page.locator('.dashboard').evaluate((d) => d.getBoundingClientRect().right);
  for (const right of await now.locator('.pinned-session > *, .event > *').evaluateAll((list) => list.map((e) => e.getBoundingClientRect().right))) expect(right).toBeLessThanOrEqual(width);
});
