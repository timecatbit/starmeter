// Pinned, the panel is watched while the user works, so the chart zooms in on now: what Summary's tools added in the
// last minutes, read every couple of seconds. The feed keeps the readings; LiveChart.svelte draws them.
import { summaryPart } from './format';
import type { Dashboard } from './types';

export interface Sample { at: number; total: number; cost: number; parts: Record<string, number> }

export class LiveFeed {
  static readonly longest = 600_000;
  static readonly shortest = 60_000;
  samples: Sample[] = [];
  started = Date.now();
  /** The pinning zoom plays once per pin, not each time the homepage comes back. */
  zoomed = false;
  private rangeKey: string | null = null;
  /** The line carries on from the trend instead of starting at zero: it begins at the trend's newest point (this hour
   *  so far for today, today so far for a range of days) and rises by what Summary adds. Fixed when a line begins, so a
   *  later hour doesn't drop the line back down. */
  offset = 0;

  /** The trend's newest point: the last bucket of the counted sources, as the chart draws it. */
  static latest(dashboard: Dashboard): number {
    const n = dashboard.dates.length, parts = dashboard.providers.map(summaryPart).filter((p) => p !== null);
    if (!n) return parts.filter((p) => p!.available).reduce((s, p) => s + p!.tokens, 0);
    return parts.reduce((s, p) => s + Math.max(0, p!.history[n - 1] ?? 0), 0);
  }

  value(sample: Sample) { return sample.total + this.offset; }

  add(dashboard: Dashboard, at = Date.now()) {
    if (dashboard.rangeKey !== this.rangeKey) { this.samples = []; this.rangeKey = dashboard.rangeKey; }
    const parts = dashboard.providers.map(summaryPart).filter((p) => p !== null && p.available);
    const sample: Sample = { at, total: parts.reduce((s, p) => s + p!.tokens, 0), cost: parts.reduce((s, p) => s + (p!.cost ?? 0), 0), parts: {} };
    for (const part of parts) sample.parts[part!.id] = part!.tokens;
    // A new day (or a source dropping out) starts the line again rather than drawing a fall.
    const last = this.samples.at(-1);
    if (last && sample.total < last.total - 1) this.samples = [];
    if (!this.samples.length) this.offset = LiveFeed.latest(dashboard) - sample.total;
    if (this.samples.length && Math.abs(at - this.samples.at(-1)!.at) < 500) this.samples[this.samples.length - 1] = sample;
    else this.samples.push(sample);
    this.samples = this.samples.filter((s) => at - s.at <= LiveFeed.longest + 60_000);
  }

  /** The window on screen: from the pin's first minute, widening to the last ten. */
  span(now: number) { return Math.min(LiveFeed.longest, Math.max(LiveFeed.shortest, now - this.started)); }

  /** Tokens per minute over the last minute of samples. */
  rate(now: number): number | null {
    const last = this.samples.at(-1);
    const first = [...this.samples].reverse().find((s) => now - s.at >= 60_000) ?? this.samples[0];
    if (!last || !first || last.at <= first.at) return null;
    return ((last.total - first.total) / (last.at - first.at)) * 60_000;
  }
}

/** A round top for the scale, close above the highest value so the line fills the chart. */
export function nice(value: number): number {
  const power = Math.pow(10, Math.floor(Math.log10(Math.max(1, value))));
  return [1, 1.2, 1.5, 2, 2.5, 3, 4, 5, 6, 8, 10].map((f) => f * power).find((v) => v >= value) ?? value;
}

export function compact(value: number): string {
  const [number, unit] = value >= 1e9 ? [value / 1e9, 'B'] : value >= 1e6 ? [value / 1e6, 'M'] : value >= 1e3 ? [value / 1e3, 'K'] : [value, ''];
  return (Math.round(number) === number ? String(number) : number.toFixed(1)) + unit;
}

export type Scale = { lo: number; height: number };

/** The scale for a window: from where the line stood at its left edge, a round height over what it added since, never
 *  below a small floor so an idle line lies flat along the bottom. */
export const scaleFor = (base: number, top: number): Scale => ({ lo: base, height: nice(Math.max(2000, (top - base) * 1.04)) });

/** Partway through the zoom between scales: the height shrinks by the same proportion each moment (a steady zoom), and
 *  the line's end moves smoothly from where it stood on the first scale to where it stands on the second. */
export function zoom(a: Scale, b: Scale, anchor: number, e: number): Scale {
  const height = a.height * Math.pow(b.height / a.height, e), from = (anchor - a.lo) / a.height, to = (anchor - b.lo) / b.height;
  return { lo: anchor - (from + (to - from) * e) * height, height };
}

export const easeInOut = (t: number) => { t = Math.min(1, Math.max(0, t)); return t < 0.5 ? 4 * t * t * t : 1 - Math.pow(-2 * t + 2, 3) / 2; };

/** The bottom mark says where the window starts, as closely as the label column allows (124.2M → 124M). */
export function baseLabel(value: number, fits: (label: string) => boolean): string {
  const [unit, suffix] = value >= 1e9 ? [1e9, 'B'] : value >= 1e6 ? [1e6, 'M'] : value >= 1e3 ? [1e3, 'K'] : [1, ''];
  let label = '';
  for (const digits of unit === 1 ? [0] : [2, 1, 0]) {
    label = (value / unit).toFixed(digits);
    if (label.includes('.')) label = label.replace(/0+$/, '').replace(/\.$/, '');
    label += suffix;
    if (fits(label)) break;
  }
  return label;
}
