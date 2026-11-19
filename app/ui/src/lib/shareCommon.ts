// What both share pictures share: the rows they show, their tags and notes, the bands of pattern, the heatmap, text on
// a canvas as AppKit set it, and the marks and signature.
import starmeterIcon from '../../../../assets/starmeter.svg?url';
import { clientLine, legendName, share, summaryPart } from './format';
import { HeatmapLayout, levels as heatmapLevels, past as heatmapPast, rows as heatmapRows, active } from './heatmap';
import { iconImage } from './icons';
import type { Dashboard, ProviderUsage, UsageHeatmap } from './types';

export const WIDTH = 420;
/** Every picture has a band of its style's pattern above the content (63pt: on an iPhone 393pt wide the island sits
 *  centred in it) and another below the signature, clear of the home indicator. */
export const TOP_BAND = 63, BOTTOM_BAND = 44;
export const HEATMAP_SQUARE = 9;

/** Only the tools with something in the range get a block, in the panel's order. */
export const shownRows = (d: Dashboard) => d.providers.filter((p) => p.available && (p.tokens > 0 || p.requests > 0 || (p.credits ?? 0) > 0));
/** Totals, the chart and the share bar use what Summary counts. */
export const countedRows = (d: Dashboard) => shownRows(d).map(summaryPart).filter((p): p is ProviderUsage => p !== null);
export const sharedRows = (d: Dashboard) => countedRows(d).filter((p) => p.tokens > 0);
/** The tag at the right of a tool's token line: what Summary leaves out or a gateway counted. */
export const tag = (p: ProviderUsage) => (p.summaryIncluded === false ? '未计入 Summary' : p.counted && p.subtitle ? p.subtitle : null);
export const priceLine = (p: ProviderUsage) => (p.priceNote ? p.priceNote + '，金额只含已定价的部分' : null);
export const title = (d: Dashboard) =>
  ({ today: '今天', yesterday: '昨天', '24h': '近 24 小时', '7d': '近 7 天', '30d': '近 30 天', week: '本周', month: '本月', all: '累计' } as Record<string, string>)[d.rangeKey]
    ?.concat(' AI 用量') ?? d.rangeLabel + ' AI 用量';
/** The heatmap as the panel shows it, once read and something used. */
export const shownHeatmap = (d: Dashboard): UsageHeatmap | null => (d.heatmap && !d.heatmap.status && active(d.heatmap).includes(true) ? d.heatmap : null);
export const heatmapGridHeight = (h: UsageHeatmap, width: number) => new HeatmapLayout(0, 0, width, h.columns, heatmapRows(h), HEATMAP_SQUARE).height;

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

export function setFont(ctx: CanvasRenderingContext2D, css: string) { ctx.font = css; }
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

// MARK: Shared pieces

export function drawHeatmap(ctx: CanvasRenderingContext2D, h: UsageHeatmap, x: number, y: number, width: number, shades: string[]) {
  const layout = new HeatmapLayout(x, y, width, h.columns, heatmapRows(h), HEATMAP_SQUARE), levels = heatmapLevels(h), past = heatmapPast(h);
  for (let i = 0; i < Math.min(h.tokens.length, layout.rowCount * layout.columns); i++) {
    const s = layout.square(i);
    if (i >= past) { ctx.strokeStyle = shades[0]; ctx.lineWidth = 0.8; roundRect(ctx, { x: s.x + 0.4, y: s.y + 0.4, w: s.size - 0.8, h: s.size - 0.8 }, 1.8); ctx.stroke(); continue; }
    ctx.fillStyle = shades[levels[i]]; roundRect(ctx, { x: s.x, y: s.y, w: s.size, h: s.size }, 1.8); ctx.fill();
  }
}

/** A gateway's clients, wrapped onto as many lines as the picture needs rather than cut off. */
export function clientLines(ctx: CanvasRenderingContext2D, p: ProviderUsage, width: number, css: string): string[] {
  if (!clientLine(p) || !p.clients) return [];
  ctx.font = css;
  const lines: string[] = [];
  let current = '客户端';
  for (const item of p.clients.map((c) => c.name + ' ' + share(c.share))) {
    const longer = current + ' · ' + item;
    if (ctx.measureText(longer).width > width && current !== '客户端') { lines.push(current); current = item; } else current = longer;
  }
  return [...lines, current];
}

export const legendNameOf = legendName;

/** Dollars with thousands separators: the picture is read at a glance. */
export const dollars = (value: number) => value.toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 2 });

/** A canvas at twice the picture's size. */
export function canvasFor(height: number): { canvas: HTMLCanvasElement; ctx: CanvasRenderingContext2D } {
  const canvas = document.createElement('canvas');
  canvas.width = WIDTH * 2; canvas.height = Math.ceil(height * 2);
  const ctx = canvas.getContext('2d')!;
  ctx.scale(2, 2);
  return { canvas, ctx };
}
