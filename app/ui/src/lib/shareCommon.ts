// What the share pictures share at the lowest level: the rows they show and their tags, the bands of pattern, text on
// a canvas as AppKit set it, and the marks.
import starmeterIcon from '../../../../assets/starmeter.svg?url';
import { active } from './heatmap';
import { iconImage } from './icons';
import type { Dashboard, ProviderUsage, UsageHeatmap } from './types';

export const WIDTH = 420;
/** Every picture keeps a band at the top for the masthead, written at its two ends (63pt: on an iPhone 393pt wide the
 *  island sits centred in it), and leaves a band under the signature clear of the home indicator. */
export const TOP_BAND = 63, BOTTOM_BAND = 44;

/** Only the tools with something in the range get a block, in the panel's order. */
export const shownRows = (d: Dashboard) => d.providers.filter((p) => p.available && (p.tokens > 0 || p.requests > 0 || (p.credits ?? 0) > 0));
/** The tag at the right of a tool's token line: what Summary leaves out or a gateway counted. */
export const tag = (p: ProviderUsage) => (p.summaryIncluded === false ? '未计入 Summary' : p.counted && p.subtitle ? p.subtitle : null);
/** The heatmap as the panel shows it, once read and something used: its streak goes beside the trend. */
export const shownHeatmap = (d: Dashboard): UsageHeatmap | null => (d.heatmap && !d.heatmap.status && active(d.heatmap).includes(true) ? d.heatmap : null);

/** The patterns' grain is fixed per day, so a picture drawn twice looks the same. */
export const crownSeed = (d: Dashboard) => [...d.date].reduce((s, c) => s + c.charCodeAt(0), 0) % 97;
export function grain(a: number, b: number, seed: number): number {
  let value = (Math.imul(a, 73856093) ^ Math.imul(b, 19349663) ^ Math.imul(Math.floor(seed), 83492791)) >>> 0;
  value = (value ^ (value << 13)) >>> 0; value = (value ^ (value >>> 17)) >>> 0; value = (value ^ (value << 5)) >>> 0;
  return value;
}
export type Fade = 'down' | 'up' | 'left';
export type Box = { x: number; y: number; w: number; h: number };
/** How strong a patch's pattern is at a point: full at the edge it grows from, nothing at the far one. */
export function fade(fading: Fade, x: number, y: number, rect: Box): number {
  const t = fading === 'down' ? (y - rect.y) / rect.h : fading === 'up' ? (rect.y + rect.h - y) / rect.h : (rect.x + rect.w - x) / rect.w;
  return Math.pow(Math.max(0, 1 - t), 0.8);
}

// MARK: Text as AppKit set it: a rect's top is the line's top, and a long line ends in an ellipsis.

export function fit(ctx: CanvasRenderingContext2D, text: string, width: number): string {
  if (ctx.measureText(text).width <= width + 0.5) return text;
  let low = 0, high = text.length;
  while (low < high) { const mid = (low + high + 1) >> 1; if (ctx.measureText(text.slice(0, mid) + '…').width <= width) low = mid; else high = mid - 1; }
  return text.slice(0, low) + '…';
}
const fontSize = (css: string) => Number(css.match(/(\d+(?:\.\d+)?)px/)?.[1] ?? 12);
/** Text in a box from its top, aligned and cut to the box's width. */
export function text(ctx: CanvasRenderingContext2D, value: string, box: Box, css: string, colour: string, align: 'left' | 'center' | 'right' = 'left') {
  ctx.font = css; ctx.fillStyle = colour; ctx.textBaseline = 'alphabetic';
  const shown = fit(ctx, value, box.w), size = fontSize(css);
  ctx.textAlign = align;
  ctx.fillText(shown, align === 'left' ? box.x : align === 'right' ? box.x + box.w : box.x + box.w / 2, box.y + size * 0.952);
}
/** Text centred on a line by its glyph box, so different fonts side by side share one middle. */
export function middleText(ctx: CanvasRenderingContext2D, value: string, x: number, middle: number, width: number, css: string, colour: string,
  align: 'left' | 'center' | 'right' = 'left') {
  const size = fontSize(css);
  text(ctx, value, { x, y: middle - (size * 1.193) / 2, w: width, h: size * 1.2 }, css, colour, align);
}
export function measure(ctx: CanvasRenderingContext2D, value: string, css: string): number {
  ctx.font = css;
  return Math.ceil(ctx.measureText(value).width);
}
export function roundRect(ctx: CanvasRenderingContext2D, box: Box, radius: number) {
  ctx.beginPath(); ctx.roundRect(box.x, box.y, box.w, box.h, radius);
}

