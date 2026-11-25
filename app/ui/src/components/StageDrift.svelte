<script lang="ts">
  // 网页版's sky, alive: behind everything, faint, the logos of the tools at work now (the sessions running), floating
  // at different depths (the nearer larger, a little faster, the farther smaller and fainter), and the Starmeter mark
  // when nothing runs; among them many small four-pointed stars, twinkling. They answer the work: while the models
  // write, more of the logos and faster, as the speed rises and falls (eased, never a jump); resting, a few drifting
  // slowly; a turn done, a small burst of stars under its banner. In the night the logos in their own colours, in the
  // light a little softer, in 手绘 in the pen's colours with a ring drawn by an unsteady hand. One canvas, a few dozen
  // things at most, drawn only while the page shows; with reduced motion, one still frame.
  import { onDestroy, onMount, untrack } from 'svelte';
  import type { PhoneLive } from '../lib/phoneLive.svelte';

  type Look = 'light' | 'dark' | 'sketch-light' | 'sketch-dark';
  let { live, look, still = false, asleep = false, burst = 0 }: {
    live: PhoneLive; look: Look; still?: boolean; asleep?: boolean;
    /** A turn done (its moment): a small burst under the banner, once each. */
    burst?: number;
  } = $props();

  let canvas: HTMLCanvasElement | undefined = $state();
  let width = 0, height = 0;
  const ratio = () => Math.min(2, (typeof window !== 'undefined' && window.devicePixelRatio) || 1);
  const sketchy = () => look.startsWith('sketch');
  const dark = () => look === 'dark' || look === 'sketch-dark';

  // The pens of the 手绘 picture, and each look's ink for the stars and the one-colour logos.
  const PENS: Record<'sketch-light' | 'sketch-dark', string[]> = {
    'sketch-light': ['59, 91, 219', '232, 89, 12', '55, 178, 77', '132, 94, 247'],
    'sketch-dark': ['145, 167, 255', '255, 146, 43', '140, 233, 154', '177, 151, 252'],
  };
  const INK: Record<Look, string> = { light: '60, 72, 150', dark: '226, 234, 255', 'sketch-light': '59, 91, 219', 'sketch-dark': '226, 232, 222' };

  // MARK: The logos

  /** The Starmeter mark: the app's dots (24 × 24). */
  const DOTS: [number, number, number][] = [[12, 2.67, 1.58], [7.33, 7.33, 0.95], [12, 7.33, 2.15], [16.67, 7.33, 0.95], [2.67, 12, 1.58], [7.33, 12, 2.15],
    [12, 12, 2.15], [16.67, 12, 2.15], [21.33, 12, 1.58], [7.33, 16.67, 0.95], [12, 16.67, 2.15], [16.67, 16.67, 0.95], [12, 21.33, 1.58]];
  interface Mark { key: string; image?: string; template?: boolean }
  /** The marks floating now: each running session's tool, once each; the Starmeter mark when none runs. */
  function marks(): Mark[] {
    const seen = new Map<string, Mark>();
    for (const s of live.sessions) {
      if (!s.running || !s.image) continue;
      const key = s.image.slice(0, 64) + s.image.length;
      if (!seen.has(key)) seen.set(key, { key, image: s.image, template: s.template });
    }
    return seen.size ? [...seen.values()] : [{ key: 'starmeter' }];
  }
  /** Each mark drawn once (64 pt square) in the look's way, stamped thereafter. Template logos and 手绘 take a colour. */
  const SPRITE = 64;
  const sprites = new Map<string, HTMLCanvasElement | null>();
  function sprite(mark: Mark, tint: string | null): HTMLCanvasElement | null {
    const key = `${mark.key}|${tint ?? ''}`;
    if (sprites.has(key)) return sprites.get(key)!;
    sprites.set(key, null);
    const made = document.createElement('canvas');
    made.width = made.height = Math.round(SPRITE * ratio());
    const c = made.getContext('2d')!;
    c.scale(ratio(), ratio());
    if (!mark.image) {
      c.fillStyle = `rgb(${tint ?? INK[look]})`;
      for (const [cx, cy, r] of DOTS) { c.beginPath(); c.arc((cx / 24) * SPRITE, (cy / 24) * SPRITE, (r / 24) * SPRITE, 0, Math.PI * 2); c.fill(); }
      sprites.set(key, made);
      return made;
    }
    const image = new Image();
    const type = mark.image.startsWith('iVBOR') ? 'image/png' : mark.image.startsWith('/9j/') ? 'image/jpeg' : 'image/svg+xml';
    image.onload = () => {
      c.drawImage(image, 6, 6, SPRITE - 12, SPRITE - 12);
      if (tint || mark.template) {
        c.globalCompositeOperation = 'source-in';
        c.fillStyle = `rgb(${tint ?? INK[look]})`;
        c.fillRect(0, 0, SPRITE, SPRITE);
      }
      sprites.set(key, made);
      // The one still frame (reduced motion) drawn again with it.
      if (still) requestAnimationFrame((time) => tick(time));
    };
    // An SVG with only a viewBox has no size of its own, and WebKit draws such an image as nothing: given one.
    let source = mark.image;
    if (type === 'image/svg+xml') {
      try {
        const text = atob(source);
        if (!/<svg[^>]*\swidth=/.test(text)) source = btoa(text.replace(/<svg(?=[\s>])/, `<svg width="${SPRITE * 2}" height="${SPRITE * 2}"`));
      } catch { /* drawn as it is */ }
    }
    image.src = `data:${type};base64,${source}`;
    return null;
  }

  // MARK: What floats

  interface Thing { kind: 'logo' | 'star'; x: number; y: number; depth: number; size: number; sway: number; phase: number;
    twinkle: number; turn: number; spin: number; shown: number; mark: string; pen: number; wobble: number[] }
  interface Burst { x: number; y: number; vx: number; vy: number; born: number; size: number; pen: number; turn: number }
  let things: Thing[] = [];
  let bursts: Burst[] = [];
  let current: Mark[] = [{ key: 'starmeter' }];
  /** How busy the sky is, 0 (resting) to 1 (writing fast), eased towards the work's own. */
  let energy = 0.1;
  let markTurn = 0;

  const random = (a: number, b: number) => a + Math.random() * (b - a);
  /** A new thing: anywhere on the screen (at the start, faded in where it stands when the marks change), or at the foot
   *  (one that floated off the top). */
  function thing(kind: Thing['kind'], anywhere: boolean, fade = false): Thing {
    const depth = random(0, 1);
    const size = kind === 'logo' ? 11 + 13 * depth : random(1.4, 3.6);
    const mark = kind === 'logo' ? current[markTurn++ % current.length].key : '';
    return { kind, x: random(0, width), y: anywhere ? random(0, height) : height + random(20, 60), depth, size, sway: random(4, 12),
      phase: random(0, Math.PI * 2), twinkle: random(0.5, 1.6), turn: random(-0.25, 0.25), spin: random(-0.08, 0.08), shown: anywhere && !fade ? 1 : 0,
      mark, pen: Math.floor(random(0, 4)), wobble: Array.from({ length: 12 }, () => random(-0.1, 0.1)) };
  }
  /** As many as the screen holds: stars a few dozen, logos small and a score or so (more on a computer's screen). */
  const starsFor = () => Math.max(22, Math.min(44, Math.round((width * height) / 9_000)));
  const logosFor = () => (width >= 1000 ? 32 : width > height ? 24 : 20);

  function size() {
    if (!canvas) return;
    const box = canvas.getBoundingClientRect();
    width = box.width; height = box.height;
    canvas.width = Math.round(width * ratio()); canvas.height = Math.round(height * ratio());
    canvas.getContext('2d')!.setTransform(ratio(), 0, 0, ratio(), 0, 0);
    const stars = things.filter((t) => t.kind === 'star'), logos = things.filter((t) => t.kind === 'logo');
    while (stars.length < starsFor()) stars.push(thing('star', true));
    while (logos.length < logosFor()) logos.push(thing('logo', true));
    things = [...stars.slice(0, starsFor()), ...logos.slice(0, logosFor())];
  }

  // MARK: Drawing

  /** The small four-pointed star: four points; in 手绘 two crossed strokes of the pen. */
  function star(ctx: CanvasRenderingContext2D, t: Pick<Thing, 'x' | 'y' | 'size' | 'turn' | 'wobble'>, alpha: number, rgb: string) {
    if (alpha <= 0.004) return;
    if (sketchy()) {
      ctx.save(); ctx.translate(t.x, t.y); ctx.rotate(t.turn);
      ctx.strokeStyle = `rgba(${rgb}, ${alpha})`; ctx.lineWidth = 1; ctx.lineCap = 'round';
      const r = t.size * 1.6, w = t.wobble;
      ctx.beginPath();
      ctx.moveTo(-r * (1 + w[0]), 0); ctx.lineTo(r * (1 + w[1]), w[2] * r);
      ctx.moveTo(w[3] * r, -r * (1 + w[4])); ctx.lineTo(0, r * (1 + w[5]));
      ctx.stroke(); ctx.restore();
      return;
    }
    const r = t.size;
    ctx.fillStyle = `rgba(${rgb}, ${alpha})`;
    ctx.beginPath();
    for (let i = 0; i < 8; i++) {
      const a = (i * Math.PI) / 4 + t.turn, d = i % 2 ? r * 0.28 : r;
      if (i) ctx.lineTo(t.x + Math.cos(a) * d, t.y + Math.sin(a) * d); else ctx.moveTo(t.x + Math.cos(a) * d, t.y + Math.sin(a) * d);
    }
    ctx.closePath(); ctx.fill();
  }
  /** One logo drawn; whether it was (its picture may still be loading). */
  function logo(ctx: CanvasRenderingContext2D, t: Thing, alpha: number): boolean {
    if (alpha <= 0.004) return false;
    const mark = current.find((m) => m.key === t.mark) ?? (t.mark === 'starmeter' ? { key: 'starmeter' } : null);
    if (!mark) return false;
    const pen = sketchy() ? PENS[look as 'sketch-light' | 'sketch-dark'][t.pen] : null;
    const image = sprite(mark, pen);
    if (!image) return false;
    const s = t.size;
    ctx.save();
    ctx.translate(t.x, t.y); ctx.rotate(t.turn);
    ctx.globalAlpha = alpha;
    ctx.drawImage(image, -s / 2, -s / 2, s, s);
    if (pen) {
      // 手绘: a ring round it, drawn by hand.
      ctx.strokeStyle = `rgb(${pen})`; ctx.lineWidth = 1; ctx.beginPath();
      for (let i = 0; i <= 12; i++) {
        const a = (i / 12) * Math.PI * 2.04, r = s * 0.62 * (1 + t.wobble[i % 12]);
        if (i) ctx.lineTo(Math.cos(a) * r, Math.sin(a) * r); else ctx.moveTo(Math.cos(a) * r, Math.sin(a) * r);
      }
      ctx.stroke();
    }
    ctx.restore();
    return true;
  }

  /** How strong each look's things are: faint, under the figures. */
  const LOGO: Record<Look, number> = { light: 0.26, dark: 0.32, 'sketch-light': 0.28, 'sketch-dark': 0.3 };
  const STAR: Record<Look, number> = { light: 0.3, dark: 0.75, 'sketch-light': 0.35, 'sketch-dark': 0.4 };

  let last = 0, frame = 0, running = false, frames = 0;
  function tick(time: number) {
    running = false;
    if (!canvas || !width) return;
    const dt = last ? Math.min(0.1, (time - last) / 1000) : 0.016;
    last = time;
    // The work's own energy: the speed on a log scale while writing; resting, a little.
    const rate = live.rate();
    const target = live.state === 'working' ? (rate === null ? 0.45 : Math.min(1, Math.max(0.3, 0.3 + Math.log10(1 + rate) / 3.2))) : 0.06;
    energy += (target - energy) * (1 - Math.exp(-dt / 1.6));
    // The marks floating: the tools at work now, or the Starmeter mark; a logo no longer among them fades and comes back
    // as one that is.
    const marksNow = marks();
    if (marksNow.map((m) => m.key).join() !== current.map((m) => m.key).join()) current = marksNow;
    const ctx = canvas.getContext('2d')!;
    ctx.clearRect(0, 0, width, height);
    const ink = INK[look], pens = sketchy() ? PENS[look as 'sketch-light' | 'sketch-dark'] : null;
    const logos = things.filter((t) => t.kind === 'logo');
    const active = Math.round(logos.length * (0.35 + 0.65 * energy)), pace = 0.25 + 1.6 * energy;
    const now = time / 1000;
    let shownLogos = 0, drawn: string[] = [];
    for (const t of things) {
      const index = t.kind === 'logo' ? logos.indexOf(t) : -1;
      const atWork = t.kind === 'star' || current.some((m) => m.key === t.mark);
      const wanted = t.kind === 'star' ? 1 : index < active && atWork ? 1 : 0;
      // Faded in and out over a second; in the one still frame, as they are.
      t.shown += (wanted - t.shown) * (still ? 1 : 1 - Math.exp(-dt / 1.2));
      // Nearer, faster: the logos rise at their depth's pace, the stars barely.
      const rise = t.kind === 'logo' ? 6 + 12 * t.depth : 1.5 + 2 * t.depth;
      t.y -= rise * pace * dt;
      t.x += Math.sin(now * 0.3 + t.phase) * t.sway * dt * 0.5;
      t.turn += t.spin * pace * dt;
      // Gone off the top: back in at the foot. Faded out with a mark no longer at work: one that is, faded in somewhere
      // on the screen (from the foot it would take a minute to be seen).
      if (t.y < -40) Object.assign(t, thing(t.kind, false));
      else if (!atWork && t.shown < 0.02) Object.assign(t, thing(t.kind, true, !still));
      if (t.kind === 'star') {
        const glint = 0.3 + 0.7 * (0.5 + 0.5 * Math.sin(now * t.twinkle * (1 + energy) + t.phase));
        star(ctx, t, STAR[look] * glint * (0.5 + 0.5 * t.depth), pens ? pens[t.pen] : ink);
      } else {
        if (t.shown > 0.5) shownLogos++;
        const clear = t.y > 150 && t.y < height - 160;
        if (logo(ctx, t, LOGO[look] * t.shown * (0.55 + 0.45 * t.depth)) && clear && t.shown > 0.5) drawn.push(t.mark === 'starmeter' ? 'starmeter' : 'tool');
      }
    }
    // A turn done: stars out from under the banner, slowing and fading.
    bursts = bursts.filter((b) => now - b.born < 1.6);
    for (const b of bursts) {
      const age = now - b.born, slow = Math.exp(-age * 2.2);
      b.x += b.vx * slow * dt; b.y += b.vy * slow * dt; b.turn += dt * 2;
      star(ctx, { x: b.x, y: b.y, size: b.size, turn: b.turn, wobble: [0, 0, 0, 0, 0, 0] },
        Math.min(1, STAR[look] * 1.4) * (1 - age / 1.6), pens ? pens[b.pen] : dark() ? '255, 236, 170' : '232, 89, 12');
    }
    frames++;
    canvas.dataset.energy = energy.toFixed(2);
    canvas.dataset.active = String(active);
    canvas.dataset.frames = String(frames);
    canvas.dataset.bursts = String(bursts.length);
    canvas.dataset.marks = current.map((m) => (m.image ? 'tool' : 'starmeter')).join(',');
    canvas.dataset.logos = String(shownLogos);
    // The logos seen this frame (drawn, faded in, on the sky's clear middle, not under its faded top or foot), by kind:
    // a tool's, or the Starmeter mark.
    canvas.dataset.drawn = [...new Set(drawn)].sort().join(',');
    canvas.dataset.drawnCount = String(drawn.length);
    canvas.dataset.stars = String(things.filter((t) => t.kind === 'star').length);
    queue();
  }
  /** Resting, half the frames: the sky barely moves, the phone saves its power. */
  let skip = false;
  function queue() {
    if (running || still || asleep || !canvas) return;
    running = true;
    frame = requestAnimationFrame((time) => {
      if (energy < 0.15 && (skip = !skip)) { running = false; queue(); return; }
      tick(time);
    });
  }

  onMount(() => {
    size();
    const observer = new ResizeObserver(() => { size(); if (still) tick(performance.now()); });
    if (canvas) observer.observe(canvas);
    return () => observer.disconnect();
  });
  onDestroy(() => cancelAnimationFrame(frame));
  // Started and stopped as the page shows and hides; with reduced motion, one still frame.
  $effect(() => {
    void asleep; void still; void look;
    untrack(() => {
      last = 0;
      if (still) { cancelAnimationFrame(frame); running = false; energy = 0.1; requestAnimationFrame((time) => tick(time)); }
      else if (asleep) { cancelAnimationFrame(frame); running = false; }
      else queue();
    });
  });
  // The tools at work changed: in the one still frame (reduced motion), drawn again with theirs.
  let marksSeen = '';
  $effect(() => {
    const now = marks().map((m) => m.key).join();
    untrack(() => {
      if (now === marksSeen) return;
      // The first time, the still frame is being drawn anyway.
      const first = !marksSeen;
      marksSeen = now;
      if (first) return;
      if (still && canvas) requestAnimationFrame((time) => tick(time));
    });
  });
  // The look changed: the marks drawn again in its way.
  $effect(() => { void look; untrack(() => sprites.clear()); });
  let burstSeen = 0;
  $effect(() => {
    const at = burst;
    untrack(() => {
      if (!at || at === burstSeen || still) return;
      burstSeen = at;
      const now = performance.now() / 1000, x = width / 2, y = Math.min(110, height * 0.15);
      for (let i = 0; i < 18; i++) {
        const a = (i / 18) * Math.PI * 2 + random(-0.15, 0.15), speed = random(70, 150);
        bursts.push({ x, y, vx: Math.cos(a) * speed, vy: Math.sin(a) * speed * 0.7 + 20, born: now, size: random(2.5, 4.5), pen: i % 4, turn: random(0, 6) });
      }
      queue();
    });
  });
</script>

<canvas class="drift" bind:this={canvas} aria-hidden="true"></canvas>

<style>
  .drift {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    pointer-events: none;
    /* Faded at the top and the foot as the lights are, so the browser's bars keep their flat colour. */
    mask-image: linear-gradient(180deg, transparent 0, transparent 40px, #000 150px, #000 calc(100% - 160px), transparent calc(100% - 60px));
    -webkit-mask-image: linear-gradient(180deg, transparent 0, transparent 40px, #000 150px, #000 calc(100% - 160px), transparent calc(100% - 60px));
  }
</style>
