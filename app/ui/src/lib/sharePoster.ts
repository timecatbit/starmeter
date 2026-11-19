// What the poster styles (报刊, 夜空, 地铁, 织物) print, worked out once: the headline figures, the chart's series, every
// tool with its models, and the marks they sign with. Each style only lays these out in its own look.
import starmeterMark from '../../../../assets/starmeter-mark.svg?url';
import { chartPositions } from './chart';
import { axisTokens, currency, headline, modelAmount, modelName, share, tokenLine, tokens } from './format';
import { countedRows, dollars, load, loadMarks, shownRows, tag, type Marks } from './shareCommon';
import type { Dashboard, ProviderUsage } from './types';

export interface PosterModel { name: string; amount: string; share: string; ratio: number; note: string | null }
export interface PosterTool {
  p: ProviderUsage; name: string; cost: string; tokens: string;
  /** 占总量 X%, or why it isn't in the total (a gateway counted it, or Summary leaves it out). */
  note: string; counted: boolean; models: PosterModel[];
}
export interface Poster {
  title: string; date: string; tokens: string; cost: string; costLabel: string; requests: string; cache: string;
  /** 每小时 Token, 每天 Token. */
  chartLabel: string; values: number[]; positions: number[]; ceiling: number; axis: [string, string, string];
  /** The x-axis: first, middle (if any) and last, with their positions. */
  ticks: { label: string; at: number; index: number }[];
  /** When the reading was taken (今天: the last point's time), for "截至 14:49". */
  until: string | null;
  tools: PosterTool[]; marks: Marks; star: HTMLImageElement | null;
}

const titles: Record<string, string> = { today: '今日', yesterday: '昨日', '24h': '近 24 小时', '7d': '近 7 天', '30d': '近 30 天', week: '本周', month: '本月', all: '累计' };

/** 2026.11.19 for a day; 11.13 – 11.19 for days in one year; with times for the 24-hour window. */
export function posterDate(d: Dashboard): string {
  const numbers = (value: string) => value.split(/[-T:]/).map(Number).filter((n) => !Number.isNaN(n));
  const day = numbers(d.date), p2 = (n: number) => String(n).padStart(2, '0');
  const single = day.length === 3 ? `${day[0]}.${p2(day[1])}.${p2(day[2])}` : d.date;
  if (d.rangeKey === 'today' || d.rangeKey === 'yesterday' || d.period?.length !== 2) return single;
  const a = numbers(d.period[0]), b = numbers(d.period[1]);
  if (a.length < 3 || b.length !== a.length) return single;
  if (a.length >= 5) return `${p2(a[1])}.${p2(a[2])} ${p2(a[3])}:${p2(a[4])} – ${p2(b[1])}.${p2(b[2])} ${p2(b[3])}:${p2(b[4])}`;
  if (a[0] !== b[0]) return `${a[0]}.${p2(a[1])}.${p2(a[2])} – ${b[0]}.${p2(b[1])}.${p2(b[2])}`;
  return a[1] === b[1] && a[2] === b[2] ? single : `${a[0]}.${p2(a[1])}.${p2(a[2])} – ${p2(b[1])}.${p2(b[2])}`;
}

