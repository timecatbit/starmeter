// Shared live-curve math used by both the pinned panel (LiveChart) and the web stage (StageLiveChart), so both views
// build the exact same time grid, smoothed rate series, linear/log scale decision, Y-axis easing, and tool lines.
import type { Point } from './chart';
import { toolColour } from './colors';
import { nice, type LiveFeed, type Sample } from './live';
import { LINE, type ToolLine } from './liveLines';

/** Integer-second time grid covering `[start, end]`, plus optional `aheadSec` whole seconds past `end` for smooth
 *  between-reading flow on the web stage. */
export function buildTimeGrid(start: number, end: number, aheadSec = 0): number[] {
  const times: number[] = [];
  for (let t = Math.floor(start / 1000) * 1000; t < end; t += 1000) times.push(t);
  times.push(end);
  for (let k = 1; k <= aheadSec; k++) times.push(end + k * 1000);
  return times;
}

export interface RateEvaluation {
  totals: number[];
  toolRates: { id: string; values: number[] }[];
  highest: number;
}

/** Evaluates smoothed total and per-tool output rates across `times`, returning the visible peak (excluding `aheadSec`
 *  lookahead tail points). */
export function evaluateRates(feed: LiveFeed, times: number[], ids: string[], aheadSec = 0): RateEvaluation {
  const totals = feed.smoothRates(times);
  const toolRates = ids.map((id) => ({ id, values: feed.smoothRates(times, id) }));
  const visibleEnd = Math.max(0, totals.length - aheadSec);
  let highest = 0;
  for (let i = 0; i < visibleEnd; i++) {
    if (totals[i] > highest) highest = totals[i];
  }
  return { totals, toolRates, highest };
}

/** Round ceiling over `highest` with `LINE.headroom` breathing room. */
export function targetRateCeiling(highest: number, floor = 4): number {
  return nice(Math.max(floor, highest * LINE.headroom));
}

/** Frame-rate independent exponential easing toward `target`, never dipping under `highest * 1.04`. */
export function easeCeiling(shown: number, target: number, highest: number, dtMs: number, still: boolean, tauMs = 120): number {
  if (still) return target;
  const next = Math.max(highest * 1.04, shown + (target - shown) * (1 - Math.exp(-Math.min(250, Math.max(0, dtMs)) / tauMs)));
  return Math.abs(target - next) < target * 0.002 ? target : next;
}

/** Whether a chart with multiple active tools should switch to a log scale (20x spread to enter, < 10x to leave).
 *  Crucially, only tools that *actually added* tokens/output inside the current window `[base, newest]` are compared:
 *  a previously finished tool's tiny decaying tail from before the window never tricks the chart into log scale. */
export function shouldUseLogScale(
  feed: LiveFeed,
  times: number[],
  ids: string[],
  base: Sample | undefined,
  newest: Sample | undefined,
  rating: boolean,
  wasLog: boolean,
): boolean {
  if (!base || !newest || ids.length < 2) return false;
  const activeIds = ids.filter((id) =>
    rating
      ? (newest.outputs?.[id] ?? 0) - (base.outputs?.[id] ?? 0) > 0.5
      : (newest.parts[id] ?? 0) - (base.parts[id] ?? 0) > 0.5,
  );
  if (activeIds.length < 2) return false;
  const amounts = rating
    ? activeIds.map((id) => Math.max(0, ...feed.rateSeries(times, id).map((r) => r ?? 0))).filter((r) => r > 0)
    : activeIds.map((id) => (newest.parts[id] ?? 0) - (base.parts[id] ?? 0)).filter((a) => a > 0);
  const spread = amounts.length > 1 ? Math.max(...amounts) / Math.min(...amounts) : 1;
  if (!wasLog && spread >= 20) return true;
  if (wasLog && spread < 10) return false;
  return wasLog;
}

/** Maps a value to `[0, 1]` along the linear or log plot axis. */
export function axisShare(value: number, lo: number, height: number, log: boolean, floor: number): number {
  const above = Math.max(0, value - lo);
  return log ? Math.log1p(above / floor) / Math.log1p(height / floor) : above / height;
}

/** Builds the `ToolLine[]` array expected by `drawLines()` (`liveLines.ts`) from per-tool point series and running state. */
export function buildToolLines(
  series: { id: string; points: Point[] }[],
  dark: boolean,
  runningSet: ReadonlySet<string>,
  startedAt: (id: string) => number | undefined,
  xOf: (at: number) => number,
): ToolLine[] {
  return series.map((tool) => {
    const began = startedAt(tool.id);
    return {
      ...tool,
      colour: toolColour(tool.id, dark),
      running: runningSet.has(tool.id),
      startedX: began !== undefined ? xOf(began) : undefined,
    };
  });
}
