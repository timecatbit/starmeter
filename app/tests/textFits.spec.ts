// No words cut off on 网页版, found by the page (textFits.ts) rather than by eye: on a phone upright, a small phone, a
// phone on its side under Safari's bars, and two computer screens; in 简体, English and 繁體; 标准 and 手绘; working
// (three sessions running, two of them with very long titles, an hour and more into their turns) and resting; with the
// cards in each grouping. Fixtures only.
import { expect, test, type Page } from '@playwright/test';
import { liveWork, PHONE_KEY, servePhone } from './phone';
import { toHant } from '../ui/src/lib/hant';
import { cutWords } from './textFits';

test.describe.configure({ timeout: 300_000 });

const LONG = {
  'session-1': '重新设计设置页里的分段控件，补齐键盘导航、读屏标签以及深浅两种主题下的对比度检查',
  'session-2': 'Rework the settings page segmented control with keyboard navigation, screen reader labels and contrast checks',
};

type Row = { key?: string; label: string; detail?: string; statusLight?: string; onOpenAction?: string[]; children: Row[] };
/** The fixtures with two sessions' titles far too long for a line (in 会话 and in what is running). */
function longTitles(data: { snapshot: { entries: Row[] } }) {
  const sessions = data.snapshot.entries.find((e) => e.onOpenAction?.[1] === 'sessions');
  for (const s of sessions?.children ?? []) if (s.key && s.key in LONG) s.label = LONG[s.key as keyof typeof LONG];
}
const hhmm = (seconds: number) => { const d = new Date(seconds * 1000); return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`; };

/** The scripted work, three sessions running (a third woken), every turn an hour and 23 minutes in: the first to the
 *  second (its clock 1:23:17), the second only by the minute its turn began (no start from the computer, as SwiftBar's
 *  panel sends them: 1 小时 23 分钟, the longest way the time is written), the third begun before the logs read (its
 *  line 更早开始, no minutes said: its clock says 运行中, never nothing). */
function longWork() {
  const base = liveWork({ now: () => Date.now(), length: 99_999, claudeEnds: 99_998, codexEnds: 99_998 });
  return (data: unknown) => {
    const answer = base(data) as { at: number; turns: Record<string, number>; sessions?: { children: Row[] } };
    const rows = (answer.sessions?.children ?? []).filter((s) => s.statusLight);
    const third = rows.find((s) => s.key !== 'session-1' && s.key !== 'session-2');
    if (third) third.statusLight = 'new';
    for (const s of rows) {
      const turn = s.children.find((c) => c.detail !== undefined);
      if (s === third) {
        s.detail = (s.detail ?? '').replace(/^(运行中|空闲)( \d+ 分钟)?/, '运行中');
        if (turn) turn.label = turn.label.replace(/^\d\d:\d\d/, '更早开始');
        continue;
      }
      if (s.statusLight === 'new') s.detail = (s.detail ?? '').replace(/^(运行中|空闲)( \d+ 分钟)?/, '运行中 83 分钟');
      if (turn) turn.label = turn.label.replace(/^\d\d:\d\d/, hhmm(answer.at - 83 * 60 - 17));
    }
    answer.turns = { 'session-1': answer.at - 83 * 60 - 17 };
    return answer;
  };
}
const resting = () => liveWork({ now: () => Date.now(), length: 99_999, claudeEnds: 0, codexEnds: 0 });

const SIZES: [string, { width: number; height: number }, number?][] = [
  ['upright 393', { width: 393, height: 852 }],
  ['small 320', { width: 320, height: 568 }],
  ['on its side under Safari\'s bars', { width: 852, height: 393 }, 286],
  ['computer 1440', { width: 1440, height: 900 }],
  ['computer 1280', { width: 1280, height: 720 }],
];
const LANGUAGES = ['zh', 'en', 'zh-Hant'];
const LOOKS = ['standard', 'sketch'];

/** What is cut, found twice running (the activity coming in moves the rows a moment; a cut stays). */
async function check(page: Page, where: string): Promise<string[]> {
  const first = await cutWords(page);
  if (!first.length) return [];
  const again = new Set(await cutWords(page));
  return first.filter((line) => again.has(line)).map((line) => `${where} · ${line}`);
}

for (const [size, viewport, seen] of SIZES) {
  test(`网页版 cuts no words: ${size}, in every language and look, working and resting, each grouping`, async ({ page }) => {
    const found: string[] = [];
    for (const state of ['working', 'resting'] as const) for (const language of LANGUAGES) for (const look of LOOKS) {
      const context = page.context();
      const tab = await context.newPage();
      if (seen) await tab.addInitScript((height) => {
        const view = Object.assign(new EventTarget(), { height, width: innerWidth, offsetTop: 0, offsetLeft: 0, scale: 1, pageTop: 0, pageLeft: 0 });
        Object.defineProperty(window, 'visualViewport', { configurable: true, value: view });
      }, seen);
      await tab.setViewportSize(viewport);
      await servePhone(tab, PHONE_KEY, { live: state === 'working' ? longWork() : resting(), edit: longTitles });
      await tab.goto(`/#k=${PHONE_KEY}&l=${language}&s=${look}`);
      await expect(tab.locator(state === 'working' ? '.stage .monitor.working' : '.stage .monitor:not(.working)')).toBeVisible();
      await tab.waitForTimeout(2200);
      if (state === 'working') {
        // What is meant to be checked is on screen: three sessions running, the long titles, the longest clocks.
        await expect(tab.locator('.stage .activity .session')).toHaveCount(3);
        await expect(tab.locator('.stage .activity .title').first()).toHaveText(language === 'zh-Hant' ? toHant(LONG['session-1']) : LONG['session-1']);
        await expect(tab.locator('.stage .activity .timer-figure').first()).toHaveText(/^1:23:\d\d$/);
        await expect(tab.locator('.stage .activity .timer').nth(1)).toHaveText(/^1\s*(小时|小時|hr)\s*2\d\s*(分钟|分鐘|min)$/);
        // Begun before the logs read: no start known, and said to be running rather than left empty (it vanished).
        await expect(tab.locator('.stage .activity .timer').nth(2)).toHaveText(/^(运行中|運行中|Running)$/);
        // Each clock whole in its card, right of its title (cut short before it), never under it.
        const clocks = await tab.locator('.stage .activity .session').evaluateAll((cards) => cards.map((card) => {
          const box = card.getBoundingClientRect(), title = card.querySelector('.title')!.getBoundingClientRect(), timer = card.querySelector('.timer')!;
          const time = timer.getBoundingClientRect();
          return { inside: time.left >= box.left && time.right <= box.right + 0.5 && time.width > 4, apart: time.left >= title.right - 0.5,
            whole: timer.scrollWidth <= timer.clientWidth + 0.5, said: (timer.textContent ?? '').trim().length > 0 };
        }));
        expect(clocks, `${language} ${look}`).toEqual(clocks.map(() => ({ inside: true, apart: true, whole: true, said: true })));
      }
      const tabs = tab.locator('.stage .tabs [role="radio"]');
      const count = await tabs.count();
      for (let i = 0; i < count; i++) {
        await tabs.nth(i).click();
        await tab.waitForTimeout(700);
        found.push(...await check(tab, `${state} ${language} ${look} tab ${i + 1}`));
      }
      await tab.close();
    }
    expect([...new Set(found)]).toEqual([]);
  });
}

