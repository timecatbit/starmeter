// Pinned, the panel is watched while the user works, so the chart zooms in on now: what Summary's tools added in the
// last minute, read every couple of seconds. The feed keeps the readings; LiveChart.svelte draws them.
import { summaryPart } from './format';
import type { Dashboard, LiveReading } from './types';

/** Two readings further apart than this are either side of a gap (a page out of sight, the computer out of reach):
 *  what was added between them happened at no moment the line knows. */
export const GAP = 15_000;
/** Exponential decay time constant for instantaneous output Token/s: each output burst peaks immediately on landing
 *  and decays naturally towards 0 when the session goes idle instead of holding a flat 60s plateau; sustained output
 *  converges to its exact true Token/s because ∫(1/τ)e^(-u/τ)du = 1. */
export const RATE_TAU = 10_000;
export const EASE = 15_000;
const ATTACK_TAU = 400;
const RELEASE_TAU = 650;
const RATE_FLOOR = 0.05;

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

/** Instant-onset live rate smoother (PERF-10): fast attack on rising rates (65% at t=0, rounded crest by t=1–2s) +
 *  responsive release that follows the exponential decay smoothly, followed by two zero-phase interior smoothing passes
 *  so the trailing curve has no sharp corners (用户：曲线有时还有直角): requests landing one after another would
 *  otherwise draw a staircase. The passes leave the end alone, so the curve's end is still the rate the figure says. */
function smoothLive(times: number[], values: (number | null)[]): number[] {
  const n = times.length;
  const out = new Array<number>(n);
  let cur: number | null = null, previous = times[0] ?? 0;
  for (let i = 0; i < n; i++) {
    const v = values[i];
    if (v !== null && v !== undefined) {
      if (cur === null) {
        cur = v;
      } else {
        const dt = Math.max(0, times[i] - previous);
        if (dt > 0) {
          const tau: number = v >= cur ? ATTACK_TAU : RELEASE_TAU;
          const k: number = 1 - Math.exp(-dt / tau);
          cur = cur + (v - cur) * k;
        }
      }
      previous = times[i];
    }
    out[i] = cur !== null && cur >= RATE_FLOOR ? cur : 0;
  }
  if (n >= 3) {
    let src = out;
    let dst = new Array<number>(n);
    for (let pass = 0; pass < 2; pass++) {
      dst[0] = src[0];
      dst[n - 1] = src[n - 1];
      for (let i = 1; i < n - 1; i++) dst[i] = 0.25 * src[i - 1] + 0.5 * src[i] + 0.25 * src[i + 1];
      [src, dst] = [dst, src];
    }
    for (let i = 0; i < n; i++) if (src[i] < RATE_FLOOR) src[i] = 0;
    return src;
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
  /** How far back a change is spread (0 by default: no retroactive smearing of past samples). */
  spread = 0;
  /** Exponential decay time constant (ms) for Token/s: 10s for live 1.5s reads, 30s for SwiftBar's 30s plugin cadence. */
  rateTau = RATE_TAU;
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
  /** Explicit session-stop timestamps per tool id ('' for total): once a tracked session stops, raw rate drops to 0
   *  so the curve glides cleanly to 0 in ~1.5s instead of decaying for 40–120s. */
  private stops = new Map<string, number[]>();

  markStopped(at: number, id?: string) {
    const key = id ?? '';
    const list = this.stops.get(key) ?? [];
    if (list.length && at <= list[list.length - 1]) return;
    list.push(at);
    this.stops.set(key, list.filter((t) => at - t <= this.history));
  }

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

  /** Instantaneous output tokens a second (exponentially decayed with time constant `this.rateTau`): peaks immediately
   *  when new output lands and glides smoothly back to 0 once idle. Evaluated on the same integer-second grid as the
   *  chart's `smoothRates(times)` so the headline figure and the curve's end match at every millisecond. */
  rate(now: number): number | null {
    if (!this.samples.length) return null;
    const grid = Math.floor(now / 1000) * 1000;
    return this.smoothRates(grid < now ? [grid, now] : [now]).at(-1) ?? 0;
  }

  /** Unsmoothed exponentially-decayed output rate at a moment `at`; with a source, that source's own. */
  rateAt(at: number, id?: string): number | null {
    return this.rateSeries([at], id)[0] ?? null;
  }

  /** The rate as the lines draw it and the figures say it, at each of `times` (rising): warmed up on integer Unix
   *  seconds so `rate(now)` and `smoothRates(times).at(-1)` always evaluate the exact same causal sequence. */
  smoothRates(times: number[], id?: string): number[] {
    if (!times.length) return [];
    const warm: number[] = [];
    const warmSpan = Math.ceil((Math.max(ATTACK_TAU, RELEASE_TAU) * 8) / 1000) * 1000;
    const anchor = Math.floor(times[0] / 1000) * 1000;
    for (let at = anchor - warmSpan; at < times[0]; at += 1000) warm.push(at);
    const all = [...warm, ...times];
    return smoothLive(all, this.rateSeries(all, id)).slice(warm.length);
  }

  /** Exponentially-decayed output rate at each of `times` (rising) in one pass over the readings; null before the first. */
  rateSeries(times: number[], id?: string): (number | null)[] {
    const list = this.samples, output = (s: Sample) => (id === undefined ? s.output : s.outputs?.[id] ?? 0);
    const stopList = this.stops.get(id ?? '') ?? [];
    const out: (number | null)[] = [];
    const tauMs = this.rateTau, tauSec = tauMs / 1000;
    let idx = 0, stopIdx = 0, seen = false, state = 0, stateAt = 0, prevOut = 0;
    for (const at of times) {
      while (
        (idx < list.length && list[idx].at <= at)
        || (stopIdx < stopList.length && stopList[stopIdx] <= at)
      ) {
        const nextSample = idx < list.length && list[idx].at <= at ? list[idx].at : Infinity;
        const nextStop = stopIdx < stopList.length && stopList[stopIdx] <= at ? stopList[stopIdx] : Infinity;
        if (nextSample <= nextStop) {
          const s = list[idx++];
          const current = output(s);
          if (!seen) {
            seen = true;
            state = 0;
            stateAt = s.at;
            prevOut = current;
          } else {
            const dt = Math.max(0, s.at - stateAt);
            if (dt > 0 && state > 0) state *= Math.exp(-dt / tauMs);
            const delta = Math.max(0, current - prevOut);
            if (delta > 0) state += delta / tauSec;
            stateAt = s.at;
            prevOut = current;
          }
        } else {
          const tStop = stopList[stopIdx++];
          if (seen && tStop >= stateAt) {
            state = 0;
            stateAt = tStop;
          }
        }
      }
      if (!seen) { out.push(null); continue; }
      const dt = Math.max(0, at - stateAt);
      const value = dt > 0 && state > 0 ? state * Math.exp(-dt / tauMs) : state;
      out.push(value < RATE_FLOOR ? 0 : value);
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
