<script lang="ts">
  // 网页版's sky, alive: things floating in it, faint, behind everything. In the light the sky's soft glows; at night
  // the website's stars (points that twinkle, now and then the logo's four-pointed sparkle); in 手绘 the pen's doodles
  // on the paper or the board (little stars, sparks, circles), drawn by an unsteady hand. They answer the work: while
  // the models write, more of them and faster, as the speed rises and falls (smoothly, never a jump); resting, a few
  // drifting slowly; a turn done, a small burst under its banner. One canvas, a few dozen at most, drawn only while the
  // page shows; with reduced motion, one still frame.
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

  // Palettes: the sky's own light colours, the website's starlight, the 手绘 picture's pens.
  const PALETTES: Record<Look, string[]> = {
    light: ['140, 150, 255', '110, 205, 185', '190, 150, 255', '255, 170, 205'],
    dark: ['235, 242, 255', '196, 216, 255', '170, 185, 255'],
    'sketch-light': ['59, 91, 219', '232, 89, 12', '55, 178, 77', '132, 94, 247'],
    'sketch-dark': ['145, 167, 255', '255, 146, 43', '140, 233, 154', '177, 151, 252'],
  };

  type Kind = 'glow' | 'point' | 'sparkle' | 'star' | 'spark' | 'ring';
  interface Mote { x: number; y: number; size: number; rise: number; sway: number; phase: number; twinkle: number; colour: number; kind: Kind; turn: number; spin: number; shown: number; wobble: number[] }
  interface Burst { x: number; y: number; vx: number; vy: number; born: number; colour: number; kind: Kind; size: number; turn: number }
  let motes: Mote[] = [];
  let bursts: Burst[] = [];
  /** How busy the sky is, 0 (resting) to 1 (writing fast), eased towards the work's own. */
  let energy = 0.1;

  const random = (a: number, b: number) => a + Math.random() * (b - a);
  const sketchy = () => look.startsWith('sketch');
  function kindFor(): Kind {
    if (sketchy()) return (['star', 'spark', 'ring'] as const)[Math.floor(Math.random() * 3)];
    if (look === 'dark') return Math.random() < 0.12 ? 'sparkle' : 'point';
    return 'glow';
  }
  function mote(anywhere: boolean): Mote {
    const kind = kindFor();
    const size = kind === 'glow' ? random(7, 20) : kind === 'point' ? random(0.7, 1.6) : kind === 'sparkle' ? random(3, 5) : random(4, 7);
    return { x: random(0, width), y: anywhere ? random(0, height) : height + random(10, 40), size, rise: random(5, 14), sway: random(4, 14),
      phase: random(0, Math.PI * 2), twinkle: random(0.5, 1.6), colour: Math.floor(Math.random() * 8), kind, turn: random(0, Math.PI * 2),
      spin: random(-0.35, 0.35), shown: 0, wobble: Array.from({ length: 12 }, () => random(-0.12, 0.12)) };
  }
  /** As many as the screen holds, a few dozen at most. */
  const most = () => Math.max(14, Math.min(56, Math.round((width * height) / 14_000)));

  function size() {
    if (!canvas) return;
    const box = canvas.getBoundingClientRect();
    width = box.width; height = box.height;
    const ratio = Math.min(2, window.devicePixelRatio || 1);
    canvas.width = Math.round(width * ratio); canvas.height = Math.round(height * ratio);
    canvas.getContext('2d')!.setTransform(ratio, 0, 0, ratio, 0, 0);
    const want = most();
    if (motes.length > want) motes.length = want;
    while (motes.length < want) motes.push(mote(true));
  }

  // MARK: Drawing

  /** A glow or a sparkle's halo drawn once into a small image and stamped thereafter: a gradient made each frame for
   *  each of them cost a phone its smoothness and its power. */
  const sprites = new Map<string, HTMLCanvasElement>();
  function halo(rgb: string, radius: number): HTMLCanvasElement {
    const r = Math.max(2, Math.round(radius)), key = `${rgb}|${r}`;
    let sprite = sprites.get(key);
    if (!sprite) {
      sprite = document.createElement('canvas');
      const ratio = Math.min(2, window.devicePixelRatio || 1);
      sprite.width = sprite.height = Math.ceil(r * 2 * ratio);
      const c = sprite.getContext('2d')!;
      c.scale(ratio, ratio);
      const light = c.createRadialGradient(r, r, 0, r, r, r);
      light.addColorStop(0, `rgba(${rgb}, 1)`); light.addColorStop(1, `rgba(${rgb}, 0)`);
      c.fillStyle = light; c.fillRect(0, 0, r * 2, r * 2);
      sprites.set(key, sprite);
    }
    return sprite;
  }
  function stamp(ctx: CanvasRenderingContext2D, rgb: string, x: number, y: number, radius: number, alpha: number) {
    const r = Math.max(2, Math.round(radius));
    ctx.globalAlpha = alpha;
    ctx.drawImage(halo(rgb, r), x - r, y - r, r * 2, r * 2);
    ctx.globalAlpha = 1;
  }

  function draw(ctx: CanvasRenderingContext2D, kind: Kind, x: number, y: number, size: number, rgb: string, alpha: number, turn: number, wobble?: number[]) {
    if (alpha <= 0.004) return;
    if (kind === 'glow') { stamp(ctx, rgb, x, y, size, alpha); return; }
    if (kind === 'point') {
      ctx.fillStyle = `rgba(${rgb}, ${alpha})`; ctx.beginPath(); ctx.arc(x, y, size, 0, Math.PI * 2); ctx.fill();
      return;
    }
    if (kind === 'sparkle') {
      // The logo's star: four points over a soft glow.
      stamp(ctx, rgb, x, y, size * 3, alpha * 0.35);
      ctx.fillStyle = `rgba(${rgb}, ${alpha})`; ctx.beginPath();
      for (let i = 0; i < 8; i++) {
        const r = i % 2 ? size * 0.22 : size, a = (i * Math.PI) / 4 + turn * 0.2;
        if (i) ctx.lineTo(x + Math.cos(a) * r, y + Math.sin(a) * r); else ctx.moveTo(x + Math.cos(a) * r, y + Math.sin(a) * r);
      }
      ctx.closePath(); ctx.fill();
      return;
    }
    // 手绘's doodles, in pen, each line a little off as a hand draws it.
    const w = (i: number) => 1 + (wobble?.[i % 12] ?? 0);
    ctx.save();
    ctx.translate(x, y); ctx.rotate(turn);
    ctx.strokeStyle = `rgba(${rgb}, ${alpha})`; ctx.lineWidth = 1.2; ctx.lineCap = 'round'; ctx.lineJoin = 'round';
    ctx.beginPath();
    if (kind === 'star') {
      for (let i = 0; i <= 10; i++) {
        const r = (i % 2 ? size * 0.45 : size) * w(i), a = (i * Math.PI) / 5 - Math.PI / 2;
        if (i) ctx.lineTo(Math.cos(a) * r, Math.sin(a) * r); else ctx.moveTo(Math.cos(a) * r, Math.sin(a) * r);
      }
    } else if (kind === 'spark') {
      for (let i = 0; i < 4; i++) {
        const a = (i * Math.PI) / 4, r = size * w(i);
        ctx.moveTo(-Math.cos(a) * r * 0.9, -Math.sin(a) * r * 0.9); ctx.lineTo(Math.cos(a) * r, Math.sin(a) * r);
      }
    } else {
      for (let i = 0; i <= 12; i++) {
        const a = (i / 12) * Math.PI * 2.05, r = size * 0.8 * w(i);
        if (i) ctx.lineTo(Math.cos(a) * r, Math.sin(a) * r); else ctx.moveTo(Math.cos(a) * r, Math.sin(a) * r);
      }
    }
    ctx.stroke();
    ctx.restore();
  }

  /** How strong each look's things are: faint, under the figures. */
  const STRENGTH: Record<Look, number> = { light: 0.16, dark: 0.75, 'sketch-light': 0.2, 'sketch-dark': 0.24 };

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
    const ctx = canvas.getContext('2d')!;
    ctx.clearRect(0, 0, width, height);
    const palette = PALETTES[look], strength = STRENGTH[look];
    const active = Math.round(motes.length * (0.3 + 0.7 * energy)), pace = 0.25 + 1.6 * energy;
    const now = time / 1000;
    motes.forEach((m, i) => {
      // Those beyond what the work asks for fade out, and back in as it picks up: never a pop.
      m.shown += ((i < active ? 1 : 0) - m.shown) * (1 - Math.exp(-dt / 1.2));
      m.y -= m.rise * pace * dt;
      m.x += Math.sin(now * 0.35 + m.phase) * m.sway * dt * 0.6;
      m.turn += m.spin * pace * dt;
      if (m.y < -30) Object.assign(m, mote(false), { shown: m.shown });
      const glint = m.kind === 'point' || m.kind === 'sparkle' ? 0.35 + 0.65 * (0.5 + 0.5 * Math.sin(now * m.twinkle * 2 + m.phase)) : 0.75 + 0.25 * Math.sin(now * m.twinkle + m.phase);
      draw(ctx, m.kind, m.x, m.y, m.size, palette[m.colour % palette.length], strength * m.shown * glint, m.turn, m.wobble);
    });
    // A turn done: out from under the banner, slowing and fading.
    bursts = bursts.filter((b) => now - b.born < 1.6);
    for (const b of bursts) {
      const age = now - b.born, slow = Math.exp(-age * 2.2);
      b.x += b.vx * slow * dt; b.y += b.vy * slow * dt; b.turn += dt * 2;
      draw(ctx, b.kind, b.x, b.y, b.size, palette[b.colour % palette.length], Math.min(1, strength * 2.2) * (1 - age / 1.6), b.turn);
    }
    frames++;
    canvas.dataset.energy = energy.toFixed(2);
    canvas.dataset.active = String(active);
    canvas.dataset.frames = String(frames);
    canvas.dataset.bursts = String(bursts.length);
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
  // The look changed: its own things.
  $effect(() => { void look; untrack(() => { motes = motes.map(() => mote(true)); }); });
  let burstSeen = 0;
  $effect(() => {
    const at = burst;
    untrack(() => {
      if (!at || at === burstSeen || still) return;
      burstSeen = at;
      const now = performance.now() / 1000, x = width / 2, y = Math.min(110, height * 0.15);
      for (let i = 0; i < 16; i++) {
        const a = (i / 16) * Math.PI * 2 + random(-0.15, 0.15), speed = random(70, 150);
        bursts.push({ x, y, vx: Math.cos(a) * speed, vy: Math.sin(a) * speed * 0.7 + 20, born: now, colour: i, kind: sketchy() ? (i % 2 ? 'spark' : 'star') : look === 'dark' ? 'sparkle' : 'glow', size: sketchy() ? random(4, 6) : look === 'dark' ? random(3, 4.5) : random(6, 10), turn: random(0, 6) });
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
