// What every share picture says, worked out once, and the pieces all six draw the same way. The styles differ in
// colour, type and ornament, never in order: our star and name with the date in the band at the top, the title, the
// API-equivalent estimate as the hero, Token · 请求 · 缓存命中, the trend, the tools, and the signature.
//
// The picture is meant to be read on a phone in a screen or so, so it says less than the panel: each tool lists its
// first models and folds the rest into “其他 N 个模型”, and only as many tools as the share page asks for (the ones
// that used the most) have rows of their own; the rest fold into “其他工具”, each adding only what Summary counts of
// it, so nothing a tool shares with a gateway is added twice.
import starmeterMark from '../../../../assets/starmeter-mark.svg?url';
import { t, t as tr } from './i18n.svelte';
import { chartPositions } from './chart';
import { axisTokens, creditsText, currency, legendName, modelAmount, modelName, share, summaryPart, tokenLine, tokens } from './format';
import { qrModules } from './qr';
import { BOTTOM_BAND, dollars, drawBrand, drawMark, load, loadMarks, measure, middleText, shownHeatmap, shownRows, tag, TOP_BAND, WIDTH, type Marks } from './shareCommon';
import type { Dashboard, ModelUsage, ProviderUsage } from './types';

/** A sum of money split for drawing: the sign set small and raised, the figure with thousands separators. */
export interface Amount { symbol: string; figure: string }
export interface PosterModel {
  name: string; amount: string; share: string; ratio: number;
  /** 未定价, or N 次未定价, in orange. */
  note: string | null;
  /** The “其他 N 个模型” row. */
  other: boolean;
}
export interface PosterTool {
  /** The tool's id, for its colour and mark; 'other' for 其他工具. */
  id: string; name: string; cost: string; tokens: string;
  /** 占 X% of the tokens Summary counts; empty when a tag explains why its figures aren't all in the total. */
  share: string;
  /** “N 次已由 Magpie 计入” or “未计入 Summary”: always shown whole. */
  tag: string | null;
  /** “N 次未定价”, in orange: its amount covers the priced part only. */
  unpriced: string | null;
  models: PosterModel[];
  /** 其他工具: the tools folded into it, each shown by its mark and name. */
  members: { id: string; name: string }[];
}
export interface PosterChart {
  /** 每小时 Token, 每天 Token. */
  label: string;
  /** The streak of days used, for ranges of several days. */
  note: string;
  values: number[];
  /** A line for each listed tool Summary counts, and what the rest add, so the lines sum to the total. */
  series: { id: string; values: number[] }[]; rest: number[];
  positions: number[]; ceiling: number; axis: [string, string, string];
  /** The x-axis: first, middle (if any) and last, with their positions. */
  ticks: { label: string; at: number; index: number }[];
}
export interface Poster {
  title: string; date: string; costLabel: string; cost: Amount; tokens: string; requests: string; cache: string;
  chart: PosterChart | null;
  tools: PosterTool[]; marks: Marks; star: HTMLImageElement | null;
}

/** How many tools have rows of their own: the share page offers 1 to SHARE_TOOLS_MOST, 3 until another is picked. */
export const SHARE_TOOLS = 3, SHARE_TOOLS_MOST = 6;
/** Rows of models per tool. */
const MODELS = 3;

const titles: Record<string, string> = { today: '今日', yesterday: '昨日', '24h': '近 24 小时', '7d': '近 7 天', '30d': '近 30 天', week: '本周', month: '本月', all: '累计' };

/** 2026.11.19 for a day; 11.13 – 11.19 for days in one year; with times for the 24-hour window. The terminal writes
 *  2026-11-19 and 11-13 → 11-19. */
export function posterDate(d: Dashboard, dot = '.', dash = ' – '): string {
  const numbers = (value: string) => value.split(/[-T:]/).map(Number).filter((n) => !Number.isNaN(n));
  const day = numbers(d.date), p2 = (n: number) => String(n).padStart(2, '0');
  const single = day.length === 3 ? [day[0], p2(day[1]), p2(day[2])].join(dot) : d.date;
  if (d.rangeKey === 'today' || d.period?.length !== 2) return single;
  const a = numbers(d.period[0]), b = numbers(d.period[1]);
  if (a.length < 3 || b.length !== a.length) return single;
  if (a.length >= 5) return `${p2(a[1])}${dot}${p2(a[2])} ${p2(a[3])}:${p2(a[4])}${dash}${p2(b[1])}${dot}${p2(b[2])} ${p2(b[3])}:${p2(b[4])}`;
  if (a[0] !== b[0]) return [a[0], p2(a[1]), p2(a[2])].join(dot) + dash + [b[0], p2(b[1]), p2(b[2])].join(dot);
  if (a[1] === b[1] && a[2] === b[2]) return [a[0], p2(a[1]), p2(a[2])].join(dot);
  return p2(a[1]) + dot + p2(a[2]) + dash + p2(b[1]) + dot + p2(b[2]);
}

