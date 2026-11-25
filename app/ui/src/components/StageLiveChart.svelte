<script lang="ts">
  // 网页版's live curve: how fast the models are writing (output tokens a second over the last minute, as the pinned
  // panel's Token/s reads it) over the last five minutes, flowing left as time runs: the total's glowing line, and each
  // tool's own line in its colour (as the pinned panel draws them), a tool writing nothing lying along the bottom under
  // the total's line, the ones writing over it; their ends step aside, and a tool ending where the total does is the
  // total's dot. Named in a legend over it. Each request that lands drops in from above onto the curve's end, ripples there, and
  // says what it added (+12.3K) as it rises and fades: the AI's work arriving, one request at a time. Reduced motion:
  // drawn again at each reading, nothing falling or flowing.
  import { onDestroy, untrack } from 'svelte';
  import { monotoneCurve, tracePath } from '../lib/chart';
  import { alpha, summaryColour, toolColour } from '../lib/colors';
  import { style } from '../lib/style.svelte';
  import { t } from '../lib/i18n.svelte';
  import { nice, rateText } from '../lib/live';
  import { LIVE_SPAN, type PhoneLive } from '../lib/phoneLive.svelte';
  import { legendName, tokens } from '../lib/format';
  import { panel } from '../lib/ui.svelte';

  let { live, height }: { live: PhoneLive; height: number } = $props();

  /** The lines named: the total's, then each tool's as drawn (the busiest first). */
  let named = $state<{ id: string; name: string; idle: boolean }[]>([]);
  let legendHeight = $state(0);
  const plotHeight = $derived(Math.max(40, height - legendHeight));

  let box: HTMLDivElement | undefined = $state();
  let canvas: HTMLCanvasElement | undefined = $state();
  let width = $state(0);
  $effect(() => {
    if (!box) return;
    const observer = new ResizeObserver(() => (width = box?.clientWidth ?? 0));
    observer.observe(box);
    width = box.clientWidth;
    return () => observer.disconnect();
  });

  /** The curve's points, worked out at each reading (not each frame): each tool's rate at each sample's moment. */
  type Point = { at: number; rates: number[] };
  let ids: string[] = [];
  let points: Point[] = [];
  let top = 2000;
  type Drop = { id: string; tokens: number; born: number };
  let drops: Drop[] = [];
  /** As a request lands, a few sparks rise off the curve's end and fade. */
  type Spark = { id: string; born: number; dx: number; rise: number; size: number };
  let sparks: Spark[] = [];
  const SPARK = 1100;
  const DROP = 650, RIPPLE = 700, RISE = 1700;

  $effect(() => {
    void live.version;
    untrack(() => {
      const samples = live.feed.samples, now = Date.now();
      const seen = new Set<string>();
      for (const s of samples) for (const id of Object.keys(s.outputs ?? {})) seen.add(id);
      // The busiest first (the legend in that order; drawn last, over the others).
      const last = samples.at(-1);
      ids = [...seen].sort((a, b) => (last?.outputs?.[b] ?? 0) - (last?.outputs?.[a] ?? 0));
      points = samples.filter((s) => now - s.at <= LIVE_SPAN + 5000).map((s) => ({ at: s.at, rates: ids.map((id) => live.feed.rateAt(s.at, id) ?? 0) }));
      // Named in the legend; a tool that wrote nothing in these minutes quieter there (its line along the bottom).
      const names = new Map((live.dashboard?.providers ?? []).map((p) => [p.id, legendName(p)]));
      const next = ids.map((id, k) => ({ id, name: names.get(id) ?? id, idle: points.every((p) => p.rates[k] <= 0) }));
      const key = (list: typeof named) => list.map((n) => `${n.id}${n.name}${n.idle}`).join();
      if (key(next) !== key(named)) named = next;
      const highest = Math.max(0, ...points.map((p) => p.rates.reduce((a, b) => a + b, 0)));
      top = nice(Math.max(4, highest * 1.18));
      for (const landing of live.landings) {
        drops.push({ id: landing.id, tokens: landing.tokens, born: now });
        for (let i = 0; i < 4; i++) sparks.push({ id: landing.id, born: now + DROP, dx: (Math.random() - 0.5) * 26, rise: 18 + Math.random() * 22, size: 1.2 + Math.random() * 0.9 });
      }
      sparks = sparks.filter((s) => now - s.born < SPARK);
      drops = drops.filter((d) => now - d.born < DROP + RISE);
      if (panel.reduceMotion) { drops = []; sparks = []; }
      queue();
    });
  });

  // The scale eases to a new top instead of jumping, so a burst grows the axis smoothly.
  let shownTop = 2000;
  let frame = 0;
  let running = false;
  /** The next drawing while nothing on the curve moves but its flow left (a timer, not every frame). */
  let later: ReturnType<typeof setTimeout> | undefined;
  function queue() {
    clearTimeout(later);
    if (running) return;
    running = true;
    frame = requestAnimationFrame(draw);
  }
  /** The page's own faces and ink, as it shows them (by hand in 手绘): read again when the look changes. */
  let faces = { text: 'system-ui, sans-serif', figure: 'ui-rounded, system-ui, sans-serif', ink: 'rgb(214, 222, 255)' };
  $effect(() => {
    void width; void plotHeight; void panel.dark; void style.current;
    untrack(() => {
      if (canvas) {
        const css = getComputedStyle(canvas);
        faces = { text: css.fontFamily, figure: css.getPropertyValue('--rounded').trim() || css.fontFamily, ink: css.getPropertyValue('--ink').trim() || css.color };
      }
      queue();
    });
  });
  onDestroy(() => { cancelAnimationFrame(frame); clearTimeout(later); running = false; });

  const ease = (x: number) => 1 - Math.pow(1 - Math.min(1, Math.max(0, x)), 3);
  const bounce = (x: number) => { const k = Math.min(1, Math.max(0, x)); return k < 0.8 ? ease(k / 0.8) * 1.04 : 1.04 - 0.04 * ease((k - 0.8) / 0.2); };

  function draw() {
    running = false;
    if (!canvas || width <= 0) return;
    const ratio = window.devicePixelRatio || 1, W = width, H = plotHeight;
    if (canvas.width !== Math.round(W * ratio) || canvas.height !== Math.round(H * ratio)) {
      canvas.width = Math.round(W * ratio); canvas.height = Math.round(H * ratio);
    }
    const ctx = canvas.getContext('2d')!;
    ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
    ctx.clearRect(0, 0, W, H);
    const now = Date.now(), still = panel.reduceMotion;
    shownTop = still ? top : shownTop + (top - shownTop) * 0.12;
    if (Math.abs(top - shownTop) < top * 0.002) shownTop = top;
    const padTop = 18, padBottom = 20, right = W - 30, plotH = H - padTop - padBottom, base = padTop + plotH;
    // The right edge is now; a reading is placed at its moment, so the curve flows left between readings.
    const end = still ? (points.at(-1)?.at ?? now) : now;
    // A minute at first, widening to five as the readings come in: the curve fills the width from the start.
    const first = live.feed.samples[0]?.at ?? end;
    const span = Math.min(LIVE_SPAN, Math.max(60_000, end - first + 15_000));
    const x = (at: number) => right - ((end - at) / span) * right;
    const y = (v: number) => base - (v / shownTop) * plotH;
    const sketch = style.current === 'sketch';
    const ink = (amount: number) => alpha(faces.ink, amount);

    // Marks: a faint line at half and at the top, the top's rate written above it.
    ctx.lineWidth = 1;
    ctx.setLineDash([2, 4]);
    ctx.strokeStyle = ink(0.1);
    for (const f of [0.5, 1]) { ctx.beginPath(); ctx.moveTo(0, y(shownTop * f)); ctx.lineTo(right, y(shownTop * f)); ctx.stroke(); }
    ctx.setLineDash([]);
    ctx.strokeStyle = ink(0.2);
    ctx.beginPath(); ctx.moveTo(0, base + 0.5); ctx.lineTo(right, base + 0.5); ctx.stroke();
    ctx.fillStyle = ink(0.45);
    ctx.font = `10px ${faces.text}`;
    ctx.textBaseline = 'alphabetic';
    ctx.textAlign = 'left';
    // The top's rate, once there is a speed to scale (a page just opened has none: no "4 Token/s" over nothing).
    if (live.ready()) ctx.fillText(`${rateText(shownTop)} Token/s`, 2, padTop - 6);
    const ago = Math.round(span / 60_000);
    ctx.fillText(t('{n} 分钟前', { n: ago }), 2, H - 4);
    ctx.textAlign = 'right';
    ctx.fillText(t('现在'), right, H - 4);

    // The lines run on to now at their last height.
    const shown = points.filter((p) => x(p.at) >= -40);
    const ends: Record<string, number> = {};
    const under: string[] = [];
    if (shown.length) {
      const tail = { at: end, rates: shown.at(-1)!.rates };
      const line = [...shown, tail];
      const totals = line.map((p) => p.rates.reduce((a, b) => a + b, 0));
      const values = ids.map((_, k) => line.map((p) => p.rates[k] ?? 0));
      const path = (of: number[]) => {
        // A smooth curve through the readings that goes no further than them (never under zero, no peak not read).
        const xy = line.map((p, i) => [x(p.at), y(of[i] ?? 0)] as [number, number]);
        tracePath(ctx, monotoneCurve(xy));
        return xy;
      };
      const total = sketch ? summaryColour(panel.dark) : panel.dark ? 'rgba(240, 245, 255, 0.95)' : 'rgba(14, 29, 76, 0.85)';
      // Under the total's line, a faint glow of it down to zero.
      ctx.beginPath();
      const area = path(totals);
      ctx.lineTo(area[area.length - 1][0], base);
      ctx.lineTo(area[0][0], base);
      ctx.closePath();
      const fill = ctx.createLinearGradient(0, padTop, 0, base);
      fill.addColorStop(0, alpha(summaryColour(panel.dark), sketch ? 0.14 : panel.dark ? 0.3 : 0.2));
      fill.addColorStop(1, alpha(summaryColour(panel.dark), 0));
      ctx.fillStyle = fill;
      ctx.fill();
      // 手绘: a line drawn twice by the hand, the second a touch off and lighter (as the panel's strokes are), here in the
      // drawing itself: the wobble filter over a canvas drawn again and again would be worked out each time.
      const again = (xy: [number, number][]) => {
        if (!sketch) return;
        ctx.save();
        ctx.globalAlpha *= 0.45; ctx.lineWidth *= 0.8;
        ctx.beginPath(); tracePath(ctx, monotoneCurve(xy.map(([px, py]) => [px + 0.8, Math.min(base, py + 0.7)]))); ctx.stroke();
        ctx.restore();
      };
      const toolLine = (k: number) => {
        ctx.beginPath();
        const xy = path(values[k]);
        ctx.lineWidth = 1.6; ctx.lineJoin = 'round'; ctx.lineCap = 'round';
        ctx.strokeStyle = toolColour(ids[k], panel.dark);
        ctx.stroke();
        again(xy);
        ends[ids[k]] = xy[xy.length - 1][1];
      };
      const idle = (k: number) => values[k].every((v) => v <= 0);
      // The idle tools along the bottom, under the total's line.
      ids.forEach((_, k) => { if (idle(k)) toolLine(k); });
      ctx.save();
      // The total's line: starlight with its glow, or by hand the coral pen, plain.
      ctx.shadowColor = sketch ? 'transparent' : panel.dark ? 'rgba(196, 216, 255, 0.85)' : 'rgba(80, 100, 220, 0.45)';
      ctx.shadowBlur = 12;
      ctx.strokeStyle = total;
      ctx.lineWidth = 2;
      ctx.lineJoin = 'round';
      ctx.beginPath();
      const xy = path(totals);
      ctx.stroke();
      ctx.restore();
      again(xy);
      // The tools writing, over it, the busiest last.
      for (let k = ids.length - 1; k >= 0; k--) if (!idle(k)) toolLine(k);
      const [ex, ey] = xy[xy.length - 1];
      // Each tool's end, a small dot of its colour: none for one ending where the total does (the total's dot is that
      // point), one for several ending together (the idle ones along the bottom).
      const marks = ids.filter((id) => { if (Math.abs(ends[id] - ey) < 6) { under.push(id); return false; } return true; });
      const dots = marks.filter((id, i) => !marks.slice(i + 1).some((later) => Math.abs(ends[later] - ends[id]) < 4.5));
      for (const id of dots) {
        ctx.fillStyle = toolColour(id, panel.dark);
        ctx.beginPath(); ctx.arc(ex, ends[id], 2.5, 0, Math.PI * 2); ctx.fill();
      }
      // Now: the total's dot at its end.
      ctx.fillStyle = sketch ? summaryColour(panel.dark) : panel.dark ? '#F5F8FF' : '#0E1D4C';
      ctx.beginPath(); ctx.arc(ex, ey, 3.5, 0, Math.PI * 2); ctx.fill();
      canvas.dataset.lines = String(ids.length);
      canvas.dataset.dots = JSON.stringify(Object.fromEntries(dots.map((id) => [id, Math.round(ends[id] * 10) / 10])));
      canvas.dataset.under = under.join(',');
      {
        // Sparks off the end as each lands, rising and fading.
        for (const s of sparks) {
          const age = now - s.born;
          if (age < 0 || age >= SPARK) continue;
          const k = ease(age / SPARK);
          ctx.globalAlpha = 0.85 * (1 - age / SPARK);
          ctx.fillStyle = toolColour(s.id, panel.dark);
          ctx.beginPath(); ctx.arc(ex + s.dx * k, ey - s.rise * k, s.size, 0, Math.PI * 2); ctx.fill();
        }
        ctx.globalAlpha = 1;
        // The requests landing: each falls onto the end, ripples, and says what it added as it rises away.
        let lift = 0;
        for (const d of drops) {
          const age = now - d.born, colour = toolColour(d.id, panel.dark);
          if (age < DROP) {
            const fy = padTop - 6 + (ey - padTop + 6) * bounce(age / DROP);
            ctx.fillStyle = colour;
            ctx.shadowColor = sketch ? 'transparent' : colour; ctx.shadowBlur = 10;
            ctx.beginPath(); ctx.arc(ex, fy, 4.5, 0, Math.PI * 2); ctx.fill();
            ctx.shadowBlur = 0;
          } else {
            const after = age - DROP;
            if (after < RIPPLE) {
              ctx.strokeStyle = colour; ctx.globalAlpha = 1 - after / RIPPLE; ctx.lineWidth = 2;
              ctx.beginPath(); ctx.arc(ex, ey, 4 + 18 * ease(after / RIPPLE), 0, Math.PI * 2); ctx.stroke();
              ctx.globalAlpha = 1;
            }
            if (after < RISE) {
              ctx.globalAlpha = Math.max(0, 1 - after / RISE);
              ctx.fillStyle = colour;
              ctx.font = `600 12px ${faces.figure}`;
              ctx.textAlign = 'right';
              ctx.fillText('+' + tokens(d.tokens), ex - 8, ey - 10 - 26 * ease(after / RISE) - lift);
              ctx.globalAlpha = 1;
              lift += 15;
            }
          }
        }
      }
    }
    canvas.dataset.hand = sketch ? 'twice' : '';
    // Keep flowing while the page shows it; reduced motion draws only at readings. Every frame while something moves on
    // it (a request landing, its sparks, the scale easing); otherwise only as the flow left moves it half a point (a
    // minute across the width: about 12 times a second; five minutes: twice).
    if (still || document.visibilityState !== 'visible') return;
    const moving = drops.length > 0 || sparks.some((s) => now - s.born < SPARK) || shownTop !== top;
    if (moving) queue();
    else later = setTimeout(queue, Math.min(1000, Math.max(33, (span / Math.max(1, right)) * 0.5)));
  }