export async function poster(d: Dashboard, markColour: string): Promise<Poster> {
  const rows = shownRows(d), counted = countedRows(d);
  const sum = counted.reduce((s, p) => s + p.tokens, 0);
  const costs = counted.map((p) => p.cost).filter((c): c is number => c !== null);
  const partial = counted.some((p) => p.cost === null || p.unpriced > 0);
  const cached = counted.filter((p) => p.cache !== null), weight = cached.reduce((s, p) => s + p.tokens, 0);
  const n = d.dates.length;
  const values = Array.from({ length: n }, (_, i) => counted.reduce((s, p) => s + Math.max(0, p.history[i] ?? 0), 0));
  const observed = Math.max(1, ...values), unit = Math.pow(10, Math.floor(Math.log10(observed)));
  const ceiling = (Math.ceil(((observed * 1.08) / unit) * 2) / 2) * unit;
  const positions = chartPositions(d);
  let middle: number | null = null;
  for (let i = 1; i < n - 1; i++) if (positions[i] >= 0.25 && positions[i] <= 0.75 && (middle === null || Math.abs(positions[i] - 0.5) < Math.abs(positions[middle] - 0.5))) middle = i;
  const ticks = n ? [0, ...(middle !== null ? [middle] : []), ...(n > 1 ? [n - 1] : [])].map((i) => ({ label: d.dates[i], at: positions[i], index: i })) : [];
  const tools: PosterTool[] = rows.map((p) => {
    const inSummary = p.summaryIncluded !== false, part = inSummary ? (p.counted?.tokens ?? p.tokens) : 0;
    const why = tag(p);
    return {
      p, name: p.name, tokens: tokenLine(p), counted: inSummary && !why,
      cost: p.cost !== null ? currency.symbol + dollars(currency.convert(p.cost)) : headline(p),
      note: why ?? (sum > 0 && part > 0 ? '占总量 ' + share(part / sum) : ''),
      models: p.models.map((m) => ({ name: modelName(m.name), amount: modelAmount(m), share: share(m.share), ratio: Math.min(1, Math.max(0, m.share)), note: m.priceNote ?? null })),
    };
  });
  const [marks, star] = await Promise.all([loadMarks(rows, markColour), load(starmeterMark)]);
  return {
    title: (titles[d.rangeKey] ?? d.rangeLabel) + ' AI 用量',
    date: posterDate(d),
    tokens: counted.length ? tokens(sum) : '—',
    cost: costs.length ? currency.symbol + dollars(currency.convert(costs.reduce((a, b) => a + b, 0))) : '—',
    costLabel: partial ? '已知 API 等值估算' : 'API 等值估算',
    requests: counted.length ? counted.reduce((s, p) => s + p.requests, 0).toLocaleString('en-US') : '—',
    cache: weight > 0 ? ((cached.reduce((s, p) => s + (p.cache ?? 0) * p.tokens, 0) / weight) * 100).toFixed(1) + '%' : '—',
    chartLabel: d.chartUnit + ' Token', values, positions, ceiling,
    axis: [axisTokens(0), axisTokens(ceiling / 2), axisTokens(ceiling)],
    ticks, until: d.rangeKey === 'today' && n ? d.dates[n - 1] : null,
    tools, marks, star,
  };
}

/** Our star in one colour (it is drawn as a template). */
export function drawStar(ctx: CanvasRenderingContext2D, star: HTMLImageElement | null, x: number, y: number, size: number, colour: string) {
  if (!star) return;
  const scale = 4, off = document.createElement('canvas');
  off.width = off.height = Math.ceil(size * scale);
  const o = off.getContext('2d')!;
  o.drawImage(star, 0, 0, off.width, off.height);
  o.globalCompositeOperation = 'source-in';
  o.fillStyle = colour; o.fillRect(0, 0, off.width, off.height);
  ctx.drawImage(off, x, y, size, size);
}

/** A deterministic number in [0, 1) for decorations, so a picture drawn twice looks the same. */
export function seeded(seed: number): () => number {
  let state = (seed * 2654435761) >>> 0 || 1;
  return () => {
    state ^= state << 13; state >>>= 0; state ^= state >>> 17; state ^= state << 5; state >>>= 0;
    return state / 4294967296;
  };
}

/** Splits 644.67M into the figure and its unit letter, for styles that size them apart. */
export function figureParts(value: string): [string, string] {
  const match = value.match(/^([\d.,—]+)([KMB]?)$/);
  return match ? [match[1], match[2]] : [value, ''];
}

/** A tool's row: tokens and the share of the total (or why it isn't in it). */
export const toolLine = (t: PosterTool) => [t.tokens, t.note].filter(Boolean).join(' · ');

/** Fonts the posters use beyond the panel's: a serif for the night sky, a heavy grotesque for print. */
export const SERIF = "'Iowan Old Style', 'Palatino', 'Songti SC', 'STSong', 'Noto Serif SC', 'SimSun', serif";
export const GROTESK = "'Helvetica Neue', 'PingFang SC', 'Microsoft YaHei UI', 'Segoe UI', sans-serif";
export const posterFont = (size: number, weight: number, family: string, italic = false) => `${italic ? 'italic ' : ''}${weight} ${size}px ${family}`;

export type Plot = { x: number; y: number; w: number; h: number };
/** One bar per reading, evenly across the plot, on the poster's scale; an empty slot keeps a hairline. */
export function drawBars(ctx: CanvasRenderingContext2D, poster: Poster, plot: Plot, colour: string, gap = 0.3, radius = 0) {
  const n = poster.values.length;
  if (!n) return;
  const slot = plot.w / n, w = Math.max(2, slot * (1 - gap));
  poster.values.forEach((v, i) => {
    const h = v > 0 ? Math.max(2, (v / poster.ceiling) * plot.h) : 1;
    ctx.fillStyle = colour;
    ctx.beginPath(); ctx.roundRect(plot.x + i * slot + (slot - w) / 2, plot.y + plot.h - h, w, h, v > 0 ? [radius, radius, 0, 0] : 0); ctx.fill();
  });
}
/** Where a reading's bar stands across the plot. */
export const slotCentre = (poster: Poster, plot: Plot, index: number) => plot.x + ((index + 0.5) * plot.w) / Math.max(1, poster.values.length);
