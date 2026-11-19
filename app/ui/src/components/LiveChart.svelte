<script lang="ts">
  // The pinned chart: the tokens Summary's tools added in the window, stepping up at each reading and flowing left at
  // 30 frames a second; the scale glides to the window's growth, a new reading lands with a ripple, and 实时 breathes.
  import { accent, alpha, label as labelColour, starAccent, totalColor } from '../lib/colors';
  import { tokens } from '../lib/format';
  import { compact, nice, type LiveFeed, type Sample } from '../lib/live';
  import { font, MONO, SANS, width } from '../lib/measure';
  import { panel } from '../lib/ui.svelte';
  import type { ProviderUsage } from '../lib/types';

  let { feed, column, counted }: { feed: LiveFeed; column: number; counted: ProviderUsage[] } = $props();

  const W = 320, H = 122;
  let canvas: HTMLCanvasElement | undefined = $state();
  const born = Date.now();

  $effect(() => {
    if (!canvas) return;
    const ctx = canvas.getContext('2d')!;
    const scale = window.devicePixelRatio || 1;
    canvas.width = W * scale; canvas.height = H * scale;
    let ceiling = 0, seen: number | null = null, ripple: number | null = null, frame = 0, last = 0;
    const order = counted.filter((p) => p.available).map((p) => p.id);
    const draw = (time: number) => {
      frame = requestAnimationFrame(draw);
      if (time - last < (panel.reduceMotion ? 1000 : 33)) return;
      last = time;
      const now = Date.now(), dark = panel.dark, span = feed.span(now), start = now - span;
      const plot = { x: column, y: 24, w: W - column, h: H - 46 };
      const samples = feed.samples;
      const newest = samples.at(-1)?.at ?? null;
      if (newest !== null && newest !== seen) { if (seen !== null) ripple = now; seen = newest; }
      const base: Sample | undefined = [...samples].reverse().find((s) => s.at <= start) ?? samples[0];
      const points = samples.filter((s) => s.at > start);
      const target = nice(Math.max(2000, Math.max(0, ...points.map((s) => s.total - (base?.total ?? 0))) * 1.04));
      ceiling = ceiling === 0 ? target : ceiling + (target - ceiling) * 0.12;
      ctx.setTransform(scale, 0, 0, scale, 0, 0);
      ctx.clearRect(0, 0, W, H);
      const text = (value: string, x: number, y: number, size: number, colour: string, align: CanvasTextAlign = 'left', weight = 400, family = SANS) => {
        ctx.font = font(size, weight, family); ctx.fillStyle = colour; ctx.textAlign = align; ctx.textBaseline = 'top';
        ctx.fillText(value, x, y + 1);
      };
      const secondary = labelColour(dark, dark ? 0.55 : 0.5), accentColour = starAccent(dark), total = totalColor(dark);
      const added = base ? (samples.at(-1)?.total ?? base.total) - base.total : 0;
      const minutes = Math.round(span / 60_000);
      let head = `近 ${minutes} 分钟 +${tokens(added)} Token`;
      const rate = feed.rate(now);
      if (rate !== null && rate > 0) head += ` · ${tokens(rate)}/分`;
      text(head, W, 2, 8, secondary, 'right');
      // 实时, breathing.
      const badgeWidth = width('实时', font(8, 500, MONO)) + 10;
      const pulse = 0.55 + 0.45 * (0.5 + 0.5 * Math.sin(((now - born) / 1600) * 2 * Math.PI));
      ctx.fillStyle = alpha(accentColour, dark ? 0.22 : 0.14);
      ctx.beginPath(); ctx.roundRect(0, 1, badgeWidth + 8, 13, 6.5); ctx.fill();
      ctx.fillStyle = alpha(accentColour, panel.reduceMotion ? 1 : pulse);
      ctx.beginPath(); ctx.arc(7, 7.5, 2, 0, Math.PI * 2); ctx.fill();
      text('实时', 12, 2, 8, accentColour, 'left', 500);
      // Round marks at round values, gliding to new heights as the scale eases.
      for (const [value, label] of [[0, '0'], [target / 2, compact(target / 2)], [target, compact(target)]] as [number, string][]) {
        const f = Math.min(1.1, value / ceiling), y = plot.y + plot.h - f * plot.h;
        text(label, 0, y - 5, 8, secondary);
        ctx.strokeStyle = labelColour(dark, 0.1 * (f === 0 ? 0.3 : 0.14)); ctx.lineWidth = 0.5;
        ctx.beginPath(); ctx.moveTo(plot.x, y); ctx.lineTo(plot.x + plot.w, y); ctx.stroke();
      }
      text(minutes <= 1 ? '1 分钟前' : `${minutes} 分钟前`, plot.x, plot.y + plot.h + 7, 8.5, secondary);
      text('现在', plot.x + plot.w, plot.y + plot.h + 7, 8.5, secondary, 'right');
      if (!base) { text('正在读取第一笔数据…', plot.x + plot.w / 2, plot.y + plot.h / 2 - 6, 9, labelColour(dark, dark ? 0.25 : 0.26), 'center'); return; }
      const x = (t: number) => plot.x + ((t - start) / span) * plot.w;
      const y = (v: number) => plot.y + plot.h - Math.min(1.08, Math.max(0, v / ceiling)) * plot.h;
      const valueAt = (id: string | null, t: number) => {
        const sample = [...samples].reverse().find((s) => s.at <= t) ?? base;
        return id ? (sample.parts[id] ?? 0) - (base.parts[id] ?? 0) : sample.total - base.total;
      };
      // Steps: a line holds its value until the next reading, then rises to it.
      const path = (id: string | null) => {
        let previous = valueAt(id, start);
        ctx.beginPath(); ctx.moveTo(plot.x, y(previous));
        for (const s of points) {
          const next = id ? (s.parts[id] ?? 0) - (base.parts[id] ?? 0) : s.total - base.total;
          if (next !== previous) {
            ctx.lineTo(x(s.at) - 6, y(previous));
            ctx.bezierCurveTo(x(s.at) - 3, y(previous), x(s.at) - 3, y(next), x(s.at), y(next));
          }
          previous = next;
        }
        ctx.lineTo(plot.x + plot.w, y(previous));
      };
      ctx.save();
      ctx.beginPath(); ctx.rect(plot.x, plot.y - 6, plot.w, plot.h + 12); ctx.clip();
      path(null);
      ctx.lineTo(plot.x + plot.w, plot.y + plot.h); ctx.lineTo(plot.x, plot.y + plot.h); ctx.closePath();
      const fill = ctx.createLinearGradient(0, plot.y, 0, plot.y + plot.h);
      fill.addColorStop(0, alpha(total, dark ? 0.28 : 0.18)); fill.addColorStop(1, alpha(total, 0));
      ctx.fillStyle = fill; ctx.fill();
      for (const id of order) {
        path(id); ctx.lineWidth = 1.1; ctx.lineJoin = 'round'; ctx.strokeStyle = alpha(accent(id, dark), 0.85); ctx.stroke();
      }
      path(null); ctx.lineWidth = 1.8; ctx.lineJoin = 'round'; ctx.lineCap = 'round'; ctx.strokeStyle = total; ctx.stroke();
      ctx.restore();
      const end = [plot.x + plot.w, y(valueAt(null, now))];
      if (ripple !== null && now - ripple < 1100 && !panel.reduceMotion) {
        const t = (now - ripple) / 1100, radius = 3 + 13 * (1 - Math.pow(1 - t, 2));
        ctx.strokeStyle = alpha(total, 0.6 * (1 - t)); ctx.lineWidth = 1.5;
        ctx.beginPath(); ctx.arc(end[0], end[1], radius, 0, Math.PI * 2); ctx.stroke();
      }
      ctx.fillStyle = alpha(total, 0.2); ctx.beginPath(); ctx.arc(end[0], end[1], 5, 0, Math.PI * 2); ctx.fill();
      ctx.fillStyle = total; ctx.beginPath(); ctx.arc(end[0], end[1], 2.5, 0, Math.PI * 2); ctx.fill();
    };
    frame = requestAnimationFrame(draw);
    return () => cancelAnimationFrame(frame);
  });
</script>

<canvas bind:this={canvas} class="live" style:width="{W}px" style:height="{H}px" aria-label="实时用量图"></canvas>

<style>
  .live {
    position: absolute;
    left: 12px;
    top: 104px;
    animation: appear 0.35s ease-out;
  }
  @keyframes appear {
    from { opacity: 0; }
  }
</style>
