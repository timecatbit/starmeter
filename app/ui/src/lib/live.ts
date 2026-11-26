// Pinned, the panel is watched while the user works, so the chart zooms in on now: what Summary's tools added in the
// last minute, read every couple of seconds. The feed keeps the readings; LiveChart.svelte draws them.
import { summaryPart } from './format';
import type { Dashboard, LiveReading } from './types';

/** Two readings further apart than this are either side of a gap (a page out of sight, the computer out of reach):
 *  what was added between them happened at no moment the line knows. */
export const GAP = 15_000;
/** How long the rate takes to come round to a change (an exponential moving average's time constant): the lines and
 *  the Token/s figures alike, so a dot and its figure agree. */
export const EASE = 15_000;

/** `values` at `times` (rising) eased twice over (two exponential moving averages in a row, each over `tau`): a step
 *  becomes an S, flat where it starts and where it ends. Eased once, a step still began with a corner (the user: 图的曲线
 *  有时候还是有直角). Nulls are none yet, and say 0 until the first value. */
export function easeTwice(times: number[], values: (number | null)[], tau: number): number[] {
  const out: number[] = [];
  let first: number | null = null, second: number | null = null, previous = times[0] ?? 0;
  for (let i = 0; i < times.length; i++) {
    const v = values[i];
    if (v !== null && v !== undefined) {
      const k = 1 - Math.exp(-Math.max(0, times[i] - previous) / tau);
      first = first === null ? v : first + (v - first) * k;
      second = second === null ? first : second + (first - second) * k;
      previous = times[i];
    }
    out.push(second ?? 0);
  }
  return out;
}

/** A reading: Summary's tokens (cache reads included, as the line and the cards count them), the output tokens the
 *  models generated, the cost, and each source's tokens and output tokens (a line of its own for each tool). `cut`: a
 *  reading not comparable with the one before (a gap, a new range): the line stood still there, and what comes after
 *  is never spread back over it. */
export interface Sample { at: number; total: number; output: number; cost: number; parts: Record<string, number>; outputs: Record<string, number>; cut?: boolean }

/** One of the computer's last minutes (a kept reading) as a dashboard: the tools of `template` (the latest figures, for
 *  their names and shares), each with that moment's own figures, and a gateway's part only where that reading had one
 *  (STAGE-12: a gateway that came or went within the minutes took the latest reading's part into every older one). */
export function readingDashboard(template: Dashboard, reading: LiveReading): Dashboard {
  const kept = new Map(reading.providers.map((p) => [p.id, p]));
  return { ...template, date: reading.date, rangeKey: reading.rangeKey, providers: template.providers.map((p) => {
    const then = kept.get(p.id);
    if (!then) return { ...p, available: false };
    const counted = then.counted ? { requests: 0, history: [], ...p.counted, ...then.counted, cost: then.counted.cost ?? null } : undefined;
    return { ...p, available: then.available ?? p.available, summaryIncluded: then.summaryIncluded ?? p.summaryIncluded,
      tokens: then.tokens ?? 0, output: then.output ?? 0, cost: then.cost ?? null, counted } as Dashboard['providers'][number];
  }) } as Dashboard;
}
type Part = { tokens: number; output: number; cost: number };

export class LiveFeed {
  /** The window on screen: always the last minute, so the line flows at one steady speed and the rate reads true. */
  static readonly window = 60_000;
  samples: Sample[] = [];
  /** How far back the readings are kept: two windows for the panel's minute; a phone's live view draws five minutes. */
  history = 2 * LiveFeed.window;
  started = Date.now();
  /** How far back a change is spread (0, the panel's own: not at all). Figures that change only now and then (SwiftBar's
   *  plugin runs every half minute) would draw the speed as steps, flat between readings and jumping at each: a change
   *  is spread evenly over the quiet stretch before it, up to this long, as the work was (a request's output is written
   *  over its time, and logged when it ends). */
  spread = 0;
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

