// 手绘: drawn by hand in coloured lines. By day coloured markers on dotted journal paper, by night coloured chalk on a
// blackboard. Every line wobbles a little (seeded, so a picture drawn twice is the same): a squiggle under the title,
// a highlighter swash behind the estimate, the figures in sketched boxes, the trend in pencil over hatching, each tool's
// mark in a circle of its colour and its models as hatched bars. Letters in Chalkboard SE, Chinese in the panel's own
// 手绘 faces (霞鹜文楷, 霞鹜文楷 TC for Traditional; sketch.css), loaded before the picture is drawn.
import { smoothCurves, tracePath } from './chart';
import { traditional } from './i18n.svelte';
import { canvasFor, drawMark, measure, middleText, text, WIDTH } from './shareCommon';
import { amountWidth, drawAmount, drawMasthead, drawMembers, drawSignature, figures, fitAmount, GROTESK, HERO_BASELINE, layout, poster, posterFont, ROWS, seeded, tickBox, SHARE_TOOLS } from './sharePoster';
import { PENS, SKETCH } from './sketchPalette';
import type { Dashboard, Project } from './types';

const M = 26, INNER = WIDTH - M * 2;
const KAI = ["'Starmeter Kai'", "'Starmeter Kai TC'"];
const hand = () => `'Chalkboard SE', 'Noteworthy', 'Segoe Print', 'Ink Free', ${(traditional() ? [...KAI].reverse() : KAI).join(', ')}, ${GROTESK}`;
let HAND = hand();
const h = (size: number, weight = 700) => posterFont(size, weight, HAND);

/** The Chinese faces, read before drawing: a canvas only writes in a face already loaded. */
async function loadKai() {
  await Promise.all(KAI.map((face) => document.fonts.load(`500 16px ${face}`, '用量').catch(() => [])));
}

const palettes = SKETCH;
const pens = PENS;