for (const [size, viewport, seen] of SIZES) {
  test(`网页版's live chart keeps its end whole, its dot and glow inside the canvas, the curve no sliver: ${size}`, async ({ page }) => {
    if (seen) await page.addInitScript((height) => {
      const view = Object.assign(new EventTarget(), { height, width: innerWidth, offsetTop: 0, offsetLeft: 0, scale: 1, pageTop: 0, pageLeft: 0 });
      Object.defineProperty(window, 'visualViewport', { configurable: true, value: view });
    }, seen);
    await page.setViewportSize(viewport);
    await servePhone(page, PHONE_KEY, { live: longWork(), edit: longTitles });
    await page.goto(`/#k=${PHONE_KEY}`);
    const canvas = page.locator('.stage .live-chart canvas');
    await expect(canvas).toBeVisible();
    // A few seconds of readings landing (the scale moving with them): the end's dot (3.5) and the line's glow (12) inside.
    for (let i = 0; i < 16; i++) {
      await page.waitForTimeout(250);
      const end = await canvas.getAttribute('data-end');
      if (!end) continue;
      const [x, y, width, height] = end.split(',').map(Number);
      const where = `${size} at ${i}: ${end}`;
      expect(y - 3.5 - 12, where).toBeGreaterThanOrEqual(0);
      expect(y + 3.5 + 12, where).toBeLessThanOrEqual(height);
      expect(x + 3.5 + 12, where).toBeLessThanOrEqual(width);
      // A curve, not a sliver under its legend: the plot at least 40 points.
      expect(height - 38, where).toBeGreaterThanOrEqual(40);
    }
  });
}

