<script lang="ts">
  // The pinned chart: what Summary's tools added in the last minute, read every couple of seconds. The line steps up at
  // each reading from 0 at the window's left edge (what was added since, not the running total: the user asked for the
  // change itself), flowing left at 30 frames a second over a fixed one-minute axis. Pinning zooms into now: the trend stretches away from its end and
  // fades, its end dot settles where the live line ends, and the live line opens out leftward from that dot while the
  // live marks come in. Unpinning plays the same in reverse and hands back to the trend. A new reading lands with a
  // ripple, 实时 breathes, and the last minute's rate reads at the top right in Token/s. Beside 实时, Token / Token/s
  // turns the line into that rate over the last minute (remembered): how fast the models are writing, rising and easing.
  import { accent, alpha, label as labelColour, starAccent, toolColour, summaryColour } from '../lib/colors';
  import { t } from '../lib/i18n.svelte';
  import { chartAxis, chartPositions, seriesOf, smoothCurves, summaryProviders, tracePath } from '../lib/chart';
  import { tokens } from '../lib/format';
  import { compact, nice, rateText, scaleFor, type LiveFeed, type Sample, type Scale } from '../lib/live';
  import { font, ROUNDED, SANS, width } from '../lib/measure';
  import { untrack } from 'svelte';
  import { panel } from '../lib/ui.svelte';
  import type { Dashboard, ProviderUsage } from '../lib/types';

  let { feed, column, counted, data, note = '', leaving = null, onleft, width: wide = 320 }: {
    feed: LiveFeed; column: number; counted: ProviderUsage[]; data: Dashboard; note?: string; leaving?: number | null; onleft?: () => void;
    width?: number;
  } = $props();

  /** The canvas sits where the trend's plot is, so the two line up exactly while one turns into the other. */
  const H = 122, ZOOM = 750, STRETCH = 16;
  /** Room past the line's end, so its dot and the ripple of a new reading (at most 10pt round, inside the panel's
   *  margin) aren't cut off. */
  const PAD = 16;
  // Read by the drawing each frame: a pinned panel's edges change it.
  let W = 320;
  $effect(() => { W = wide; });
  let canvas: HTMLCanvasElement | undefined = $state();
  const born = Date.now();
  const MODE = 'starmeter.liveMode';
  let mode = $state<'total' | 'rate'>(typeof localStorage !== 'undefined' && localStorage.getItem(MODE) === 'rate' ? 'rate' : 'total');
  // Read by the drawing each frame.
  let modeNow: 'total' | 'rate' = 'total';
  $effect(() => { modeNow = mode; });
  function pickMode(next: 'total' | 'rate') {
    mode = next;
    if (typeof localStorage !== 'undefined') localStorage.setItem(MODE, next);
  }

  type Trend = ReturnType<typeof trendNow>;
  /** The trend as SummaryChart draws it, to zoom in from or back out to. */
  function trendNow() {
    const axis = chartAxis(data), n = data.dates.length, series = seriesOf(data), positions = chartPositions(data);
    const totals = Array.from({ length: n }, (_, i) => series.reduce((s, row) => s + row[i], 0));
    let middle: number | null = null;
    for (let i = 1; i < n - 1; i++) {
      if (positions[i] >= 0.25 && positions[i] <= 0.75 && (middle === null || Math.abs(positions[i] - 0.5) < Math.abs(positions[middle] - 0.5))) middle = i;
    }
    return {
      ceiling: axis.ceiling, labels: axis.labels, positions, series, totals, ids: summaryProviders(data).map((p) => p.id),
      dates: data.dates.slice(), dateIndices: n > 1 ? [0, ...(middle !== null ? [middle] : []), n - 1] : [], note,
    };
  }

  // The tools' lines, kept current without restarting the drawing (the figures roll every frame after a reading).
  let order: string[] = [];
  $effect(() => { order = counted.filter((p) => p.available).map((p) => p.id); });
  // Read by the drawing each frame.
  let leavingAt: number | null = null;
  $effect(() => { leavingAt = leaving; });

  // The drawing runs for as long as the canvas is there; it reads the feed each frame, never the props reactively.
  $effect(() => {
    if (!canvas) return;
    return untrack(() => start(canvas!));
  });

  const clamp = (t: number) => Math.min(1, Math.max(0, t));
  /** 0 before a, 1 after b, straight in between (the zoom's own easing shapes it). */
  const ramp = (a: number, b: number, x: number) => clamp((x - a) / (b - a));
  /** Quick to start, gentle to land: the chart answers the click at once. */
  const easeOut = (t: number) => 1 - Math.pow(1 - clamp(t), 3);

  function start(canvas: HTMLCanvasElement) {
    const ctx = canvas.getContext('2d')!;
    const scale = window.devicePixelRatio || 1;
    canvas.height = H * scale;
    let shown: Scale | null = null, seen: number | null = null, ripple: number | null = null, frame = 0, last = 0, left = false;
    // A log scale while the tools' lines are far apart in size, so a small one doesn't lie along the bottom.
    let logNow = false;
    let drawnMode = modeNow, drawnOffset = feed.offset;
    // progress runs from 0 (the trend) to 1 (live): up when pinning, back down when unpinning.
    let zoom: { from: number; to: number; start: number; trend: Trend } | null = null;
    let progress = 1;
    if (!feed.zoomed) {
      feed.zoomed = true;
      if (!panel.reduceMotion) { zoom = { from: 0, to: 1, start: Date.now(), trend: trendNow() }; progress = 0; }
    }
    const draw = (time: number) => {
      frame = requestAnimationFrame(draw);
      if (time - last < (panel.reduceMotion ? 1000 : 33)) return;
      last = time;
      if (canvas.width !== Math.round((W + PAD) * scale)) canvas.width = Math.round((W + PAD) * scale);
      const now = Date.now(), dark = panel.dark;
      // Unpinning: zoom back out from wherever the chart stands, then hand back to the trend.
      if (leavingAt !== null && !left && (!zoom || zoom.to !== 0)) {
        if (panel.reduceMotion) { left = true; onleft?.(); return; }
        zoom = { from: progress, to: 0, start: leavingAt, trend: zoom?.trend ?? trendNow() };
      }
      if (zoom) {
        const t = clamp((now - zoom.start) / (ZOOM * Math.max(0.35, Math.abs(zoom.to - zoom.from))));
        progress = zoom.from + (zoom.to - zoom.from) * easeOut(t);
        if (t >= 1) {
          if (zoom.to === 0) { if (!left) { left = true; onleft?.(); } return; }
          zoom = null; progress = 1;
        }
      }
      const p = progress, trend = zoom?.trend ?? null;
      const span = feed.span(), start = now - span;
      const plot = { x: column, y: 23, w: W - column, h: 78 }, bottom = plot.y + plot.h;
      const samples = feed.samples;
      const newest = samples.at(-1)?.at ?? null;
      if (newest !== null && newest !== seen) { if (seen !== null) ripple = now; seen = newest; }
      const base: Sample | undefined = [...samples].reverse().find((s) => s.at <= start) ?? samples[0];
      const points = samples.filter((s) => s.at > start);
      const from = base ? feed.value(base) : 0;
      // Token/s: the last minute's output rate as it stood at each whole second, so a point once drawn keeps its value
      // as the window flows past it (sampling moving instants made the drawn line jump); 0 until there is a rate.
      const rating = modeNow === 'rate';
      // A new mode, or a new range moving the line's height on the axis: the scale goes straight there, not sliding.
      if (drawnMode !== modeNow || drawnOffset !== feed.offset) { drawnMode = modeNow; drawnOffset = feed.offset; shown = null; }
      const rates: [number, number][] = [];
      const times: number[] = [];
      if (rating) {
        for (let t = Math.floor(start / 1000) * 1000; t <= now; t += 1000) times.push(t);
        times.push(now);
        for (const t of times) rates.push([t, feed.rateAt(t) ?? 0]);
      }
      // Each tool's own rate, a line of its own under the total's: every tool on the homepage, one not writing anything
      // lying along zero (a tool used now and then is still there to see, and to see idle).
      const toolRates = rating
        ? order.map((id) => ({ id, points: times.map((t): [number, number] => [t, feed.rateAt(t, id) ?? 0]) }))
        : [];
      // The tools far apart (the most twenty times the least of those moving): a log scale, every line clear of the
      // bottom; back to a straight one under ten times, the gap between the two keeping it from flickering.
      const newestSample = points.at(-1);
      const amounts = rating
        ? toolRates.map((tool) => Math.max(...tool.points.map(([, r]) => r))).filter((r) => r > 0)
        : base && newestSample ? order.map((id) => (newestSample.parts[id] ?? 0) - (base.parts[id] ?? 0)).filter((a) => a > 0) : [];
      const spread = amounts.length > 1 ? Math.max(...amounts) / Math.min(...amounts) : 1;
      if (!logNow && spread >= 20) { logNow = true; shown = null; }
      else if (logNow && spread < 10) { logNow = false; shown = null; }
      canvas.dataset.scale = logNow ? 'log' : 'linear';
      const target = rating ? { lo: 0, height: nice(Math.max(5, ...rates.map(([, r]) => r * 1.15))) }
        : scaleFor(0, Math.max(0, ...points.map((s) => feed.value(s) - from)));
      // While zooming the live scale stands still; afterwards it eases to each new window.
      const range: Scale = zoom || !shown ? target
        : { lo: shown.lo + (target.lo - shown.lo) * 0.12, height: shown.height + (target.height - shown.height) * 0.12 };
      shown = range;
      // Where a value stands on the axis, 0 at the bottom and 1 at the top: straight, or by its logarithm from a floor
      // a thousandth (Token/s) or a ten-thousandth (Token) of the top, so four decades or three fill the height.
      const floor = rating ? Math.max(0.1, range.height / 1000) : Math.max(1, range.height / 10_000);
      const share = (value: number) => {
        const above = Math.max(0, value - range.lo);
        return logNow ? Math.log1p(above / floor) / Math.log1p(range.height / floor) : above / range.height;
      };
      const y = (value: number) => bottom - share(value) * plot.h;
      const secondary = labelColour(dark, dark ? 0.55 : 0.5), tertiary = labelColour(dark, dark ? 0.25 : 0.26);
      const accentColour = starAccent(dark), total = summaryColour(dark);
      ctx.setTransform(scale, 0, 0, scale, 0, 0);
      ctx.clearRect(0, 0, W + PAD, H);
      // The page's own fonts (手绘 writes in its hand, the default in the system's): the labels in its text font, the
      // rate in the rounded figures every other number on the panel uses.
      const sans = getComputedStyle(canvas).fontFamily || SANS;
      const rounded = getComputedStyle(canvas).getPropertyValue('--rounded').trim() || ROUNDED;
      const text = (value: string, x: number, top: number, size: number, colour: string, align: CanvasTextAlign = 'left', weight = 400, family = sans) => {
        ctx.font = font(size, weight, family); ctx.fillStyle = colour; ctx.textAlign = align; ctx.textBaseline = 'top';
        ctx.fillText(value, x, top + 1);
      };
      const grid = (at: number, isBottom: boolean, strength: number) => {
        ctx.strokeStyle = labelColour(dark, 0.1 * (isBottom ? 0.3 : 0.14) * strength); ctx.lineWidth = 0.5;
        ctx.beginPath(); ctx.moveTo(plot.x, at); ctx.lineTo(plot.x + plot.w, at); ctx.stroke();
      };

      // The trend, stretched away from its end (its last hour's point stays at the right edge) and fading.
      const trendAlpha = trend ? 1 - ramp(0.05, 0.6, p) : 0;
      const stretch = Math.pow(STRETCH, p);
      const at = (position: number) => plot.x + (1 - (1 - position) * stretch) * plot.w;
      let trendEnd: [number, number] | null = null;
      if (trend && trend.positions.length) {
        const n = trend.positions.length;
        const trendY = (value: number) => bottom - (value / trend.ceiling) * plot.h;
        trendEnd = [at(trend.positions[n - 1]), trendY(trend.totals[n - 1])];
        if (trendAlpha > 0.01) {
          ctx.save();
          ctx.globalAlpha = trendAlpha;
          trend.labels.forEach((label, i) => {
            const level = bottom - [0, 0.5, 1][i] * plot.h;
            text(label, 0, level - 5, 8, secondary);
            grid(level, i === 0, 1);
          });
          if (trend.note) text(trend.note, W, 3, 7.5, tertiary, 'right');
          ctx.save();
          ctx.beginPath(); ctx.rect(plot.x - 1, plot.y - 10, plot.w + 12, plot.h + 20); ctx.clip();
          if (n > 1) {
            const positions = trend.positions.map((position) => 1 - (1 - position) * stretch);
            const curves = smoothCurves(trend.series, positions, plot, trend.ceiling);
            const totalCurve = curves.at(-1)!;
            ctx.beginPath(); tracePath(ctx, totalCurve);
            ctx.lineTo(plot.x + positions[n - 1] * plot.w, bottom); ctx.lineTo(plot.x + positions[0] * plot.w, bottom); ctx.closePath();
            const fill = ctx.createLinearGradient(0, plot.y, 0, bottom);
            fill.addColorStop(0, alpha(total, dark ? 0.16 : 0.11)); fill.addColorStop(1, alpha(total, 0));
            ctx.fillStyle = fill; ctx.fill();
            curves.slice(0, -1).forEach((curve, i) => {
              ctx.beginPath(); tracePath(ctx, curve);
              ctx.lineWidth = 1.2; ctx.lineJoin = 'round'; ctx.lineCap = 'round'; ctx.strokeStyle = alpha(accent(trend.ids[i] ?? '', dark), 0.8); ctx.stroke();
            });
            ctx.beginPath(); tracePath(ctx, totalCurve);
            ctx.lineWidth = 1.8; ctx.lineJoin = 'round'; ctx.lineCap = 'round'; ctx.strokeStyle = total; ctx.stroke();
          }
          ctx.restore();
          // The dates ride with the stretch: the first ones slide off to the left, the last stays at the right edge.
          ctx.save();
          ctx.beginPath(); ctx.rect(plot.x - 2, 0, plot.w + 2, H); ctx.clip();
          for (const i of trend.dateIndices) {
            const x = at(trend.positions[i]), last = i === n - 1;
            text(trend.dates[i], last ? W : x, plot.y + plot.h + 7, 8.5, secondary, last ? 'right' : i === 0 ? 'left' : 'center');
          }
          ctx.restore();
          ctx.restore();
        }
      }

      // The live chart comes in as the trend goes.
      const liveAlpha = trend ? ramp(0.4, 1, p) : 1;
      const reach = trend ? ramp(0.3, 1, p) : 1;
      if (liveAlpha > 0.01) {
        ctx.save();
        ctx.globalAlpha = liveAlpha;
        // Marks at round values: the bottom says where the window starts, the two above what it has added.
        const mark = (label: string, value: number, isBottom: boolean, labelStrength = 1) => {
          const level = y(value);
          if (level < plot.y - 12 || level > bottom + 12) return;
          if (labelStrength > 0.01) { ctx.globalAlpha = liveAlpha * labelStrength; text(label, 0, level - 5, 8, secondary); ctx.globalAlpha = liveAlpha; }
          grid(level, isBottom, 1);
        };
        const apart = Math.min(1, Math.max(0, (Math.abs(y(range.lo) - y(range.lo + range.height / 2)) - 8) / 6));
        if (logNow) {
          // A mark at each power of ten, labelled as the straight scale's are; 对数 says the steps aren't even.
          mark('0', range.lo, true);
          for (let power = Math.ceil(Math.log10(floor * 10)); 10 ** power <= range.height * 1.0001; power++) {
            mark((rating ? '' : '+') + compact(10 ** power), range.lo + 10 ** power, false);
          }
          text(t('对数'), plot.x + 3, plot.y - 1, 7.5, tertiary);
        } else if (rating) {
          mark('0', 0, true);
          mark(compact(target.height / 2), range.height / 2, false, apart);
          mark(compact(target.height), range.height, false, apart);
        } else {
          mark('0', range.lo, true);
          mark('+' + compact(target.height / 2), range.lo + range.height / 2, false, apart);
          mark('+' + compact(target.height), range.lo + range.height, false, apart);
        }
        // 实时, breathing, at the top left; the last minute's additions and its rate at the top right.
        const badgeWidth = width(t('实时'), font(8, 500, sans)) + 10;
        const pulse = 0.55 + 0.45 * (0.5 + 0.5 * Math.sin(((now - born) / 1600) * 2 * Math.PI));
        ctx.fillStyle = alpha(accentColour, dark ? 0.22 : 0.14);
        ctx.beginPath(); ctx.roundRect(0, 1, badgeWidth + 8, 13, 6.5); ctx.fill();
        ctx.fillStyle = alpha(accentColour, panel.reduceMotion ? 1 : pulse);
        ctx.beginPath(); ctx.arc(7, 7.5, 2, 0, Math.PI * 2); ctx.fill();
        text(t('实时'), 12, 2, 8, accentColour, 'left', 500);
        ctx.globalAlpha = liveAlpha;
        const added = base ? Math.max(0, (samples.at(-1)?.total ?? base.total) - base.total) : 0;
        const rate = feed.rate(now);
        // What the top right says, for whatever reads the page rather than the pixels (tests among them).
        canvas.dataset.added = String(Math.round(added));
        canvas.dataset.rate = String(Math.round(rate ?? 0));
        const unit = ' Token/s', figure = rateText(rate ?? 0);
        const lead = t('近 1 分钟 +{n} Token · 输出 ', { n: tokens(added) });
        const baseline = 11;
        ctx.textBaseline = 'alphabetic';
        ctx.font = font(8, 400, sans); ctx.fillStyle = secondary; ctx.textAlign = 'right';
        ctx.fillText(unit, W, baseline);
        const unitWidth = width(unit, font(8, 400, sans));
        ctx.font = font(11.5, 600, rounded); ctx.fillStyle = total;
        ctx.fillText(figure, W - unitWidth, baseline);
        const figureWidth = width(figure, font(11.5, 600, rounded));
        ctx.font = font(8, 400, sans); ctx.fillStyle = secondary;
        ctx.fillText(lead, W - unitWidth - figureWidth - 1, baseline);
        text(t('1 分钟前'), plot.x, bottom + 7, 8.5, secondary);
        text(t('现在'), plot.x + plot.w, bottom + 7, 8.5, secondary, 'right');
        if (!base && !rating) text(t('正在读取第一笔数据…'), plot.x + plot.w / 2, plot.y + plot.h / 2 - 6, 9, labelColour(dark, dark ? 0.25 : 0.26), 'center');
        ctx.restore();
      }
      if (!base && !rating) return;

      const x = (t: number) => plot.x + ((t - start) / span) * plot.w;
      const clamped = (value: number) => bottom - Math.min(1.08, share(value)) * plot.h;
      // (Only drawn with a first reading: Token/s alone goes on without one, along zero.)
      const added = (id: string, sample: Sample) => (sample.parts[id] ?? 0) - (base!.parts[id] ?? 0);
      // Each tool a line of its own, as on the trend: from where the window starts, up by what that tool added since, the
      // total's line over them all. Every tool on the homepage, one that added nothing lying along the bottom; the
      // biggest drawn first, so a small one isn't hidden under it.
      const drawn = base ? [...order] : [];
      const newestPoint = points.at(-1);
      if (newestPoint) drawn.sort((a, b) => added(b, newestPoint) - added(a, newestPoint));
      // What the total and each tool added since the window's left edge: the axis runs from 0.
      const level = (id: string | null, sample: Sample) => (id === null ? feed.value(sample) - from : added(id, sample));
      // Which lines are drawn, and each tool's rate now, for whatever reads the page rather than the pixels (tests).
      canvas.dataset.lines = (rating ? toolRates.map((tool) => tool.id) : drawn).join(',');
      canvas.dataset.toolRates = JSON.stringify(Object.fromEntries(toolRates.map((tool) => [tool.id, Math.round(tool.points.at(-1)![1] * 10) / 10])));
      type Step = { kind: 'line'; to: [number, number] } | { kind: 'curve'; c1: [number, number]; c2: [number, number]; to: [number, number] };
      // Steps: a line holds its value until the next reading, then rises to it.
      const steps = (valueOf: (sample: Sample) => number): { from: [number, number]; steps: Step[] } => {
        let previous = valueOf(base!);
        const from: [number, number] = [plot.x, clamped(previous)];
        const out: Step[] = [];
        for (const s of points) {
          const next = valueOf(s);
          if (next !== previous) {
            out.push({ kind: 'line', to: [x(s.at) - 6, clamped(previous)] });
            out.push({ kind: 'curve', c1: [x(s.at) - 3, clamped(previous)], c2: [x(s.at) - 3, clamped(next)], to: [x(s.at), clamped(next)] });
          }
          previous = next;
        }
        out.push({ kind: 'line', to: [plot.x + plot.w, clamped(previous)] });
        return { from, steps: out };
      };
      const forward = (line: ReturnType<typeof steps>) => {
        for (const step of line.steps) {
          if (step.kind === 'line') ctx.lineTo(...step.to);
          else ctx.bezierCurveTo(...step.c1, ...step.c2, ...step.to);
        }
      };
      // A tool's line ends in a small dot of its colour, so one lying along the bottom is still seen and told apart: dots
      // that would fall on one another (the idle tools, all at the bottom) step leftward, side by side.
      const ends: Record<string, [number, number]> = {};
      // The total's own end dot, larger, is kept clear too.
      const totalEnd = rating ? clamped(rates.at(-1)?.[1] ?? 0) : clamped(level(null, [...samples].reverse().find((s) => s.at <= now) ?? base!));
      // A tool ending where the total does (the one adding nearly all of it) draws no dot of its own: the total's dot is
      // that point, and one stepped aside read as a dot trailing behind it.
      const under: string[] = [];
      const toolEnd = (id: string, at: number) => {
        if (Math.abs(at - totalEnd) < 6) { under.push(id); return; }
        let end = plot.x + plot.w;
        const taken = (x: number) => Object.values(ends).some(([ex, ey]) => Math.abs(ex - x) < 4.5 && Math.abs(ey - at) < 4.5);
        while (taken(end)) end -= 5;
        ends[id] = [end, at];
        ctx.fillStyle = toolColour(id, dark); ctx.beginPath(); ctx.arc(end, at, 2, 0, Math.PI * 2); ctx.fill();
      };
      const path = (id: string | null) => {
        const line = steps((s) => level(id, s));
        ctx.beginPath(); ctx.moveTo(...line.from); forward(line);
      };
      const rateTip = rates.at(-1);
      if (rating && reach > 0.001) {
        ctx.save();
        ctx.globalAlpha = trend ? ramp(0.3, 0.7, p) : 1;
        const opened = plot.w * reach;
        ctx.beginPath(); ctx.rect(plot.x + plot.w - opened, plot.y - 6, opened, plot.h + 12); ctx.clip();
        if (rates.length > 1) {
          const trace = () => { ctx.beginPath(); rates.forEach(([t, r], i) => (i ? ctx.lineTo(x(t), clamped(r)) : ctx.moveTo(x(t), clamped(r)))); };
          const idle = (tool: (typeof toolRates)[number]) => tool.points.every(([, r]) => r <= 0);
          const toolLine = (tool: (typeof toolRates)[number]) => {
            ctx.beginPath(); tool.points.forEach(([t, r], i) => (i ? ctx.lineTo(x(t), clamped(r)) : ctx.moveTo(x(t), clamped(r))));
            ctx.lineWidth = 1.4; ctx.lineJoin = 'round'; ctx.lineCap = 'round'; ctx.strokeStyle = toolColour(tool.id, dark); ctx.stroke();
            toolEnd(tool.id, clamped(tool.points.at(-1)![1]));
          };
          // The idle tools under the total's line, which stays on top along zero; the ones writing over it.
          toolRates.filter(idle).forEach(toolLine);
          trace();
          ctx.lineTo(x(rates.at(-1)![0]), bottom); ctx.lineTo(x(rates[0][0]), bottom); ctx.closePath();
          const fill = ctx.createLinearGradient(0, plot.y, 0, bottom);
          fill.addColorStop(0, alpha(total, dark ? 0.28 : 0.18)); fill.addColorStop(1, alpha(total, 0));
          ctx.fillStyle = fill; ctx.fill();
          trace(); ctx.lineWidth = 1.8; ctx.lineJoin = 'round'; ctx.lineCap = 'round'; ctx.strokeStyle = total; ctx.stroke();
          toolRates.filter((tool) => !idle(tool)).forEach(toolLine);
        }
        ctx.restore();
      } else if (reach > 0.001) {
        // The live line opens out leftward from the end dot as the zoom finishes.
        ctx.save();
        ctx.globalAlpha = trend ? ramp(0.3, 0.7, p) : 1;
        const opened = plot.w * reach;
        ctx.beginPath(); ctx.rect(plot.x + plot.w - opened, plot.y - 6, opened, plot.h + 12); ctx.clip();
        const latest = [...samples].reverse().find((s) => s.at <= now) ?? base!;
        const toolLine = (id: string) => {
          path(id);
          ctx.lineWidth = 1.4; ctx.lineJoin = 'round'; ctx.lineCap = 'round'; ctx.strokeStyle = toolColour(id, dark); ctx.stroke();
          toolEnd(id, clamped(level(id, latest)));
        };
        // The tools that added nothing lie under the total's line, which stays on top along the bottom.
        const idle = (id: string) => points.every((s) => added(id, s) <= 0);
        drawn.filter(idle).forEach(toolLine);
        path(null);
        ctx.lineTo(plot.x + plot.w, bottom); ctx.lineTo(plot.x, bottom); ctx.closePath();
        const fill = ctx.createLinearGradient(0, plot.y, 0, bottom);
        fill.addColorStop(0, alpha(total, dark ? 0.28 : 0.18)); fill.addColorStop(1, alpha(total, 0));
        ctx.fillStyle = fill; ctx.fill();
        path(null); ctx.lineWidth = 1.8; ctx.lineJoin = 'round'; ctx.lineCap = 'round'; ctx.strokeStyle = total; ctx.stroke();
        // The tools adding over the total's line, so the one adding most shows in its colour where it runs along it.
        drawn.filter((id) => !idle(id)).forEach(toolLine);
        ctx.restore();
      }
      canvas.dataset.under = under.join(',');
      canvas.dataset.ends = JSON.stringify(Object.fromEntries(Object.entries(ends).map(([id, [ex, ey]]) => [id, [Math.round(ex * 10) / 10, Math.round(ey * 10) / 10]])));
      // The end dot carries the eye between the charts: from the trend's newest point to the live line's end.
      const liveTip: [number, number] = rating
        ? [plot.x + plot.w, clamped(rateTip ? rateTip[1] : 0)]
        : [plot.x + plot.w, clamped(level(null, [...samples].reverse().find((s) => s.at <= now) ?? base!))];
      const move = trendEnd ? ramp(0.1, 0.8, p) : 1;
      const tip: [number, number] = trendEnd
        ? [trendEnd[0] + (liveTip[0] - trendEnd[0]) * move, trendEnd[1] + (liveTip[1] - trendEnd[1]) * move]
        : liveTip;
      if (!zoom && ripple !== null && now - ripple < 1100 && !panel.reduceMotion) {
        const t = (now - ripple) / 1100, radius = 3 + 7 * (1 - Math.pow(1 - t, 2));
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

<canvas bind:this={canvas} class="live" style:width="{wide + PAD}px" style:height="{H}px" aria-label={t('实时用量图')}></canvas>
{#if leaving === null}
  <div class="modes" role="radiogroup" aria-label={t('实时图显示')}>
    <button class="mode pressable" class:chosen={mode === 'total'} role="radio" aria-checked={mode === 'total'} onclick={() => pickMode('total')}>{t('Token')}</button>
    <button class="mode pressable" class:chosen={mode === 'rate'} role="radio" aria-checked={mode === 'rate'} onclick={() => pickMode('rate')}>Token/s</button>
  </div>
{/if}

<style>
  .live {
    position: absolute;
    left: 12px;
    top: 104px;
  }
  /* Beside 实时: the line's two readings. The chart's layer lets clicks through; these take them. */
  .modes {
    position: absolute;
    left: 56px;
    top: 104px;
    height: 15px;
    display: flex;
    gap: 2px;
    pointer-events: auto;
  }
  .mode {
    --press-scale: 0.94;
    height: 15px;
    padding: 0 6px;
    border-radius: 7.5px;
    font-size: 8.5px;
    color: var(--secondary);
  }
  .mode:hover { background: var(--hover); }
  .mode.chosen { color: var(--label); font-weight: 600; background: color-mix(in srgb, var(--label) 9%, transparent); }
</style>
