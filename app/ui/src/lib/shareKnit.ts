// 织物: the day knitted into a scarf and sewn onto a care label, on blue cloth by day and on navy denim with a wine
// scarf by night. The title, the estimate and the curve are worked in yarn, stitch by stitch, with the three figures on
// small woven tags sewn on between them; each tool and its first models are printed on the care label below, with care
// symbols and our signature at its foot.
import { BOTTOM_BAND, canvasFor, drawMark, measure, middleText, shadow, text, TOP_BAND, WIDTH } from './shareCommon';
import { drawMasthead, drawMembers, drawSignature, figures, GROTESK, poster, posterFont, seeded, SIGNATURE, type PosterTool } from './sharePoster';
import type { Dashboard, Project } from './types';

const palettes = {
  light: {
    yarn: { base: '#D9532A', shade: '#C24722', cream: '#F4E7CF', pink: '#F3A3C5', lilac: '#B8A5E3' },
    sky: '#A9C5DC', weave: 'rgba(30, 60, 90, 0.05)', clothInk: '#1E1B16', drop: 'rgba(20, 40, 60, 0.28)', tagDrop: 'rgba(60, 20, 10, 0.35)',
    labelPaper: '#F4EFE3', labelWeave: 'rgba(90, 80, 60, 0.05)', labelInk: '#1E1B16', thread: '#5E584D', soft: 'rgba(30, 27, 22, 0.66)',
    rule: 'rgba(30, 27, 22, 0.55)', orange: '#B4410E', qrInk: '#1E1B16', qrPaper: '#FFFDF6',
  },
  dark: {
    yarn: { base: '#8E2F3E', shade: '#792735', cream: '#F1E4CC', pink: '#F29BB5', lilac: '#9E8BD6' },
    sky: '#1B2636', weave: 'rgba(255, 255, 255, 0.035)', clothInk: '#EFE6D3', drop: 'rgba(0, 0, 0, 0.5)', tagDrop: 'rgba(0, 0, 0, 0.45)',
    labelPaper: '#2A2724', labelWeave: 'rgba(255, 255, 255, 0.03)', labelInk: '#EFE6D3', thread: '#9C9283', soft: 'rgba(239, 230, 211, 0.66)',
    rule: 'rgba(239, 230, 211, 0.42)', orange: '#F0915A', qrInk: '#1E1B16', qrPaper: '#EFE6D3',
  },
};
const CONDENSED = `'Avenir Next Condensed', 'DIN Condensed', 'Arial Narrow', ${GROTESK}`;
const c = (size: number, weight = 700) => posterFont(size, weight, CONDENSED);
const CELL_W = 3.4, CELL_H = 3.8;
const HEAD = 28, SUB = 18, MODEL = 20, AFTER = 8;
/** Rows of the scarf, top to bottom: the title, the label over the estimate, the estimate, room for the tags, the curve
 *  and its times. */
const TITLE = 3, LABEL_ROW = 17, AMOUNT_ROW = 30, TAG_ROWS = 15, CURVE_ROWS = 17, TIME_ROWS = 10;

const blockHeight = (t: PosterTool) => HEAD + SUB + (t.members.length ? 1 : t.models.length) * MODEL + AFTER + 10;

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