  /** `fresh`: a reading after a long gap (a phone's page out of sight, the computer out of reach): what was added while
   *  nobody looked happened at no moment the line knows, so it adds nothing, as a new range doesn't. */
  add(dashboard: Dashboard, at = Date.now(), fresh = false) {
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
    const comparable = !fresh && !newRange && this.raw.size > 0 && raw.size === this.raw.size && [...raw].every(([id, now]) => {
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
    if (!comparable && this.samples.length) sample.cut = true;
    this.keep(sample, at);
  }

  /** The computer's last minutes, before the readings to come: each as a dashboard of `template`'s tools (on this
   *  clock: the computer's seconds less `skew`), a reading more than `gap` after the one before it taken as after a gap
   *  (what was added in it happened at no moment the line knows). Returns when the last one was, on this clock. */
  replay(template: Dashboard, history: LiveReading[], skew: number, gap: number): number {
    let last = this.samples.at(-1)?.at ?? 0;
    for (const reading of history) {
      const at = reading.at * 1000 - skew;
      if (at <= last + 1) continue;
      this.add(readingDashboard(template, reading), at, last > 0 && at - last > gap);
      last = at;
    }
    return last;
  }

  private keep(sample: Sample, at: number) {
    // Within half a second of the last, it takes the last's place; a cut there stays one (STAGE-18: the next rise
    // otherwise spread back over the gap or the new range).
    if (this.samples.length && Math.abs(at - this.samples.at(-1)!.at) < 500) {
      if (this.samples.at(-1)!.cut) sample.cut = true;
      this.samples[this.samples.length - 1] = sample;
    }
    else { this.spreadOut(sample); this.samples.push(sample); }
    this.samples = this.samples.filter((s) => at - s.at <= this.history);
  }

  /** The readings since the line last moved (within `spread`), raised evenly towards this one, which moved it; never
   *  back over a gap or a new range (a cut: the line stood still there, STAGE-13). */
  private spreadOut(sample: Sample) {
    const list = this.samples, last = list.at(-1);
    if (!this.spread || !last || sample.cut || same(last, sample) || sample.at <= last.at) return;
    let from = list.length - 1;
    while (from > 0 && !list[from].cut && same(list[from - 1], last) && sample.at - list[from - 1].at <= this.spread) from--;
    const anchor = list[from], length = sample.at - anchor.at;
    for (let i = from + 1; i < list.length; i++) list[i] = between(anchor, sample, (list[i].at - anchor.at) / length, list[i].at);
  }

  span() { return LiveFeed.window; }

  /** Output tokens a second over the last minute, up to now: what the models generated since the window's left edge
   *  over its 60 seconds, so a burst eases off as the window moves past it. Over 60 seconds in the first minute after
   *  pinning too: a request is logged whole when it finishes, with output written long before, and over the few seconds
   *  since pinning it read as thousands a second for a moment. Not all tokens: each request re-reads its whole context
   *  from the cache, so those run to tens of thousands a second and say nothing about speed. Eased as the lines are
   *  (smoothRates), so the figure is the height of the line's end. */
  rate(now: number): number | null { return this.samples.length ? this.smoothRates([now])[0] : null; }

  /** The last minute's rate as it stood at an earlier moment, from the readings up to then, before it is eased; with a
   *  source, that source's own. */
  rateAt(at: number, id?: string): number | null {
    const upto = this.samples.filter((s) => s.at <= at), last = upto.at(-1);
    if (!last) return null;
    const start = at - LiveFeed.window;
    const from = [...upto].reverse().find((s) => s.at <= start) ?? upto[0];
    const output = (sample: Sample) => (id === undefined ? sample.output : sample.outputs?.[id] ?? 0);
    return Math.max(0, output(last) - output(from)) / (LiveFeed.window / 1000);
  }

  /** The rate as the lines draw it and the figures say it, at each of `times` (rising): the last minute's (rateAt),
   *  eased twice over half of EASE (easeTwice, begun EASE × 3 earlier so it has settled by the first). A request
   *  landing rises and settles as an S, and leaves the minute the same way, never as a step or a corner: drawn as a
   *  step function, the curve through it was straight plateaus and sharp corners; eased once, a corner where it rose. */
  smoothRates(times: number[], id?: string): number[] {
    if (!times.length) return [];
    const warm: number[] = [];
    for (let at = times[0] - EASE * 3; at < times[0]; at += 1000) warm.push(at);
    const all = [...warm, ...times];
    return easeTwice(all, this.rateSeries(all, id), EASE / 2).slice(warm.length);
  }

  /** rateAt at each of `times` (rising) in one pass over the readings; null before the first. */
  private rateSeries(times: number[], id?: string): (number | null)[] {
    const list = this.samples, output = (s: Sample) => (id === undefined ? s.output : s.outputs?.[id] ?? 0);
    const out: (number | null)[] = [];
    let last = -1, from = 0;
    for (const at of times) {
      while (last + 1 < list.length && list[last + 1].at <= at) last++;
      if (last < 0) { out.push(null); continue; }
      while (from + 1 <= last && list[from + 1].at <= at - LiveFeed.window) from++;
      const first = list[from].at <= at - LiveFeed.window ? list[from] : list[0];
      out.push(Math.max(0, output(list[last]) - output(first)) / (LiveFeed.window / 1000));
    }
    return out;
  }
}

/** Two readings the same: nothing added between them. */
function same(a: Sample, b: Sample): boolean {
  return Math.abs(a.total - b.total) < 0.5 && Math.abs(a.output - b.output) < 0.5 && Math.abs(a.cost - b.cost) < 1e-9;
}

/** The reading `f` of the way from `a` to `b`, at `at`. */
function between(a: Sample, b: Sample, f: number, at: number): Sample {
  const mix = (x: number, y: number) => x + (y - x) * f;
  const each = (from: Record<string, number>, to: Record<string, number>) =>
    Object.fromEntries([...new Set([...Object.keys(from), ...Object.keys(to)])].map((id) => [id, mix(from[id] ?? 0, to[id] ?? 0)]));
  return { at, total: mix(a.total, b.total), output: mix(a.output, b.output), cost: mix(a.cost, b.cost), parts: each(a.parts, b.parts), outputs: each(a.outputs ?? {}, b.outputs ?? {}) };
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

/** The scale for a window: from `base` (0, the window's left edge), a round height over what was added since, never
 *  below a small floor so an idle line lies flat along the bottom. */
export const scaleFor = (base: number, top: number): Scale => ({ lo: base, height: nice(Math.max(2000, (top - base) * 1.04)) });

/** Partway through the zoom between scales: the height shrinks by the same proportion each moment (a steady zoom), and
 *  the line's end moves smoothly from where it stood on the first scale to where it stands on the second. */
export function zoom(a: Scale, b: Scale, anchor: number, e: number): Scale {
  const height = a.height * Math.pow(b.height / a.height, e), from = (anchor - a.lo) / a.height, to = (anchor - b.lo) / b.height;
  return { lo: anchor - (from + (to - from) * e) * height, height };
}

export const easeInOut = (t: number) => { t = Math.min(1, Math.max(0, t)); return t < 0.5 ? 4 * t * t * t : 1 - Math.pow(-2 * t + 2, 3) / 2; };
