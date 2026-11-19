// 报刊: a printed sheet. Cream paper, black rules, a giant ultramarine figure, the hour by hour in bars, and each tool
// with its models as a ruled table.
import { canvasFor, drawMark, measure, text, WIDTH } from './shareCommon';
import { drawBars, drawStar, GROTESK, poster, posterFont, slotCentre, type Poster, type PosterTool } from './sharePoster';
import type { Dashboard, Project } from './types';

const M = 26, INNER = WIDTH - M * 2;
const ink = '#17150F', paper = '#F6F1E6', blue = '#1A2BD8', grey = '#6E685B', hair = '#CDC5B3';
const f = (size: number, weight = 700) => posterFont(size, weight, GROTESK);
const ROW = 30, HEAD = 36, SUB = 24, TABLE_HEAD = 34, AFTER = 22;

const blockHeight = (t: PosterTool) => HEAD + SUB + (t.models.length ? TABLE_HEAD + t.models.length * ROW : ROW) + AFTER;

export async function drawPaperPicture(d: Dashboard, project: Project): Promise<HTMLCanvasElement> {
  const p = await poster(d, ink);
  const chart = p.values.length > 1;
  const height = 300 + (chart ? 196 : 0) + 56 + p.tools.reduce((s, t) => s + blockHeight(t), 0) + 112;
  const { canvas, ctx } = canvasFor(height);
  const rule = (y: number, weight = 1.5, colour = ink) => { ctx.fillStyle = colour; ctx.fillRect(M, y, INNER, weight); };

  ctx.fillStyle = paper; ctx.fillRect(0, 0, WIDTH, height);
  // A faint newsprint grain.
  ctx.fillStyle = 'rgba(80, 60, 20, 0.035)';
  for (let y = 0; y < height; y += 3) for (let x = (y * 7) % 5; x < WIDTH; x += 5) if (((x * 31 + y * 17) % 11) < 3) ctx.fillRect(x, y, 1, 1);

  // Masthead: our star and name at the left, the date at the right (the middle stays clear for a phone's island).
  drawStar(ctx, p.star, M, 33, 20, ink);
  text(ctx, project.name, { x: M + 27, y: 31, w: 200, h: 24 }, f(20, 800), ink);
  text(ctx, p.date, { x: WIDTH / 2, y: 34, w: WIDTH / 2 - M, h: 20 }, f(16, 600), ink, 'right');
  rule(64, 2);

  let y = 78;
  text(ctx, p.title, { x: M, y, w: INNER, h: 38 }, f(34, 900), ink);
  y += 46;
  // The figure as large as the line allows, with its unit beside it.
  const unitFont = f(32, 900), unitWidth = measure(ctx, 'Token', unitFont);
  let size = 104;
  while (size > 40 && measure(ctx, p.tokens, f(size, 900)) > INNER - unitWidth - 10) size -= 2;
  ctx.font = f(size, 900); ctx.fillStyle = blue; ctx.textAlign = 'left'; ctx.textBaseline = 'alphabetic';
  ctx.letterSpacing = `${-size * 0.035}px`;
  const baseline = y + size * 0.8;
  ctx.fillText(p.tokens, M - 3, baseline);
  const figureWidth = ctx.measureText(p.tokens).width;
  ctx.letterSpacing = '0px';
  ctx.font = unitFont; ctx.fillText('Token', M + figureWidth + 8, baseline);
  y = baseline + 22;

  // Three figures between rules.
  const column = INNER / 3;
  [[p.costLabel, p.cost], ['请求数', p.requests], ['缓存命中率', p.cache]].forEach(([name, value], i) => {
    const x = M + i * column + (i ? 14 : 0), w = column - (i ? 14 : 0) - 6;
    if (i) { ctx.fillStyle = ink; ctx.fillRect(M + i * column, y + 2, 1.2, 64); }
    text(ctx, name, { x, y, w, h: 18 }, f(13.5, 600), ink);
    let vs = 34;
    while (vs > 18 && measure(ctx, value, f(vs, 900)) > w) vs -= 1;
    text(ctx, value, { x, y: y + 24, w, h: 40 }, f(vs, 900), ink);
  });
  y += 84;
  rule(y, 2);
  y += 14;

  if (chart) {
    text(ctx, p.chartLabel, { x: M, y, w: INNER - 120, h: 28 }, f(24, 900), ink);
    if (p.until) text(ctx, '截至 ' + p.until, { x: WIDTH - M - 140, y: y + 7, w: 140, h: 16 }, f(13, 500), ink, 'right');
    y += 44;
    const labelWidth = Math.max(...p.axis.map((a) => measure(ctx, a, f(12, 500)))) + 8;
    const plot = { x: M + labelWidth, y: y + 6, w: INNER - labelWidth, h: 104 };
    p.axis.forEach((label, i) => {
      const at = plot.y + plot.h - (plot.h * i) / 2;
      ctx.fillStyle = hair; ctx.fillRect(plot.x, at, plot.w, 1);
      text(ctx, label, { x: M, y: at - 8, w: labelWidth - 6, h: 15 }, f(12, 500), ink);
    });
    drawBars(ctx, p, plot, blue, 0.22);
    ctx.fillStyle = ink; ctx.fillRect(plot.x, plot.y + plot.h, plot.w, 1.5);
    p.ticks.forEach((tick, i) => {
      const cx = slotCentre(p, plot, tick.index), last = i === p.ticks.length - 1 && i > 0;
      text(ctx, tick.label, { x: last ? WIDTH - M - 80 : i === 0 ? plot.x - 2 : cx - 40, y: plot.y + plot.h + 8, w: 80, h: 16 }, f(13, 500), ink,
        last ? 'right' : i === 0 ? 'left' : 'center');
    });
    y += 146;
    rule(y, 2);
    y += 14;
  }

  text(ctx, '工具与模型', { x: M, y, w: INNER, h: 34 }, f(30, 900), ink);
  y += 50;
  for (const t of p.tools) {
    const mark = p.marks.tools.get(t.p.id);
    const costWidth = Math.min(160, measure(ctx, t.cost, f(28, 900)) + 2);
    if (mark) drawMark(ctx, mark, { x: M, y: y + 6, w: 22, h: 22 }, ink);
    text(ctx, t.name, { x: M + (mark ? 30 : 0), y, w: INNER - costWidth - 40, h: 34 }, f(30, 900), ink);
    text(ctx, t.cost, { x: WIDTH - M - costWidth, y: y + 1, w: costWidth, h: 32 }, f(28, 900), ink, 'right');
    y += HEAD;
    text(ctx, [t.tokens, t.note].filter(Boolean).join(' · '), { x: M + (mark ? 30 : 0), y, w: INNER - 30, h: 18 }, f(14, 500), grey);
    y += SUB;
    rule(y, 1, hair);
    if (!t.models.length) {
      text(ctx, '此时段暂无模型调用', { x: M + 4, y: y + 8, w: INNER, h: 16 }, f(13, 500), grey);
      y += ROW;
    } else {
      text(ctx, '模型', { x: M + 4, y: y + 9, w: 120, h: 16 }, f(13.5, 800), ink);
      text(ctx, 'Token', { x: WIDTH - M - 210, y: y + 9, w: 90, h: 16 }, f(13.5, 800), ink, 'right');
      text(ctx, '工具内占比', { x: WIDTH - M - 100, y: y + 9, w: 96, h: 16 }, f(13.5, 800), ink, 'right');
      y += TABLE_HEAD;
      rule(y - 3, 1.5);
      for (const [i, m] of t.models.entries()) {
        const nameRoom = INNER - 230;
        text(ctx, m.name, { x: M + 4, y: y + 6, w: nameRoom, h: 18 }, f(15, 700), ink);
        if (m.note) text(ctx, m.note, { x: M + 8 + Math.min(nameRoom, measure(ctx, m.name, f(15, 700))), y: y + 8, w: 80, h: 14 }, f(11, 600), '#C2410C');
        text(ctx, m.amount, { x: WIDTH - M - 210, y: y + 6, w: 90, h: 18 }, f(15, 500), ink, 'right');
        text(ctx, m.share, { x: WIDTH - M - 100, y: y + 6, w: 96, h: 18 }, f(15, 500), ink, 'right');
        y += ROW;
        if (i < t.models.length - 1) rule(y - 2, 1, hair);
      }
    }
    rule(y - 2, 1, hair);
    y += AFTER;
  }

  // Signature: the rule, our star and name, where to find it.
  rule(y, 2);
  const signFont = f(20, 900), signWidth = measure(ctx, project.name, signFont) + 28;
  drawStar(ctx, p.star, WIDTH / 2 - signWidth / 2, y + 22, 20, ink);
  text(ctx, project.name, { x: WIDTH / 2 - signWidth / 2 + 28, y: y + 20, w: signWidth, h: 24 }, signFont, ink);
  text(ctx, project.url.replace(/^https:\/\//, ''), { x: M, y: y + 52, w: INNER, h: 18 }, f(14, 500), ink, 'center');
  return canvas;
}

export type { Poster };
