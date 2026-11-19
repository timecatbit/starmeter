// 报刊: a printed sheet. Cream paper and black rules (by night, a dark sheet printed in cream), the estimate as a giant
// ultramarine figure, the hour by hour in bars, and each tool with its first models in ruled rows.
import { canvasFor, drawMark, measure, middleText, text, TOP_BAND, WIDTH } from './shareCommon';
import { drawAmount, drawBars, drawMasthead, drawMembers, drawSignature, figures, fitAmount, GROTESK, poster, posterFont, SIGNATURE, slotCentre, tickBox, type PosterTool } from './sharePoster';
import type { Dashboard, Project } from './types';

const M = 26, INNER = WIDTH - M * 2;
const palettes = {
  light: { ink: '#17150F', paper: '#F6F1E6', blue: '#1A2BD8', grey: '#6E685B', hair: '#CDC5B3', orange: '#C2410C', grain: 'rgba(80, 60, 20, 0.035)', qrInk: '#17150F', qrPaper: '#FFFDF7' },
  dark: { ink: '#EFE8D8', paper: '#17160F', blue: '#8590FF', grey: '#A59E8D', hair: '#3D3A30', orange: '#F0915A', grain: 'rgba(255, 235, 190, 0.03)', qrInk: '#17160F', qrPaper: '#EFE8D8' },
};
const f = (size: number, weight = 700) => posterFont(size, weight, GROTESK);
const HEAD = 30, SUB = 22, ROW = 24, AFTER = 12;

const blockHeight = (t: PosterTool) => HEAD + SUB + (t.members.length ? 1 : t.models.length) * ROW + AFTER;