export async function drawSketchPicture(d: Dashboard, project: Project, dark: boolean, tools = SHARE_TOOLS): Promise<HTMLCanvasElement> {
  HAND = hand();
  await loadKai();
  const c = dark ? palettes.dark : palettes.light;
  const p = await poster(d, c.ink, tools), L = layout(p), height = L.height;
  const { canvas, ctx } = canvasFor(height);
  const random = seeded([...d.date].reduce((s, ch) => s + ch.charCodeAt(0), 0) + 5);
  const wobble = (amount: number) => (random() - 0.5) * 2 * amount;
  const pen = (id: string) => c.pens[pens[id] ?? 'blue'];

  // The hand's strokes: each line twice, a little off each time, bowing slightly in the middle.
  const line = (x1: number, y1: number, x2: number, y2: number, colour: string, width = 1.6, passes = 2) => {
    ctx.strokeStyle = colour; ctx.lineWidth = width; ctx.lineCap = 'round'; ctx.lineJoin = 'round';
    const shake = Math.min(1.8, Math.hypot(x2 - x1, y2 - y1) * 0.01 + 0.5);
    for (let i = 0; i < passes; i++) {
      ctx.beginPath(); ctx.moveTo(x1 + wobble(shake), y1 + wobble(shake));
      ctx.quadraticCurveTo((x1 + x2) / 2 + wobble(shake * 1.6), (y1 + y2) / 2 + wobble(shake * 1.6), x2 + wobble(shake), y2 + wobble(shake));
      ctx.stroke();
    }
  };
  const box = (x: number, y: number, w: number, hh: number, colour: string, width = 1.6) => {
    line(x - 3, y, x + w + 3, y, colour, width); line(x + w, y - 3, x + w, y + hh + 3, colour, width);
    line(x + w + 3, y + hh, x - 3, y + hh, colour, width); line(x, y + hh + 3, x, y - 3, colour, width);
  };
  const circle = (cx: number, cy: number, r: number, colour: string, width = 1.6) => {
    ctx.strokeStyle = colour; ctx.lineWidth = width; ctx.lineCap = 'round';
    const start = random() * Math.PI * 2, phase = random() * 6;
    ctx.beginPath();
    for (let t = 0; t <= 1.12; t += 0.02) {
      const a = start + t * Math.PI * 2, rr = r * (1 + 0.06 * Math.sin(a * 2 + phase)) + t * 1.2;
      const x = cx + Math.cos(a) * rr, y = cy + Math.sin(a) * rr * 0.94;
      if (t === 0) ctx.moveTo(x, y); else ctx.lineTo(x, y);
    }
    ctx.stroke();
  };
  const hatch = (x: number, y: number, w: number, hh: number, colour: string, gap = 3.4, width = 1.1) => {
    ctx.save(); ctx.beginPath(); ctx.rect(x, y, w, hh); ctx.clip();
    ctx.strokeStyle = colour; ctx.lineWidth = width; ctx.lineCap = 'round';
    for (let at = x - hh; at < x + w; at += gap) { ctx.beginPath(); ctx.moveTo(at + wobble(0.6), y + hh + 1); ctx.lineTo(at + hh * 0.75 + wobble(0.6), y - 1); ctx.stroke(); }
    ctx.restore();
  };
  const squiggle = (x: number, y: number, w: number, colour: string, width = 2.2) => {
    ctx.strokeStyle = colour; ctx.lineWidth = width; ctx.lineCap = 'round'; ctx.lineJoin = 'round';
    ctx.beginPath(); ctx.moveTo(x, y);
    for (let at = 0; at <= w; at += 4) ctx.lineTo(x + at, y + Math.sin(at / 7) * 2.2 + wobble(0.3));
    ctx.stroke();
  };

  // The page: dotted journal paper, or a blackboard with faint chalk dust.
  ctx.fillStyle = c.paper; ctx.fillRect(0, 0, WIDTH, height);
  ctx.fillStyle = c.dots;
  for (let y = 7; y < height; y += 14) for (let x = 7; x < WIDTH; x += 14) { ctx.beginPath(); ctx.arc(x, y, dark ? 0.9 : 0.8, 0, Math.PI * 2); ctx.fill(); }
  if (dark) {
    for (let i = 0; i < 14; i++) {
      const x = random() * WIDTH, y = random() * height, r = 40 + random() * 90;
      const dust = ctx.createRadialGradient(x, y, 0, x, y, r);
      dust.addColorStop(0, 'rgba(255, 255, 255, 0.035)'); dust.addColorStop(1, 'rgba(255, 255, 255, 0)');
      ctx.fillStyle = dust; ctx.fillRect(x - r, y - r, r * 2, r * 2);
    }
  }

  drawMasthead(ctx, p, project.name, { ink: c.ink, font: h, name: 18, date: 15, left: M, right: WIDTH - M, weight: 700 });

  // The title with a coral squiggle under it.
  text(ctx, p.title, { x: M, y: L.title + 6, w: INNER, h: 34 }, h(28, 700), c.ink);
  squiggle(M, L.title + 46, Math.min(INNER, measure(ctx, p.title, h(28, 700)) + 6), c.pens.coral);

  // The estimate over a highlighter swash.
  text(ctx, p.costLabel, { x: M, y: L.hero + 6, w: INNER, h: 17 }, h(13.5, 400), c.soft);
  const hero = fitAmount(ctx, p.cost, INNER - 16, 84, 32, (s) => h(s, 700)), baseline = L.hero + HERO_BASELINE;
  const heroWidth = Math.min(INNER, amountWidth(ctx, p.cost, hero, (s) => h(s, 700)));
  ctx.strokeStyle = c.highlight; ctx.lineCap = 'round';
  for (const [at, thick] of [[0.62, hero * 0.34], [0.32, hero * 0.3]] as const) {
    ctx.lineWidth = thick; ctx.beginPath();
    ctx.moveTo(M - 4 + thick / 2 + wobble(2), baseline - hero * at + wobble(1.5)); ctx.lineTo(M + heroWidth + 8 - thick / 2 + wobble(3), baseline - hero * at - 3 + wobble(1.5)); ctx.stroke();
  }
  drawAmount(ctx, p.cost, M, baseline, hero, (s) => h(s, 700), c.ink, c.ink);

  // The three figures, each in a box sketched in its own colour.
  const column = (INNER - 20) / 3, colours = [c.pens.blue, c.pens.green, c.pens.violet];
  figures(p).forEach(([name, value], i) => {
    const x = M + i * (column + 10), y = L.stats + 4;
    box(x, y, column, 58, colours[i], 1.5);
    text(ctx, name, { x: x + 10, y: y + 8, w: column - 20, h: 16 }, h(12.5, 400), c.soft);
    let size = 22;
    while (size > 13 && measure(ctx, value, h(size, 700)) > column - 20) size -= 1;
    text(ctx, value, { x: x + 10, y: y + 26, w: column - 20, h: 28 }, h(size, 700), colours[i]);
  });

  // The trend in pencil: sketched axes, hatching under the line, the line itself in coral.
  const chart = p.chart;
  if (chart && L.chart !== null) {
    const y = L.chart;
    text(ctx, chart.label, { x: M, y, w: INNER / 2, h: 18 }, h(14, 700), c.ink);
    if (chart.note) text(ctx, chart.note, { x: M + INNER / 2, y: y + 1, w: INNER / 2, h: 16 }, h(12.5, 400), c.soft, 'right');
    const plot = { x: M + 6, y: y + 32, w: INNER - 10, h: ROWS.chart - 72 }, bottom = plot.y + plot.h;
    const curve = smoothCurves([chart.values], chart.positions, plot, chart.ceiling).at(-1)!;
    ctx.save();
    ctx.beginPath(); tracePath(ctx, curve); ctx.lineTo(plot.x + plot.w, bottom); ctx.lineTo(plot.x, bottom); ctx.closePath(); ctx.clip();
    hatch(plot.x, plot.y - 4, plot.w, plot.h + 4, c.pens.teal, 4.2, 1);
    ctx.restore();
    line(plot.x - 4, bottom, plot.x + plot.w + 4, bottom, c.ink, 1.5);
    line(plot.x, bottom + 4, plot.x, plot.y - 6, c.ink, 1.5);
    for (let pass = 0; pass < 2; pass++) {
      ctx.save(); ctx.translate(wobble(0.8), wobble(0.8));
      ctx.beginPath(); tracePath(ctx, curve); ctx.strokeStyle = c.pens.coral; ctx.lineWidth = pass ? 1.4 : 2.4; ctx.lineCap = 'round'; ctx.lineJoin = 'round'; ctx.stroke();
      ctx.restore();
    }
    circle(curve.end[0], curve.end[1], 5, c.pens.coral, 1.8);
    chart.ticks.forEach((tick, i) => {
      const at = tickBox(i, chart.ticks.length, plot.x + tick.at * plot.w, M, WIDTH - M, 80);
      text(ctx, tick.label, { x: at.x, y: bottom + 8, w: 80, h: 16 }, h(12, 400), c.soft, at.align);
    });
  }

  // Each tool: its mark in a circle of its colour, name and amount, a line of figures, then its models as hatched bars.
  p.tools.forEach((t, index) => {
    const top = L.tools[index], colour = pen(t.id), mark = p.marks.tools.get(t.id);
    circle(M + 12, top + 16, 12, colour, 1.8);
    if (mark) drawMark(ctx, mark, { x: M + 4, y: top + 8, w: 16, h: 16 }, c.ink);
    else { line(M + 7, top + 16, M + 17, top + 16, colour, 2, 1); line(M + 12, top + 11, M + 12, top + 21, colour, 2, 1); }
    const indent = 34, costWidth = measure(ctx, t.cost, h(19, 700));
    middleText(ctx, t.name, M + indent, top + 16, INNER - indent - costWidth - 12, h(19, 700), c.ink);
    middleText(ctx, t.cost, WIDTH - M - costWidth - 2, top + 16, costWidth + 2, h(19, 700), colour, 'right');
    const figuresLine = [t.tokens, t.share, t.tag].filter(Boolean).join(' · '), used = Math.min(INNER - indent, measure(ctx, figuresLine, h(12.5, 400)));
    middleText(ctx, figuresLine, M + indent, top + 37, INNER - indent, h(12.5, 400), c.soft);
    if (t.unpriced && used + 70 < INNER - indent) middleText(ctx, '· ' + t.unpriced, M + indent + used + 6, top + 37, INNER - indent - used - 6, h(12, 700), c.warn);
    let y = top + ROWS.head;
    if (t.members.length) { drawMembers(ctx, p, t, M + indent, y + 10.5, INNER - indent, h(13.5, 400), c.ink, c.ink, 13); y += ROWS.model; }
    for (const m of t.models) {
      const mid = y + 10.5, nameRoom = INNER - indent - 190, nameWidth = Math.min(nameRoom, measure(ctx, m.name, h(13.5, 400)));
      middleText(ctx, m.name, M + indent, mid, nameRoom, h(13.5, 400), m.other ? c.soft : c.ink);
      if (m.note && nameWidth + 30 < nameRoom) middleText(ctx, m.note, M + indent + nameWidth + 6, mid, nameRoom - nameWidth - 6, h(11, 700), c.warn);
      const bar = { x: WIDTH - M - 182, y: mid - 4, w: 66, h: 8 }, length = Math.max(4, bar.w * m.ratio);
      hatch(bar.x, bar.y, length, bar.h, m.other ? c.pens.grey : colour, 2.6, 1.2);
      line(bar.x, bar.y + bar.h + 1.5, bar.x + bar.w, bar.y + bar.h + 1.5, c.soft, 0.9, 1);
      middleText(ctx, m.amount, WIDTH - M - 108, mid, 64, h(13.5, 700), c.ink, 'right');
      middleText(ctx, m.share, WIDTH - M - 40, mid, 40, h(13, 400), c.soft, 'right');
      y += ROWS.model;
    }
  });

  // A row of coloured squiggles to close the page, then the signature.
  const marks = [c.pens.coral, c.pens.yellow, c.pens.green, c.pens.blue, c.pens.violet];
  marks.forEach((colour, i) => squiggle(WIDTH / 2 - 120 + i * 50, L.foot + 14 + wobble(2), 40, colour, 2));
  drawSignature(ctx, p, project, L.sign + 20, { name: h(18, 700), small: (s) => h(s, 400), size: 12.5, ink: c.ink, muted: c.soft });
  return canvas;
}