export async function drawKnitPicture(d: Dashboard, project: Project, dark: boolean): Promise<HTMLCanvasElement> {
  const { yarn, sky, weave, clothInk, drop, tagDrop, labelPaper, labelWeave, labelInk, thread, soft, rule, orange, qrInk, qrPaper } = dark ? palettes.dark : palettes.light;
  const p = await poster(d, labelInk);
  const random = seeded([...d.date].reduce((s, ch) => s + ch.charCodeAt(0), 0) + 7);
  const panelX = 16, panelW = WIDTH - 32, cols = Math.floor(panelW / CELL_W);

  // What the yarn spells, worked out at one pixel a stitch: the estimate as large as the scarf is wide, its sign small
  // and raised as everywhere else.
  const sized = document.createElement('canvas').getContext('2d')!;
  const figureFont = (s: number) => posterFont(s, 900, GROTESK);
  const amountWidth = (s: number) => { sized.font = figureFont(Math.round(s * 0.5)); const a = sized.measureText(p.cost.symbol).width; sized.font = figureFont(s); return a + 1 + sized.measureText(p.cost.figure).width; };
  let size = 34;
  while (size > 12 && amountWidth(size) > cols - 12) size -= 1;
  const tagsRow = AMOUNT_ROW + Math.round(size * 0.86) + 3, curveTop = tagsRow + TAG_ROWS + 3, chart = p.chart;
  const rows = chart ? curveTop + CURVE_ROWS + TIME_ROWS : tagsRow + TAG_ROWS + 2;
  const panel = { x: panelX, y: TOP_BAND + 4, w: cols * CELL_W, h: rows * CELL_H };
  const label = { x: 24, y: panel.y + panel.h + 64, w: WIDTH - 48, h: 0 };
  label.h = 22 + p.tools.reduce((s, t) => s + blockHeight(t), 0) + 40 + SIGNATURE + 18;
  const height = label.y + label.h + BOTTOM_BAND - 8;
  const { canvas, ctx } = canvasFor(height);

  // A blue cloth underneath, with its own weave.
  ctx.fillStyle = sky; ctx.fillRect(0, 0, WIDTH, height);
  ctx.fillStyle = weave;
  for (let y = 0; y < height; y += 2.5) ctx.fillRect(0, y, WIDTH, 0.6);
  for (let x = 0; x < WIDTH; x += 2.5) ctx.fillRect(x, 0, 0.5, height);
  drawMasthead(ctx, p, project.name, { ink: clothInk, font: c, name: 19, date: 16, left: 24, right: WIDTH - 24, weight: 800 });

  const mask = document.createElement('canvas');
  mask.width = cols; mask.height = rows;
  const m = mask.getContext('2d', { willReadFrequently: true })!;
  m.fillStyle = '#000'; m.fillRect(0, 0, cols, rows);
  m.textBaseline = 'top'; m.textAlign = 'left';
  m.fillStyle = '#F00'; m.font = posterFont(11, 900, GROTESK); m.fillText(p.title, 6, TITLE);
  m.fillStyle = '#00F'; m.font = posterFont(10, 700, GROTESK); m.fillText(p.costLabel, 6, LABEL_ROW);
  m.fillStyle = '#F00';
  const sign = p.cost.figure === '—' ? '' : p.cost.symbol;
  m.font = figureFont(Math.round(size * 0.5)); m.fillText(sign, 6, AMOUNT_ROW);
  const signWidth = sign ? m.measureText(sign).width : -1;
  m.font = figureFont(size); m.fillText(p.cost.figure, 6 + signWidth + 1, AMOUNT_ROW - Math.round(size * 0.1));
  // The curve as steps in pink, with its ends' times.
  if (chart) {
    const top = curveTop, bottom = curveTop + CURVE_ROWS - 2, span = cols - 14;
    m.fillStyle = '#0F0';
    let last: number | null = null;
    for (let x = 0; x <= span; x++) {
      const at = x / span;
      let i = 0;
      while (i < chart.positions.length - 1 && chart.positions[i + 1] <= at) i++;
      const level = Math.round(bottom - (chart.values[i] / chart.ceiling) * (bottom - top));
      if (last !== null && last !== level) m.fillRect(7 + x, Math.min(last, level), 2, Math.abs(last - level) + 2);
      m.fillRect(7 + x, level, 2, 2);
      last = level;
    }
    m.fillStyle = '#F00'; m.font = posterFont(7, 800, GROTESK);
    m.fillText(chart.ticks[0]?.label ?? '', 7, rows - 8);
    const end = chart.ticks.at(-1)?.label ?? '';
    m.fillText(end, cols - 7 - m.measureText(end).width, rows - 8);
  }
  const spelled = m.getImageData(0, 0, cols, rows).data;

  // The scarf: its shadow, every stitch, a lilac stripe near one edge, and the fringe.
  ctx.save(); shadow(ctx, drop, 8, 3);
  ctx.fillStyle = yarn.shade; ctx.fillRect(panel.x, panel.y, panel.w, panel.h); ctx.restore();
  const stamps = Object.fromEntries(Object.entries(yarn).map(([k, v]) => [k, stitch(v, CELL_W, CELL_H)])) as Record<keyof typeof yarn, HTMLCanvasElement>;
  for (let row = 0; row < rows; row++) {
    for (let col = 0; col < cols; col++) {
      const i = (row * cols + col) * 4;
      const kind: keyof typeof yarn = spelled[i] > 110 ? 'cream' : spelled[i + 1] > 110 ? 'pink' : spelled[i + 2] > 110 ? 'lilac' : col === 3 || col === 4 ? 'lilac' : (row + col * 3) % 7 === 0 ? 'shade' : 'base';
      ctx.drawImage(stamps[kind], panel.x + col * CELL_W, panel.y + row * CELL_H, CELL_W, CELL_H);
    }
  }
  const fringeTop = panel.y + panel.h;
  ctx.lineCap = 'round';
  for (let x = panel.x + 4; x < panel.x + panel.w - 2; x += 8) {
    for (let strand = 0; strand < 3; strand++) {
      const length = 30 + random() * 14, sway = (random() - 0.5) * 6, sx = x + strand * 1.6;
      ctx.strokeStyle = x < panel.x + 22 ? yarn.lilac : strand === 1 ? yarn.base : yarn.shade; ctx.lineWidth = 1.5;
      ctx.beginPath(); ctx.moveTo(sx, fringeTop - 1); ctx.quadraticCurveTo(sx + sway, fringeTop + length / 2, sx + sway * 1.6, fringeTop + length); ctx.stroke();
    }
  }

  // The three figures on small woven tags sewn across the scarf.
  const tagY = panel.y + tagsRow * CELL_H + 4, tagH = TAG_ROWS * CELL_H - 8, gap = 10, tagW = (panel.w - 28 - gap * 2) / 3;
  figures(p).forEach(([name, value], i) => {
    const x = panel.x + 14 + i * (tagW + gap);
    ctx.save(); shadow(ctx, tagDrop, 2.5, 1);
    ctx.fillStyle = labelPaper; ctx.fillRect(x, tagY, tagW, tagH); ctx.restore();
    ctx.strokeStyle = thread; ctx.lineWidth = 1; ctx.setLineDash([3, 2.5]);
    ctx.strokeRect(x + 3.5, tagY + 3.5, tagW - 7, tagH - 7); ctx.setLineDash([]);
    text(ctx, name, { x: x + 10, y: tagY + 8, w: tagW - 20, h: 14 }, c(11.5, 600), labelInk);
    let vs = 22;
    while (vs > 13 && measure(ctx, value, c(vs, 800)) > tagW - 20) vs -= 1;
    text(ctx, value, { x: x + 10, y: tagY + 24, w: tagW - 20, h: 26 }, c(vs, 800), labelInk);
  });

  // The label, sewn on: woven paper, a stitched border, then each tool.
  ctx.save(); shadow(ctx, drop, 9, 3.5);
  ctx.fillStyle = labelPaper; ctx.fillRect(label.x, label.y, label.w, label.h); ctx.restore();
  ctx.fillStyle = labelWeave;
  for (let y = label.y; y < label.y + label.h; y += 2) ctx.fillRect(label.x, y, label.w, 0.7);
  for (let x = label.x; x < label.x + label.w; x += 2) ctx.fillRect(x, label.y, 0.6, label.h);
  ctx.strokeStyle = thread; ctx.lineWidth = 1.2; ctx.setLineDash([6, 4]);
  ctx.strokeRect(label.x + 8, label.y + 8, label.w - 16, label.h - 16);
  ctx.setLineDash([]);
  const L = label.x + 22, R = label.x + label.w - 22, W = R - L;
  const dashed = (y: number) => { ctx.strokeStyle = rule; ctx.lineWidth = 1; ctx.setLineDash([4, 3]); ctx.beginPath(); ctx.moveTo(L, y); ctx.lineTo(R, y); ctx.stroke(); ctx.setLineDash([]); };

  let y = label.y + 22;
  for (const t of p.tools) {
    const mark = p.marks.tools.get(t.id), indent = mark ? 26 : 0;
    if (mark) drawMark(ctx, mark, { x: L, y: y + 3, w: 19, h: 19 }, labelInk);
    const costWidth = Math.min(W * 0.5, measure(ctx, t.cost, c(21, 800)) + 2);
    middleText(ctx, t.name, L + indent, y + 13, W - costWidth - indent - 8, c(23, 800), labelInk);
    middleText(ctx, t.cost, R - costWidth, y + 13, costWidth, c(21, 800), labelInk, 'right');
    y += HEAD;
    const line = [t.tokens, t.share, t.tag].filter(Boolean).join(' · '), used = Math.min(W - indent, measure(ctx, line, c(13.5, 600)));
    middleText(ctx, line, L + indent, y + 7, W - indent, c(13.5, 600), soft);
    if (t.unpriced && used + 70 < W - indent) middleText(ctx, '· ' + t.unpriced, L + indent + used + 5, y + 7, W - indent - used - 5, c(13, 700), orange);
    y += SUB;
    if (t.members.length) { drawMembers(ctx, p, t, L + indent, y + 9, W - indent, c(15, 600), labelInk, labelInk, 14); y += MODEL; }
    for (const model of t.models) {
      const nameRoom = W - indent - 140, nameWidth = Math.min(nameRoom, measure(ctx, model.name, c(15, 500)));
      middleText(ctx, model.name, L + indent, y + 9, nameRoom, c(15, 500), model.other ? soft : labelInk);
      if (model.note && nameWidth + 30 < nameRoom) middleText(ctx, model.note, L + indent + nameWidth + 6, y + 9, nameRoom - nameWidth - 6, c(12, 700), orange);
      middleText(ctx, model.amount, R - 140, y + 9, 86, c(15, 500), labelInk, 'right');
      middleText(ctx, model.share, R - 50, y + 9, 50, c(15, 500), labelInk, 'right');
      y += MODEL;
    }
    y += AFTER;
    dashed(y);
    y += 10;
  }

  // Care symbols, then our signature.
  y += 4;
  ctx.strokeStyle = labelInk; ctx.lineWidth = 1.5; ctx.lineJoin = 'round';
  const symbolsLeft = WIDTH / 2 - 80;
  const icon = (x: number, draw: () => void) => { ctx.save(); ctx.translate(x, y); draw(); ctx.restore(); };
  icon(symbolsLeft, () => { ctx.beginPath(); ctx.moveTo(0, 4); ctx.lineTo(3, 20); ctx.lineTo(21, 20); ctx.lineTo(24, 4); ctx.stroke(); ctx.beginPath(); ctx.moveTo(0, 6); ctx.quadraticCurveTo(6, 2, 12, 6); ctx.quadraticCurveTo(18, 10, 24, 6); ctx.stroke(); text(ctx, '30', { x: 3, y: 9, w: 18, h: 10 }, c(9, 700), labelInk, 'center'); });
  icon(symbolsLeft + 34, () => { ctx.beginPath(); ctx.moveTo(12, 2); ctx.lineTo(24, 21); ctx.lineTo(0, 21); ctx.closePath(); ctx.stroke(); ctx.beginPath(); ctx.moveTo(3, 4); ctx.lineTo(21, 22); ctx.moveTo(21, 4); ctx.lineTo(3, 22); ctx.stroke(); });
  icon(symbolsLeft + 68, () => { ctx.strokeRect(1, 1, 20, 20); ctx.beginPath(); ctx.moveTo(8, 5); ctx.lineTo(8, 17); ctx.moveTo(14, 5); ctx.lineTo(14, 17); ctx.stroke(); });
  icon(symbolsLeft + 100, () => { ctx.beginPath(); ctx.moveTo(1, 19); ctx.lineTo(25, 19); ctx.lineTo(23, 8); ctx.quadraticCurveTo(13, 6, 7, 10); ctx.lineTo(1, 19); ctx.stroke(); ctx.beginPath(); ctx.moveTo(10, 5); ctx.lineTo(23, 5); ctx.stroke(); ctx.fillStyle = labelInk; ctx.beginPath(); ctx.arc(14, 14, 1.4, 0, Math.PI * 2); ctx.fill(); });
  icon(symbolsLeft + 136, () => { ctx.beginPath(); ctx.arc(11, 11, 10, 0, Math.PI * 2); ctx.stroke(); ctx.beginPath(); ctx.moveTo(4, 4); ctx.lineTo(18, 18); ctx.moveTo(18, 4); ctx.lineTo(4, 18); ctx.stroke(); });
  drawSignature(ctx, p, project, y + 40, { name: c(17, 800), small: (s) => c(s, 600), size: 13, ink: labelInk, muted: soft, qrInk, qrPaper });
  return canvas;
}