export async function drawPaperPicture(d: Dashboard, project: Project, dark: boolean): Promise<HTMLCanvasElement> {
  const { ink, paper, blue, grey, hair, orange, grain, qrInk, qrPaper } = dark ? palettes.dark : palettes.light;
  const p = await poster(d, ink);
  const measuring = document.createElement('canvas').getContext('2d')!;
  const hero = fitAmount(measuring, p.cost, INNER, 100, 44, (s) => f(s, 900), -0.035);
  const height = TOP_BAND + 15 + 46 + 20 + Math.round(hero * 0.78) + 18 + 12 + 66 + 14 + (p.chart ? 178 : 0) + p.tools.reduce((s, t) => s + blockHeight(t), 0) + 16 + 22 + SIGNATURE + 30;
  const { canvas, ctx } = canvasFor(height);
  const rule = (y: number, weight = 1.5, colour = ink) => { ctx.fillStyle = colour; ctx.fillRect(M, y, INNER, weight); };

  ctx.fillStyle = paper; ctx.fillRect(0, 0, WIDTH, height);
  // A faint newsprint grain.
  ctx.fillStyle = grain;
  for (let y = 0; y < height; y += 3) for (let x = (y * 7) % 5; x < WIDTH; x += 5) if (((x * 31 + y * 17) % 11) < 3) ctx.fillRect(x, y, 1, 1);

  // Masthead over a heavy rule, then the headline.
  drawMasthead(ctx, p, project.name, { ink, font: f, name: 18, date: 15, left: M, right: WIDTH - M, weight: 800 });
  rule(TOP_BAND - 1, 2);
  let y = TOP_BAND + 15;
  text(ctx, p.title, { x: M, y, w: INNER, h: 34 }, f(30, 900), ink);
  y += 46;

  // The estimate as large as the line allows, in ultramarine.
  text(ctx, p.costLabel, { x: M, y, w: INNER, h: 16 }, f(13, 700), ink);
  y += 20;
  const baseline = y + Math.round(hero * 0.78);
  drawAmount(ctx, p.cost, M - 3, baseline, hero, (s) => f(s, 900), blue, blue, -0.035);
  y = baseline + 18;
  rule(y, 1.5);
  y += 12;

  // Three figures between rules.
  const column = INNER / 3;
  figures(p).forEach(([name, value], i) => {
    const x = M + i * column + (i ? 14 : 0), w = column - (i ? 14 : 0) - 14;
    if (i) { ctx.fillStyle = ink; ctx.fillRect(M + i * column, y + 2, 1.2, 50); }
    text(ctx, name, { x, y, w, h: 17 }, f(13, 600), ink);
    let size = 28;
    while (size > 16 && measure(ctx, value, f(size, 900)) > w) size -= 1;
    text(ctx, value, { x, y: y + 22, w, h: 32 }, f(size, 900), ink);
  });
  y += 66;
  rule(y, 2);
  y += 14;

  const chart = p.chart;
  if (chart) {
    text(ctx, chart.label, { x: M, y, w: INNER / 2, h: 22 }, f(18, 900), ink);
    if (chart.note) text(ctx, chart.note, { x: M + INNER / 2, y: y + 4, w: INNER / 2, h: 15 }, f(12, 500), ink, 'right');
    y += 32;
    const labelWidth = Math.max(...chart.axis.map((a) => measure(ctx, a, f(11, 500)))) + 8;
    const plot = { x: M + labelWidth, y: y + 6, w: INNER - labelWidth, h: 96 };
    chart.axis.forEach((label, i) => {
      const at = plot.y + plot.h - (plot.h * i) / 2;
      ctx.fillStyle = hair; ctx.fillRect(plot.x, at, plot.w, 1);
      text(ctx, label, { x: M, y: at - 7, w: labelWidth - 6, h: 14 }, f(11, 500), ink);
    });
    drawBars(ctx, chart, plot, blue, 0.22);
    ctx.fillStyle = ink; ctx.fillRect(plot.x, plot.y + plot.h, plot.w, 1.5);
    chart.ticks.forEach((tick, i) => {
      const at = tickBox(i, chart.ticks.length, slotCentre(chart, plot, tick.index), plot.x - 2, WIDTH - M, 80);
      text(ctx, tick.label, { x: at.x, y: plot.y + plot.h + 8, w: 80, h: 15 }, f(12, 500), ink, at.align);
    });
    y += 132;
    rule(y, 2);
    y += 14;
  }

  // Each tool: its mark, name and amount, a line of figures (or the tag that explains them), then its models.
  for (const [index, t] of p.tools.entries()) {
    if (index) rule(y - 8, 1);
    const mark = p.marks.tools.get(t.id), costWidth = Math.min(170, measure(ctx, t.cost, f(20, 900)) + 2), indent = mark ? 28 : 0;
    if (mark) drawMark(ctx, mark, { x: M, y: y + 4, w: 20, h: 20 }, ink);
    middleText(ctx, t.name, M + indent, y + 14, INNER - costWidth - indent - 10, f(20, 900), ink);
    middleText(ctx, t.cost, WIDTH - M - costWidth, y + 14, costWidth, f(20, 900), ink, 'right');
    y += HEAD;
    const line = [t.tokens, t.share, t.tag].filter(Boolean).join(' · '), lineWidth = Math.min(INNER - indent, measure(ctx, line, f(13, 500)));
    middleText(ctx, line, M + indent, y + 8, INNER - indent, f(13, 500), grey);
    if (t.unpriced && lineWidth + 80 < INNER - indent) middleText(ctx, '· ' + t.unpriced, M + indent + lineWidth + 5, y + 8, INNER - indent - lineWidth - 5, f(12, 700), orange);
    y += SUB;
    if (t.members.length) {
      rule(y - 2, 1, hair);
      drawMembers(ctx, p, t, M + indent, y + ROW / 2, INNER - indent, f(13.5, 600), ink, ink, 14);
      y += ROW;
    }
    for (const m of t.models) {
      rule(y - 2, 1, hair);
      const nameRoom = INNER - indent - 150, nameWidth = Math.min(nameRoom, measure(ctx, m.name, f(14, 600)));
      middleText(ctx, m.name, M + indent, y + 10, nameRoom, f(14, 600), m.other ? grey : ink);
      if (m.note && nameWidth + 30 < nameRoom) middleText(ctx, m.note, M + indent + nameWidth + 6, y + 10, nameRoom - nameWidth - 6, f(11, 700), orange);
      middleText(ctx, m.amount, WIDTH - M - 150, y + 10, 90, f(14, 500), ink, 'right');
      middleText(ctx, m.share, WIDTH - M - 50, y + 10, 50, f(14, 500), ink, 'right');
      y += ROW;
    }
    y += AFTER;
  }

  // Signature under a heavy rule.
  rule(y + 2, 2);
  drawSignature(ctx, p, project, y + 22, { name: f(17, 900), small: (s) => f(s, 500), size: 12, ink, muted: grey, qrInk, qrPaper });
  return canvas;
}
