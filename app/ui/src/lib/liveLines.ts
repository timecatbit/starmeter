// The live line's look, one for the pinned panel's 实时 and for 网页版 alike: the total the line the eye follows (a
// little heavier, a soft area under it, its end a dot that breathes), each tool a thin line in its colour where it is
// writing, a tool not writing barely there along the bottom; dots only at the ends of lines that are moving; a faint
// dotted grid. The panel draws it quietly (no glow, its labels in the trend's column); 网页版 around it with more going
// on (starlight on the total in the dark, requests dropping onto the end).
import { monotoneCurve, tracePath, type Point } from './chart';
import { alpha } from './colors';

export const LINE = {
  /** The total's line and each tool's. */
  total: 2,
  tool: 1.2,
  /** A tool writing, at this strength of its colour; one not writing in the whole window, along the bottom, at this. */
  toolAlpha: 0.72,
  idleAlpha: 0.16,
  /** The dots at the ends: the total's, and a tool's. */
  dot: 3,
  toolDot: 2,
  /** The total's halo breathing out from its dot and back: its radius and strength each way, over its period. */
  halo: [4.5, 8] as const,
  haloAlpha: [0.32, 0.1] as const,
  haloPeriod: 2400,
  /** The area under the total: its colour at this strength at the top, fading to nothing at the bottom. */
  area: { light: 0.16, dark: 0.26 },
  /** Room over the highest the line reaches in the window, so a rise is never at the edge. */
  headroom: 1.2,
  /** The grid's dotted lines and their strength. */
  dash: [1.5, 3.5],
  grid: 0.1,
  /** Room right of the line's end for its halo (and the ring of a reading landing), inside the chart's own width. */
  end: 10,
};

/** The halo's radius and strength at `now`: breathing (ease in and out, as the CSS one does), or still. */
export function halo(now: number, still: boolean): { radius: number; strength: number } {
  const k = still ? 0.5 : 0.5 - 0.5 * Math.cos(((now % LINE.haloPeriod) / LINE.haloPeriod) * 2 * Math.PI);
  return { radius: LINE.halo[0] + (LINE.halo[1] - LINE.halo[0]) * k, strength: LINE.haloAlpha[0] + (LINE.haloAlpha[1] - LINE.haloAlpha[0]) * k };
}

export type ToolLine = { id: string; points: Point[]; colour: string; running?: boolean; startedX?: number };

export interface Drawn {
  /** The total's end. */
  end: Point;
  /** Each tool's end height; the tools with a dot there; the tools whose end is the total's (no dot of their own). */
  ends: Record<string, number>;
  dots: string[];
  under: string[];
}

/** Writing at a point: clear of the bottom by more than a hair. */
const writing = (base: number) => (p: Point) => base - p[1] > 0.75;

/** The lines on the screen (points in pixels, x rising): the area under the total, each tool's line, the total's over
 *  them, each tool writing its dot at its end. One tool writing alone is the total: its line and its dot would lie on
 *  the total's, so it draws neither (two lines and two dots in one place read as a fault). `again`: 手绘 draws each line
 *  a second time by hand. The total's own dot and halo the caller draws (网页版's breathes on the page, the panel's on
 *  its canvas). */
