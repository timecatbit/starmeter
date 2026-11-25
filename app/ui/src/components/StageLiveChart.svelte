<script lang="ts">
  // 网页版's live curve: how fast each tool's models are writing (output tokens a second over the last minute, as the
  // pinned panel's Token/s reads it), stacked a band per tool over the last five minutes, flowing left as time runs, a
  // glowing line along the top. Each request that lands drops in from above onto the curve's end, ripples there, and
  // says what it added (+12.3K) as it rises and fades: the AI's work arriving, one request at a time. Reduced motion:
  // drawn again at each reading, nothing falling or flowing.
  import { onDestroy, untrack } from 'svelte';
  import { alpha, summaryColour, toolColour } from '../lib/colors';
  import { style } from '../lib/style.svelte';
  import { t } from '../lib/i18n.svelte';
  import { nice, rateText } from '../lib/live';
  import { LIVE_SPAN, type PhoneLive } from '../lib/phoneLive.svelte';
  import { tokens } from '../lib/format';
  import { panel } from '../lib/ui.svelte';

  let { live, height }: { live: PhoneLive; height: number } = $props();

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
  const DROP = 650, RIPPLE = 700, RISE = 1700;

  $effect(() => {
    void live.version;
    untrack(() => {
      const samples = live.feed.samples, now = Date.now();
      const seen = new Set<string>();
      for (const s of samples) for (const id of Object.keys(s.outputs ?? {})) seen.add(id);
      // The busiest at the bottom, so the bands above it are the smaller ones.
      const last = samples.at(-1);
      ids = [...seen].sort((a, b) => (last?.outputs?.[b] ?? 0) - (last?.outputs?.[a] ?? 0));
      points = samples.filter((s) => now - s.at <= LIVE_SPAN + 5000).map((s) => ({ at: s.at, rates: ids.map((id) => live.feed.rateAt(s.at, id) ?? 0) }));
      const highest = Math.max(0, ...points.map((p) => p.rates.reduce((a, b) => a + b, 0)));
      top = nice(Math.max(4, highest * 1.18));
      for (const landing of live.landings) drops.push({ id: landing.id, tokens: landing.tokens, born: now });
      drops = drops.filter((d) => now - d.born < DROP + RISE);
      if (panel.reduceMotion) drops = [];
      queue();
    });
  });

  // The scale eases to a new top instead of jumping, so a burst grows the axis smoothly.
  let shownTop = 2000;
  let frame = 0;
  let running = false;
  function queue() {
    if (running) return;
    running = true;
    frame = requestAnimationFrame(draw);
  }
  /** The page's own faces and ink, as it shows them (by hand in 手绘): read again when the look changes. */
  let faces = { text: 'system-ui, sans-serif', figure: 'ui-rounded, system-ui, sans-serif', ink: 'rgb(214, 222, 255)' };
  $effect(() => {
    void width; void height; void panel.dark; void style.current;
    untrack(() => {
      if (canvas) {
        const css = getComputedStyle(canvas);
        faces = { text: css.fontFamily, figure: css.getPropertyValue('--rounded').trim() || css.fontFamily, ink: css.getPropertyValue('--ink').trim() || css.color };
      }
      queue();
    });
  });
  onDestroy(() => { cancelAnimationFrame(frame); running = false; });

  const ease = (x: number) => 1 - Math.pow(1 - Math.min(1, Math.max(0, x)), 3);
  const bounce = (x: number) => { const k = Math.min(1, Math.max(0, x)); return k < 0.8 ? ease(k / 0.8) * 1.04 : 1.04 - 0.04 * ease((k - 0.8) / 0.2); };

  function draw() {
    running = false;
    if (!canvas || width <= 0) return;
    const ratio = window.devicePixelRatio || 1, W = width, H = height;
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
    ctx.fillText(`${rateText(shownTop)} Token/s`, 2, padTop - 6);
    const ago = Math.round(span / 60_000);
    ctx.fillText(t('{n} 分钟前', { n: ago }), 2, H - 4);
    ctx.textAlign = 'right';
    ctx.fillText(t('现在'), right, H - 4);

    // The bands, bottom up, each tool's rate on top of the ones under it; the curve runs on to now at its last height.
    const shown = points.filter((p) => x(p.at) >= -40);
    if (shown.length) {
      const tail = { at: end, rates: shown.at(-1)!.rates };
      const line = [...shown, tail];
      const stack = line.map((p) => { let sum = 0; return p.rates.map((r) => (sum += r)); });
      const curve = (k: number) => {
        // A smooth path through the points: midpoints joined by quadratic curves.
        const xy = line.map((p, i) => [x(p.at), y(stack[i][k] ?? 0)] as const);
        ctx.moveTo(xy[0][0], xy[0][1]);
        for (let i = 1; i < xy.length - 1; i++) { const mx = (xy[i][0] + xy[i + 1][0]) / 2, my = (xy[i][1] + xy[i + 1][1]) / 2; ctx.quadraticCurveTo(xy[i][0], xy[i][1], mx, my); }
        ctx.lineTo(xy[xy.length - 1][0], xy[xy.length - 1][1]);
        return xy;
      };
      for (let k = ids.length - 1; k >= 0; k--) {
        const colour = toolColour(ids[k], panel.dark);
        ctx.beginPath();
        const xy = curve(k);
        ctx.lineTo(xy[xy.length - 1][0], base);
        ctx.lineTo(xy[0][0], base);
        ctx.closePath();
        const fill = ctx.createLinearGradient(0, padTop, 0, base);
        fill.addColorStop(0, alpha(colour, sketch ? 0.3 : panel.dark ? 0.55 : 0.45));
        fill.addColorStop(1, alpha(colour, sketch ? 0.04 : 0.06));
        ctx.fillStyle = fill;
        ctx.fill();
      }
      // The total's line, glowing, over the bands.
      const k = ids.length - 1;
      if (k >= 0) {
        ctx.save();
        // The total's line: starlight with its glow, or by hand the coral pen, plain.
        ctx.shadowColor = sketch ? 'transparent' : panel.dark ? 'rgba(196, 216, 255, 0.85)' : 'rgba(80, 100, 220, 0.45)';
        ctx.shadowBlur = 12;
        ctx.strokeStyle = sketch ? summaryColour(panel.dark) : panel.dark ? 'rgba(240, 245, 255, 0.95)' : 'rgba(14, 29, 76, 0.85)';
        ctx.lineWidth = 2;
        ctx.lineJoin = 'round';
        ctx.beginPath();
        const xy = curve(k);
        ctx.stroke();
        ctx.restore();
        // Now: a dot at the curve's end.
        const [ex, ey] = xy[xy.length - 1];
        ctx.fillStyle = sketch ? summaryColour(panel.dark) : panel.dark ? '#F5F8FF' : '#0E1D4C';
        ctx.beginPath(); ctx.arc(ex, ey, 3.5, 0, Math.PI * 2); ctx.fill();
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
    // Keep flowing while the page shows it; reduced motion draws only at readings.
    if (!still && document.visibilityState === 'visible') queue();
  }
</script>

<div class="live-chart" bind:this={box} style:height="{height}px" role="img" aria-label={t('实时用量图')}>
  <canvas bind:this={canvas} style:width="{width}px" style:height="{height}px"></canvas>
</div>

<style>
  .live-chart { position: relative; }
  canvas { display: block; }
</style>