// MARK: Images

export function load(src: string): Promise<HTMLImageElement | null> {
  return new Promise((resolve) => {
    const image = new Image();
    image.onload = () => resolve(image);
    image.onerror = () => resolve(null);
    image.src = src;
  });
}
const imageSource = (raw: string) => `data:${raw.startsWith('iVBOR') ? 'image/png' : 'image/svg+xml'};base64,${raw}`;

export interface Marks { tools: Map<string, { image: HTMLImageElement; template: boolean } | null>; brand: HTMLImageElement | null }

/** Every tool's mark and the signature, loaded before drawing starts. */
export async function loadMarks(rows: ProviderUsage[], symbolColour: string): Promise<Marks> {
  const tools = new Map<string, { image: HTMLImageElement; template: boolean } | null>();
  await Promise.all(rows.map(async (p) => {
    if (p.image) { const image = await load(imageSource(p.image)); tools.set(p.id, image ? { image, template: p.template ?? false } : null); }
    else if (p.symbol) { const image = await load(iconImage(p.symbol, symbolColour).src); tools.set(p.id, image ? { image, template: false } : null); }
    else tools.set(p.id, null);
  }));
  return { tools, brand: await load(starmeterIcon) };
}
/** Symbols tinted per picture: [name, colour] pairs. */
export async function loadSymbols(pairs: [string, string][]): Promise<Map<string, HTMLImageElement>> {
  const out = new Map<string, HTMLImageElement>();
  await Promise.all(pairs.map(async ([name, tint]) => { const image = await load(iconImage(name, tint).src); if (image) out.set(name, image); }));
  return out;
}

/** A tool's mark in its logo's own colours: monochrome logos in the text colour, colour ones as they are. */
export function drawMark(ctx: CanvasRenderingContext2D, mark: { image: HTMLImageElement; template: boolean }, box: Box, textColour: string) {
  if (!mark.template) { ctx.drawImage(mark.image, box.x, box.y, box.w, box.h); return; }
  const scale = 4, off = document.createElement('canvas');
  off.width = box.w * scale; off.height = box.h * scale;
  const o = off.getContext('2d')!;
  o.drawImage(mark.image, 0, 0, off.width, off.height);
  o.globalCompositeOperation = 'source-in';
  o.fillStyle = textColour; o.fillRect(0, 0, off.width, off.height);
  ctx.drawImage(off, box.x, box.y, box.w, box.h);
}
/** The app icon signs the picture: its tile only, without the margin the macOS icon grid leaves (100 of 1024). */
export function drawBrand(ctx: CanvasRenderingContext2D, brand: HTMLImageElement | null, box: Box) {
  if (!brand) return;
  ctx.drawImage(brand, 100, 100, 824, 824, box.x, box.y, box.w, box.h);
}
export function drawSymbol(ctx: CanvasRenderingContext2D, image: HTMLImageElement | undefined, cx: number, cy: number, size: number) {
  if (image) ctx.drawImage(image, cx - size / 2, cy - size / 2, size, size);
}

/** Dollars with thousands separators: the picture is read at a glance. */
export const dollars = (value: number) => value.toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 2 });

/** Pictures are drawn at three times their size in points (1260 pixels wide): sharp on a phone that shows them across
 *  its whole screen, where twice the size was enlarged and soft. */
export const SCALE = 3;
/** A canvas at the picture's scale. */
export function canvasFor(height: number): { canvas: HTMLCanvasElement; ctx: CanvasRenderingContext2D } {
  const canvas = document.createElement('canvas');
  canvas.width = WIDTH * SCALE; canvas.height = Math.ceil(height * SCALE);
  const ctx = canvas.getContext('2d')!;
  ctx.scale(SCALE, SCALE);
  return { canvas, ctx };
}
/** A shadow or glow sized in points: the canvas sets shadows (and filters) in pixels, whatever its scale. */
export function shadow(ctx: CanvasRenderingContext2D, colour: string, blur: number, offsetY = 0) {
  ctx.shadowColor = colour; ctx.shadowBlur = blur * SCALE; ctx.shadowOffsetX = 0; ctx.shadowOffsetY = offsetY * SCALE;
}
export const blurFilter = (points: number) => `blur(${points * SCALE}px)`;