</script>

<div class="live-chart" bind:this={box} style:height="{height}px">
  {#if named.length}
    <!-- What each line is: the total's, then each tool's in its colour. -->
    <ul class="legend" bind:offsetHeight={legendHeight}>
      <li><span class="swatch total"></span>{t('总量')}</li>
      {#each named as n (n.id)}<li class:idle={n.idle}><span class="swatch" style:background={toolColour(n.id, panel.dark)}></span>{n.name}</li>{/each}
    </ul>
  {/if}
  <div class="plot" role="img" aria-label={t('实时用量图')}><canvas bind:this={canvas} style:width="{width}px" style:height="{plotHeight}px"></canvas></div>
</div>

<style>
  .live-chart { position: relative; display: flex; flex-direction: column; }
  canvas { display: block; }
  /* The room under the legend inside it (padding, not margin): its height, measured, is what the plot leaves, and a
     margin outside it would push the plot's foot, its 1 分钟前 · 现在, 4 points past the curve's box (cut off). */
  .legend { display: flex; flex-wrap: wrap; gap: 2px 12px; margin: 0; padding: 0 0 4px; list-style: none; font-size: 11px; line-height: 16px; color: var(--ink-2); }
  .legend li { display: inline-flex; align-items: center; gap: 5px; white-space: nowrap; transition: opacity 0.4s; }
  .legend li.idle { opacity: 0.45; }
  .swatch { width: 10px; height: 2.5px; border-radius: 2px; }
  .swatch.total { height: 3px; background: var(--ink); box-shadow: 0 0 6px var(--halo); }
  :global(.sketch) .swatch.total { background: var(--coral); box-shadow: none; }
</style>
