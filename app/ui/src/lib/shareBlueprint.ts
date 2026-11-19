// 蓝图: an engineering drawing. Classic blueprint blue by night, a whiteprint (blue lines on white) by day, ruled with a
// grid to every edge. The estimate is dimensioned like a part, the three figures sit in a ruled table, the trend is a
// plotted figure with hatching under it, each tool is a numbered callout with its models measured off, and the
// signature fills the title block.
import { alpha } from './colors';
import { t as tr } from './i18n.svelte';
import { canvasFor, drawMark, measure, middleText, text, WIDTH } from './shareCommon';
import { drawAmount, drawMasthead, drawMembers, drawSignature, figures, fitAmount, GROTESK, HERO_BASELINE, layout, poster, posterFont, ROWS, tickBox, SHARE_TOOLS } from './sharePoster';
import type { Dashboard, Project } from './types';

const M = 26, INNER = WIDTH - M * 2;
const TECH = `'DIN Alternate', 'Avenir Next', ${GROTESK}`;
const t = (size: number, weight = 600) => posterFont(size, weight, TECH);

const palettes = {
  dark: { paper: 'rgb(18, 63, 120)', minor: 'rgba(255, 255, 255, 0.06)', major: 'rgba(255, 255, 255, 0.13)', ink: 'rgb(236, 243, 255)',
    soft: 'rgba(236, 243, 255, 0.68)', line: 'rgb(160, 210, 255)', warn: 'rgb(255, 196, 128)' },
  light: { paper: 'rgb(246, 249, 253)', minor: 'rgba(18, 63, 120, 0.06)', major: 'rgba(18, 63, 120, 0.13)', ink: 'rgb(18, 58, 112)',
    soft: 'rgba(18, 58, 112, 0.66)', line: 'rgb(30, 111, 217)', warn: 'rgb(194, 65, 12)' },
};

