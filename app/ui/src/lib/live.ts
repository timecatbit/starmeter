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
  private rangeKey: string | null = null;

  add(dashboard: Dashboard, at = Date.now()) {
    if (dashboard.rangeKey !== this.rangeKey) { this.samples = []; this.rangeKey = dashboard.rangeKey; }
    const parts = dashboard.providers.map(summaryPart).filter((p) => p !== null && p.available);
    const sample: Sample = { at, total: parts.reduce((s, p) => s + p!.tokens, 0), cost: parts.reduce((s, p) => s + (p!.cost ?? 0), 0), parts: {} };
    for (const part of parts) sample.parts[part!.id] = part!.tokens;
    // A new day (or a source dropping out) starts the line again rather than drawing a fall.
    const last = this.samples.at(-1);
    if (last && sample.total < last.total - 1) this.samples = [];
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
