// 收据: a till receipt. Thermal paper to the edges with a torn foot, everything in a monospaced face: the estimate
// printed large and centred, the three figures on dotted leaders, the hour by hour in banded bars, each tool as a
// line item with its models under it, a barcode, and the signature. White paper by day, a dark slip by night.
import { font, MONO } from './measure';
import { BOTTOM_BAND, canvasFor, drawMark, measure, middleText, text, WIDTH } from './shareCommon';
import { drawAmount, drawBars, drawMasthead, drawMembers, drawSignature, figures, fitAmount, HERO_BASELINE, layout, poster, posterDate, ROWS, seeded, slotCentre, tickBox, SHARE_TOOLS } from './sharePoster';
import type { Dashboard, Project } from './types';

const M = 28, INNER = WIDTH - M * 2;
const mono = (size: number, weight = 500) => font(size, weight, MONO);

const palettes = {
  light: { paper: '#FBFAF6', desk: '#D8D4CC', ink: '#2A2926', faded: '#76726B', rule: 'rgba(42, 41, 38, 0.45)', band: 'rgba(251, 250, 246, 0.4)', warn: '#B4410E' },
  dark: { paper: '#1E1E1C', desk: '#0D0D0C', ink: '#ECE9E2', faded: '#A19D94', rule: 'rgba(236, 233, 226, 0.4)', band: 'rgba(30, 30, 28, 0.4)', warn: '#F0915A' },
};

