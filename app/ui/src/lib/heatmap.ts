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

/** The panel's square: 8pt at its own width, growing with a pinned panel made wider (to 14pt) so the gaps don't. */
export const panelSquare = (panelWidth: number) => Math.min(14, Math.max(HeatmapLayout.cell, Math.round((HeatmapLayout.cell * panelWidth) / 360 * 2) / 2));

/** Every square is the same size in every range; the gap stretches so each row runs the full width, and rows sit that
 *  same gap apart. */
export class HeatmapLayout {
  static readonly cell = 8;
  static readonly rowGap = 1.6;
  constructor(readonly x: number, readonly y: number, readonly width: number, readonly columns: number, readonly rowCount: number,
    readonly size = HeatmapLayout.cell) {}
  get columnPitch() { return this.columns > 1 ? (this.width - this.size) / (this.columns - 1) : 0; }
  get gap() { return this.columns > 1 ? this.columnPitch - this.size : HeatmapLayout.rowGap; }
  get rowPitch() { return this.size + this.gap; }
  get height() { return this.rowCount * this.rowPitch - this.gap; }
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