const amount = (value: number): Amount => ({ symbol: currency.symbol, figure: dollars(currency.convert(value)) });
const amountText = (a: Amount) => (a.figure === '—' ? '—' : a.symbol + a.figure);
const ratio = (value: number) => Math.min(1, Math.max(0, value));

/** A tool's first models, the rest folded into one row (never a row for a single model: it would take the same room). */
function models(p: ProviderUsage): PosterModel[] {
  const row = (m: ModelUsage): PosterModel => ({ name: modelName(m.name), amount: modelAmount(m), share: share(m.share), ratio: ratio(m.share), note: m.priceNote ?? null, other: false });
  if (p.models.length <= MODELS) return p.models.map(row);
  const rest = p.models.slice(MODELS - 1), sum = rest.reduce((s, m) => s + m.tokens, 0), part = rest.reduce((s, m) => s + m.share, 0);
  const credits = sum === 0 && rest.some((m) => m.credits !== undefined) ? rest.reduce((s, m) => s + (m.credits ?? 0), 0) : null;
  return [...p.models.slice(0, MODELS - 1).map(row), {
    name: t('其他 {n} 个模型', { n: rest.length }), amount: credits !== null ? creditsText(credits) : tokens(sum), share: share(part), ratio: ratio(part),
    note: rest.some((m) => m.priceNote) ? t('含未定价') : null, other: true,
  }];
}

const costText = (p: ProviderUsage) => (p.cost !== null ? amountText(amount(p.cost)) : p.credits !== null ? creditsText(p.credits) : t('费用待定价'));

