<script lang="ts">
  // 网页版's sky, alive: behind everything, faint, the logos of the tools at work now (the sessions running), floating
  // at different depths (the nearer larger, a little faster, the farther smaller and fainter), and the Starmeter mark
  // when nothing runs; among them many small four-pointed stars, twinkling. They answer the work: while the models
  // write, more of the logos and faster, as the speed rises and falls (eased, never a jump); resting, a few drifting
  // slowly; a turn done, a small burst of stars under its banner. A tap or click on the sky itself (not on a control or
  // a pane) sends a little burst of stars out from there and a faint ring, and the logos near it are pushed gently
  // away, drifting back over a couple of seconds; a mouse over the sky makes the stars near it glint. In the night the
  // logos in their own colours, in the light a little softer, in 手绘 in the pen's colours with a ring drawn by an
  // unsteady hand.
  // Every frame at the screen's own rate, at the same pace whatever the frames take (the time between them moves
  // things): each logo and star is drawn once into a small picture (with its ring, its glow) and stamped thereafter, a
  // transform and an opacity each; no paths, gradients, shadows or masks per frame (the faded top and foot are the
  // opacity too), a few dozen things at most, nothing read from the page while drawing. Only while the page shows; with
  // reduced motion one still frame and no touch.
  import { onDestroy, onMount, untrack } from 'svelte';
  import type { PhoneLive } from '../lib/phoneLive.svelte';

  type Look = 'light' | 'dark' | 'sketch-light' | 'sketch-dark';
  let { live, look, still = false, asleep = false, burst = 0, overlay }: {
    live: PhoneLive; look: Look; still?: boolean; asleep?: boolean;
    /** A turn done (its moment): a small burst under the banner, once each. */
    burst?: number;
    /** Over the page (PhoneStage places it, above the panes, letting every touch through): a tap's stars, its ring and
     *  the light on a curve are drawn there, so a tap on a card or the figures shows over them, not behind. */
    overlay?: HTMLCanvasElement;
  } = $props();

  let canvas: HTMLCanvasElement | undefined = $state();
  let width = 0, height = 0, ratio = 1;
  let canvasLeft = 0, canvasTop = 0;
  /** Cached from live.version so 60fps tick() never recomputes smoothRates() on every frame (PERF-1). */
  let cachedRate: number | null = null;
  let cachedWorking = false;
  $effect(() => {
    void live.version;
    cachedWorking = live.state === 'working';
    cachedRate = untrack(() => live.rate());
  });
  const setAttr = (d: DOMStringMap, key: string, value: string) => { if (d[key] !== value) d[key] = value; };
  const sketchy = () => look.startsWith('sketch');
  const dark = () => look === 'dark' || look === 'sketch-dark';

  // The pens of the 手绘 picture, and each look's ink for the stars and the one-colour logos.
  const PENS: Record<'sketch-light' | 'sketch-dark', string[]> = {
    'sketch-light': ['59, 91, 219', '232, 89, 12', '55, 178, 77', '132, 94, 247'],
    'sketch-dark': ['145, 167, 255', '255, 146, 43', '140, 233, 154', '177, 151, 252'],
  };
  const INK: Record<Look, string> = { light: '60, 72, 150', dark: '226, 234, 255', 'sketch-light': '59, 91, 219', 'sketch-dark': '226, 232, 222' };
  const pens = () => (sketchy() ? PENS[look as 'sketch-light' | 'sketch-dark'] : null);
  /** A burst's stars: warm by night, the pens in 手绘, a deep orange by day. */
  const burstInk = (pen: number) => pens()?.[pen] ?? (dark() ? '255, 236, 170' : '232, 89, 12');

  // MARK: The pictures, each drawn once

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

  /** A blank picture `size` points square at the screen's density, drawn on in points. */
  function blank(size: number): [HTMLCanvasElement, CanvasRenderingContext2D] {
    const made = document.createElement('canvas');
    made.width = made.height = Math.ceil(size * ratio);
    const c = made.getContext('2d')!;
    c.scale(ratio, ratio);
    return [made, c];
  }

  /** A logo's picture: 64 points, the logo the middle 40 of them (template logos and 手绘 in a colour), and in 手绘 a ring
   *  round it by an unsteady hand (each pen's a little different). */
  const BOX = 64, GLYPH = 40;
  const sprites = new Map<string, HTMLCanvasElement | null>();
  /** Counted up each time the drawn marks are let go (a new look, a new pixel ratio): a logo still loading for an
   *  older one is drawn into nothing (STAGE-19: it was kept, in the old colour or at the old density). */
  let spriteRound = 0;
  function clearSprites() { sprites.clear(); spriteRound++; }
  function ring(c: CanvasRenderingContext2D, rgb: string, seed: number) {
    c.strokeStyle = `rgb(${rgb})`; c.lineWidth = 1.6; c.lineCap = 'round'; c.beginPath();
    for (let i = 0; i <= 14; i++) {
      const a = (i / 14) * Math.PI * 2.08 + seed, r = 26 * (1 + 0.08 * Math.sin(i * 2.3 + seed * 5));
      const x = BOX / 2 + Math.cos(a) * r, y = BOX / 2 + Math.sin(a) * r;
      if (i) c.lineTo(x, y); else c.moveTo(x, y);
    }
    c.stroke();
  }
  function logoSprite(mark: Mark, pen: number | null): HTMLCanvasElement | null {
    const tint = pen === null ? null : pens()![pen];
    const key = `${look}|${mark.key}|${pen ?? ''}`, ink = INK[look], round = spriteRound;
    if (sprites.has(key)) return sprites.get(key)!;
    sprites.set(key, null);
    const [made, c] = blank(BOX);
    const at = (BOX - GLYPH) / 2;
    if (!mark.image) {
      c.fillStyle = `rgb(${tint ?? ink})`;
      for (const [cx, cy, r] of DOTS) { c.beginPath(); c.arc(at + (cx / 24) * GLYPH, at + (cy / 24) * GLYPH, (r / 24) * GLYPH, 0, Math.PI * 2); c.fill(); }
      if (tint) ring(c, tint, pen!);
      sprites.set(key, made);
      return made;
    }
    const image = new Image();
    const type = mark.image.startsWith('iVBOR') ? 'image/png' : mark.image.startsWith('/9j/') ? 'image/jpeg' : 'image/svg+xml';
    image.onload = () => {
      if (round !== spriteRound) return;
      c.drawImage(image, at, at, GLYPH, GLYPH);
      if (tint || mark.template) {
        c.globalCompositeOperation = 'source-in';
        c.fillStyle = `rgb(${tint ?? ink})`;
        c.fillRect(0, 0, BOX, BOX);
        c.globalCompositeOperation = 'source-over';
      }
      if (tint) ring(c, tint, pen!);
      sprites.set(key, made);
      // The one still frame (reduced motion) drawn again with it.
      if (still) requestAnimationFrame((time) => tick(time));
    };
    // An SVG with only a viewBox has no size of its own, and WebKit draws such an image as nothing: given one.
    let source = mark.image;
    if (type === 'image/svg+xml') {
      try {
        const text = atob(source);
        if (!/<svg[^>]*\swidth=/.test(text)) source = btoa(text.replace(/<svg(?=[\s>])/, `<svg width="${BOX * 2}" height="${BOX * 2}"`));
      } catch { /* drawn as it is */ }
    }
    image.src = `data:${type};base64,${source}`;
    return null;
  }

  /** A star's picture: 24 points, a four-pointed star of radius 9 (by night with a faint glow round it); in 手绘 two
   *  crossed strokes of the pen, three hands of them. */
  const STAR = 24, POINT = 9;
  function starSprite(rgb: string, hand: number): HTMLCanvasElement {
    const key = `star|${look}|${rgb}|${hand}`;
    const kept = sprites.get(key);
    if (kept) return kept;
    const [made, c] = blank(STAR);
    const m = STAR / 2;
    if (sketchy()) {
      const w = [0.08, -0.06, 0.1, -0.09, 0.05, 0.07].map((v, i) => v * (1 + ((hand + i) % 3) * 0.4));
      c.strokeStyle = `rgb(${rgb})`; c.lineWidth = 1.5; c.lineCap = 'round';
      c.beginPath();
      c.moveTo(m - POINT * (1 + w[0]), m + w[1] * POINT); c.lineTo(m + POINT * (1 + w[2]), m - w[1] * POINT);
      c.moveTo(m + w[3] * POINT, m - POINT * (1 + w[4])); c.lineTo(m - w[3] * POINT, m + POINT * (1 + w[5]));
      c.stroke();
    } else {
      if (dark()) {
        const glow = c.createRadialGradient(m, m, 0, m, m, m);
        glow.addColorStop(0, `rgba(${rgb}, 0.35)`); glow.addColorStop(1, `rgba(${rgb}, 0)`);
        c.fillStyle = glow; c.fillRect(0, 0, STAR, STAR);
      }
      c.fillStyle = `rgb(${rgb})`;
      c.beginPath();
      for (let i = 0; i < 8; i++) {
        const a = (i * Math.PI) / 4, d = i % 2 ? POINT * 0.28 : POINT;
        if (i) c.lineTo(m + Math.cos(a) * d, m + Math.sin(a) * d); else c.moveTo(m + Math.cos(a) * d, m + Math.sin(a) * d);
      }
      c.closePath(); c.fill();
    }
    sprites.set(key, made);
    return made;
  }
  /** A tap's ring, drawn once (radius 30 in a 64-point picture), stamped larger as it spreads. */
  function ringSprite(): HTMLCanvasElement {
    const key = `ring|${look}`;
    const kept = sprites.get(key);
    if (kept) return kept;
    const [made, c] = blank(64);
    c.strokeStyle = `rgb(${pens()?.[0] ?? INK[look]})`; c.lineWidth = sketchy() ? 1.4 : 1.1;
    c.beginPath(); c.arc(32, 32, 30, 0, Math.PI * 2); c.stroke();
    sprites.set(key, made);
    return made;
  }

  // MARK: What floats

  interface Thing { kind: 'logo' | 'star'; x: number; y: number; depth: number; size: number; sway: number; phase: number;
    twinkle: number; turn: number; spin: number; shown: number; mark: string; pen: number;
    /** Pushed by a tap: how far off its course, and how fast it is moving there; and a star's glint by the mouse. */
    ox: number; oy: number; vx: number; vy: number; glint: number }
  interface Spark { x: number; y: number; vx: number; vy: number; born: number; life: number; size: number; pen: number; turn: number; spin: number;
    /** A tap's, on the overlay over the page; a turn's, in the sky. */
    top: boolean }
  interface Ripple { x: number; y: number; born: number }
  /** A tap on a curve: a light up it there, from the plot's top to its foot, fading. */
  interface Beam { x: number; top: number; bottom: number; born: number }
  let things: Thing[] = [];
  let sparks: Spark[] = [];
  let ripples: Ripple[] = [];
  let beams: Beam[] = [];
  /** Something was drawn on the overlay last frame (it is cleared once when it is over). */
  let overlayDrawn = false;
  let current: Mark[] = [{ key: 'starmeter' }];
  /** How busy the sky is, 0 (resting) to 1 (writing fast), eased towards the work's own. */
  let energy = 0.1;
  let markTurn = 0;
  let pokes = 0;
  /** Where the mouse is over the sky (canvas points), for the stars near it to glint. */
  let pointer: { x: number; y: number } | null = null;

  const random = (a: number, b: number) => a + Math.random() * (b - a);
  /** A new thing: anywhere on the screen (at the start, faded in where it stands when the marks change), or at the foot
   *  (one that floated off the top). */
  function thing(kind: Thing['kind'], anywhere: boolean, fade = false): Thing {
    const depth = random(0, 1);
    const size = kind === 'logo' ? 11 + 13 * depth : random(1.4, 3.6);
    const mark = kind === 'logo' ? current[markTurn++ % current.length].key : '';
    return { kind, x: random(0, width), y: anywhere ? random(0, height) : height + random(20, 60), depth, size, sway: random(4, 12),
      phase: random(0, Math.PI * 2), twinkle: random(0.5, 1.6), turn: random(-0.25, 0.25), spin: random(-0.14, 0.14), shown: anywhere && !fade ? 1 : 0,
      mark, pen: Math.floor(random(0, 4)), ox: 0, oy: 0, vx: 0, vy: 0, glint: 0 };
  }
  /** As many as the screen holds: stars a few dozen, logos small and a score or so (more on a computer's screen). */
  const starsFor = () => Math.max(22, Math.min(44, Math.round((width * height) / 9_000)));
  const logosFor = () => (width >= 1000 ? 32 : width > height ? 24 : 20);
  /** The most of a burst's stars at once, however fast the taps come. */
  const SPARKS = 140;

  function size() {
    if (!canvas) return;
    const box = canvas.getBoundingClientRect();
    canvasLeft = box.left; canvasTop = box.top;
    width = box.width; height = box.height;
    const next = Math.min(2, window.devicePixelRatio || 1);
    if (next !== ratio) { ratio = next; clearSprites(); }
    canvas.width = Math.round(width * ratio); canvas.height = Math.round(height * ratio);
    if (overlay) { overlay.width = canvas.width; overlay.height = canvas.height; overlayDrawn = false; }
    const stars = things.filter((t) => t.kind === 'star'), logos = things.filter((t) => t.kind === 'logo');
    while (stars.length < starsFor()) stars.push(thing('star', true));
    while (logos.length < logosFor()) logos.push(thing('logo', true));
    things = [...stars.slice(0, starsFor()), ...logos.slice(0, logosFor())];
  }

  // MARK: Drawing

  /** Faded at the top and the foot (the browser's bars keep their flat colour): nothing above 40 points or in the last
   *  60, whole from 150 down to 160 above the foot. */
  function edgeFade(y: number) {
    const k = Math.min(1, Math.max(0, Math.min((y - 40) / 110, (height - 60 - y) / 100)));
    return k * k * (3 - 2 * k);
  }
  /** One picture stamped at (x, y), turned, `across` points wide, at an opacity; whether it was. */
  function stamp(ctx: CanvasRenderingContext2D, image: HTMLCanvasElement, x: number, y: number, turn: number, across: number, alpha: number) {
    if (alpha <= 0.004) return false;
    const cos = Math.cos(turn) * ratio, sin = Math.sin(turn) * ratio;
    ctx.setTransform(cos, sin, -sin, cos, x * ratio, y * ratio);
    ctx.globalAlpha = Math.min(1, alpha);
    ctx.drawImage(image, -across / 2, -across / 2, across, across);
    return true;
  }

  /** How strong each look's things are: faint, under the figures. */
  const LOGO: Record<Look, number> = { light: 0.26, dark: 0.32, 'sketch-light': 0.28, 'sketch-dark': 0.3 };
  const STARS: Record<Look, number> = { light: 0.3, dark: 0.75, 'sketch-light': 0.35, 'sketch-dark': 0.4 };
  /** A pushed thing's way back: a soft spring, a little under-damped, about a second and a half to settle. */
  const SPRING = 4.5, DAMPING = 3.2;
  /** How fast things rise (points a second): a logo by its depth, a star barely; times the sky's pace, from about 0.6
   *  resting (a logo 8 to 19 points a second: plainly drifting, not standing) to 2 writing fast (up to about 60). */
  const RISE = { logo: [12, 18], star: [3, 5] } as const;
  const paceAt = (energy: number) => 0.55 + 1.5 * energy;

  let last = 0, frame = 0, running = false, frames = 0;
  function tick(time: number) {
    running = false;
    if (!canvas || !width) return;
    const dt = last ? Math.min(0.1, (time - last) / 1000) : 0.016;
    last = time;
    // The work's own energy: the speed on a log scale while writing; resting, a little.
    const rate = cachedRate;
    const target = cachedWorking ? (rate === null ? 0.45 : Math.min(1, Math.max(0.3, 0.3 + Math.log10(1 + rate) / 3.2))) : 0.06;
    energy += (target - energy) * (1 - Math.exp(-dt / 1.6));
    // The marks floating: the tools at work now, or the Starmeter mark; a logo no longer among them fades and comes back
    // as one that is.
    const marksNow = marks();
    if (marksNow.map((m) => m.key).join() !== current.map((m) => m.key).join()) current = marksNow;
    const ctx = canvas.getContext('2d')!;
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.globalAlpha = 1;
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    const ink = INK[look], pen = pens();
    const logos = things.filter((t) => t.kind === 'logo');
    const active = Math.round(logos.length * (0.35 + 0.65 * energy)), pace = paceAt(energy);
    const now = time / 1000;
    let shownLogos = 0, pushed = 0;
    const drawn: string[] = [];
    for (const t of things) {
      const atWork = t.kind === 'star' || current.some((m) => m.key === t.mark);
      const wanted = t.kind === 'star' ? 1 : logos.indexOf(t) < active && atWork ? 1 : 0;
      // Faded in and out over a second; in the one still frame, as they are.
      t.shown += (wanted - t.shown) * (still ? 1 : 1 - Math.exp(-dt / 1.2));
      // Nearer, faster: the logos rise at their depth's pace, the stars barely.
      const [from, by] = RISE[t.kind];
      t.y -= (from + by * t.depth) * pace * dt;
      t.x += Math.sin(now * 0.45 + t.phase) * t.sway * dt * 0.6;
      t.turn += t.spin * pace * dt;
      if (t.ox || t.oy || t.vx || t.vy) {
        t.vx += (-SPRING * t.ox - DAMPING * t.vx) * dt; t.vy += (-SPRING * t.oy - DAMPING * t.vy) * dt;
        t.ox += t.vx * dt; t.oy += t.vy * dt;
        if (Math.abs(t.ox) + Math.abs(t.oy) < 0.05 && Math.abs(t.vx) + Math.abs(t.vy) < 0.05) t.ox = t.oy = t.vx = t.vy = 0;
        pushed = Math.max(pushed, Math.hypot(t.ox, t.oy));
      }
      // Gone off the top: back in at the foot. Faded out with a mark no longer at work: one that is, faded in somewhere
      // on the screen (from the foot it would take a minute to be seen).
      if (t.y < -40) Object.assign(t, thing(t.kind, false));
      else if (!atWork && t.shown < 0.02) Object.assign(t, thing(t.kind, true, !still));
      const x = t.x + t.ox, y = t.y + t.oy, fade = edgeFade(y);
      if (t.kind === 'star') {
        // The mouse near: a glint, eased in and out.
        const near = pointer ? Math.max(0, 1 - Math.hypot(x - pointer.x, y - pointer.y) / 90) : 0;
        t.glint += (near - t.glint) * (1 - Math.exp(-dt / 0.25));
        const glint = 0.3 + 0.7 * (0.5 + 0.5 * Math.sin(now * t.twinkle * (1 + energy) + t.phase));
        stamp(ctx, starSprite(pen ? pen[t.pen] : ink, t.pen % 3), x, y, t.turn, (t.size / POINT) * STAR * (1 + 0.5 * t.glint),
          STARS[look] * glint * (0.5 + 0.5 * t.depth) * fade * (1 + 0.8 * t.glint));
      } else {
        if (t.shown > 0.5) shownLogos++;
        const mark = current.find((m) => m.key === t.mark) ?? (t.mark === 'starmeter' ? { key: 'starmeter' } : null);
        const sprite = mark ? logoSprite(mark, pen ? t.pen : null) : null;
        const clear = y > 150 && y < height - 160;
        if (sprite && stamp(ctx, sprite, x, y, t.turn, (t.size / GLYPH) * BOX, LOGO[look] * t.shown * (0.55 + 0.45 * t.depth) * fade) && clear && t.shown > 0.5)
          drawn.push(t.mark === 'starmeter' ? 'starmeter' : 'tool');
      }
    }
    // Bursts (a turn done in the sky; a tap's over the page): stars out, slowing and fading; a tap's faint ring
    // spreading, and on a curve a light up it.
    sparks = sparks.filter((s) => now - s.born < s.life);
    ripples = ripples.filter((r) => now - r.born < 0.9);
    beams = beams.filter((b) => now - b.born < 0.7);
    const top = overlay?.getContext('2d') ?? null;
    if (top && (overlayDrawn || sparks.some((s) => s.top) || ripples.length || beams.length)) {
      top.setTransform(1, 0, 0, 1, 0, 0);
      top.clearRect(0, 0, overlay!.width, overlay!.height);
      overlayDrawn = false;
    }
    for (const s of sparks) {
      const age = now - s.born, slow = Math.exp(-age * 2.4), left = 1 - age / s.life;
      s.x += s.vx * slow * dt; s.y += s.vy * slow * dt + 6 * dt; s.turn += s.spin * dt;
      const on = s.top && top ? top : ctx;
      // Over the panes a little stronger than in the sky, to be seen on them.
      const strength = Math.min(1, STARS[look] * (on === ctx ? 1.5 : 2.4));
      if (stamp(on, starSprite(burstInk(s.pen), s.pen % 3), s.x, s.y, s.turn, (s.size / POINT) * STAR, strength * left * left) && on !== ctx) overlayDrawn = true;
    }
    for (const r of ripples) {
      const k = (now - r.born) / 0.9;
      if (stamp(top ?? ctx, ringSprite(), r.x, r.y, 0, 12 + 110 * (1 - Math.pow(1 - k, 3)), (sketchy() ? 0.55 : 0.45) * (1 - k)) && top) overlayDrawn = true;
    }
    if (top && beams.length) {
      for (const b of beams) {
        const k = (now - b.born) / 0.7, ink = pens()?.[0] ?? INK[look];
        top.setTransform(ratio, 0, 0, ratio, 0, 0);
        top.globalAlpha = 1;
        const light = top.createLinearGradient(0, b.top, 0, b.bottom);
        light.addColorStop(0, `rgba(${ink}, 0)`); light.addColorStop(0.5, `rgba(${ink}, ${0.5 * (1 - k)})`); light.addColorStop(1, `rgba(${ink}, 0)`);
        top.fillStyle = light;
        const w = 1.5 + 10 * k;
        top.fillRect(b.x - w / 2, b.top, w, b.bottom - b.top);
        overlayDrawn = true;
      }
      top.setTransform(1, 0, 0, 1, 0, 0);
      top.globalAlpha = 1;
    }
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.globalAlpha = 1;
    frames++;
    const d = canvas.dataset;
    setAttr(d, 'energy', energy.toFixed(2));
    setAttr(d, 'active', String(active));
    setAttr(d, 'frames', String(frames));
    setAttr(d, 'bursts', String(sparks.length));
    setAttr(d, 'ripples', String(ripples.length));
    setAttr(d, 'pokes', String(pokes));
    setAttr(d, 'tap', lastTap);
    setAttr(d, 'overlay', String(overlayDrawn));
    // A logo halfway deep: how fast it rises now, points a second.
    setAttr(d, 'rise', ((RISE.logo[0] + RISE.logo[1] / 2) * pace).toFixed(1));
    setAttr(d, 'pushed', pushed.toFixed(1));
    setAttr(d, 'marks', current.map((m) => (m.image ? 'tool' : 'starmeter')).join(','));
    setAttr(d, 'logos', String(shownLogos));
    // The logos seen this frame (drawn, faded in, on the sky's clear middle, not under its faded top or foot), by kind:
    // a tool's, or the Starmeter mark.
    setAttr(d, 'drawn', [...new Set(drawn)].sort().join(','));
    setAttr(d, 'drawnCount', String(drawn.length));
    setAttr(d, 'stars', String(things.filter((t) => t.kind === 'star').length));
    // Every frame, resting too: the drift is what the page shows while the AI rests, and at a lower rate it stuttered
    // (用户：漂浮物的帧率特别特别低，不可以).
    queue();
  }
  function queue() {
    if (running || still || asleep || !canvas) return;
    running = true;
    frame = requestAnimationFrame(tick);
  }

  /** Stars out from (x, y): a turn's burst (in the sky), or a tap's (`top`: over the page). */
  function scatter(x: number, y: number, count: number, speed: [number, number], squash: number, top = false) {
    const now = performance.now() / 1000;
    for (let i = 0; i < count; i++) {
      const a = (i / count) * Math.PI * 2 + random(-0.2, 0.2), v = random(...speed);
      sparks.push({ x, y, vx: Math.cos(a) * v, vy: Math.sin(a) * v * squash, born: now, life: random(0.9, 1.5), size: random(1.8, 3.6),
        pen: i % 4, turn: random(0, 6), spin: random(-3, 3), top });
    }
    if (sparks.length > SPARKS) sparks = sparks.slice(sparks.length - SPARKS);
  }

  // MARK: Touch

  /** What does its own thing with a tap, and nothing more: the controls (a card's or a session's head that opens it,
   *  the choices, the buttons and links). Everything else is the sky's to answer: the sky itself, a pane's blank and its
   *  words, the figures, the curves, a running session's card, 动态's rows (the user: 鼠标能点击的地方好少，只有空白处). */
  const OWNED = ['button', 'a', 'input', 'select', 'textarea', 'label', 'summary', '[contenteditable]', '[tabindex]:not([tabindex="-1"])',
    ...['button', 'radio', 'radiogroup', 'tab', 'link', 'switch', 'checkbox', 'menuitem', 'option', 'slider'].map((role) => `[role="${role}"]`)].join(', ');
  let press: { id: number; x: number; y: number; at: number } | null = null;
  function onSky(target: EventTarget | null) {
    const element = target instanceof Element ? target : null;
    return !!element && !!canvas && !!element.closest('.stage') && !element.closest(OWNED);
  }
  /** Played once again on an element (its CSS answers `.tapped`): the class off, a reflow, on, and off a second later
   *  (its own timer, begun again with each tap: not the end of any animation inside, a chip's or a card's coming in). */
  const tapTimers = new WeakMap<Element, ReturnType<typeof setTimeout>>();
  function replay(element: Element) {
    clearTimeout(tapTimers.get(element));
    element.classList.remove('tapped');
    void (element as HTMLElement).offsetWidth;
    element.classList.add('tapped');
    tapTimers.set(element, setTimeout(() => element.classList.remove('tapped'), 1000));
  }
  /** A tap at (x, y) on the screen: a little burst there over the page, a faint ring, the logos near it pushed away; and
   *  where the tap fell says a little more of itself (`data-tap`): the big figure hops, a curve lights up where it was
   *  tapped, a running session's mark turns once. */
  function poke(clientX: number, clientY: number, target: Element | null = null) {
    if (!canvas || still) return;
    const box = canvas.getBoundingClientRect();
    const x = clientX - box.left, y = clientY - box.top;
    pokes++;
    scatter(x, y, 12, [50, 130], 1, !!overlay);
    ripples.push({ x, y, born: performance.now() / 1000 });
    if (ripples.length > 6) ripples.shift();
    const own = target?.closest<HTMLElement>('[data-tap]');
    if (own) {
      const kind = own.dataset.tap;
      if (kind === 'beam') {
        const plot = own.getBoundingClientRect();
        beams.push({ x, top: plot.top - box.top, bottom: plot.bottom - box.top, born: performance.now() / 1000 });
        if (beams.length > 4) beams.shift();
      } else replay(own);
      lastTap = kind ?? '';
    }
    for (const t of things) {
      const dx = t.x + t.ox - x, dy = t.y + t.oy - y, d = Math.hypot(dx, dy), reach = t.kind === 'logo' ? 160 : 110;
      if (d >= reach || d < 0.5) continue;
      const push = (1 - d / reach) ** 2 * (t.kind === 'logo' ? 190 : 90);
      t.vx += (dx / d) * push; t.vy += (dy / d) * push;
    }
    queue();
  }
  /** The kind of the last tap's own answer, for the tests. */
  let lastTap = '';
  function down(event: PointerEvent) {
    // The main button only: a right-click (its menu) or a middle one is no tap on the sky.
    if (still || !event.isPrimary || (event.pointerType === 'mouse' && event.button !== 0) || !onSky(event.target)) { press = null; return; }
    press = { id: event.pointerId, x: event.clientX, y: event.clientY, at: performance.now() };
  }
  function up(event: PointerEvent) {
    const p = press;
    press = null;
    // A tap, not a scroll or a drag: let go where it went down, soon (a long press selects words, and is no tap); and no
    // words left selected by it.
    if (!p || p.id !== event.pointerId || Math.hypot(event.clientX - p.x, event.clientY - p.y) > 10 || performance.now() - p.at > 700) return;
    if ((window.getSelection()?.toString() ?? '').trim()) return;
    poke(event.clientX, event.clientY, event.target instanceof Element ? event.target : null);
  }
  function move(event: PointerEvent) {
    if (event.pointerType !== 'mouse' || still || !canvas) { pointer = null; return; }
    pointer = onSky(event.target) ? { x: event.clientX - canvasLeft, y: event.clientY - canvasTop } : null;
  }
  const cancel = () => { press = null; };
  const leave = () => { pointer = null; };

  onMount(() => {
    size();
    const observer = new ResizeObserver(() => { size(); if (still) tick(performance.now()); });
    if (canvas) observer.observe(canvas);
    // Listened to on the page, never in the way: the controls take their taps as ever, at once.
    const options = { passive: true } as const;
    window.addEventListener('pointerdown', down, options);
    window.addEventListener('pointerup', up, options);
    window.addEventListener('pointercancel', cancel, options);
    window.addEventListener('pointermove', move, options);
    document.documentElement.addEventListener('pointerleave', leave, options);
    return () => {
      observer.disconnect();
      window.removeEventListener('pointerdown', down);
      window.removeEventListener('pointerup', up);
      window.removeEventListener('pointercancel', cancel);
      window.removeEventListener('pointermove', move);
      document.documentElement.removeEventListener('pointerleave', leave);
    };
  });
  onDestroy(() => cancelAnimationFrame(frame));
  // Started and stopped as the page shows and hides; with reduced motion, one still frame.
  $effect(() => {
    void asleep; void still; void look;
    untrack(() => {
      last = 0;
      if (still) { cancelAnimationFrame(frame); running = false; energy = 0.1; sparks = []; ripples = []; beams = []; requestAnimationFrame((time) => tick(time)); }
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
  // The overlay placed: as large as the sky.
  $effect(() => { void overlay; untrack(() => size()); });
  // The look changed: the pictures drawn again in its way.
  $effect(() => { void look; untrack(() => clearSprites()); });
  let burstSeen = 0;
  $effect(() => {
    const at = burst;
    untrack(() => {
      if (!at || at === burstSeen || still) return;
      burstSeen = at;
      scatter(width / 2, Math.min(110, height * 0.15), 18, [70, 150], 0.7);
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
  }
</style>
