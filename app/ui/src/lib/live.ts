// Pinned, the panel is watched while the user works, so the chart zooms in on now: what Summary's tools added in the
// last minute, read every couple of seconds. The feed keeps the readings; LiveChart.svelte draws them.
import { summaryPart } from './format';
import type { Dashboard } from './types';

/** A reading: Summary's tokens (cache reads included, as the line and the cards count them), the output tokens the
 *  models generated, the cost, and each source's tokens and output tokens (a line of its own for each tool). */
export interface Sample { at: number; total: number; output: number; cost: number; parts: Record<string, number>; outputs: Record<string, number> }
type Part = { tokens: number; output: number; cost: number };

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

  // Each source's tokens, output and cost as the line counts them: the raw reading plus a shift that keeps the line
  // continuous. Only a reading comparable with the one before it (the same range, the same sources, none counting
  // less) adds to the line. Anything else is not usage and adds nothing: a range picked, a source arriving late (a new
  // range's figures come in source by source), a gateway taking over requests its client counted until then, a new day.
  // The line then stands where it was, and only its height on the axis lines up again with the trend's newest point.
  private raw = new Map<string, Part>();
  private shifts = new Map<string, Part>();
  private shifted = new Map<string, Part>();

  add(dashboard: Dashboard, at = Date.now()) {
    const raw = new Map<string, Part>();
    for (const p of dashboard.providers) {
      const part = summaryPart(p);
      if (!part || !p.available) continue;
      // Output as Summary counts it: a source a gateway partly logged adds only what was not logged there, as the engine
      // matched it (a share of the card's output by tokens fell when a long prompt with a short answer went through the
      // gateway, and the reading read as not comparable: the request vanished from the line).
      const output = p.counted ? p.counted.output ?? p.output * (p.tokens > 0 ? p.counted.tokens / p.tokens : 0) : p.output;
      raw.set(p.id, { tokens: part.tokens, output, cost: part.cost ?? 0 });
    }
    // A reading with no source read yet (a range just picked, everything still loading) says nothing: it is let go
    // rather than taken as the line standing at zero.
    if (!raw.size) return;
    const newRange = dashboard.rangeKey !== this.rangeKey;
    this.rangeKey = dashboard.rangeKey;
    const comparable = !newRange && this.raw.size > 0 && raw.size === this.raw.size && [...raw].every(([id, now]) => {
      const before = this.raw.get(id);
      return !!before && now.tokens >= before.tokens - 0.5 && now.output >= before.output - 0.5 && now.cost >= before.cost - 1e-9;
    });
    if (!comparable) {
      for (const [id, now] of raw) {
        const stood = this.shifted.get(id) ?? { tokens: 0, output: 0, cost: 0 };
        this.shifts.set(id, { tokens: stood.tokens - now.tokens, output: stood.output - now.output, cost: stood.cost - now.cost });
      }
    }
    // A source missing from this reading keeps where its line stood.
    const shifted = new Map(this.shifted);
    for (const [id, now] of raw) {
      const by = this.shifts.get(id)!;
      shifted.set(id, { tokens: now.tokens + by.tokens, output: now.output + by.output, cost: now.cost + by.cost });
    }
    this.raw = raw;
    this.shifted = shifted;
    const sum = (key: keyof Part) => [...shifted.values()].reduce((s, part) => s + part[key], 0);
    const sample: Sample = { at, total: sum('tokens'), output: sum('output'), cost: sum('cost'),
      parts: Object.fromEntries([...shifted].map(([id, part]) => [id, part.tokens])),
      outputs: Object.fromEntries([...shifted].map(([id, part]) => [id, part.output])) };
    if (!comparable || !this.samples.length) this.offset = LiveFeed.latest(dashboard) - sample.total;
    this.keep(sample, at);
  }

  private keep(sample: Sample, at: number) {
    if (this.samples.length && Math.abs(at - this.samples.at(-1)!.at) < 500) this.samples[this.samples.length - 1] = sample;
    else this.samples.push(sample);
    this.samples = this.samples.filter((s) => at - s.at <= 2 * LiveFeed.window);
  }

  span() { return LiveFeed.window; }

  /** Output tokens a second over the last minute, up to now: what the models generated since the window's left edge
   *  over its 60 seconds, so a burst eases off as the window moves past it. Over 60 seconds in the first minute after
   *  pinning too: a request is logged whole when it finishes, with output written long before, and over the few seconds
   *  since pinning it read as thousands a second for a moment. Not all tokens: each request re-reads its whole context
   *  from the cache, so those run to tens of thousands a second and say nothing about speed. */
  rate(now: number): number | null { return this.rateAt(now); }

  /** The same rate as it stood at an earlier moment, from the readings up to then: the Token/s line is drawn from it;
   *  with a source, that source's own (its line beside the total's). */
  rateAt(at: number, id?: string): number | null {
    const upto = this.samples.filter((s) => s.at <= at), last = upto.at(-1);
    if (!last) return null;
    const start = at - LiveFeed.window;
    const from = [...upto].reverse().find((s) => s.at <= start) ?? upto[0];
    const output = (sample: Sample) => (id === undefined ? sample.output : sample.outputs?.[id] ?? 0);
    return Math.max(0, output(last) - output(from)) / (LiveFeed.window / 1000);
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
