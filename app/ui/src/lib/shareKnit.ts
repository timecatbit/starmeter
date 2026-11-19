// 织物: the day knitted into a scarf and sewn onto a care label. The title, the figure and the curve are worked in yarn,
// stitch by stitch; the figures, each tool and its models are printed on a woven label below, with our star and care
// symbols at its foot.
import { canvasFor, drawMark, measure, text, WIDTH } from './shareCommon';
import { drawStar, GROTESK, poster, posterFont, seeded, type PosterTool } from './sharePoster';
import type { Dashboard, Project } from './types';

const yarn = { base: '#D9532A', shade: '#C24722', cream: '#F4E7CF', pink: '#F3A3C5', lilac: '#B8A5E3' };
const thread = '#5E584D', labelInk = '#1E1B16', labelPaper = '#F4EFE3', sky = '#A9C5DC';
const CONDENSED = `'Avenir Next Condensed', 'DIN Condensed', 'Arial Narrow', ${GROTESK}`;
const c = (size: number, weight = 700) => posterFont(size, weight, CONDENSED);
const CELL_W = 3.4, CELL_H = 3.8;
const MODEL = 21;

const blockHeight = (t: PosterTool) => 34 + (t.note ? 18 : 0) + Math.max(1, t.models.length) * MODEL + 18;

/** One knit stitch, a V of two loops, lit from above; drawn once per colour and stamped. */
function stitch(colour: string, w: number, h: number): HTMLCanvasElement {
  const scale = 3, s = document.createElement('canvas');
  s.width = Math.ceil(w * scale); s.height = Math.ceil(h * scale);
  const g = s.getContext('2d')!;
  g.scale(scale, scale);
  for (const [cx, tilt] of [[w * 0.3, -0.5], [w * 0.7, 0.5]] as const) {
    const loop = g.createLinearGradient(0, 0, 0, h);
    loop.addColorStop(0, colour); loop.addColorStop(1, 'rgba(0, 0, 0, 0.28)');
    g.fillStyle = colour;
    g.beginPath(); g.ellipse(cx, h * 0.52, w * 0.3, h * 0.6, tilt, 0, Math.PI * 2); g.fill();
    g.fillStyle = loop; g.globalAlpha = 0.35; g.fill(); g.globalAlpha = 1;
  }
  return s;
}

