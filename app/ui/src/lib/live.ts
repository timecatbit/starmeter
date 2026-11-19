// Pinned, the panel is watched while the user works, so the chart zooms in on now: what Summary's tools added in the
// last minute, read every couple of seconds. The feed keeps the readings; LiveChart.svelte draws them.
import { summaryPart } from './format';
import type { Dashboard } from './types';

/** A reading: Summary's tokens (cache reads included, as the line and the cards count them), the output tokens the
 *  models generated, the cost and each source's tokens. */
export interface Sample { at: number; total: number; output: number; cost: number; parts: Record<string, number> }

export class LiveFeed {
  /** The window on screen: always the last minute, so the line flows at one steady speed and the rate reads true. */
  static readonly window = 60_000;
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

  /** What a reading of the range on screen adds to the readings before it: another range totals differently, so its
   *  readings are shifted to carry on from where the last one stood. */
  private shift: Omit<Sample, 'at'> = { total: 0, output: 0, cost: 0, parts: {} };

  add(dashboard: Dashboard, at = Date.now()) {
    const parts = dashboard.providers.map(summaryPart).filter((p) => p !== null && p.available);
    // Output as Summary counts it: a source a gateway partly logged adds only its share not logged there.
    const output = dashboard.providers.filter((p) => p.available && summaryPart(p) !== null)
      .reduce((s, p) => s + p.output * (p.counted ? (p.tokens > 0 ? p.counted.tokens / p.tokens : 0) : 1), 0);
    const raw: Sample = { at, total: parts.reduce((s, p) => s + p!.tokens, 0), output, cost: parts.reduce((s, p) => s + (p!.cost ?? 0), 0), parts: {} };
    for (const part of parts) raw.parts[part!.id] = part!.tokens;
    const last = this.samples.at(-1);
    if (dashboard.rangeKey !== this.rangeKey) {
      // A new range while pinned: the line and the rate carry on (what is added keeps adding up, nothing drops to
      // zero); only its height on the axis moves to the new range's newest point.
      this.rangeKey = dashboard.rangeKey;
      const ids = new Set([...Object.keys(raw.parts), ...Object.keys(last?.parts ?? {})]);
      this.shift = last
        ? { total: last.total - raw.total, output: last.output - raw.output, cost: last.cost - raw.cost,
          parts: Object.fromEntries([...ids].map((id) => [id, (last.parts[id] ?? 0) - (raw.parts[id] ?? 0)])) }
        : { total: 0, output: 0, cost: 0, parts: {} };
      const sample = this.shifted(raw);
      this.offset = LiveFeed.latest(dashboard) - sample.total;
      this.keep(sample, at);
      return;
    }
    let sample = this.shifted(raw);
    // A new day (or a source dropping out) starts the line again rather than drawing a fall.
    if (last && sample.total < last.total - 1) { this.samples = []; this.shift = { total: 0, output: 0, cost: 0, parts: {} }; sample = raw; }
    if (!this.samples.length) this.offset = LiveFeed.latest(dashboard) - sample.total;
    this.keep(sample, at);
  }

  private shifted(raw: Sample): Sample {
    const parts: Record<string, number> = {};
    for (const id of new Set([...Object.keys(raw.parts), ...Object.keys(this.shift.parts)])) parts[id] = (raw.parts[id] ?? 0) + (this.shift.parts[id] ?? 0);
    return { at: raw.at, total: raw.total + this.shift.total, output: raw.output + this.shift.output, cost: raw.cost + this.shift.cost, parts };
  }

  private keep(sample: Sample, at: number) {
    if (this.samples.length && Math.abs(at - this.samples.at(-1)!.at) < 500) this.samples[this.samples.length - 1] = sample;
    else this.samples.push(sample);
    this.samples = this.samples.filter((s) => at - s.at <= 2 * LiveFeed.window);
  }

  span() { return LiveFeed.window; }

  /** Output tokens a second over the last minute, up to now: what the models generated since the window's left edge (or
   *  since pinning, in the first minute) over the time gone by, so a burst eases off as the window moves past it. Not
   *  all tokens: each request re-reads its whole context from the cache, so those run to tens of thousands a second and
   *  say nothing about speed. Requests are logged whole when they finish, so a per-reading rate would mostly be 0 with an
   *  occasional spike. */
  rate(now: number): number | null { return this.rateAt(now); }

  /** The same rate as it stood at an earlier moment, from the readings up to then: the Token/s line is drawn from it. */
  rateAt(at: number): number | null {
    const upto = this.samples.filter((s) => s.at <= at), last = upto.at(-1);
    if (!last) return null;
    const start = at - LiveFeed.window;
    const base = [...upto].reverse().find((s) => s.at <= start);
    const from = base ?? upto[0];
    const seconds = base ? LiveFeed.window / 1000 : (at - from.at) / 1000;
    if (seconds < 1) return null;
    return Math.max(0, last.output - from.output) / seconds;
  }
}

/** A rate as the header writes it: 1.2K, 340, 12, 3.4, 0. */
export function rateText(value: number): string {
  if (value >= 1000) return compact(Math.round(value / 100) * 100);
  if (value >= 10) return String(Math.round(value));
  if (value > 0) return value.toFixed(1).replace(/\.0$/, '');
  return '0';
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
