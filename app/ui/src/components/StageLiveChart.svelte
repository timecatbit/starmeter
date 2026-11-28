<script lang="ts">
  // 网页版's live curve: how fast the models are writing (output tokens a second, the last minute's eased as the pinned
  // panel's Token/s is: LiveFeed.smoothRates) over the last minute, flowing left as time runs, in the live line's
  // one look (liveLines.ts): the total the line the eye follows, a soft area under it and its end a dot whose halo
  // breathes; each tool thin in its colour where it writes; one writing alone is the total (no second line on it).
  // Named in a legend over it. The plot is the monitor's whole width: the end's halo in its own room at the right,
  // 现在 under it at the edge. Each request that lands drops in from above onto the end, ripples there, and says what
  // it added (+12.3K) as it rises and fades. Reduced motion: drawn again at each reading, nothing falling or flowing.
  import { onDestroy, untrack } from 'svelte';
  import { monotoneCurve, tracePath, type Point } from '../lib/chart';
  import { alpha, summaryColour, toolColour } from '../lib/colors';
  import { style } from '../lib/style.svelte';
  import { t } from '../lib/i18n.svelte';
  import { nice, rateText } from '../lib/live';
  import { drawLines, gridLine, LINE } from '../lib/liveLines';
  import { LIVE_SPAN, type PhoneLive } from '../lib/phoneLive.svelte';
  import { legendName, tokens } from '../lib/format';
  import { panel } from '../lib/ui.svelte';

  let { live, height, legendHeight = $bindable(0) }: {
    live: PhoneLive; height: number;
    /** The legend's height over the curve, for the monitor to give the curve its room under it. */
    legendHeight?: number;
  } = $props();

  /** The lines named: the total's, then each tool's as drawn (the busiest first). */
  let named = $state<{ id: string; name: string; idle: boolean }[]>([]);
  const plotHeight = $derived(Math.max(40, height - legendHeight));

  let box: HTMLDivElement | undefined = $state();
  let canvas: HTMLCanvasElement | undefined = $state();
  /** The total's halo, breathing on the page (a CSS animation, not a canvas drawn every frame). */
  let tip: HTMLSpanElement | undefined = $state();
  let width = $state(0);
  $effect(() => {
    if (!box) return;
    const observer = new ResizeObserver(() => (width = box?.clientWidth ?? 0));
    observer.observe(box);
    width = box.clientWidth;
    return () => observer.disconnect();
  });

  /** The curve worked out at each reading (not each frame): the total's rate and each tool's, a point a second. */
  let ids: string[] = [];
  let runningIds = new Set<string>();
  let times: number[] = [];
  let totals: number[] = [];
  let rates: number[][] = [];
  let top = 4;
  /** When the last reading was, and how many seconds past it the curve is worked out. */
  let readingAt = 0;
  const AHEAD = 6;
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
      const feed = live.feed, samples = feed.samples, now = Date.now();
      const seen = new Set<string>();
      for (const s of samples) for (const id of Object.keys(s.outputs ?? {})) seen.add(id);
      const runningNow = new Set<string>();
      const countedIds = new Set((live.dashboard?.providers ?? []).filter((p) => p.available && p.summaryIncluded !== false).map((p) => p.id));
      for (const s of live.sessions) {
        if (s.running && s.tool) {
          runningNow.add(s.tool);
          if (countedIds.has(s.tool)) seen.add(s.tool);
        }
      }
      runningIds = runningNow;
      // The busiest first (the legend in that order).
      const last = samples.at(-1);
      ids = [...seen].sort((a, b) => (last?.outputs?.[b] ?? 0) - (last?.outputs?.[a] ?? 0));
      // A point a second from the first reading (or a minute back) to the last, and on a few seconds past it as
      // the rate eases with nothing new (the curve runs on to now between readings, not flat for the last two).
      times = [];
      readingAt = last?.at ?? 0;
      if (samples.length) {
        const from = Math.max(samples[0].at, last!.at - LIVE_SPAN - 5000);
        for (let at = last!.at; at >= from; at -= 1000) times.unshift(at);
        for (let k = 1; k <= AHEAD; k++) times.push(last!.at + k * 1000);
      }
      totals = feed.smoothRates(times);
      rates = ids.map((id) => feed.smoothRates(times, id));
      // Named in the legend; a tool that wrote nothing in these minutes quieter there unless it has a running session.
      const names = new Map((live.dashboard?.providers ?? []).map((p) => [p.id, legendName(p)]));
      const next = ids.map((id, k) => ({ id, name: names.get(id) ?? id, idle: !runningIds.has(id) && rates[k].every((r) => r <= 0.05) }));
      const key = (list: typeof named) => list.map((n) => `${n.id}${n.name}${n.idle}`).join();
      if (key(next) !== key(named)) named = next;
      highest = Math.max(0, ...totals.slice(0, totals.length - AHEAD));
      top = nice(Math.max(4, highest * LINE.headroom));
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

  // The scale eases to a new top (a third of a second, up or down) instead of jumping; never under what is drawn.
  let shownTop = 4;
  /** The highest the line reaches now. */
  let highest = 0;
  let frame = 0;
  let running = false;
  let lastDrawn = 0;
  /** The next drawing while nothing on the curve moves but its flow left (a timer, not every frame). */
  let later: ReturnType<typeof setTimeout> | undefined;
  /** How many times drawn (for the tests: every frame only while something moves). */
  let draws = 0;
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
  const setAttr = (d: DOMStringMap, key: string, value: string) => { if (d[key] !== value) d[key] = value; };

  function draw() {
    running = false;
    if (!canvas || width <= 0) return;
    // At the screen's own density: the curve is the page's main figure, and at 2x on a phone's 3x screen it went soft.
    const ratio = window.devicePixelRatio || 1, W = width, H = plotHeight;
    if (canvas.width !== Math.round(W * ratio) || canvas.height !== Math.round(H * ratio)) {
      canvas.width = Math.round(W * ratio); canvas.height = Math.round(H * ratio);
    }
    const ctx = canvas.getContext('2d')!;
    ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
    ctx.clearRect(0, 0, W, H);
    const now = Date.now(), still = panel.reduceMotion, dt = lastDrawn ? Math.min(250, now - lastDrawn) : 1000;
    lastDrawn = now;
    shownTop = still ? top : Math.max(highest * 1.04, shownTop + (top - shownTop) * (1 - Math.exp(-dt / 120)));
    if (Math.abs(top - shownTop) < top * 0.002) shownTop = top;
    // The plot the whole width: the end's halo (8) and a reading's ripple in LINE.end at the right; the top's rate written
    // over its grid line inside, the times under the bottom.
    const padTop = 16, padBottom = 18, right = W - LINE.end, plotH = H - padTop - padBottom, base = padTop + plotH;
    // The right edge is now; a reading is placed at its moment, so the curve flows left between readings.
    const end = still ? readingAt || now : now;
    // The last minute, as the pinned panel's.
    const span = LIVE_SPAN;
    const x = (at: number) => right - ((end - at) / span) * right;
    const y = (v: number) => Math.min(base, Math.max(padTop, base - (v / shownTop) * plotH));
    const sketch = style.current === 'sketch';
    const ink = (amount: number) => alpha(faces.ink, amount);

    // The grid: dotted at half and at the top, the bottom a faint rule.
    for (const f of [0.5, 1]) gridLine(ctx, y(shownTop * f), 0, right, faces.ink);
    ctx.strokeStyle = ink(0.16); ctx.lineWidth = 1;
    ctx.beginPath(); ctx.moveTo(0, base + 0.5); ctx.lineTo(right, base + 0.5); ctx.stroke();
    ctx.fillStyle = ink(0.42);
    ctx.font = `9.5px ${faces.text}`;
    ctx.textBaseline = 'alphabetic';
    ctx.textAlign = 'left';
    // The top's rate over its line, once there is a speed to scale (a page just opened has none: no "4 Token/s" over
    // nothing).
    if (live.ready()) ctx.fillText(`${rateText(shownTop)} Token/s`, 0, y(shownTop) - 4);
    const ago = Math.round(span / 60_000);
    ctx.fillText(t('{n} 分钟前', { n: ago }), 0, H - 3);
    ctx.textAlign = 'right';
    ctx.fillText(t('现在'), W, H - 3);

    // The lines up to now, the last point between the two seconds either side of it.
    const shownAt = times.map((at, i) => [at, i] as const).filter(([at]) => x(at) >= -40 && at <= end);
    let drawn: ReturnType<typeof drawLines> | null = null;
    if (shownAt.length) {
      const [lastAt, lastIndex] = shownAt.at(-1)!, after = times[lastIndex + 1];
      // Past the seconds worked out (nothing read for a while): on flat.
      const f = after !== undefined && after > lastAt ? Math.min(1, (end - lastAt) / (after - lastAt)) : 0;
      const line = (of: number[]): Point[] => {
        const points = shownAt.map(([at, i]): Point => [x(at), y(of[i] ?? 0)]);
        if (end > lastAt) points.push([x(end), y((of[lastIndex] ?? 0) + ((of[lastIndex + 1] ?? of[lastIndex] ?? 0) - (of[lastIndex] ?? 0)) * f)]);
        return points;
      };
      const total = sketch ? summaryColour(panel.dark) : panel.dark ? 'rgba(240, 245, 255, 0.95)' : 'rgba(14, 29, 76, 0.85)';
      // 手绘: a line drawn twice by the hand, the second a touch off and lighter (as the panel's strokes are), here in the
      // drawing itself: the wobble filter over a canvas drawn again and again would be worked out each time.
      const again = sketch ? (points: Point[]) => {
        ctx.save();
        ctx.globalAlpha *= 0.45; ctx.lineWidth *= 0.8;
        ctx.beginPath(); tracePath(ctx, monotoneCurve(points.map(([px, py]) => [px + 0.8, Math.min(base, py + 0.7)]))); ctx.stroke();
        ctx.restore();
      } : undefined;
      drawn = drawLines(ctx, {
        total: line(totals), base, plotTop: padTop, totalColour: total, areaColour: summaryColour(panel.dark), dark: panel.dark,
        tools: ids.map((id, k) => ({ id, points: line(rates[k]), colour: toolColour(id, panel.dark), running: runningIds.has(id) })),
        // Starlight on the total at night; by day and by hand, none.
        glow: !sketch && panel.dark ? 'rgba(196, 216, 255, 0.7)' : null, again,
      });
      const [ex, ey] = drawn.end;
      // Now: the total's dot at its end (its halo breathes on the page over it).
      ctx.fillStyle = sketch ? summaryColour(panel.dark) : panel.dark ? '#F5F8FF' : '#0E1D4C';
      ctx.beginPath(); ctx.arc(ex, ey, LINE.dot, 0, Math.PI * 2); ctx.fill();
      if (tip) { tip.style.translate = `${ex}px ${ey}px`; tip.style.opacity = '1'; tip.style.color = ctx.fillStyle as string; }
      setAttr(canvas.dataset, 'lines', String(ids.length));
      setAttr(canvas.dataset, 'end', `${Math.round(ex * 10) / 10},${Math.round(ey * 10) / 10},${W},${H}`);
      setAttr(canvas.dataset, 'dots', JSON.stringify(Object.fromEntries(drawn.dots.map((id) => [id, Math.round(drawn!.ends[id] * 10) / 10]))));
      setAttr(canvas.dataset, 'under', drawn.under.join(','));
      setAttr(canvas.dataset, 'top', String(Math.round(shownTop * 10) / 10));
      setAttr(canvas.dataset, 'highest', String(Math.round(highest * 10) / 10));
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
            // Translucent halo ring instead of shadowBlur=10 (avoids per-frame Gaussian blur on WebKit, PERF-2).
            if (!sketch) {
              ctx.globalAlpha = 0.28;
              ctx.beginPath(); ctx.arc(ex, fy, 8, 0, Math.PI * 2); ctx.fill();
              ctx.globalAlpha = 1;
            }
            ctx.beginPath(); ctx.arc(ex, fy, 4.5, 0, Math.PI * 2); ctx.fill();
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
    } else if (tip) tip.style.opacity = '0';
    setAttr(canvas.dataset, 'hand', sketch ? 'twice' : '');
    setAttr(canvas.dataset, 'draws', String(++draws));
    // Keep flowing while the page shows it; reduced motion draws only at readings. Every frame while something moves on
    // it (a request landing, its sparks, the scale easing); otherwise only as the flow left moves it half a point (a
    // minute across the width: about 12 times a second).
    if (still || document.visibilityState !== 'visible') return;
    // A drop is moving for its fall and its rise, not for as long as it is kept (until the next reading: STAGE-19, a
    // computer gone quiet just after one kept every frame drawn).
    const moving = drops.some((d) => now - d.born < DROP + RISE) || sparks.some((s) => now - s.born < SPARK) || shownTop !== top;
    if (moving) queue();
    // Resting on a completely flat zero line: nothing changes between readings, so stop idle redraws until the next reading (R10-12).
    else if (highest > 0 || live.state === 'working') later = setTimeout(queue, Math.min(1000, Math.max(33, (span / Math.max(1, right)) * 0.5)));
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
  <div class="plot" role="img" aria-label={t('实时用量图')} data-tap="beam">
    <canvas bind:this={canvas} style:width="{width}px" style:height="{plotHeight}px"></canvas>
    <span class="tip" class:still={panel.reduceMotion} bind:this={tip} aria-hidden="true"></span>
  </div>
</div>

<style>
  .live-chart { position: relative; display: flex; flex-direction: column; }
  canvas { display: block; }
  .plot { position: relative; }
  /* The total's halo round its dot, breathing (the panel's on its canvas: liveLines.ts, the same sizes and time). */
  .tip {
    position: absolute; left: 0; top: 0; width: 16px; height: 16px; margin: -8px 0 0 -8px; border-radius: 50%;
    background: currentColor; opacity: 0; pointer-events: none; transition: opacity 0.4s;
  }
  .tip:not(.still) { animation: breathe 2.4s ease-in-out infinite; }
  .tip.still { transform: scale(0.78); filter: opacity(0.21); }
  @keyframes breathe {
    0%, 100% { transform: scale(0.5625); filter: opacity(0.32); }
    50% { transform: scale(1); filter: opacity(0.1); }
  }
  /* The room under the legend inside it (padding, not margin): its height, measured, is what the plot leaves, and a
     margin outside it would push the plot's foot, its 1 分钟前 · 现在, 4 points past the curve's box (cut off). */
  .legend { display: flex; flex-wrap: wrap; gap: 2px 12px; margin: 0; padding: 0 0 4px; list-style: none; font-size: 11px; line-height: 16px; color: var(--ink-2); }
  .legend li { display: inline-flex; align-items: center; gap: 5px; white-space: nowrap; transition: opacity 0.4s; }
  .legend li.idle { opacity: 0.45; }
  .swatch { width: 10px; height: 2.5px; border-radius: 2px; }
  .swatch.total { height: 3px; background: var(--ink); box-shadow: 0 0 6px var(--halo); }
  :global(.sketch) .swatch.total { background: var(--coral); box-shadow: none; }
</style>