export async function poster(d: Dashboard, markColour: string, limit = SHARE_TOOLS): Promise<Poster> {
  const rows = shownRows(d), counted = rows.map(summaryPart).filter((p): p is ProviderUsage => p !== null);
  const sumTokens = counted.reduce((s, p) => s + p.tokens, 0);
  const costs = counted.map((p) => p.cost).filter((c): c is number => c !== null), sumCost = costs.reduce((a, b) => a + b, 0);
  const partial = counted.some((p) => p.cost === null || p.unpriced > 0);
  const cached = counted.filter((p) => p.cache !== null), weight = cached.reduce((s, p) => s + p.tokens, 0);

  // The tools that used the most (the larger of their share of the tokens and of the estimate) keep their rows, in
  // homepage order, tagged ones too; the rest fold into 其他工具 with what Summary counts of them. One that Summary
  // doesn't count at all (a gateway counted every request, or it is 未计入 Summary) adds nothing there and is left out.
  const size = (p: ProviderUsage) => Math.max(sumTokens > 0 ? p.tokens / sumTokens : 0, sumCost > 0 && p.cost !== null ? p.cost / sumCost : 0);
  const kept = new Set([...rows].sort((a, b) => size(b) - size(a)).slice(0, Math.max(1, Math.round(limit))));
  const listed = rows.filter((p) => kept.has(p));
  const folded = rows.filter((p) => !kept.has(p)).map(summaryPart).filter((p): p is ProviderUsage => p !== null && (p.requests > 0 || p.tokens > 0));
  const tools: PosterTool[] = listed.map((p) => {
    const why = tag(p), part = summaryPart(p);
    return {
      id: p.id, name: p.name, cost: costText(p), tokens: tokenLine(p), tag: why, unpriced: p.priceNote ?? null, models: models(p), members: [],
      share: !why && part && part.tokens > 0 && sumTokens > 0 ? t('占 {n}', { n: share(part.tokens / sumTokens) }) : '',
    };
  });
  if (folded.length) {
    const t = folded.reduce((s, p) => s + p.tokens, 0), known = folded.filter((p) => p.cost !== null);
    const credits = folded.reduce((s, p) => s + (p.cost === null ? p.credits ?? 0 : 0), 0);
    const unpriced = folded.reduce((s, p) => s + (p.priceNote ? p.unpriced : 0), 0);
    tools.push({
      id: 'other', name: tr('其他工具'), tokens: t > 0 ? tokens(t) + ' Token' : tr('Token 未记录'), tag: null, models: [],
      cost: known.length ? amountText(amount(known.reduce((s, p) => s + (p.cost ?? 0), 0))) : credits > 0 ? creditsText(credits) : '—',
      share: t > 0 && sumTokens > 0 ? tr('占 {n}', { n: share(t / sumTokens) }) : '', unpriced: unpriced > 0 ? tr('{n} 次未定价', { n: unpriced }) : null,
      members: folded.map((p) => ({ id: p.id, name: legendName(p) })),
    });
  }

  // The trend: the total, and a line for each listed tool Summary counts.
  const n = d.dates.length;
  let chart: PosterChart | null = null;
  if (n > 1) {
    const history = (p: ProviderUsage) => Array.from({ length: n }, (_, i) => Math.max(0, p.history[i] ?? 0));
    const values = Array.from({ length: n }, (_, i) => counted.reduce((s, p) => s + Math.max(0, p.history[i] ?? 0), 0));
    const series = listed.map((p) => [p, summaryPart(p)] as const).filter(([, part]) => part !== null).map(([p, part]) => ({ id: p.id, values: history(part!) }));
    const rest = values.map((v, i) => Math.max(0, v - series.reduce((s, line) => s + line.values[i], 0)));
    const observed = Math.max(1, ...values), unit = Math.pow(10, Math.floor(Math.log10(observed)));
    const ceiling = (Math.ceil(((observed * 1.08) / unit) * 2) / 2) * unit;
    const positions = chartPositions(d);
    let middle: number | null = null;
    for (let i = 1; i < n - 1; i++) if (positions[i] >= 0.25 && positions[i] <= 0.75 && (middle === null || Math.abs(positions[i] - 0.5) < Math.abs(positions[middle] - 0.5))) middle = i;
    chart = {
      label: d.chartUnit + ' Token', note: shownHeatmap(d)?.streak ?? '', values, series, rest, positions, ceiling,
      axis: [axisTokens(0), axisTokens(ceiling / 2), axisTokens(ceiling)],
      ticks: [0, ...(middle !== null ? [middle] : []), n - 1].map((i) => ({ label: d.dates[i], at: positions[i], index: i })),
    };
  }

  const [marks, star] = await Promise.all([loadMarks(rows, markColour), load(starmeterMark)]);
  return {
    title: t('{range} AI 用量', { range: titles[d.rangeKey] ? t(titles[d.rangeKey]) : d.rangeLabel }),
    date: posterDate(d),
    costLabel: partial ? t('已知 API 等值估算') : t('API 等值估算'),
    cost: costs.length ? amount(sumCost) : { symbol: currency.symbol, figure: '—' },
    tokens: counted.length ? tokens(sumTokens) : '—',
    requests: counted.length ? counted.reduce((s, p) => s + p.requests, 0).toLocaleString('en-US') : '—',
    cache: weight > 0 ? ((cached.reduce((s, p) => s + (p.cache ?? 0) * p.tokens, 0) / weight) * 100).toFixed(1) + '%' : '—',
    chart, tools, marks, star,
  };
}

/** The three figures under the hero, in this order in every style. */
export const figures = (p: Poster): [string, string][] => [['Token', p.tokens], [t('请求'), p.requests], [t('缓存命中'), p.cache]];

// MARK: Drawing every style shares

/** Our star in one colour (it is drawn as a template). */
function drawStar(ctx: CanvasRenderingContext2D, star: HTMLImageElement | null, x: number, y: number, size: number, colour: string) {
  if (!star) return;
  const scale = 4, off = document.createElement('canvas');
  off.width = off.height = Math.ceil(size * scale);
  const o = off.getContext('2d')!;
  o.drawImage(star, 0, 0, off.width, off.height);
  o.globalCompositeOperation = 'source-in';
  o.fillStyle = colour; o.fillRect(0, 0, off.width, off.height);
  ctx.drawImage(off, x, y, size, size);
}

/** The band at the top is where a phone's island sits when the picture fills the screen (the picture is 420 wide, a
 *  phone 393): the masthead keeps to its ends, our star and name at the left, the date at the right. */
