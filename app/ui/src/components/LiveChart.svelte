<script lang="ts">
  // The pinned chart. The line carries on from the trend's newest point and steps up at each reading, flowing left at
  // 30 frames a second; its axis starts where the window starts. Pinning doesn't swap charts: over 0.9 s the trend's
  // scale zooms in on its end (the trend line and fill stretch and fade, the old marks slide away, the new ones open
  // out), the end dot staying put between the two. A new reading lands with a ripple, and 实时 breathes.
  import { accent, alpha, label as labelColour, starAccent, totalColor } from '../lib/colors';
  import { chartAxis, chartPositions, seriesOf, smoothCurves, tracePath } from '../lib/chart';
  import { tokens } from '../lib/format';
  import { baseLabel, compact, easeInOut, scaleFor, zoom, type LiveFeed, type Sample, type Scale } from '../lib/live';
  import { font, MONO, SANS, width } from '../lib/measure';
  import { untrack } from 'svelte';
  import { panel } from '../lib/ui.svelte';
  import type { Dashboard, ProviderUsage } from '../lib/types';

  let { feed, column, counted, data }: { feed: LiveFeed; column: number; counted: ProviderUsage[]; data: Dashboard } = $props();

  const W = 320, H = 122, ZOOM = 900;
  let canvas: HTMLCanvasElement | undefined = $state();
  const born = Date.now();

  /** The trend on screen when the panel was pinned: its scale and marks and its total line, to zoom in from. */
  function trendNow() {
    const axis = chartAxis(data), n = data.dates.length, series = seriesOf(data);
    return { ceiling: axis.ceiling, labels: axis.labels, positions: chartPositions(data), totals: Array.from({ length: n }, (_, i) => series.reduce((s, row) => s + row[i], 0)) };
  }

  // The tools' lines, kept current without restarting the drawing (the figures roll every frame after a reading).
  let order: string[] = [];
  $effect(() => { order = counted.filter((p) => p.available).map((p) => p.id); });

  // The drawing runs for as long as the canvas is there; it reads the feed each frame, never the props reactively.
  $effect(() => {
    if (!canvas) return;
    return untrack(() => start(canvas!));
  });

  function start(canvas: HTMLCanvasElement) {
    const ctx = canvas.getContext('2d')!;
    const scale = window.devicePixelRatio || 1;
    canvas.width = W * scale; canvas.height = H * scale;
    let shown: Scale | null = null, seen: number | null = null, ripple: number | null = null, frame = 0, last = 0;
    let intro: { trend: ReturnType<typeof trendNow>; start: number } | null = null;
    if (!feed.zoomed) {
      feed.zoomed = true;
      if (!panel.reduceMotion) intro = { trend: trendNow(), start: Date.now() };
    }
    const draw = (time: number) => {
      frame = requestAnimationFrame(draw);
      if (time - last < (panel.reduceMotion ? 1000 : 33)) return;
      last = time;
      const now = Date.now(), dark = panel.dark, span = feed.span(now), start = now - span;
      const plot = { x: column, y: 24, w: W - column, h: H - 46 }, bottom = plot.y + plot.h;
      const samples = feed.samples;
      const newest = samples.at(-1)?.at ?? null;
      if (newest !== null && newest !== seen) { if (seen !== null) ripple = now; seen = newest; }
      const base: Sample | undefined = [...samples].reverse().find((s) => s.at <= start) ?? samples[0];
      const points = samples.filter((s) => s.at > start);
      const from = base ? feed.value(base) : 0, end = samples.length ? feed.value(samples.at(-1)!) : from;
      const target = scaleFor(from, Math.max(from, ...points.map((s) => feed.value(s))));
      // Pinning: the trend's scale zooms in on the line's end over a moment, then the scale eases to each new window.
      let progress = 1;
      const trend = intro && base ? intro.trend : null;
      if (intro && base) { progress = easeInOut((now - intro.start) / ZOOM); if (progress >= 1) intro = null; }
      const range: Scale = trend && progress < 1 ? zoom({ lo: 0, height: trend.ceiling }, target, end, progress)
        : shown ? { lo: shown.lo + (target.lo - shown.lo) * 0.12, height: shown.height + (target.height - shown.height) * 0.12 } : target;
      shown = range;
      const y = (value: number) => bottom - ((value - range.lo) / range.height) * plot.h;
      ctx.setTransform(scale, 0, 0, scale, 0, 0);
      ctx.clearRect(0, 0, W, H);
      const secondary = labelColour(dark, dark ? 0.55 : 0.5), accentColour = starAccent(dark), total = totalColor(dark);
      const text = (value: string, x: number, top: number, size: number, colour: string, align: CanvasTextAlign = 'left', weight = 400, family = SANS) => {
        ctx.font = font(size, weight, family); ctx.fillStyle = colour; ctx.textAlign = align; ctx.textBaseline = 'top';
        ctx.fillText(value, x, top + 1);
      };

      // Marks at round values: as the scale zooms in, the trend's slide away and fade (first half) and the live ones
      // open out from the line's end (second half); afterwards they glide to new heights as the scale eases.
      ctx.save();
      ctx.beginPath(); ctx.rect(0, plot.y - 6, W, plot.h + 12); ctx.clip();
      const mark = (label: string, value: number, isBottom: boolean, strength: number, labelStrength = 1) => {
        if (strength <= 0.01) return;
        const at = y(value);
        if (at < plot.y - 12 || at > bottom + 12) return;
        if (labelStrength > 0.01) {
          ctx.globalAlpha = strength * labelStrength;
          text(label, 0, at - 5, 8, secondary);
        }
        ctx.globalAlpha = 1;
        ctx.strokeStyle = labelColour(dark, 0.1 * (isBottom ? 0.3 : 0.14) * strength); ctx.lineWidth = 0.5;
        ctx.beginPath(); ctx.moveTo(plot.x, at); ctx.lineTo(plot.x + plot.w, at); ctx.stroke();
      };
      const leaving = Math.max(0, 1 - progress * 2), opening = Math.min(1, Math.max(0, progress * 2 - 1));
      if (trend && progress < 1) trend.labels.forEach((label, i) => mark(label, trend.ceiling * [0, 0.5, 1][i], i === 0, leaving));
      mark(baseLabel(target.lo, (label) => width(label, font(8)) <= column - 4), target.lo, true, opening);
      // The upper marks' labels wait until they have opened far enough apart to be read.
      const apart = Math.min(1, Math.max(0, (Math.abs(y(target.lo) - y(target.lo + target.height / 2)) - 8) / 6));
      mark('+' + compact(target.height / 2), target.lo + target.height / 2, false, opening, apart);
      mark('+' + compact(target.height), target.lo + target.height, false, opening, apart);
      // The trend's total line zooms with the scale, stretched toward now at the right edge, fading as the live line comes in.
      if (trend && progress < 1 && trend.positions.length >= 2 && trend.positions.length === trend.totals.length) {
        ctx.beginPath(); ctx.rect(plot.x, plot.y - 6, plot.w, plot.h + 12); ctx.clip();
        const stretch = Math.pow(40, progress), positions = trend.positions.map((p) => 1 - (1 - p) * stretch);
        const box = { x: plot.x, y: bottom + (range.lo / range.height) * plot.h - plot.h, w: plot.w, h: plot.h };
        const curve = smoothCurves([trend.totals], positions, box, range.height).at(-1);
        if (curve) {
          const floor = Math.min(bottom, y(0));
          ctx.beginPath(); tracePath(ctx, curve);
          ctx.lineTo(box.x + positions.at(-1)! * box.w, floor); ctx.lineTo(box.x + positions[0] * box.w, floor); ctx.closePath();
          const fill = ctx.createLinearGradient(0, plot.y, 0, bottom);
          fill.addColorStop(0, alpha(total, (dark ? 0.16 : 0.11) * (1 - progress))); fill.addColorStop(1, alpha(total, 0));
          ctx.fillStyle = fill; ctx.fill();
          ctx.beginPath(); tracePath(ctx, curve);
          ctx.lineWidth = 1.8; ctx.lineJoin = 'round'; ctx.lineCap = 'round'; ctx.strokeStyle = alpha(total, 1 - progress); ctx.stroke();
        }
      }
      ctx.restore();

      // Everything live comes in with the zoom, except the dot at the line's end, which stays where the trend's newest
      // point was and carries the eye from one chart to the other.
      ctx.save();
      ctx.globalAlpha = progress;
      const added = base ? (samples.at(-1)?.total ?? base.total) - base.total : 0;
      const minutes = Math.round(span / 60_000);
      let head = `近 ${minutes} 分钟 +${tokens(added)} Token`;
      const rate = feed.rate(now);
      if (rate !== null && rate > 0) head += ` · ${tokens(rate)}/分`;
      text(head, W, 2, 8, secondary, 'right');
      const badgeWidth = width('实时', font(8, 500, MONO)) + 10;
      const pulse = 0.55 + 0.45 * (0.5 + 0.5 * Math.sin(((now - born) / 1600) * 2 * Math.PI));
      ctx.fillStyle = alpha(accentColour, dark ? 0.22 : 0.14);
      ctx.beginPath(); ctx.roundRect(0, 1, badgeWidth + 8, 13, 6.5); ctx.fill();
      ctx.fillStyle = alpha(accentColour, panel.reduceMotion ? 1 : pulse);
      ctx.beginPath(); ctx.arc(7, 7.5, 2, 0, Math.PI * 2); ctx.fill();
      text('实时', 12, 2, 8, accentColour, 'left', 500);
      text(minutes <= 1 ? '1 分钟前' : `${minutes} 分钟前`, plot.x, bottom + 7, 8.5, secondary);
      text('现在', plot.x + plot.w, bottom + 7, 8.5, secondary, 'right');
      if (!base) {
        text('正在读取第一笔数据…', plot.x + plot.w / 2, plot.y + plot.h / 2 - 6, 9, labelColour(dark, dark ? 0.25 : 0.26), 'center');
        ctx.restore();
        return;
      }
      const x = (t: number) => plot.x + ((t - start) / span) * plot.w;
      const clamped = (value: number) => bottom - Math.min(1.08, Math.max(0, (value - range.lo) / range.height)) * plot.h;
      // A line's height from a sample: the total where it stands; a tool's line from where the total stood at the
      // window's left edge, up by what that tool added.
      const level = (id: string | null, sample: Sample) => (id ? feed.value(base) + (sample.parts[id] ?? 0) - (base.parts[id] ?? 0) : feed.value(sample));
      const atStart = [...samples].reverse().find((s) => s.at <= start) ?? base;
      // Steps: a line holds its value until the next reading, then rises to it.
      const path = (id: string | null) => {
        let previous = level(id, atStart);
        ctx.beginPath(); ctx.moveTo(plot.x, clamped(previous));
        for (const s of points) {
          const next = level(id, s);
          if (next !== previous) {
            ctx.lineTo(x(s.at) - 6, clamped(previous));
            ctx.bezierCurveTo(x(s.at) - 3, clamped(previous), x(s.at) - 3, clamped(next), x(s.at), clamped(next));
          }
          previous = next;
        }
        ctx.lineTo(plot.x + plot.w, clamped(previous));
      };
      ctx.save();
      ctx.beginPath(); ctx.rect(plot.x, plot.y - 6, plot.w, plot.h + 12); ctx.clip();
      path(null);
      ctx.lineTo(plot.x + plot.w, bottom); ctx.lineTo(plot.x, bottom); ctx.closePath();
      const fill = ctx.createLinearGradient(0, plot.y, 0, bottom);
      fill.addColorStop(0, alpha(total, dark ? 0.28 : 0.18)); fill.addColorStop(1, alpha(total, 0));
      ctx.fillStyle = fill; ctx.fill();
      for (const id of order) { path(id); ctx.lineWidth = 1.1; ctx.lineJoin = 'round'; ctx.strokeStyle = alpha(accent(id, dark), 0.85); ctx.stroke(); }
      path(null); ctx.lineWidth = 1.8; ctx.lineJoin = 'round'; ctx.lineCap = 'round'; ctx.strokeStyle = total; ctx.stroke();
      ctx.restore();
      ctx.restore();
      // The newest reading lands with a ripple at the line's end.
      const tip = [plot.x + plot.w, clamped(level(null, [...samples].reverse().find((s) => s.at <= now) ?? base))];
      if (ripple !== null && now - ripple < 1100 && !panel.reduceMotion) {
        const t = (now - ripple) / 1100, radius = 3 + 13 * (1 - Math.pow(1 - t, 2));
        ctx.strokeStyle = alpha(total, 0.6 * (1 - t)); ctx.lineWidth = 1.5;
        ctx.beginPath(); ctx.arc(tip[0], tip[1], radius, 0, Math.PI * 2); ctx.stroke();
      }
      ctx.fillStyle = alpha(total, 0.2); ctx.beginPath(); ctx.arc(tip[0], tip[1], 5, 0, Math.PI * 2); ctx.fill();
      ctx.fillStyle = total; ctx.beginPath(); ctx.arc(tip[0], tip[1], 2.5, 0, Math.PI * 2); ctx.fill();
    };
    frame = requestAnimationFrame(draw);
    return () => cancelAnimationFrame(frame);
  }
</script>

<canvas bind:this={canvas} class="live" style:width="{W}px" style:height="{H}px" aria-label="实时用量图"></canvas>

<style>
  .live {
    position: absolute;
    left: 12px;
    top: 104px;
  }
</style>
