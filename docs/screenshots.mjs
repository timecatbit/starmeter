#!/usr/bin/env node
// The website's and the READMEs' screenshots, taken from the panel's made-up fixtures (app/ui/fixtures), never from
// a real machine: usage, accounts, keys and sessions there are all invented. Each picture comes in Chinese and in
// English (the panel's 外观与设置 → 语言) and in a light and a dark copy, at 2x: the Chinese ones into docs/images/, the
// English ones into docs/images/en/, beside copies of the logo (Pages serves docs/ alone; scripts/make_logo.py makes
// the originals in assets/). Run it again whenever the panel, the share pictures or the logo change:
//
//   node docs/screenshots.mjs
//   node docs/screenshots.mjs live share     (only those pictures)
//   node docs/screenshots.mjs en             (only the English ones)
//
// It starts the UI's Vite dev server on a port of its own and stops it at the end. To use a server that is already
// running (the sandbox can't always start one), point PLAYWRIGHT_BASE_URL at it, as for the UI tests:
//
//   (cd app && npx vite --host 127.0.0.1 --port 1437 --strictPort)
//   PLAYWRIGHT_BASE_URL=http://127.0.0.1:1437 node docs/screenshots.mjs
//
// Needs app/node_modules (`npm ci` in app/) and Playwright's WebKit (`npx playwright install webkit` in app/), the
// engine closest to the app's WKWebView.
import { spawn } from 'node:child_process';
import { copyFileSync, mkdirSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';

const app = new URL('../app/', import.meta.url);
const images = new URL('./images/', import.meta.url);
const LANGUAGES = { zh: { folder: images, locale: 'zh-CN' }, en: { folder: new URL('en/', images), locale: 'en-US' } };
/** What the pictures find their way by: rows, tabs and buttons as each language names them. */
const NAMES = {
  zh: { subscription: 'Claude 订阅', sessions: '会话', pin: /^钉住/, live: '实时用量图', model: '模型', modules: '主页模块与设置', share: '分享用量图' },
  en: { subscription: 'Claude plan', sessions: 'Sessions', pin: /^Pin/, live: 'Live usage chart', model: 'Model', modules: 'Modules and settings', share: 'Share usage picture' },
};
const { webkit } = createRequire(new URL('package.json', app))('@playwright/test');

const PORT = 1439;
const SCALE = 2;
// Room around the panel for its shadow, on a clear background; the shadow is made to fade out within it. The page is
// as wide as the module grid (588), the widest page.
const MARGIN = 40;
// What the panel looks like on a laptop screen: it scrolls past this height.
const PANEL_HEIGHT = 760;
// The website shows the share pictures in a row about 300pt wide each; at twice that they stay sharp and light.
const SHARE_WIDTH = 600;

// MARK: The dev server

async function reachable(url) {
  try { return (await fetch(url)).ok; } catch { return false; }
}

async function startServer() {
  const given = process.env.PLAYWRIGHT_BASE_URL;
  if (given) return { url: given.replace(/\/$/, ''), stop() {} };
  const url = `http://127.0.0.1:${PORT}`;
  if (await reachable(url)) throw new Error(`Port ${PORT} is taken; stop that server or set PLAYWRIGHT_BASE_URL.`);
  const server = spawn('npx', ['vite', '--host', '127.0.0.1', '--port', String(PORT), '--strictPort'],
    { cwd: fileURLToPath(app), stdio: ['ignore', 'ignore', 'inherit'], detached: process.platform !== 'win32', shell: process.platform === 'win32' });
  for (let i = 0; i < 120 && !(await reachable(url)); i++) await new Promise((resolve) => setTimeout(resolve, 250));
  if (!(await reachable(url))) { server.kill(); throw new Error('The Vite dev server did not start; see the output above.'); }
  return { url, stop: () => { try { process.kill(-server.pid); } catch { server.kill(); } } };
}

// MARK: Taking pictures

// No desktop behind the panel and a solid fill instead of the browser's stand-in for glass, so the picture reads the
// same on any page, with a shadow that ends inside the margin; the page's root shrinks to the panel and its margin, to
// be cut out whole.
const STILL = `
  body.preview { background: transparent !important; }
  #app { display: inline-block; }
  .window.browser { margin: ${MARGIN}px !important; box-shadow: 0 12px 26px rgba(0, 0, 0, 0.15), 0 0 0 0.5px rgba(0, 0, 0, 0.12) !important; }
  .window.browser:not(.solid) { background: var(--solid) !important; backdrop-filter: none !important; }
  *, *::after { caret-color: transparent !important; }
`;

async function open(context, { height = PANEL_HEIGHT } = {}) {
  const page = await context.newPage();
  // The panel takes the window's height less 16 (shell.info in backend.ts); the page is made taller than that so the
  // margin under the panel fits too.
  await page.addInitScript((inner) => Object.defineProperty(window, 'innerHeight', { get: () => inner, configurable: true }), height + 16);
  await page.addInitScript((language) => localStorage.setItem('starmeter.language', language), context.lang);
  await page.setViewportSize({ width: 588 + 2 * MARGIN + 40, height: height + 2 * MARGIN + 40 });
  const errors = [];
  // A resize observer that sees a size change it caused in the same frame says so; it is a notice, not a failure.
  page.on('pageerror', (error) => { if (!/ResizeObserver loop/.test(String(error))) errors.push(String(error)); });
  await page.goto(`${context.base}/`);
  await page.locator('.dashboard').waitFor();
  await page.addStyleTag({ content: STILL });
  // Opening asks for a fresh reading; let it land and the trend draw itself in.
  await page.waitForTimeout(1200);
  page.errors = errors;
  return page;
}

/** The panel and its shadow, on a clear background. */
async function panelShot(page, file) {
  await page.mouse.move(1, 1);
  await page.waitForTimeout(450);
  await page.locator('#app').screenshot({ path: fileURLToPath(new URL(file, page.context().out)), omitBackground: true, animations: 'disabled' });
  if (page.errors.length) throw new Error(`${file}: ${page.errors.join('; ')}`);
  await page.close();
}

const row = (page, title) => page.locator('.row', { hasText: title }).first();
const button = (page, name) => page.getByRole('button', { name, exact: true });

/** Each picture: what to open and what to do before taking it. */
const shots = {
  // The homepage as it opens on a laptop: Summary, the trend, the heatmap and the first cards.
  async home(context) {
    const page = await open(context);
    await panelShot(page, `home-${context.look}.png`);
  },
  // The whole homepage, as tall as it gets: every card and every row.
  async homeFull(context) {
    const page = await open(context, { height: 4000 });
    await panelShot(page, `home-full-${context.look}.png`);
  },
  // A subscription's page: each window used, its bar with the even-pace mark, and when it resets.
  async subscription(context) {
    const page = await open(context);
    await row(page, context.names.subscription).click();
    await page.locator('.page').waitFor();
    await page.waitForTimeout(500);
    await panelShot(page, `subscription-${context.look}.png`);
  },
  // An API with several keys, one of them open on its guide line.
  async api(context) {
    const page = await open(context);
    await row(page, 'DeepSeek API').click();
    await page.locator('.page').waitFor();
    await page.locator('.detail-row', { hasText: 'lab' }).first().click();
    await page.waitForTimeout(500);
    await panelShot(page, `api-${context.look}.png`);
  },
  // 会话, the fourth choice beside the groupings: the sessions as cards, the first one opened to its recent turns,
  // scrolled so the choices sit at the top.
  async sessions(context) {
    const page = await open(context, { height: 620 });
    await page.getByRole('radio', { name: context.names.sessions }).click();
    await page.locator('.session').first().waitFor();
    await page.locator('.session .head').first().click();
    await page.evaluate(() => document.querySelector('.tabs')?.scrollIntoView({ block: 'start' }));
    await page.waitForTimeout(600);
    await panelShot(page, `sessions-${context.look}.png`);
  },
  // Pinned: the live chart, after a few readings have come in. The fixtures add a little more on each 2-second tick
  // until the seventh, which adds less: the line would start again (as after midnight), so the picture is taken first.
  async live(context) {
    const page = await open(context, { height: 560 });
    await page.getByRole('button', { name: context.names.pin }).first().click();
    await page.getByLabel(context.names.live).waitFor();
    await page.waitForTimeout(9_000);
    await panelShot(page, `live-${context.look}.png`);
  },
  // The cards grouped by model instead of by tool.
  async models(context) {
    const page = await open(context);
    await page.getByRole('radio', { name: context.names.model }).click();
    await page.waitForTimeout(600);
    await panelShot(page, `models-${context.look}.png`);
  },
  // The module grid, where each source is switched on and set up.
  async modules(context) {
    const page = await open(context, { height: 900 });
    await button(page, context.names.modules).click();
    await page.locator('.modules').waitFor();
    await page.waitForTimeout(600);
    await panelShot(page, `modules-${context.look}.png`);
  },
  // The share page: the picture previewed in the panel, with its styles and what to do with it.
  async sharePage(context) {
    const page = await open(context, { height: 900 });
    await button(page, context.names.share).click();
    await page.locator('.share-page .stage img').waitFor();
    await page.waitForTimeout(600);
    await panelShot(page, `share-page-${context.look}.png`);
  },
  // Every share picture, drawn by the panel's own code (renderPicture and shareStyles in app/ui/src/lib/share.ts)
  // from the same dashboard. A style that looks the same in light and dark (the posters keep their own colours) is
  // saved once, as share-<style>.jpg; the others as share-<style>-light.jpg and -dark.jpg. Scaled to SHARE_WIDTH and
  // JPEG, to keep the long pictures light; one that isn't opaque stays PNG.
  async share(context) {
    if (context.look !== 'light') return;
    const page = await open(context);
    const pictures = await page.evaluate(async (width) => {
      const { backend } = await import('/src/lib/backend.ts');
      const { renderPicture, shareStyles } = await import('/src/lib/share.ts');
      // As the panel shows it, in the language chosen.
      const { localize } = await import('/src/lib/i18n.svelte.ts');
      const snapshot = localize(await backend.snapshot());
      const pixels = async (picture) => {
        const image = new Image();
        image.src = picture.url;
        await image.decode();
        const scale = Math.min(1, width / image.naturalWidth);
        const canvas = Object.assign(document.createElement('canvas'), { width: Math.round(image.naturalWidth * scale), height: Math.round(image.naturalHeight * scale) });
        const ctx = canvas.getContext('2d');
        ctx.imageSmoothingQuality = 'high';
        ctx.drawImage(image, 0, 0, canvas.width, canvas.height);
        return { canvas, data: ctx.getImageData(0, 0, canvas.width, canvas.height).data };
      };
      // Within a level on average: a poster's scattered stars may land elsewhere each time.
      const alike = (a, b) => {
        if (a.canvas.width !== b.canvas.width || a.canvas.height !== b.canvas.height) return false;
        let difference = 0;
        for (let i = 0; i < a.data.length; i += 4) difference += Math.abs(a.data[i] - b.data[i]) + Math.abs(a.data[i + 1] - b.data[i + 1]) + Math.abs(a.data[i + 2] - b.data[i + 2]);
        return difference / (a.data.length / 4) / 3 < 1;
      };
      const opaque = (p) => { for (let i = 3; i < p.data.length; i += 4) if (p.data[i] < 255) return false; return true; };
      const encode = async (p) => {
        const jpeg = opaque(p);
        const blob = await new Promise((resolve) => p.canvas.toBlob(resolve, jpeg ? 'image/jpeg' : 'image/png', 0.85));
        const bytes = new Uint8Array(await blob.arrayBuffer());
        let text = '';
        for (let i = 0; i < bytes.length; i += 0x8000) text += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
        return { extension: jpeg ? 'jpg' : 'png', data: btoa(text) };
      };
      const files = [];
      for (const { key } of shareStyles) {
        const [light, dark] = await Promise.all([false, true].map(async (dark) => pixels(await renderPicture(key, snapshot.dashboard, snapshot.project, dark))));
        const copies = alike(light, dark) ? [['', light]] : [['-light', light], ['-dark', dark]];
        for (const [suffix, picture] of copies) {
          const { extension, data } = await encode(picture);
          files.push({ name: `share-${key}${suffix}.${extension}`, data });
        }
      }
      return files;
    }, SHARE_WIDTH);
    for (const { name, data } of pictures) {
      if (name.endsWith('.png')) console.warn(`${name} has clear pixels, so it stays PNG; the website links the .jpg names.`);
      writeFileSync(new URL(name, context.out), Buffer.from(data, 'base64'));
    }
    await page.close();
  },
};

// MARK: Main

const asked = process.argv.slice(2);
const only = asked.filter((name) => !(name in LANGUAGES)), languages = asked.filter((name) => name in LANGUAGES);
const unknown = only.filter((name) => !(name in shots));
if (unknown.length) throw new Error(`Unknown picture ${unknown.join(', ')}; pick from ${Object.keys(shots).join(', ')}, or zh / en.`);
for (const { folder } of Object.values(LANGUAGES)) mkdirSync(folder, { recursive: true });
for (const logo of ['starmeter.svg', 'starmeter-small.svg', 'starmeter-mark.svg']) copyFileSync(new URL(`../assets/${logo}`, import.meta.url), new URL(logo, images));
const server = await startServer();
const browser = await webkit.launch();
try {
  for (const [lang, { folder, locale }] of Object.entries(LANGUAGES)) {
    if (languages.length && !languages.includes(lang)) continue;
    for (const look of ['light', 'dark']) {
      const context = await browser.newContext({ colorScheme: look, deviceScaleFactor: SCALE, locale, timezoneId: 'Asia/Shanghai' });
      Object.assign(context, { look, lang, names: NAMES[lang], out: folder, base: server.url });
      for (const [name, take] of Object.entries(shots)) {
        if (only.length && !only.includes(name)) continue;
        await take(context);
        console.log(`${name} (${lang}, ${look})`);
      }
      await context.close();
    }
  }
} finally {
  await browser.close();
  server.stop();
}
