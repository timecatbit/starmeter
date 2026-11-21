// 手绘's strokes as SVG paths, drawn as the 手绘 picture (shareSketch.ts) draws them on its canvas: every line twice, a
// little off each time and bowing slightly in the middle; boxes whose sides run past the corners; rings that go round
// once and a little more, widening as they go; squiggles. Seeded, so a page draws the same strokes every time.

/** A small, fast random sequence that is the same for the same seed (xorshift). */
export function seeded(seed: number): () => number {
  let state = (seed * 2654435761) >>> 0 || 1;
  return () => {
    state ^= state << 13; state >>>= 0; state ^= state >>> 17; state ^= state << 5; state >>>= 0;
    return state / 4294967296;
  };
}

type Random = () => number;
const at = (n: number) => n.toFixed(1);

/** A line by hand: `passes` slightly bowed strokes from one end to the other. */
export function handLine(random: Random, x1: number, y1: number, x2: number, y2: number, passes = 2): string {
  const shake = Math.min(1.8, Math.hypot(x2 - x1, y2 - y1) * 0.01 + 0.5);
  const off = (amount: number) => (random() - 0.5) * 2 * amount;
  let d = '';
  for (let i = 0; i < passes; i++) {
    d += `M${at(x1 + off(shake))} ${at(y1 + off(shake))}Q${at((x1 + x2) / 2 + off(shake * 1.6))} ${at((y1 + y2) / 2 + off(shake * 1.6))} ${at(x2 + off(shake))} ${at(y2 + off(shake))}`;
  }
  return d;
}

/** A box by hand, each side running a little past the corners. */
export function handBox(random: Random, x: number, y: number, w: number, h: number, over = 3): string {
  return handLine(random, x - over, y, x + w + over, y) + handLine(random, x + w, y - over, x + w, y + h + over)
    + handLine(random, x + w + over, y + h, x - over, y + h) + handLine(random, x, y + h + over, x, y - over);
}

/** A ring by hand: once round and a little more from a random start, a little wider as it goes. */
export function handRing(random: Random, cx: number, cy: number, r: number): string {
  const start = random() * Math.PI * 2, phase = random() * 6;
  let d = '';
  for (let t = 0; t <= 1.12; t += 0.02) {
    const a = start + t * Math.PI * 2, rr = r * (1 + 0.06 * Math.sin(a * 2 + phase)) + t * 1.2;
    d += `${t === 0 ? 'M' : 'L'}${at(cx + Math.cos(a) * rr)} ${at(cy + Math.sin(a) * rr * 0.94)}`;
  }
  return d;
}

/** A squiggle along a line, `w` long. */
export function squiggle(random: Random, x: number, y: number, w: number): string {
  let d = `M${at(x)} ${at(y)}`;
  for (let step = 4; step <= w; step += 4) d += `L${at(x + step)} ${at(y + Math.sin(step / 7) * 2.2 + (random() - 0.5) * 0.6)}`;
  return d;
}
