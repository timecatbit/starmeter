// 织物: the day knitted into a scarf and sewn onto a care label, on blue cloth by day and on navy denim with a wine
// scarf by night. The title, the estimate and the curve are worked in yarn, stitch by stitch, with the three figures on
// small woven tags sewn on between them; each tool and its first models are printed on the care label below, with care
// symbols and our signature at its foot.
import { canvasFor, drawMark, measure, middleText, shadow, text, WIDTH } from './shareCommon';
import { drawMasthead, drawMembers, drawSignature, figures, GROTESK, HERO_BASELINE, layout, poster, posterFont, ROWS, seeded } from './sharePoster';
import type { Dashboard, Project } from './types';

const palettes = {
  light: {
    yarn: { base: '#D9532A', shade: '#C24722', cream: '#F4E7CF', pink: '#F3A3C5', lilac: '#B8A5E3' },
    sky: '#A9C5DC', weave: 'rgba(30, 60, 90, 0.05)', clothInk: '#1E1B16', drop: 'rgba(20, 40, 60, 0.28)', tagDrop: 'rgba(60, 20, 10, 0.35)',
    labelPaper: '#F4EFE3', labelWeave: 'rgba(90, 80, 60, 0.05)', labelInk: '#1E1B16', thread: '#5E584D', soft: 'rgba(30, 27, 22, 0.66)',
    rule: 'rgba(30, 27, 22, 0.55)', orange: '#B4410E',
  },
  dark: {
    yarn: { base: '#8E2F3E', shade: '#792735', cream: '#F1E4CC', pink: '#F29BB5', lilac: '#9E8BD6' },
    sky: '#1B2636', weave: 'rgba(255, 255, 255, 0.035)', clothInk: '#EFE6D3', drop: 'rgba(0, 0, 0, 0.5)', tagDrop: 'rgba(0, 0, 0, 0.45)',
    labelPaper: '#2A2724', labelWeave: 'rgba(255, 255, 255, 0.03)', labelInk: '#EFE6D3', thread: '#9C9283', soft: 'rgba(239, 230, 211, 0.66)',
    rule: 'rgba(239, 230, 211, 0.42)', orange: '#F0915A',
  },
};
const CONDENSED = `'Avenir Next Condensed', 'DIN Condensed', 'Arial Narrow', ${GROTESK}`;
const c = (size: number, weight = 700) => posterFont(size, weight, CONDENSED);
const CELL_W = 3.4, CELL_H = 3.8;
/** The scarf ends this far above the care label, for its fringe. */
const FRINGE = 58;

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
  const { yarn, sky, weave, clothInk, drop, tagDrop, labelPaper, labelWeave, labelInk, thread, soft, rule, orange } = dark ? palettes.dark : palettes.light;
  const p = await poster(d, labelInk), G = layout(p), height = G.height;
  const random = seeded([...d.date].reduce((s, ch) => s + ch.charCodeAt(0), 0) + 7);
  const panelX = 16, panelW = WIDTH - 32, cols = Math.floor(panelW / CELL_W);

  // The care label holds the tools, the care symbols and the signature; the scarf runs from under the masthead to just
  // above it, leaving room for the fringe, and holds the title, the estimate, the tags and the curve on the rows every
  // style uses.
  const label = { x: 24, y: (G.tools[0] ?? G.foot) - 16, w: WIDTH - 48, h: 0 };
  label.h = G.sign + 100 - label.y;
  const chart = p.chart, scarfTop = G.title + 2, rows = Math.floor((Math.max(label.y - FRINGE, G.stats + 62) - scarfTop) / CELL_H);
  const panel = { x: panelX, y: scarfTop, w: cols * CELL_W, h: rows * CELL_H };
  const rowAt = (y: number) => Math.round((y - scarfTop) / CELL_H);
  const { canvas, ctx } = canvasFor(height);

  // What the yarn spells, worked out at one pixel a stitch: the estimate as large as the scarf is wide (and the space
  // under its label is tall), its sign small and raised as everywhere else.
  const figureFont = (s: number) => posterFont(s, 900, GROTESK);
  const labelRow = rowAt(G.hero + 4), baselineRow = rowAt(G.hero + HERO_BASELINE);
  const amountWidth = (s: number) => { ctx.font = figureFont(Math.round(s * 0.5)); const a = ctx.measureText(p.cost.symbol).width; ctx.font = figureFont(s); return a + 1 + ctx.measureText(p.cost.figure).width; };
  let size = Math.floor((baselineRow - labelRow - 13) / 0.74);
  while (size > 12 && amountWidth(size) > cols - 12) size -= 1;

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
  m.fillStyle = '#F00'; m.font = posterFont(11, 900, GROTESK); m.fillText(p.title, 6, rowAt(G.title + 12));
  m.fillStyle = '#00F'; m.font = posterFont(10, 700, GROTESK); m.fillText(p.costLabel, 6, labelRow);
  m.fillStyle = '#F00'; m.textBaseline = 'alphabetic';
  const sign = p.cost.figure === '—' ? '' : p.cost.symbol;
  m.font = figureFont(Math.round(size * 0.5)); m.fillText(sign, 6, baselineRow - Math.round(size * 0.3));
  const signWidth = sign ? m.measureText(sign).width : -1;
  m.font = figureFont(size); m.fillText(p.cost.figure, 6 + signWidth + 1, baselineRow);
  m.textBaseline = 'top';
  // The curve as steps in pink, with its ends' times.
  if (chart && G.chart !== null) {
    const top = rowAt(G.chart + 4), bottom = rows - 12, span = cols - 14;
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
  const tagY = G.stats + 4, tagH = 52, gap = 10, tagW = (panel.w - 28 - gap * 2) / 3;
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

  p.tools.forEach((t, index) => {
    let y = G.tools[index];
    if (index) dashed(y - ROWS.gap / 2);
    const mark = p.marks.tools.get(t.id), indent = mark ? 26 : 0;
    if (mark) drawMark(ctx, mark, { x: L, y: y + 6.5, w: 19, h: 19 }, labelInk);
    const costWidth = Math.min(W * 0.5, measure(ctx, t.cost, c(21, 800)) + 2);
    middleText(ctx, t.name, L + indent, y + 16, W - costWidth - indent - 8, c(23, 800), labelInk);
    middleText(ctx, t.cost, R - costWidth, y + 16, costWidth, c(21, 800), labelInk, 'right');
    const line = [t.tokens, t.share, t.tag].filter(Boolean).join(' · '), used = Math.min(W - indent, measure(ctx, line, c(13.5, 600)));
    middleText(ctx, line, L + indent, y + 37, W - indent, c(13.5, 600), soft);
    if (t.unpriced && used + 70 < W - indent) middleText(ctx, '· ' + t.unpriced, L + indent + used + 5, y + 37, W - indent - used - 5, c(13, 700), orange);
    y += ROWS.head;
    const middle = () => y + ROWS.model / 2;
    if (t.members.length) { drawMembers(ctx, p, t, L + indent, middle(), W - indent, c(15, 600), labelInk, labelInk, 14); y += ROWS.model; }
    for (const model of t.models) {
      const nameRoom = W - indent - 140, nameWidth = Math.min(nameRoom, measure(ctx, model.name, c(15, 500)));
      middleText(ctx, model.name, L + indent, middle(), nameRoom, c(15, 500), model.other ? soft : labelInk);
      if (model.note && nameWidth + 30 < nameRoom) middleText(ctx, model.note, L + indent + nameWidth + 6, middle(), nameRoom - nameWidth - 6, c(12, 700), orange);
      middleText(ctx, model.amount, R - 140, middle(), 86, c(15, 500), labelInk, 'right');
      middleText(ctx, model.share, R - 50, middle(), 50, c(15, 500), labelInk, 'right');
      y += ROWS.model;
    }
  });
  dashed(G.foot - ROWS.gap / 2);

  // Care symbols, then our signature.
  const y = G.foot + 2;
  ctx.strokeStyle = labelInk; ctx.lineWidth = 1.5; ctx.lineJoin = 'round';
  const symbolsLeft = WIDTH / 2 - 80;
  const icon = (x: number, draw: () => void) => { ctx.save(); ctx.translate(x, y); draw(); ctx.restore(); };
  icon(symbolsLeft, () => { ctx.beginPath(); ctx.moveTo(0, 4); ctx.lineTo(3, 20); ctx.lineTo(21, 20); ctx.lineTo(24, 4); ctx.stroke(); ctx.beginPath(); ctx.moveTo(0, 6); ctx.quadraticCurveTo(6, 2, 12, 6); ctx.quadraticCurveTo(18, 10, 24, 6); ctx.stroke(); text(ctx, '30', { x: 3, y: 9, w: 18, h: 10 }, c(9, 700), labelInk, 'center'); });
  icon(symbolsLeft + 34, () => { ctx.beginPath(); ctx.moveTo(12, 2); ctx.lineTo(24, 21); ctx.lineTo(0, 21); ctx.closePath(); ctx.stroke(); ctx.beginPath(); ctx.moveTo(3, 4); ctx.lineTo(21, 22); ctx.moveTo(21, 4); ctx.lineTo(3, 22); ctx.stroke(); });
  icon(symbolsLeft + 68, () => { ctx.strokeRect(1, 1, 20, 20); ctx.beginPath(); ctx.moveTo(8, 5); ctx.lineTo(8, 17); ctx.moveTo(14, 5); ctx.lineTo(14, 17); ctx.stroke(); });
  icon(symbolsLeft + 100, () => { ctx.beginPath(); ctx.moveTo(1, 19); ctx.lineTo(25, 19); ctx.lineTo(23, 8); ctx.quadraticCurveTo(13, 6, 7, 10); ctx.lineTo(1, 19); ctx.stroke(); ctx.beginPath(); ctx.moveTo(10, 5); ctx.lineTo(23, 5); ctx.stroke(); ctx.fillStyle = labelInk; ctx.beginPath(); ctx.arc(14, 14, 1.4, 0, Math.PI * 2); ctx.fill(); });
  icon(symbolsLeft + 136, () => { ctx.beginPath(); ctx.arc(11, 11, 10, 0, Math.PI * 2); ctx.stroke(); ctx.beginPath(); ctx.moveTo(4, 4); ctx.lineTo(18, 18); ctx.moveTo(18, 4); ctx.lineTo(4, 18); ctx.stroke(); });
  drawSignature(ctx, p, project, G.sign + 20, { name: c(17, 800), small: (s) => c(s, 600), size: 13, ink: labelInk, muted: soft });
  return canvas;
}