export async function drawKnitPicture(d: Dashboard, project: Project): Promise<HTMLCanvasElement> {
  const p = await poster(d, labelInk);
  const random = seeded([...d.date].reduce((s, ch) => s + ch.charCodeAt(0), 0) + 7);
  const panel = { x: 16, y: 26, w: WIDTH - 32, h: 288 };
  const cols = Math.floor(panel.w / CELL_W), rows = Math.floor(panel.h / CELL_H);
  const label = { x: 26, y: panel.y + panel.h + 62, w: WIDTH - 52, h: 0 };
  label.h = 62 + 86 + p.tools.reduce((s, t) => s + blockHeight(t), 0) + 84;
  const height = label.y + label.h + 44;
  const { canvas, ctx } = canvasFor(height);

  // A blue cloth underneath, with its own weave.
  ctx.fillStyle = sky; ctx.fillRect(0, 0, WIDTH, height);
  ctx.fillStyle = 'rgba(30, 60, 90, 0.05)';
  for (let y = 0; y < height; y += 2.5) ctx.fillRect(0, y, WIDTH, 0.6);
  for (let x = 0; x < WIDTH; x += 2.5) ctx.fillRect(x, 0, 0.5, height);

  // What the yarn spells, worked out at one pixel a stitch.
  const mask = document.createElement('canvas');
  mask.width = cols; mask.height = rows;
  const m = mask.getContext('2d', { willReadFrequently: true })!;
  m.fillStyle = '#000'; m.fillRect(0, 0, cols, rows);
  m.textBaseline = 'top'; m.textAlign = 'left';
  m.fillStyle = '#F00';
  m.font = posterFont(11, 900, GROTESK); m.fillText(p.title, 6, 3);
  let size = 30;
  const unitSize = (s: number) => Math.max(8, Math.round(s * 0.36));
  const fits = (s: number) => { m.font = posterFont(s, 900, GROTESK); const a = m.measureText(p.tokens).width; m.font = posterFont(unitSize(s), 900, GROTESK); return a + 3 + m.measureText('Token').width <= cols - 10; };
  while (size > 12 && !fits(size)) size -= 1;
  m.font = posterFont(size, 900, GROTESK); m.fillText(p.tokens, 6, 17);
  const figure = m.measureText(p.tokens).width;
  m.font = posterFont(unitSize(size), 900, GROTESK); m.fillText('Token', 6 + figure + 3, 17 + size * 0.12);
  // The curve as steps in pink along the lower rows, with its ends' times.
  const chartTop = 17 + size + 6, chartBottom = rows - 11;
  if (p.values.length > 1 && chartBottom - chartTop > 6) {
    m.fillStyle = '#0F0';
    const span = cols - 14;
    let last: number | null = null;
    for (let x = 0; x <= span; x++) {
      const at = x / span;
      let i = 0;
      while (i < p.positions.length - 1 && p.positions[i + 1] <= at) i++;
      const level = Math.round(chartBottom - (p.values[i] / p.ceiling) * (chartBottom - chartTop));
      if (last !== null && last !== level) m.fillRect(7 + x, Math.min(last, level), 2, Math.abs(last - level) + 2);
      m.fillRect(7 + x, level, 2, 2);
      last = level;
    }
    m.fillStyle = '#F00'; m.font = posterFont(7, 800, GROTESK);
    m.fillText(p.ticks[0]?.label ?? '', 7, rows - 8);
    const end = p.ticks.at(-1)?.label ?? '';
    m.fillText(end, cols - 7 - m.measureText(end).width, rows - 8);
  }
  const spelled = m.getImageData(0, 0, cols, rows).data;

  // The scarf: its shadow, every stitch, a lilac stripe near one edge, and the fringe.
  ctx.save(); ctx.shadowColor = 'rgba(20, 40, 60, 0.28)'; ctx.shadowBlur = 16; ctx.shadowOffsetY = 6;
  ctx.fillStyle = yarn.shade; ctx.fillRect(panel.x, panel.y, cols * CELL_W, rows * CELL_H); ctx.restore();
  const stamps = Object.fromEntries(Object.entries(yarn).map(([k, v]) => [k, stitch(v, CELL_W, CELL_H)])) as Record<keyof typeof yarn, HTMLCanvasElement>;
  for (let row = 0; row < rows; row++) {
    for (let col = 0; col < cols; col++) {
      const i = (row * cols + col) * 4;
      const kind: keyof typeof yarn = spelled[i] > 110 ? 'cream' : spelled[i + 1] > 110 ? 'pink' : col === 3 || col === 4 ? 'lilac' : (row + col * 3) % 7 === 0 ? 'shade' : 'base';
      ctx.drawImage(stamps[kind], panel.x + col * CELL_W, panel.y + row * CELL_H, CELL_W, CELL_H);
    }
  }
  const fringeTop = panel.y + rows * CELL_H;
  ctx.lineCap = 'round';
  for (let x = panel.x + 4; x < panel.x + cols * CELL_W - 2; x += 8) {
    for (let strand = 0; strand < 3; strand++) {
      const length = 30 + random() * 14, sway = (random() - 0.5) * 6, sx = x + strand * 1.6;
      ctx.strokeStyle = x < panel.x + 22 ? yarn.lilac : strand === 1 ? yarn.base : yarn.shade; ctx.lineWidth = 1.5;
      ctx.beginPath(); ctx.moveTo(sx, fringeTop - 1); ctx.quadraticCurveTo(sx + sway, fringeTop + length / 2, sx + sway * 1.6, fringeTop + length); ctx.stroke();
    }
  }

  // The label, sewn on: woven paper, a stitched border, then what it says.
  ctx.save(); ctx.shadowColor = 'rgba(20, 40, 60, 0.25)'; ctx.shadowBlur = 18; ctx.shadowOffsetY = 7;
  ctx.fillStyle = labelPaper; ctx.fillRect(label.x, label.y, label.w, label.h); ctx.restore();
  ctx.fillStyle = 'rgba(90, 80, 60, 0.05)';
  for (let y = label.y; y < label.y + label.h; y += 2) ctx.fillRect(label.x, y, label.w, 0.7);
  for (let x = label.x; x < label.x + label.w; x += 2) ctx.fillRect(x, label.y, 0.6, label.h);
  ctx.strokeStyle = thread; ctx.lineWidth = 1.2; ctx.setLineDash([6, 4]);
  ctx.strokeRect(label.x + 8, label.y + 8, label.w - 16, label.h - 16);
  ctx.setLineDash([]);
  const L = label.x + 22, R = label.x + label.w - 22, W = R - L;
  const dashed = (y: number) => { ctx.strokeStyle = 'rgba(30, 27, 22, 0.55)'; ctx.lineWidth = 1; ctx.setLineDash([4, 3]); ctx.beginPath(); ctx.moveTo(L, y); ctx.lineTo(R, y); ctx.stroke(); ctx.setLineDash([]); };

  let y = label.y + 24;
  const brand = 'STARMETER  ·  ' + p.date, brandFont = c(14, 700);
  ctx.letterSpacing = '2px';
  const brandWidth = measure(ctx, brand, brandFont) + 24;
  drawStar(ctx, p.star, WIDTH / 2 - brandWidth / 2, y, 16, labelInk);
  text(ctx, brand, { x: WIDTH / 2 - brandWidth / 2 + 24, y: y - 1, w: brandWidth, h: 18 }, brandFont, labelInk);
  ctx.letterSpacing = '0px';
  y += 34;
  dashed(y);
  y += 12;
  const column = W / 3;
  [[p.costLabel, p.cost], ['请求数', p.requests], ['缓存命中率', p.cache]].forEach(([name, value], i) => {
    const x = L + i * column + (i ? 12 : 0), w = column - (i ? 12 : 0) - 4;
    if (i) { ctx.strokeStyle = 'rgba(30, 27, 22, 0.5)'; ctx.setLineDash([3, 3]); ctx.beginPath(); ctx.moveTo(L + i * column, y + 2); ctx.lineTo(L + i * column, y + 58); ctx.stroke(); ctx.setLineDash([]); }
    text(ctx, name, { x, y, w, h: 16 }, c(12.5, 600), labelInk);
    let vs = 30;
    while (vs > 16 && measure(ctx, value, c(vs, 800)) > w) vs -= 1;
    text(ctx, value, { x, y: y + 22, w, h: 36 }, c(vs, 800), labelInk);
  });
  y += 74;
  dashed(y);
  y += 12;
  for (const t of p.tools) {
    const mark = p.marks.tools.get(t.p.id);
    if (mark) drawMark(ctx, mark, { x: L, y: y + 4, w: 20, h: 20 }, labelInk);
    const right = `${t.tokens}  |  ${t.cost}`, rightWidth = Math.min(W * 0.66, measure(ctx, right, c(17, 700)) + 2);
    text(ctx, t.name, { x: L + (mark ? 26 : 0), y, w: W - rightWidth - (mark ? 34 : 8), h: 28 }, c(25, 800), labelInk);
    text(ctx, right, { x: R - rightWidth, y: y + 4, w: rightWidth, h: 22 }, c(17, 700), labelInk, 'right');
    y += 34;
    if (t.note) { text(ctx, t.note, { x: L, y, w: W, h: 16 }, c(13, 600), 'rgba(30, 27, 22, 0.7)'); y += 18; }
    if (!t.models.length) text(ctx, '此时段暂无模型调用', { x: L + 6, y, w: W, h: 16 }, c(14, 500), labelInk);
    for (const model of t.models) {
      text(ctx, model.name, { x: L + 6, y, w: W - 150, h: 17 }, c(15, 500), labelInk);
      text(ctx, model.amount, { x: R - 140, y, w: 84, h: 17 }, c(15, 500), labelInk, 'right');
      text(ctx, model.share, { x: R - 50, y, w: 50, h: 17 }, c(15, 500), labelInk, 'right');
      y += MODEL;
    }
    y += 8;
    dashed(y);
    y += 10;
  }

  // Care symbols, then where to find us.
  ctx.strokeStyle = labelInk; ctx.lineWidth = 1.5; ctx.lineJoin = 'round';
  const icon = (x: number, draw: () => void) => { ctx.save(); ctx.translate(x, y + 8); draw(); ctx.restore(); };
  icon(L, () => { ctx.beginPath(); ctx.moveTo(0, 4); ctx.lineTo(3, 20); ctx.lineTo(21, 20); ctx.lineTo(24, 4); ctx.stroke(); ctx.beginPath(); ctx.moveTo(0, 6); ctx.quadraticCurveTo(6, 2, 12, 6); ctx.quadraticCurveTo(18, 10, 24, 6); ctx.stroke(); text(ctx, '30', { x: 3, y: 9, w: 18, h: 10 }, c(9, 700), labelInk, 'center'); });
  icon(L + 34, () => { ctx.beginPath(); ctx.moveTo(12, 2); ctx.lineTo(24, 21); ctx.lineTo(0, 21); ctx.closePath(); ctx.stroke(); ctx.beginPath(); ctx.moveTo(3, 4); ctx.lineTo(21, 22); ctx.moveTo(21, 4); ctx.lineTo(3, 22); ctx.stroke(); });
  icon(L + 68, () => { ctx.strokeRect(1, 1, 20, 20); ctx.beginPath(); ctx.moveTo(8, 5); ctx.lineTo(8, 17); ctx.moveTo(14, 5); ctx.lineTo(14, 17); ctx.stroke(); });
  icon(L + 100, () => { ctx.beginPath(); ctx.moveTo(1, 19); ctx.lineTo(25, 19); ctx.lineTo(23, 8); ctx.quadraticCurveTo(13, 6, 7, 10); ctx.lineTo(1, 19); ctx.stroke(); ctx.beginPath(); ctx.moveTo(10, 5); ctx.lineTo(23, 5); ctx.stroke(); ctx.fillStyle = labelInk; ctx.beginPath(); ctx.arc(14, 14, 1.4, 0, Math.PI * 2); ctx.fill(); });
  icon(L + 136, () => { ctx.beginPath(); ctx.arc(11, 11, 10, 0, Math.PI * 2); ctx.stroke(); ctx.beginPath(); ctx.moveTo(4, 4); ctx.lineTo(18, 18); ctx.moveTo(18, 4); ctx.lineTo(4, 18); ctx.stroke(); });
  text(ctx, project.url.replace(/^https:\/\//, ''), { x: L, y: y + 40, w: W, h: 16 }, c(13.5, 600), labelInk, 'center');
  return canvas;
}
