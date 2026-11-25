// Words cut off, found by the page itself rather than by eye: every piece of text under `root`, as the browser lays it
// out (Range.getClientRects), must at some scroll of its page be seen whole: inside the screen across, inside every
// ancestor that clips (overflow hidden or clip) and inside what is seen of its scrolling ancestors (below a bar that
// sticks, above Safari's bars: the visual viewport), and not under another element. A text cut short on purpose
// (text-overflow: ellipsis) counts as its whole element, as do a rolling figure (its wheels clip by design) and a chart
// drawn on a canvas (its words are pixels: the canvas itself must fit). What is hidden on purpose is passed over: not
// shown (visibility, opacity 0, no box), for a screen reader only, or wholly outside a clipping ancestor (a folded
// list). Returns what is wrong, one line each.
import type { Page } from '@playwright/test';

export async function cutWords(page: Page, root = '.stage'): Promise<string[]> {
  return page.evaluate(async (rootSelector) => {
    const top = document.querySelector(rootSelector);
    if (!top) return [`no ${rootSelector}`];
    const frame = () => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    const css = (e: Element) => getComputedStyle(e);
    const name = (e: Element) => `${e.tagName.toLowerCase()}${e.getAttribute('class') ? '.' + e.getAttribute('class')!.trim().split(/\s+/).filter((c) => !c.startsWith('svelte-')).join('.') : ''}`;
    const shown = (e: Element) => {
      if (css(e).visibility === 'hidden') return false;
      let opacity = 1;
      for (let a: Element | null = e; a; a = a.parentElement) opacity *= Number(css(a).opacity);
      return opacity > 0.05;
    };
    const readerOnly = (e: Element) => {
      const box = e.getBoundingClientRect();
      return (box.width <= 1 && box.height <= 1) || css(e).clipPath.includes('inset(50%)');
    };

    // The pieces: a text node, or an element taken whole.
    type Piece = { element: Element; text?: Text; label: string };
    const pieces: Piece[] = [];
    const whole = new Set<Element>();
    const take = (element: Element, label: string) => { if (!whole.has(element)) { whole.add(element); pieces.push({ element, label }); } };
    const walker = document.createTreeWalker(top, NodeFilter.SHOW_TEXT);
    for (let node = walker.nextNode() as Text | null; node; node = walker.nextNode() as Text | null) {
      if (!node.textContent?.trim()) continue;
      const element = node.parentElement!;
      if (element.closest('.sky, .edge, script, style, title') || !shown(element) || readerOnly(element)) continue;
      const number = element.closest('.number');
      if (number) { if (!number.closest('.said')) take(number, number.getAttribute('data-value') ?? number.textContent ?? ''); continue; }
      // Cut short on purpose: the block that says so, whole.
      let cut: Element | null = null;
      for (let a: Element | null = element; a && a !== top; a = a.parentElement) {
        const style = css(a);
        if (style.textOverflow === 'ellipsis' && style.overflowX !== 'visible') { cut = a; break; }
        if (!style.display.startsWith('inline')) break;
      }
      if (cut) take(cut, cut.textContent ?? '');
      else pieces.push({ element, text: node, label: node.textContent });
    }
    for (const canvas of top.querySelectorAll('canvas')) if (!canvas.closest('.sky') && shown(canvas)) take(canvas, `(${name(canvas)})`);

    const rects = (piece: Piece) => {
      if (!piece.text) { const box = piece.element.getBoundingClientRect(); return box.width > 0 && box.height > 0 ? [box] : []; }
      const range = document.createRange();
      range.selectNodeContents(piece.text);
      return [...range.getClientRects()].filter((r) => r.width > 0.5 && r.height > 0.5);
    };
    // What scrolls: inside the root, and around it (a tab of a page that scrolls).
    const around: Element[] = [];
    for (let a = top.parentElement; a; a = a.parentElement) around.push(a);
    const all = [...around.reverse(), top, ...top.querySelectorAll('*')];
    const scrollers = all.filter((e) => /auto|scroll/.test(css(e).overflowY) && e.scrollHeight > e.clientHeight + 1);
    const sideways = all.filter((e) => /auto|scroll/.test(css(e).overflowX) && e.scrollWidth > e.clientWidth + 1);
    const seenHeight = () => window.visualViewport?.height ?? innerHeight;
    /** What a scrolling ancestor shows of a piece: its box, under its sticky bar (unless the piece is on the bar), above
     *  what the browser covers. */
    const seenOf = (scroller: Element, piece: Piece) => {
      const box = scroller.getBoundingClientRect();
      let from = Math.max(box.top + scroller.clientTop, 0);
      for (const stuck of scroller.querySelectorAll('*')) {
        if (css(stuck).position !== 'sticky' || stuck.contains(piece.element)) continue;
        const b = stuck.getBoundingClientRect();
        if (b.top <= from + 1 && b.bottom > from) from = b.bottom;
      }
      return { from, to: Math.min(box.top + scroller.clientTop + scroller.clientHeight, seenHeight()) };
    };
    const E = 0.75;
    const wrong = new Map<Piece, string>();
    const seenWhole = new Set<Piece>();
    const hidden = new Set<Piece>();
    /** On its way in or out (a row sliding in, a card rising): judged once it has come to rest. */
    const moving = new Set<Piece>();
    const settling = () => new Set(document.getAnimations()
      .filter((a) => a.playState === 'running' && a.effect?.getComputedTiming().iterations !== Infinity)
      .map((a) => (a.effect as KeyframeEffect | null)?.target).filter((e): e is Element => !!e));
    let busy = settling();
    const judge = (piece: Piece) => {
      if (wrong.has(piece)) return;
      for (let a: Element | null = piece.element; a && a !== top.parentElement; a = a.parentElement) if (busy.has(a)) { moving.add(piece); return; }
      const all = rects(piece);
      if (!all.length) { hidden.add(piece); return; }
      for (const r of all) {
        // Its ancestors outward: a scrolling one not showing it all yet means wait for that scroll (the outer ones would
        // only see it beyond the fold); one that clips must hold it whole (or not at all: hidden on purpose).
        for (let a = piece.element.parentElement; a; a = a.parentElement) {
          const style = css(a), box = a.getBoundingClientRect();
          const left = box.left + a.clientLeft, right = left + a.clientWidth, upper = box.top + a.clientTop, lower = upper + a.clientHeight;
          if (/auto|scroll/.test(style.overflowY) && a.scrollHeight > a.clientHeight + 1) {
            const { from, to } = seenOf(a, piece);
            if (r.top < from - E || r.bottom > to + E) return;
          } else if (/hidden|clip/.test(style.overflowY)) {
            if (r.bottom <= upper + E || r.top >= lower - E) { hidden.add(piece); return; }
            if (r.top < upper - E || r.bottom > lower + E) { wrong.set(piece, `cut top or foot by ${name(a)}`); return; }
          }
          if (/auto|scroll/.test(style.overflowX) && a.scrollWidth > a.clientWidth + 1) {
            if (r.left < left - E || r.right > right + E) return;
          } else if (/hidden|clip/.test(style.overflowX)) {
            if (r.right <= left + E || r.left >= right - E) { hidden.add(piece); return; }
            if (r.left < left - E || r.right > right + E) { wrong.set(piece, `cut across by ${name(a)}`); return; }
          }
        }
        if (r.left < -E || r.right > innerWidth + E) { wrong.set(piece, 'off the screen across'); return; }
        if (r.top < -E || r.bottom > seenHeight() + E) return;
        // Not under anything else (unless it passes over the others by design): the middle and four corners, a little in.
        if (piece.element.closest('[data-passing]')) continue;
        const inset = Math.min(1.5, r.width / 4, r.height / 4);
        for (const [x, y] of [[(r.left + r.right) / 2, (r.top + r.bottom) / 2], [r.left + inset, r.top + inset], [r.right - inset, r.top + inset], [r.left + inset, r.bottom - inset], [r.right - inset, r.bottom - inset]]) {
          const hit = document.elementFromPoint(x, y);
          if (hit && !piece.element.contains(hit) && !hit.contains(piece.element)) { wrong.set(piece, `under ${name(hit)}`); return; }
        }
      }
      seenWhole.add(piece);
    };

    // Each scrolling ancestor through its length (the others where they are); then the rest as they stand.
    const kept = scrollers.map((s) => s.scrollTop);
    for (const piece of pieces) judge(piece);
    for (const scroller of scrollers) {
      const step = Math.max(60, scroller.clientHeight - 160);
      for (let at = 0; ; at = Math.min(at + step, scroller.scrollHeight - scroller.clientHeight)) {
        scroller.scrollTop = at;
        await frame();
        busy = settling();
        for (const piece of pieces) if (scroller.contains(piece.element) && !seenWhole.has(piece)) judge(piece);
        if (at >= scroller.scrollHeight - scroller.clientHeight) break;
      }
    }
    // Each row that scrolls across, through its width (at the scroll where it is seen, if it needs one).
    for (const row of sideways) {
      const outer = scrollers.find((s) => s.contains(row));
      if (outer) { outer.scrollTop = Math.max(0, outer.scrollTop + row.getBoundingClientRect().top - seenOf(outer, { element: row } as Piece).from - 20); await frame(); }
      const keptLeft = row.scrollLeft, step = Math.max(60, row.clientWidth - 60);
      for (let at = 0; ; at = Math.min(at + step, row.scrollWidth - row.clientWidth)) {
        row.scrollLeft = at;
        await frame();
        busy = settling();
        for (const piece of pieces) if (row.contains(piece.element) && !seenWhole.has(piece)) judge(piece);
        if (at >= row.scrollWidth - row.clientWidth) break;
      }
      row.scrollLeft = keptLeft;
    }
    scrollers.forEach((s, i) => (s.scrollTop = kept[i]));
    await frame();
    const lines = [...wrong].map(([piece, why]) => `${why}: "${piece.label.trim().slice(0, 48)}" (${name(piece.element)})`);
    for (const piece of pieces) if (!wrong.has(piece) && !seenWhole.has(piece) && !hidden.has(piece) && !moving.has(piece)) lines.push(`never seen whole: "${piece.label.trim().slice(0, 48)}" (${name(piece.element)})`);
    return lines;
  }, root);
}
