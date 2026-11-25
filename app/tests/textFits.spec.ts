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
 *  second (its clock 1:23:17), the others only by the minute their turn began (no start from the computer, as
 *  SwiftBar's panel sends them: 1 小时 23 分钟, the longest way the time is written). */
function longWork() {
  const base = liveWork({ now: () => Date.now(), length: 99_999, claudeEnds: 99_998, codexEnds: 99_998 });
  return (data: unknown) => {
    const answer = base(data) as { at: number; turns: Record<string, number>; sessions?: { children: Row[] } };
    const rows = (answer.sessions?.children ?? []).filter((s) => s.statusLight);
    const third = rows.find((s) => s.key !== 'session-1' && s.key !== 'session-2');
    if (third) third.statusLight = 'new';
    for (const s of rows) {
      if (s.statusLight === 'new') s.detail = (s.detail ?? '').replace(/^(运行中|空闲)( \d+ 分钟)?/, '运行中 83 分钟');
      const turn = s.children.find((c) => c.detail !== undefined);
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
        // Each clock whole in its card, right of its title (cut short before it), never under it.
        const clocks = await tab.locator('.stage .activity .session').evaluateAll((cards) => cards.map((card) => {
          const box = card.getBoundingClientRect(), title = card.querySelector('.title')!.getBoundingClientRect(), timer = card.querySelector('.timer')!;
          const time = timer.getBoundingClientRect();
          return { inside: time.left >= box.left && time.right <= box.right + 0.5 && time.width > 0, apart: time.left >= title.right - 0.5, whole: timer.scrollWidth <= timer.clientWidth + 0.5 };
        }));
        expect(clocks, `${language} ${look}`).toEqual(clocks.map(() => ({ inside: true, apart: true, whole: true })));
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