const MAST_MIDDLE = 31.5, ISLAND_LEFT = 140, ISLAND_RIGHT = 282;
interface MastStyle { ink: string; dateInk?: string; font: (size: number, weight: number) => string; name?: number; date?: number; left: number; right: number; weight?: number }
export function drawMasthead(ctx: CanvasRenderingContext2D, p: Poster, name: string, s: MastStyle) {
  const nameSize = s.name ?? 18, starSize = Math.round(nameSize * 1.05), weight = s.weight ?? 800;
  drawStar(ctx, p.star, s.left, MAST_MIDDLE - starSize / 2, starSize, s.ink);
  const nameLeft = s.left + starSize + 7;
  let fitted = nameSize;
  while (fitted > 12 && measure(ctx, name, s.font(fitted, weight)) > ISLAND_LEFT - nameLeft) fitted -= 0.5;
  middleText(ctx, name, nameLeft, MAST_MIDDLE, ISLAND_LEFT - nameLeft, s.font(fitted, weight), s.ink);
  // A date too long for the end of the band breaks after its dash onto two lines.
  const room = s.right - ISLAND_RIGHT, dateSize = s.date ?? 15, colour = s.dateInk ?? s.ink;
  if (measure(ctx, p.date, s.font(dateSize, weight - 100)) <= room) { middleText(ctx, p.date, ISLAND_RIGHT, MAST_MIDDLE, room, s.font(dateSize, weight - 100), colour, 'right'); return; }
  const cut = p.date.search(/ [–→] /), lines = cut > 0 ? [p.date.slice(0, cut + 2), p.date.slice(cut + 3)] : [p.date];
  let small = dateSize * 0.8;
  while (small > 9 && Math.max(...lines.map((l) => measure(ctx, l, s.font(small, weight - 100)))) > room) small -= 0.5;
  lines.forEach((line, i) => middleText(ctx, line, ISLAND_RIGHT, MAST_MIDDLE + (i - (lines.length - 1) / 2) * small * 1.2, room, s.font(small, weight - 100), colour, 'right'));
}

/** How wide a sum of money is drawn at a size (its sign at half the size). */
export function amountWidth(ctx: CanvasRenderingContext2D, a: Amount, size: number, font: (size: number) => string, tracking = 0): number {
  if (a.figure === '—') return measure(ctx, '—', font(size));
  ctx.letterSpacing = `${tracking * size}px`;
  const width = measure(ctx, a.symbol, font(size * 0.5)) + size * 0.05 + measure(ctx, a.figure, font(size));
  ctx.letterSpacing = '0px';
  return width;
}
/** The largest size up to `start` at which the sum fits the width. */
export function fitAmount(ctx: CanvasRenderingContext2D, a: Amount, width: number, start: number, min: number, font: (size: number) => string, tracking = 0): number {
  let size = start;
  while (size > min && amountWidth(ctx, a, size, font, tracking) > width) size -= 1;
  return size;
}
/** A sum on a baseline: the sign small and raised to the figure's cap height, then the figure. Returns its width. */
export function drawAmount(ctx: CanvasRenderingContext2D, a: Amount, x: number, baseline: number, size: number, font: (size: number) => string,
  colour: string, symbolColour = colour, tracking = 0, align: 'left' | 'center' | 'right' = 'left'): number {
  const width = amountWidth(ctx, a, size, font, tracking);
  let left = align === 'left' ? x : align === 'right' ? x - width : x - width / 2;
  ctx.textAlign = 'left'; ctx.textBaseline = 'alphabetic';
  if (a.figure === '—') { ctx.font = font(size); ctx.fillStyle = colour; ctx.fillText('—', left, baseline); return width; }
  ctx.font = font(size * 0.5); ctx.fillStyle = symbolColour;
  ctx.fillText(a.symbol, left, baseline - size * 0.3);
  left += ctx.measureText(a.symbol).width + size * 0.05;
  ctx.letterSpacing = `${tracking * size}px`;
  ctx.font = font(size); ctx.fillStyle = colour;
  ctx.fillText(a.figure, left, baseline);
  ctx.letterSpacing = '0px';
  return width;
}

/** The tools folded into 其他工具 on one line, each by its mark and name; when they don't all fit, the first ones
 *  and “等 N 个”. */
