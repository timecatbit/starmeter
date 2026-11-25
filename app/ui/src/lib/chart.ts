// The trend chart's arithmetic, shared by the panel and the share pictures: the scale, where each point sits, the
// smooth curves, and the legend's justified columns.
import { axisTokens, legendName, summaryPart } from './format';
import { t } from './i18n.svelte';
import { font, width } from './measure';
import type { Dashboard } from './types';

export type Point = [number, number];
/** A curve as a start point and cubic segments: [control 1, control 2, end]. */
export interface Curve { start: Point; segments: [Point, Point, Point][]; end: Point }

export const summaryProviders = (d: Dashboard) => d.providers.map(summaryPart).filter((p) => p !== null);

/** Each counted source's series over the chart's slots, missing points as zero. */
export function seriesOf(d: Dashboard): number[][] {
  const n = d.dates.length;
  return summaryProviders(d).map((p) => Array.from({ length: n }, (_, i) => Math.max(0, p.history[i] ?? 0)));
}

/** A rounded ceiling over the highest total, labelled at zero, half and full. */
export function chartAxis(d: Dashboard): { ceiling: number; labels: string[] } {
  const series = seriesOf(d), n = d.dates.length;
  const totals = Array.from({ length: n }, (_, i) => series.reduce((s, row) => s + row[i], 0));
  const observed = Math.max(Math.max(0, ...totals), 1), unit = Math.pow(10, Math.floor(Math.log10(observed)));
  const ceiling = (Math.ceil(((observed * 1.08) / unit) * 2) / 2) * unit;
  return { ceiling, labels: [0, 0.5, 1].map((f) => axisTokens(ceiling * f)) };
}

export function chartPositions(d: Dashboard): number[] {
  const n = d.dates.length, values = d.chartPositions;
  if (values && values.length === n && values.every(Number.isFinite) && values.every((v, i) => i === 0 || values[i - 1] < v)) return values;
  return Array.from({ length: n }, (_, i) => (n > 1 ? i / (n - 1) : 0.5));
}

/** Smoothed curves for each series followed by their total. Slopes and limiters are shared, so the smoothed total
 *  equals the sum of the tool curves everywhere and no line overshoots its readings. Needs at least two points. */
export function smoothCurves(series: number[][], positions: number[], plot: { x: number; y: number; w: number; h: number }, ceiling: number): Curve[] {
  const n = positions.length;
  const totals = Array.from({ length: n }, (_, i) => series.reduce((s, row) => s + row[i], 0));
  const intervals = Array.from({ length: n - 1 }, (_, i) => positions[i + 1] - positions[i]);
  const slopes = (values: number[]) => {
    const d = intervals.map((dt, i) => (values[i + 1] - values[i]) / dt);
    const result = new Array<number>(n).fill(0);
    result[0] = d[0]; result[n - 1] = d[n - 2];
    for (let i = 1; i < n - 1; i++) result[i] = (d[i - 1] + d[i]) / 3;
    return result;
  };
  const shapes = [...series, totals];
  const tangents = shapes.map(slopes);
  for (let i = 0; i < n; i++) {
    let factor = 1;
    shapes.forEach((values, j) => {
      const t = tangents[j][i];
      for (const k of [i - 1, i]) {
        if (k < 0 || k >= n - 1) continue;
        const d = (values[k + 1] - values[k]) / intervals[k];
        if (d === 0 || t * d < 0) factor = 0;
        else if (Math.abs(t) > 0) factor = Math.min(factor, Math.abs((3 * d) / t));
      }
    });
    for (const row of tangents) row[i] *= factor;
  }
  for (let i = 0; i < n - 1; i++) {
    let factor = 1;
    shapes.forEach((values, j) => {
      const delta = values[i + 1] - values[i];
      if (delta !== 0) {
        const a = (tangents[j][i] * intervals[i]) / delta, b = (tangents[j][i + 1] * intervals[i]) / delta, s = a * a + b * b;
        if (s > 9) factor = Math.min(factor, 3 / Math.sqrt(s));
      }
    });
    for (const row of tangents) { row[i] *= factor; row[i + 1] *= factor; }
  }
  const scale = plot.h / ceiling, bottom = plot.y + plot.h;
  return shapes.map((values, j) => {
    const points: Point[] = values.map((v, i) => [plot.x + positions[i] * plot.w, bottom - v * scale]);
    const segments: [Point, Point, Point][] = [];
    for (let i = 0; i < n - 1; i++) {
      const dt = intervals[i], dx = plot.w * dt;
      segments.push([[points[i][0] + dx / 3, points[i][1] - (tangents[j][i] * dt * scale) / 3],
        [points[i + 1][0] - dx / 3, points[i + 1][1] + (tangents[j][i + 1] * dt * scale) / 3], points[i + 1]]);
    }
    return { start: points[0], segments, end: points[n - 1] };
  });
}