export async function drawBlueprintPicture(d: Dashboard, project: Project, dark: boolean, tools = SHARE_TOOLS): Promise<HTMLCanvasElement> {
  const c = dark ? palettes.dark : palettes.light;
  const p = await poster(d, c.ink, tools), L = layout(p), height = L.height;
  const { canvas, ctx } = canvasFor(height);
  const hero = fitAmount(ctx, p.cost, INNER - 20, 84, 32, (s) => t(s, 700));
  const stroke = (colour: string, width = 1, dash: number[] = []) => { ctx.strokeStyle = colour; ctx.lineWidth = width; ctx.setLineDash(dash); };
  const segment = (x1: number, y1: number, x2: number, y2: number) => { ctx.beginPath(); ctx.moveTo(x1, y1); ctx.lineTo(x2, y2); ctx.stroke(); };
  const arrow = (x: number, y: number, direction: 1 | -1) => {
    ctx.fillStyle = c.ink; ctx.beginPath(); ctx.moveTo(x, y); ctx.lineTo(x + direction * 7, y - 2.5); ctx.lineTo(x + direction * 7, y + 2.5); ctx.closePath(); ctx.fill();
  };

  // The sheet, ruled to every edge.
  ctx.fillStyle = c.paper; ctx.fillRect(0, 0, WIDTH, height);
  for (let x = 0; x <= WIDTH; x += 10) { stroke(x % 50 === 0 ? c.major : c.minor, x % 50 === 0 ? 0.8 : 0.5); segment(x, 0, x, height); }
  for (let y = 0; y <= height; y += 10) { stroke(y % 50 === 0 ? c.major : c.minor, y % 50 === 0 ? 0.8 : 0.5); segment(0, y, WIDTH, y); }
  ctx.setLineDash([]);

  drawMasthead(ctx, p, project.name, { ink: c.ink, font: t, name: 18, date: 15, left: M, right: WIDTH - M, weight: 700 });
  let y = L.title + 6;
  text(ctx, p.title, { x: M, y, w: INNER, h: 32 }, t(28, 700), c.ink);
  stroke(c.ink, 1.4); segment(M, y + 40, WIDTH - M, y + 40);
  y = L.hero;

  // The estimate dimensioned like a part: a dimension line over it carrying its label, extension lines down its sides.
  const dimension = y + 12;
  const baseline = L.hero + HERO_BASELINE;
  const amountWidth = drawAmount(ctx, p.cost, M, baseline, hero, (s) => t(s, 700), c.ink, c.soft);
  const span = Math.max(amountWidth, measure(ctx, p.costLabel, t(11.5, 600)) + 40);
  stroke(c.soft, 0.8); segment(M, dimension - 7, M, baseline - hero * 0.72); segment(M + span, dimension - 7, M + span, baseline - hero * 0.72);
  stroke(c.ink, 0.9); segment(M + 1, dimension, M + span - 1, dimension);
  arrow(M + 1, dimension, 1); arrow(M + span - 1, dimension, -1);
  const label = p.costLabel, labelWidth = measure(ctx, label, t(11.5, 600)) + 12;
  ctx.fillStyle = c.paper; ctx.fillRect(M + span / 2 - labelWidth / 2, dimension - 7, labelWidth, 14);
  middleText(ctx, label, M + span / 2 - labelWidth / 2, dimension, labelWidth, t(11.5, 600), c.ink, 'center');
  y = L.stats + 2;

  // The three figures in a ruled table.
  const column = INNER / 3;
  stroke(c.ink, 1.2); ctx.strokeRect(M, y, INNER, 62);
  figures(p).forEach(([name, value], i) => {
    const x = M + i * column;
    if (i) segment(x, y, x, y + 62);
    text(ctx, name, { x: x + 10, y: y + 9, w: column - 20, h: 14 }, t(11.5, 600), c.soft);
    let size = 24;
    while (size > 14 && measure(ctx, value, t(size, 700)) > column - 20) size -= 1;
    text(ctx, value, { x: x + 10, y: y + 28, w: column - 20, h: 28 }, t(size, 700), c.ink);
  });

  // 图 1: the trend plotted on its axes, nodes at each reading, hatching under the line.
  const chart = p.chart;
  if (chart && L.chart !== null) {
    y = L.chart;
    text(ctx, tr('图 1 · ') + chart.label, { x: M, y, w: INNER / 2 + 40, h: 16 }, t(13, 700), c.ink);
    if (chart.note) text(ctx, chart.note, { x: M + INNER / 2, y: y + 1, w: INNER / 2, h: 15 }, t(11.5, 500), c.soft, 'right');
    const axisWidth = Math.max(...chart.axis.map((a) => measure(ctx, a, t(10.5, 500)))) + 10;
    const plot = { x: M + axisWidth, y: y + 30, w: INNER - axisWidth - 4, h: ROWS.chart - 68 }, bottom = plot.y + plot.h;
    const points = chart.values.map((v, i) => [plot.x + chart.positions[i] * plot.w, bottom - (v / chart.ceiling) * plot.h] as const);
    ctx.save();
    ctx.beginPath(); ctx.moveTo(points[0][0], bottom); points.forEach(([x, py]) => ctx.lineTo(x, py)); ctx.lineTo(points.at(-1)![0], bottom); ctx.closePath(); ctx.clip();
    stroke(alpha(c.line, 0.4), 0.7);
    for (let x = plot.x - plot.h; x < plot.x + plot.w; x += 5) segment(x, bottom, x + plot.h, plot.y);
    ctx.restore();
    chart.axis.forEach((value, i) => {
      const at = bottom - (plot.h * i) / 2;
      stroke(i ? c.soft : c.ink, i ? 0.6 : 1.2, i ? [6, 3, 1.5, 3] : []); segment(plot.x, at, plot.x + plot.w, at);
      text(ctx, value, { x: M, y: at - 7, w: axisWidth - 8, h: 14 }, t(10.5, 500), c.soft, 'right');
    });
    stroke(c.ink, 1.2, []); segment(plot.x, plot.y - 6, plot.x, bottom);
    stroke(c.line, 1.8); ctx.lineJoin = 'round';
    ctx.beginPath(); points.forEach(([x, py], i) => (i ? ctx.lineTo(x, py) : ctx.moveTo(x, py))); ctx.stroke();
    if (points.length <= 31) points.forEach(([x, py]) => { ctx.fillStyle = c.paper; ctx.beginPath(); ctx.arc(x, py, 2.4, 0, Math.PI * 2); ctx.fill(); stroke(c.line, 1.2); ctx.stroke(); });
    chart.ticks.forEach((tick, i) => {
      const x = plot.x + tick.at * plot.w, at = tickBox(i, chart.ticks.length, x, plot.x, WIDTH - M, 80);
      stroke(c.ink, 1); segment(x, bottom, x, bottom + 5);
      text(ctx, tick.label, { x: at.x, y: bottom + 8, w: 80, h: 14 }, t(11, 500), c.ink, at.align);
    });
    ctx.setLineDash([]);
  }

  // Each tool as a numbered callout: its mark, name and amount, a line of figures, then its models measured off.
  p.tools.forEach((tool, index) => {
    y = L.tools[index];
    if (index) { stroke(c.soft, 0.8, [14, 3, 2, 3]); segment(M, y - ROWS.gap / 2, WIDTH - M, y - ROWS.gap / 2); ctx.setLineDash([]); }
    stroke(c.ink, 1.2); ctx.beginPath(); ctx.arc(M + 9, y + 16, 9, 0, Math.PI * 2); ctx.stroke();
    middleText(ctx, tool.id === 'other' ? '+' : String(index + 1), M, y + 16.5, 18, t(11, 700), c.ink, 'center');
    const mark = p.marks.tools.get(tool.id), indent = 26 + (mark ? 22 : 0), costWidth = measure(ctx, tool.cost, t(17, 700));
    if (mark) drawMark(ctx, mark, { x: M + 26, y: y + 8, w: 16, h: 16 }, c.ink);
    middleText(ctx, tool.name, M + indent, y + 16, INNER - indent - costWidth - 12, t(17, 700), c.ink);
    middleText(ctx, tool.cost, WIDTH - M - costWidth - 2, y + 16, costWidth + 2, t(17, 700), c.ink, 'right');
    const line = [tool.tokens, tool.share, tool.tag].filter(Boolean).join(' · '), used = Math.min(INNER - 26, measure(ctx, line, t(12, 500)));
    middleText(ctx, line, M + 26, y + 37, INNER - 26, t(12, 500), c.soft);
    if (tool.unpriced && used + 70 < INNER - 26) middleText(ctx, '· ' + tool.unpriced, M + 26 + used + 6, y + 37, INNER - 26 - used - 6, t(12, 600), c.warn);
    y += ROWS.head;
    const MODEL = ROWS.model;
    if (tool.members.length) { drawMembers(ctx, p, tool, M + 26, y + 10.5, INNER - 26, t(13, 600), c.ink, c.ink, 13); y += MODEL; }
    for (const m of tool.models) {
      const nameRoom = 150, barLeft = M + 26 + nameRoom + 6, barRoom = WIDTH - M - 112 - barLeft;
      const mid = y + 10.5;
      middleText(ctx, m.name, M + 26, mid, nameRoom, t(13, 500), m.other ? c.soft : c.ink);
      const length = Math.max(2, barRoom * m.ratio);
      stroke(m.other ? c.soft : c.line, 1.2); segment(barLeft, mid, barLeft + length, mid);
      stroke(m.other ? c.soft : c.line, 1); segment(barLeft, mid - 3.5, barLeft, mid + 3.5); segment(barLeft + length, mid - 3.5, barLeft + length, mid + 3.5);
      if (m.note) middleText(ctx, m.note, barLeft + length + 6, mid, barRoom - length - 6, t(11, 600), c.warn);
      middleText(ctx, m.amount, WIDTH - M - 106, mid, 64, t(13, 600), c.ink, 'right');
      middleText(ctx, m.share, WIDTH - M - 38, mid, 38, t(13, 500), c.soft, 'right');
      y += MODEL;
    }
  });

  // The title block: the signature in a ruled box.
  stroke(c.ink, 1.4); ctx.strokeRect(M, L.sign + 8, INNER, ROWS.sign - 14);
  drawSignature(ctx, p, project, L.sign + 20, { name: t(16, 700), small: (s) => t(s, 500), size: 12, ink: c.ink, muted: c.soft });
  return canvas;
}