export function drawMembers(ctx: CanvasRenderingContext2D, p: Poster, t: PosterTool, x: number, middle: number, width: number, css: string, colour: string,
  markInk: string, mark = 13) {
  const widths = t.members.map((m) => (p.marks.tools.get(m.id) ? mark + 4 : 0) + measure(ctx, m.name, css));
  const rest = tr('等 {n} 个', { n: t.members.length }), restWidth = measure(ctx, rest, css);
  let shown = t.members.length;
  const needed = (count: number) => widths.slice(0, count).reduce((s, w) => s + w + 12, 0) + (count < t.members.length ? restWidth : -12);
  while (shown > 1 && needed(shown) > width) shown--;
  let at = x;
  for (const m of t.members.slice(0, shown)) {
    const logo = p.marks.tools.get(m.id);
    if (logo) { drawMark(ctx, logo, { x: at, y: middle - mark / 2, w: mark, h: mark }, markInk); at += mark + 4; }
    middleText(ctx, m.name, at, middle, x + width - at, css, colour);
    at += measure(ctx, m.name, css) + 12;
  }
  if (shown < t.members.length) middleText(ctx, rest, at, middle, x + width - at, css, colour);
}

/** How tall the signature is: its QR code's tile. */
export const SIGNATURE = 66;
export interface SignStyle { name: string; small: (size: number) => string; ink: string; muted: string; size?: number }

/** The QR code in the logo's look: midnight-navy dots on a pale tile, rounded finder eyes, our icon in the middle (level
 *  Q, so the modules under the icon are recovered). Dark modules on light, which every scanner reads. */
function drawQr(ctx: CanvasRenderingContext2D, brand: HTMLImageElement | null, url: string, x: number, y: number, size: number): boolean {
  const modules = qrModules(url, 'Q');
  if (!modules) return false;
  const n = modules.length, quiet = 2, cell = size / (n + quiet * 2), ox = x + quiet * cell, oy = y + quiet * cell, paper = '#F4F7FF';
  ctx.fillStyle = paper; ctx.beginPath(); ctx.roundRect(x, y, size, size, size * 0.16); ctx.fill();
  ctx.strokeStyle = 'rgba(31, 58, 122, 0.16)'; ctx.lineWidth = 0.8; ctx.beginPath(); ctx.roundRect(x + 0.4, y + 0.4, size - 0.8, size - 0.8, size * 0.16); ctx.stroke();
  const ink = ctx.createLinearGradient(x, y, x + size * 0.35, y + size);
  ink.addColorStop(0, '#1F3A7A'); ink.addColorStop(0.55, '#0E1D4C'); ink.addColorStop(1, '#050A24');
  const logo = n >= 29 ? 7 : 5, clear = logo + 2, from = Math.floor((n - clear) / 2);
  const eye = (mx: number, my: number) => (mx < 7 && my < 7) || (mx >= n - 7 && my < 7) || (mx < 7 && my >= n - 7);
  const centre = (mx: number, my: number) => mx >= from && mx < from + clear && my >= from && my < from + clear;
  ctx.fillStyle = ink;
  modules.forEach((row, my) => row.forEach((dark, mx) => {
    if (!dark || eye(mx, my) || centre(mx, my)) return;
    ctx.beginPath(); ctx.arc(ox + (mx + 0.5) * cell, oy + (my + 0.5) * cell, cell * 0.43, 0, Math.PI * 2); ctx.fill();
  }));
  for (const [fx, fy] of [[0, 0], [n - 7, 0], [0, n - 7]]) {
    const ex = ox + fx * cell, ey = oy + fy * cell;
    ctx.fillStyle = ink; ctx.beginPath(); ctx.roundRect(ex, ey, cell * 7, cell * 7, cell * 2.2); ctx.fill();
    ctx.fillStyle = paper; ctx.beginPath(); ctx.roundRect(ex + cell, ey + cell, cell * 5, cell * 5, cell * 1.5); ctx.fill();
    ctx.fillStyle = ink; ctx.beginPath(); ctx.roundRect(ex + cell * 2, ey + cell * 2, cell * 3, cell * 3, cell * 1); ctx.fill();
  }
  drawBrand(ctx, brand, { x: ox + (from + 1) * cell, y: oy + (from + 1) * cell, w: logo * cell, h: logo * cell });
  return true;
}

/** The signature at the foot, centred: the QR code to the project, then the app icon with our name over the full
 *  address. */