export const svgPath = (c: Curve) => `M${c.start[0]},${c.start[1]}` + c.segments.map(([a, b, e]) => `C${a[0]},${a[1]} ${b[0]},${b[1]} ${e[0]},${e[1]}`).join('');

/** A smooth curve through points already on the screen (x rising): monotone cubic pieces (Fritsch and Carlson), each
 *  between its two points and no further, so a line never goes past its readings (below zero, or a peak that was
 *  never read) and a flat stretch stays flat. For the live lines, a straight line from point to point before. */
export function monotoneCurve(given: Point[]): Curve {
  // Each point right of the last one kept (not of the one before it in the list, which may have been left out).
  const points: Point[] = [];
  for (const p of given) if (!points.length || p[0] > points[points.length - 1][0] + 0.01) points.push(p);
  const n = points.length;
  if (n < 2) return { start: points[0] ?? [0, 0], segments: [], end: points[0] ?? [0, 0] };
  const dx = points.slice(1).map((p, i) => p[0] - points[i][0]);
  const d = points.slice(1).map((p, i) => (p[1] - points[i][1]) / dx[i]);
  const m = points.map((_, i) => (i === 0 ? d[0] : i === n - 1 ? d[n - 2] : d[i - 1] * d[i] <= 0 ? 0 : (d[i - 1] + d[i]) / 2));
  for (let i = 0; i < n - 1; i++) {
    if (d[i] === 0) { m[i] = 0; m[i + 1] = 0; continue; }
    const a = m[i] / d[i], b = m[i + 1] / d[i], s = a * a + b * b;
    if (a < 0) m[i] = 0;
    if (b < 0) m[i + 1] = 0;
    if (s > 9) { const k = 3 / Math.sqrt(s); m[i] = k * a * d[i]; m[i + 1] = k * b * d[i]; }
  }
  const segments: [Point, Point, Point][] = [];
  for (let i = 0; i < n - 1; i++) {
    const [x0, y0] = points[i], [x1, y1] = points[i + 1], h = dx[i] / 3;
    segments.push([[x0 + h, y0 + m[i] * h], [x1 - h, y1 - m[i + 1] * h], [x1, y1]]);
  }
  return { start: points[0], segments, end: points[n - 1] };
}

export function tracePath(ctx: CanvasRenderingContext2D, c: Curve, move = true) {
  if (move) ctx.moveTo(c.start[0], c.start[1]); else ctx.lineTo(c.start[0], c.start[1]);
  for (const [a, b, e] of c.segments) ctx.bezierCurveTo(a[0], a[1], b[0], b[1], e[0], e[1]);
}

export const legendLabels = (d: Dashboard) => [t('总量'), ...summaryProviders(d).map(legendName)];

/** Legend columns are justified: the first at the left edge, the last column's longest label ending at the right edge,
 *  column starts evenly spaced. Four per row when they fit, otherwise three, with rows as even as possible. */
export function legendLayout(labels: string[], total: number): { x: number; w: number }[][] {
  if (!labels.length) return [];
  const gap = 6, sizes = labels.map((l) => width(l, font(8.5)) + 16);
  const shape = (most: number) => {
    const rows = Math.ceil(labels.length / most), count = Math.ceil(labels.length / rows);
    let last = 0;
    for (let i = count - 1; i < sizes.length; i += count) last = Math.max(last, sizes[i]);
    return { count, pitch: count > 1 ? (total - last) / (count - 1) : 0 };
  };
  const fits = (s: { count: number; pitch: number }) => sizes.every((size, i) => i % s.count === s.count - 1 || size + gap <= s.pitch);
  let s = shape(4);
  if (!fits(s)) s = shape(3);
  if (!fits(s)) s = { ...s, pitch: total / s.count };
  const rows: { x: number; w: number }[][] = [];
  for (let start = 0; start < labels.length; start += s.count) {
    rows.push(Array.from({ length: Math.min(s.count, labels.length - start) }, (_, c) => {
      const x = c * s.pitch;
      return { x, w: c === s.count - 1 ? total - x : s.pitch - gap };
    }));
  }
  return rows;
}

/** The legend's extra rows push the chart down 17pt each; the dashboard's legend has 320pt between its margins (more
 *  in a pinned panel made wider). */
export const chartExtra = (d: Dashboard, room = 320) => Math.max(0, legendLayout(legendLabels(d), room).length - 1) * 17;

/** One column holds the chart's axis labels and the heatmap's row names, so the plot and the squares start together. */
export function labelColumn(d: Dashboard): number {
  const widest = (values: string[], f: string) => Math.max(0, ...values.map((v) => width(v, f)));
  const axis = d.showSummary === false || !d.dates.length ? 0 : widest(chartAxis(d).labels, font(8, 400));
  return Math.max(axis, widest(d.heatmap?.rowLabels ?? [], font(8))) + 10;
}
