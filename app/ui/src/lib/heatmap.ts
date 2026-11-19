// The heatmap's squares: their shades, what a square says when pointed at, and where each one goes.
import { money, tokens } from './format';
import type { UsageHeatmap } from './types';

export const rows = (h: UsageHeatmap) => Math.max(1, Math.ceil(h.tokens.length / Math.max(1, h.columns)));
export const past = (h: UsageHeatmap) => Math.max(0, h.tokens.length - h.future);
export const active = (h: UsageHeatmap) => h.tokens.map((t, i) => t > 0 || (h.requests[i] ?? 0) > 0);

/** Four shades by rank among the squares with tokens, so the busiest is always the darkest; squares with requests and
 *  no tokens (Qoder's Credits) take the lightest. */
export function levels(h: UsageHeatmap): number[] {
  const values = h.tokens.filter((t) => t > 0).sort((a, b) => a - b), flags = active(h);
  return h.tokens.map((t, i) => {
    if (!flags[i]) return 0;
    if (t <= 0) return 1;
    let low = 0, high = values.length;
    while (low < high) { const mid = (low + high) >> 1; if (values[mid] <= t) low = mid + 1; else high = mid; }
    return Math.max(1, Math.min(4, Math.ceil((4 * low) / values.length)));
  });
}

/** The square pointed at, in one line: when, tokens, known cost and requests. */
export function detail(h: UsageHeatmap, index: number): string {
  if (index < 0 || index >= h.labels.length) return '';
  const n = h.requests[index] ?? 0;
  if (!active(h)[index]) return h.labels[index] + ' · 无用量';
  const parts = [h.labels[index]];
  if (h.tokens[index] > 0) parts.push(tokens(h.tokens[index]) + ' Token');
  const cost = h.costs[index] ?? 0, partial = (h.unpriced[index] ?? 0) > 0;
  if (cost > 0) parts.push((partial ? '已知 ' : '') + money(cost));
  parts.push(`${n} 请求`);
  return parts.join(' · ');
}

/** What the bottom left says while no square is pointed at: why there's nothing yet, what's missing, or the streak. */
export function note(h: UsageHeatmap): string | null {
  if (h.status) return h.status;
  if (!active(h).includes(true)) return '暂无用量';
  if (h.notice) return h.notice;
  return h.streak ?? null;
}

/** A punch card in the logo's halftone: a dot for each hour or day, its size the amount, spread across each row from
 *  the label column to the right edge. Dots are at most `max` across (2pt apart at the least), 8pt where there are
 *  several rows, the rows 2.5pt apart, so seven days by the hour stay low; an empty hour is a speck, as the logo's faint
 *  background dots are. */
export class HeatmapLayout {
  constructor(readonly x: number, readonly y: number, readonly width: number, readonly columns: number, readonly rowCount: number,
    readonly max = 10) {}
  get size() {
    const n = Math.max(1, this.columns);
    return Math.max(3, Math.min(this.rowCount > 1 ? Math.min(this.max, 8) : this.max, this.width / n - 2));
  }
  get columnPitch() { return this.columns > 1 ? (this.width - this.size) / (this.columns - 1) : 0; }
  get rowPitch() { return this.size + 2.5; }
  get height() { return this.rowCount * this.rowPitch - 2.5; }
  /** An hour axis (0 6 12 18 24) under a grid of several days by the hour; one row of hours lines up with the trend. */
  hourAxis(h: UsageHeatmap) { return h.columns === 24 && this.rowCount > 1; }
  /** The box a dot sits in (its largest size). */
  square(index: number) {
    const columns = Math.max(1, this.columns);
    return { x: this.x + (index % columns) * this.columnPitch, y: this.y + Math.floor(index / columns) * this.rowPitch, size: this.size };
  }
  /** The square nearest a point, so the space between squares belongs to them too. */
  index(px: number, py: number): number | null {
    if (this.columns <= 0 || px < this.x - 1 || px > this.x + this.width + 1 || py < this.y - 1 || py > this.y + this.height + 1) return null;
    const column = this.columns > 1 ? Math.round((px - this.x - this.size / 2) / this.columnPitch) : 0, row = Math.floor((py - this.y) / this.rowPitch);
    return Math.min(this.rowCount - 1, Math.max(0, row)) * this.columns + Math.min(this.columns - 1, Math.max(0, column));
  }
}

/** A dot's radius for its shade: four sizes by rank, a speck when empty (fainter still when yet to come). */
export function dotRadius(size: number, level: number, future: boolean): number {
  if (future) return Math.max(0.75, size * 0.09);
  if (level <= 0) return Math.max(0.9, size * 0.12);
  return (size / 2) * [0.42, 0.6, 0.8, 1][Math.min(4, level) - 1];
}

/** The largest dot in a panel this wide: 10pt at its own width, up to 13 in a pinned panel made wider. */
export const panelDot = (panelWidth: number) => Math.min(13, Math.max(10, Math.round((10 * panelWidth) / 360)));

/** The block's height on the homepage: 8pt above, the squares, the hour axis when there is one, and the caption line. */
export const blockHeight = (layout: HeatmapLayout, h: UsageHeatmap) => 8 + layout.height + (layout.hourAxis(h) ? 11 : 0) + 15;