/** Each running card's title made exactly as long as its room (the most characters that fit whole), then one character
 *  longer (cut short): both times the clock beside it is there, whole, wider than nothing, right of the title, inside the
 *  card. A title that just fit once pushed the clock out of sight. Run in the page: the next reading writes the titles
 *  back. */
async function clocksAtTheEdge(page: Page, card: string, title: string, clock: string, letter: string) {
  return page.evaluate(({ card, title, clock, letter }) => {
    const seen: string[] = [];
    for (const row of document.querySelectorAll(card)) {
      const name = row.querySelector<HTMLElement>(title)!, time = row.querySelector<HTMLElement>(clock)!;
      const fits = (n: number) => { name.textContent = letter.repeat(n); return name.scrollWidth <= name.clientWidth + 0.5; };
      let n = 1;
      while (fits(n + 1) && n < 400) n++;
      for (const [length, how] of [[n, 'just fits'], [n + 1, 'just over']] as const) {
        name.textContent = letter.repeat(length);
        const box = row.getBoundingClientRect(), named = name.getBoundingClientRect(), at = time.getBoundingClientRect();
        const ok = at.width > 4 && at.left >= named.right - 0.5 && at.right <= box.right + 0.5 && time.scrollWidth <= time.clientWidth + 0.5 && (time.textContent ?? '').trim() !== '';
        if (!ok) seen.push(`${how} (${length}): clock ${Math.round(at.left)}–${Math.round(at.right)}, title to ${Math.round(named.right)}, card to ${Math.round(box.right)}, "${time.textContent}"`);
      }
    }
    return seen;
  }, { card, title, clock, letter });
}

for (const [language, letter] of [['zh', '国'], ['en', 'm'], ['zh-Hant', '國']] as const) {
  test(`网页版 and 实时: a running card's clock stays whole beside a title that just fits and one just over, in ${language}`, async ({ page }) => {
    for (const viewport of [{ width: 393, height: 852 }, { width: 320, height: 568 }]) {
      const tab = await page.context().newPage();
      await tab.setViewportSize(viewport);
      await servePhone(tab, PHONE_KEY, { live: longWork(), edit: longTitles });
      await tab.goto(`/#k=${PHONE_KEY}&l=${language}`);
      await expect(tab.locator('.stage .activity .session')).toHaveCount(3);
      expect(await clocksAtTheEdge(tab, '.stage .activity .session', '.title', '.timer', letter), `${viewport.width}`).toEqual([]);
      await tab.close();
    }
    // The pinned panel's 实时, narrow.
    await page.addInitScript((lang) => localStorage.setItem('starmeter.language', lang), language);
    await page.goto('/');
    await expect(page.locator('.dashboard')).toBeVisible();
    await page.getByRole('button', { name: /^(钉住面板|Pin the panel|釘住面板)$/ }).click();
    await expect(page.locator('.panel-live .live-session').first()).toBeVisible();
    expect(await clocksAtTheEdge(page, '.panel-live .live-session', '.title', '.clock', letter), 'pinned').toEqual([]);
  });
}