export function drawLines(ctx: CanvasRenderingContext2D, o: {
  total: Point[]; tools: ToolLine[]; base: number; plotTop: number; totalColour: string; areaColour: string; dark: boolean;
  glow?: string | null; again?: (points: Point[]) => void; now?: number; still?: boolean;
}): Drawn {
  const end = o.total.at(-1) ?? [0, o.base];
  const on = writing(o.base);
  const active = o.tools.filter((tool) => tool.points.some(on));
  const alone = active.length === 1 ? active[0].id : null;
  const curve = (points: Point[]) => { ctx.beginPath(); tracePath(ctx, monotoneCurve(points)); };
  ctx.lineJoin = 'round'; ctx.lineCap = 'round';
  // The area under the total.
  if (o.total.length > 1) {
    curve(o.total);
    ctx.lineTo(end[0], o.base); ctx.lineTo(o.total[0][0], o.base); ctx.closePath();
    const fill = ctx.createLinearGradient(0, o.plotTop, 0, o.base);
    const peak = o.dark ? LINE.area.dark : LINE.area.light;
    fill.addColorStop(0, alpha(o.areaColour, peak));
    fill.addColorStop(0.55, alpha(o.areaColour, peak * 0.36));
    fill.addColorStop(1, alpha(o.areaColour, 0));
    ctx.fillStyle = fill; ctx.fill();
  }
  // Each tool under the total: a tool with a running session stays continuous all the way to 现在 even while its rate
  // rests at 0; a finished tool writes only where it rose off the bottom, and an idle tool stays faint along the bottom.
  for (const tool of o.tools) {
    if (tool.id === alone || tool.points.length < 2) continue;
    ctx.lineWidth = LINE.tool;
    if (tool.running) {
      ctx.strokeStyle = alpha(tool.colour, LINE.toolAlpha);
      curve(tool.points); ctx.stroke(); o.again?.(tool.points);
      continue;
    }
    ctx.strokeStyle = alpha(tool.colour, active.includes(tool) ? LINE.idleAlpha * 0.6 : LINE.idleAlpha);
    curve(tool.points); ctx.stroke();
    if (!active.includes(tool)) continue;
    ctx.strokeStyle = alpha(tool.colour, LINE.toolAlpha);
    for (const run of runs(tool.points, on)) { curve(run); ctx.stroke(); o.again?.(run); }
  }
  // Immediate visual feedback when a session is actively running before its first token chunk flushes to disk: draw a
  // delicate traveling activity ripple along the active wait stretch and a pulsing radar ring at its tip in the tool's
  // own brand colour (honest visual activity without fabricating Token/s numbers).
  if (!o.still && o.now !== undefined) {
    const phase = (o.now / 1000) * 3.8;
    const ring = (o.now % 1400) / 1400;
    for (const tool of o.tools) {
      const last = tool.points.at(-1);
      if (!tool.running || !last || on(last)) continue;
      const minX = tool.points[0]?.[0] ?? last[0];
      const fromX = Math.max(minX, Math.min(last[0] - 8, tool.startedX ?? (last[0] - 28)));
      const spanX = last[0] - fromX;
      if (spanX >= 6) {
        const steps = Math.max(8, Math.min(36, Math.round(spanX / 3)));
        const ripple: Point[] = [];
        for (let i = 0; i <= steps; i++) {
          const u = i / steps;
          const px = fromX + u * spanX;
          const env = Math.sin(u * Math.PI);
          const wave = 1.6 + 1.5 * Math.sin(u * Math.PI * 3 - phase);
          ripple.push([px, Math.max(o.plotTop + 1, o.base - Math.max(0.4, wave * env))]);
        }
        ctx.save();
        ctx.lineWidth = LINE.tool + 0.4;
        ctx.strokeStyle = alpha(tool.colour, 0.78);
        curve(ripple); ctx.stroke();
        ctx.restore();
      }
      ctx.save();
      ctx.strokeStyle = alpha(tool.colour, (1 - ring) * 0.65);
      ctx.lineWidth = 1.5;
      ctx.beginPath();
      ctx.arc(last[0], last[1], 3 + ring * 7.5, 0, Math.PI * 2);
      ctx.stroke();
      ctx.restore();
    }
  }
  // The total's line over them. Use a fast translucent halo pass instead of ctx.shadowBlur (which forces expensive
  // Gaussian blur over the whole Bezier path on every frame in CoreGraphics/Skia, PERF-2).
  if (o.total.length > 1) {
    if (o.glow) {
      // Two widening strokes, fainter outward: soft at its edge as the blur was, not a band.
      ctx.save();
      ctx.strokeStyle = o.glow;
      for (const [wider, strength] of [[8, 0.12], [4, 0.2]] as const) {
        ctx.lineWidth = LINE.total + wider;
        ctx.globalAlpha = strength;
        curve(o.total); ctx.stroke();
      }
      ctx.restore();
    }
    ctx.lineWidth = LINE.total; ctx.strokeStyle = o.totalColour;
    curve(o.total); ctx.stroke();
    o.again?.(o.total);
  }
  // A dot at the end of each tool still writing (or running) at the end, one a little off the total's; none on top of
  // it or of another's.
  const ends: Record<string, number> = {}, dots: string[] = [], under: string[] = [];
  for (const tool of o.tools) {
    const last = tool.points.at(-1);
    if (!last) continue;
    ends[tool.id] = last[1];
    const liveEnd = on(last) || !!tool.running;
    if (tool.id === alone || (liveEnd && Math.abs(last[1] - end[1]) < 6)) { under.push(tool.id); continue; }
    if (!liveEnd || dots.some((id) => Math.abs(ends[id] - last[1]) < 4.5)) continue;
    dots.push(tool.id);
    ctx.fillStyle = tool.colour;
    ctx.beginPath(); ctx.arc(last[0], last[1], LINE.toolDot, 0, Math.PI * 2); ctx.fill();
  }
  return { end, ends, dots, under };
}

/** The stretches of a line where it writes, each with a point either side on the bottom, so it rises off it and back. */
function runs(points: Point[], on: (p: Point) => boolean): Point[][] {
  const out: Point[][] = [];
  let run: Point[] | null = null;
  points.forEach((p, i) => {
    const near = on(p) || (i > 0 && on(points[i - 1])) || (i + 1 < points.length && on(points[i + 1]));
    if (near) (run ??= []).push(p);
    else if (run) { out.push(run); run = null; }
  });
  if (run) out.push(run);
  return out.filter((r) => r.length > 1);
}

/** The faint dotted grid line at `y`, from `left` to `right`. */
export function gridLine(ctx: CanvasRenderingContext2D, y: number, left: number, right: number, ink: string) {
  ctx.save();
  ctx.setLineDash(LINE.dash); ctx.lineWidth = 1; ctx.strokeStyle = alpha(ink, LINE.grid);
  ctx.beginPath(); ctx.moveTo(left, Math.round(y) + 0.5); ctx.lineTo(right, Math.round(y) + 0.5); ctx.stroke();
  ctx.restore();
}