export async function drawReceiptPicture(d: Dashboard, project: Project, dark: boolean, tools = SHARE_TOOLS): Promise<HTMLCanvasElement> {
  const c = dark ? palettes.dark : palettes.light;
  const p = await poster(d, c.ink, tools), L = layout(p), height = L.height;
  p.date = posterDate(d, '-', ' ~ ');
  const { canvas, ctx } = canvasFor(height);
  const dashed = (y: number) => { ctx.strokeStyle = c.rule; ctx.lineWidth = 1; ctx.setLineDash([4, 3]); ctx.beginPath(); ctx.moveTo(M, y); ctx.lineTo(WIDTH - M, y); ctx.stroke(); ctx.setLineDash([]); };
  const double = (y: number) => { ctx.fillStyle = c.ink; ctx.fillRect(M, y, INNER, 1); ctx.fillRect(M, y + 3, INNER, 1); };

  // The desk, then the slip over it, torn at its foot.
  const tear = 8, foot = height - BOTTOM_BAND + 14 - tear;
  ctx.fillStyle = c.desk; ctx.fillRect(0, 0, WIDTH, height);
  ctx.fillStyle = c.paper;
  ctx.beginPath(); ctx.moveTo(0, 0); ctx.lineTo(WIDTH, 0); ctx.lineTo(WIDTH, foot);
  for (let x = WIDTH, i = 0; x >= 0; x -= 7, i++) ctx.lineTo(x, foot + (i % 2 ? tear : 0));
  ctx.lineTo(0, foot); ctx.closePath(); ctx.fill();
  // Thermal print: faint grain across the paper.
  const random = seeded([...d.date].reduce((s, ch) => s + ch.charCodeAt(0), 0) + 3);
  ctx.fillStyle = dark ? 'rgba(255, 255, 255, 0.025)' : 'rgba(0, 0, 0, 0.025)';
  for (let i = 0; i < 1400; i++) ctx.fillRect(random() * WIDTH, random() * foot, 0.8, 0.8);

  // The shop's header: our star and name, the date; then the title between double rules.
  drawMasthead(ctx, p, project.name, { ink: c.ink, font: (s, w) => mono(s, w), name: 15, date: 13, left: M, right: WIDTH - M, weight: 800 });
  double(L.title + 2);
  text(ctx, p.title, { x: M, y: L.title + 13, w: INNER, h: 26 }, font(21, 800), c.ink, 'center');
  double(L.title + 44);

  // The estimate, large and centred, as a receipt's total.
  text(ctx, '** ' + p.costLabel + ' **', { x: M, y: L.hero + 6, w: INNER, h: 15 }, mono(11.5, 600), c.faded, 'center');
  const hero = fitAmount(ctx, p.cost, INNER - 20, 70, 30, (s) => mono(s, 800), -0.04);
  drawAmount(ctx, p.cost, WIDTH / 2, L.hero + HERO_BASELINE, hero, (s) => mono(s, 800), c.ink, c.ink, -0.04, 'center');

  // The three figures on dotted leaders.
  figures(p).forEach(([name, value], i) => {
    const middle = L.stats + 12 + i * 22, nameWidth = measure(ctx, name, mono(13, 600)), valueWidth = measure(ctx, value, mono(15, 800));
    middleText(ctx, name, M, middle, nameWidth + 2, mono(13, 600), c.ink);
    middleText(ctx, value, WIDTH - M - valueWidth - 2, middle, valueWidth + 2, mono(15, 800), c.ink, 'right');
    ctx.fillStyle = c.faded;
    for (let x = M + nameWidth + 8; x < WIDTH - M - valueWidth - 8; x += 5) ctx.fillRect(x, middle + 3, 1.3, 1.3);
  });
  dashed(L.stats + ROWS.stats - 4);

  // The hour by hour in bars banded like thermal print.
  const chart = p.chart;
  if (chart && L.chart !== null) {
    const y = L.chart + 8;
    text(ctx, chart.label, { x: M, y, w: INNER / 2, h: 15 }, mono(12, 700), c.ink);
    if (chart.note) text(ctx, chart.note, { x: M + INNER / 2, y, w: INNER / 2, h: 15 }, mono(11), c.faded, 'right');
    const plot = { x: M, y: y + 28, w: INNER, h: ROWS.chart - 78 };
    drawBars(ctx, chart, plot, c.ink, 0.28);
    ctx.fillStyle = c.band;
    for (let by = plot.y; by < plot.y + plot.h; by += 2.4) ctx.fillRect(plot.x, by, plot.w, 0.8);
    ctx.fillStyle = c.ink; ctx.fillRect(plot.x, plot.y + plot.h, plot.w, 1);
    chart.ticks.forEach((tick, i) => {
      const at = tickBox(i, chart.ticks.length, slotCentre(chart, plot, tick.index), M, WIDTH - M, 80);
      text(ctx, tick.label, { x: at.x, y: plot.y + plot.h + 6, w: 80, h: 13 }, mono(10.5), c.faded, at.align);
    });
    dashed(L.chart + ROWS.chart - 4);
  }

  // Each tool as a line item: its mark, name and amount, a line of figures (or its tag in brackets), then its models.
  p.tools.forEach((t, index) => {
    let y = L.tools[index];
    const mark = p.marks.tools.get(t.id), indent = mark || t.id === 'other' ? 22 : 0, costWidth = measure(ctx, t.cost, mono(14.5, 800));
    if (mark) drawMark(ctx, mark, { x: M, y: y + 8.5, w: 15, h: 15 }, c.ink);
    middleText(ctx, t.name, M + indent, y + 16, INNER - indent - costWidth - 12, font(14.5, 700), c.ink);
    middleText(ctx, t.cost, WIDTH - M - costWidth - 2, y + 16, costWidth + 2, mono(14.5, 800), c.ink, 'right');
    const line = [t.tokens, t.share].filter(Boolean).join(' · ') + (t.tag ? ` [${t.tag}]` : ''), lineWidth = Math.min(INNER - indent, measure(ctx, line, mono(11)));
    middleText(ctx, line, M + indent, y + 36, INNER - indent, mono(11), c.faded);
    if (t.unpriced && lineWidth + 70 < INNER - indent) middleText(ctx, '* ' + t.unpriced, M + indent + lineWidth + 8, y + 36, INNER - indent - lineWidth - 8, mono(10.5, 600), c.warn);
    y += ROWS.head;
    const middle = () => y + ROWS.model / 2;
    if (t.members.length) { drawMembers(ctx, p, t, M + indent, middle(), INNER - indent, mono(11.5), c.ink, c.ink, 12); y += ROWS.model; }
    for (const m of t.models) {
      const nameRoom = INNER - indent - 140, nameWidth = Math.min(nameRoom, measure(ctx, '- ' + m.name, mono(11.5)));
      middleText(ctx, '- ' + m.name, M + indent, middle(), nameRoom, mono(11.5), m.other ? c.faded : c.ink);
      if (m.note && nameWidth + 30 < nameRoom) middleText(ctx, m.note, M + indent + nameWidth + 6, middle(), nameRoom - nameWidth - 6, mono(10, 600), c.warn);
      middleText(ctx, m.amount, WIDTH - M - 140, middle(), 90, mono(11.5, 600), c.ink, 'right');
      middleText(ctx, m.share, WIDTH - M - 46, middle(), 46, mono(11.5), c.faded, 'right');
      y += ROWS.model;
    }
  });
  dashed(L.foot - 4);

  // A barcode of the day, then the signature.
  const bars = seeded([...p.date].reduce((s, ch) => s + ch.charCodeAt(0), 0) + 11), unit = 1.15;
  let x = WIDTH / 2 - 110;
  ctx.fillStyle = c.ink;
  while (x < WIDTH / 2 + 108) { const w = (1 + Math.floor(bars() * 3)) * unit; ctx.fillRect(x, L.foot + 6, w, 22); x += w + (1 + Math.floor(bars() * 3)) * unit; }
  drawSignature(ctx, p, project, L.sign + 20, { name: mono(14, 800), small: (s) => mono(s), size: 10.5, ink: c.ink, muted: c.faded });
  return canvas;
}