export function drawSignature(ctx: CanvasRenderingContext2D, p: Poster, project: { name: string; url: string }, y: number, s: SignStyle) {
  const room = WIDTH - 48 - SIGNATURE - 16;
  let size = s.size ?? 11.5;
  while (size > 8 && measure(ctx, project.url, s.small(size)) > room) size -= 0.5;
  const nameWidth = measure(ctx, project.name, s.name), block = Math.max(28 + nameWidth, measure(ctx, project.url, s.small(size)));
  const shown = qrModules(project.url, 'Q') !== null, tile = shown ? SIGNATURE : 0, gap = shown ? 16 : 0;
  const left = Math.round((WIDTH - tile - gap - block) / 2), middle = y + SIGNATURE / 2;
  if (shown) drawQr(ctx, p.marks.brand, project.url, left, y, tile);
  const x = left + tile + gap;
  drawBrand(ctx, p.marks.brand, { x, y: middle - 21, w: 20, h: 20 });
  middleText(ctx, project.name, x + 28, middle - 11, nameWidth + 2, s.name, s.ink);
  middleText(ctx, project.url, x, middle + 13, room, s.small(size), s.muted);
}

// MARK: The rows every style lays its picture out on

/** Each section's height, the same in every style, so a picture is as tall in one style as in another and switching
 *  styles never moves anything. A tool takes a head (its name row and figures line), a row per model, and padding. */
export const ROWS = { title: 52, hero: 120, stats: 76, chart: 168, head: 52, model: 21, pad: 12, gap: 8, foot: 28, sign: 106 };
/** Where the hero's figure sits, from the top of its section, in every style. */
export const HERO_BASELINE = 104;
export const toolHeight = (t: PosterTool) => ROWS.head + (t.members.length ? 1 : t.models.length) * ROWS.model + ROWS.pad;
export interface Layout { title: number; hero: number; stats: number; chart: number | null; tools: number[]; foot: number; sign: number; height: number }
export function layout(p: Poster): Layout {
  let y = TOP_BAND;
  const at = (height: number) => { const top = y; y += height; return top; };
  const title = at(ROWS.title), hero = at(ROWS.hero), stats = at(ROWS.stats), chart = p.chart ? at(ROWS.chart) : null;
  const tools = p.tools.map((t) => at(toolHeight(t) + ROWS.gap));
  const foot = at(ROWS.foot), sign = at(ROWS.sign);
  return { title, hero, stats, chart, tools, foot, sign, height: y + BOTTOM_BAND };
}

/** A deterministic number in [0, 1) for decorations, so a picture drawn twice looks the same. */
export function seeded(seed: number): () => number {
  let state = (seed * 2654435761) >>> 0 || 1;
  return () => {
    state ^= state << 13; state >>>= 0; state ^= state >>> 17; state ^= state << 5; state >>>= 0;
    return state / 4294967296;
  };
}

/** Fonts the posters use beyond the panel's: a serif for the night sky, a heavy grotesque for print. */
export const SERIF = "'Iowan Old Style', 'Palatino', 'Songti SC', 'STSong', 'Noto Serif SC', 'SimSun', serif";
export const GROTESK = "'Helvetica Neue', 'PingFang SC', 'Microsoft YaHei UI', 'Segoe UI', sans-serif";
export const posterFont = (size: number, weight: number, family: string, italic = false) => `${italic ? 'italic ' : ''}${weight} ${size}px ${family}`;

type Plot = { x: number; y: number; w: number; h: number };
/** One bar per reading, evenly across the plot, on the poster's scale; an empty slot keeps a hairline. */
export function drawBars(ctx: CanvasRenderingContext2D, chart: PosterChart, plot: Plot, colour: string, gap = 0.3, radius = 0) {
  const n = chart.values.length, slot = plot.w / n, w = Math.max(2, slot * (1 - gap));
  chart.values.forEach((v, i) => {
    const h = v > 0 ? Math.max(2, (v / chart.ceiling) * plot.h) : 1;
    ctx.fillStyle = colour;
    ctx.beginPath(); ctx.roundRect(plot.x + i * slot + (slot - w) / 2, plot.y + plot.h - h, w, h, v > 0 ? [radius, radius, 0, 0] : 0); ctx.fill();
  });
}
/** Where a reading's bar stands across the plot. */
export const slotCentre = (chart: PosterChart, plot: Plot, index: number) => plot.x + ((index + 0.5) * plot.w) / Math.max(1, chart.values.length);
/** A tick's label box under a plot: the first starts at the left edge, the last ends at the right, the middle is centred. */
export function tickBox(i: number, count: number, x: number, left: number, right: number, width = 90): { x: number; align: 'left' | 'center' | 'right' } {
  if (i === 0) return { x: left, align: 'left' };
  if (i === count - 1) return { x: right - width, align: 'right' };
  return { x: x - width / 2, align: 'center' };
}
