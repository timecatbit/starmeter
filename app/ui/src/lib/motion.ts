// The panel's motion: one curve for pages, rows and controls (cubic-bezier 0.22, 1, 0.36, 1), and the transitions built
// on it. Reduced motion makes them instant.
import type { TransitionConfig } from 'svelte/transition';
import { panel } from './ui.svelte';

/** A cubic Bézier easing, as CSS defines it. */
export function bezier(x1: number, y1: number, x2: number, y2: number): (t: number) => number {
  const sample = (a: number, b: number, t: number) => ((1 - 3 * b + 3 * a) * t + (3 * b - 6 * a)) * t * t + 3 * a * t;
  const slope = (a: number, b: number, t: number) => 3 * (1 - 3 * b + 3 * a) * t * t + 2 * (3 * b - 6 * a) * t + 3 * a;
  return (x) => {
    if (x <= 0 || x >= 1) return x;
    let t = x;
    for (let i = 0; i < 8; i++) {
      const error = sample(x1, x2, t) - x, d = slope(x1, x2, t);
      if (Math.abs(error) < 1e-5 || Math.abs(d) < 1e-6) break;
      t -= error / d;
    }
    return sample(y1, y2, Math.min(1, Math.max(0, t)));
  };
}
export const pageCurve = bezier(0.22, 1, 0.36, 1);
const quick = (ms: number) => (panel.reduceMotion ? 0 : ms);

/** A row that appears under an opened one: it fades in from just above its place. */
export function rowIn(_node: Element): TransitionConfig {
  return { duration: quick(260), easing: pageCurve, css: (t, u) => `opacity: ${t}; transform: translateY(${-6 * u}px)` };
}
export function rowOut(_node: Element): TransitionConfig {
  return { duration: quick(220), easing: pageCurve, css: (t) => `opacity: ${t}` };
}

/** A page slides in from the side it comes from (dir 1: deeper, -1: back) while the old one slides away and fades. */
export function pageIn(_node: Element, { dir }: { dir: number }): TransitionConfig {
  if (!dir) return { duration: 0 };
  return { duration: quick(320), easing: pageCurve, css: (t, u) => `opacity: ${t}; transform: translateX(${dir * 72 * u}px)` };
}
export function pageOut(node: HTMLElement, { dir }: { dir: number }): TransitionConfig {
  if (!dir) return { duration: 0 };
  // The old page leaves the flow so the new one takes its place at once.
  node.style.position = 'absolute';
  // A page kept to the panel's width in the middle of a phone's screen (App.svelte's .centred) stays where it is.
  if (!node.classList.contains('centred')) node.style.inset = '0';
  return { duration: quick(220), easing: pageCurve, css: (t, u) => `opacity: ${t}; transform: translateX(${-dir * 43 * u}px)` };
}
