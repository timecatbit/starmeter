// The terminal style: the terminal is the whole picture (no desk around it: chat apps shrink a picture to fit, and a
// frame only made the text smaller), its title bar the band at the top, mostly in a monospaced face, with hex rain
// beside the estimate and under the signature. It follows light and dark.
import { smoothCurves, tracePath } from './chart';
import { accent, alpha, systemColor } from './colors';
import { font, MONO } from './measure';
import { BOTTOM_BAND, canvasFor, crownSeed, drawMark, fade, grain, measure, middleText, roundRect, shadow, text, TOP_BAND, WIDTH, type Box, type Fade } from './shareCommon';
import { drawAmount, drawMasthead, drawMembers, drawSignature, figures, fitAmount, HERO_BASELINE, layout, poster, posterDate, ROWS, tickBox, SHARE_TOOLS } from './sharePoster';
import type { Dashboard, Project } from './types';

const PAD = 26;
const mono = (size: number, weight = 400) => font(size, weight, MONO);
const ui = (size: number, weight: number) => font(size, weight);


const rgb = (r: number, g: number, b: number, a = 1) => `rgba(${Math.round(r * 255)}, ${Math.round(g * 255)}, ${Math.round(b * 255)}, ${a})`;
const palettes = {
  dark: { window: rgb(0.06, 0.08, 0.1), bar: rgb(0.09, 0.11, 0.14), text: rgb(0.9, 0.93, 0.95),
    muted: rgb(0.62, 0.66, 0.7), comment: rgb(0.45, 0.5, 0.55), accent: rgb(0.25, 0.85, 0.72), rule: rgb(1, 1, 1, 0.1) },
  light: { window: rgb(1, 1, 1), bar: rgb(0.95, 0.96, 0.97), text: rgb(0.11, 0.13, 0.15),
    muted: rgb(0.36, 0.4, 0.44), comment: rgb(0.52, 0.56, 0.6), accent: rgb(0.02, 0.54, 0.46), rule: rgb(0, 0, 0, 0.09) },
};

export async function drawTerminalPicture(d: Dashboard, project: Project, dark: boolean, tools = SHARE_TOOLS): Promise<HTMLCanvasElement> {
  const c = dark ? palettes.dark : palettes.light;
  const p = await poster(d, c.text, tools);
  p.date = posterDate(d, '-', ' → ');
  const left = PAD, width = WIDTH - PAD * 2, right = left + width, L = layout(p), height = L.height;
  const { canvas, ctx } = canvasFor(height);
  const seed = crownSeed(d);

  // The pattern: columns of hex digits raining down, each led by a bright head with a fading trail, keyed by their
  // place in the picture so every patch belongs to the same field.
  const rain = (rect: Box, fading: Fade, brightness = 1) => {
    const pitch = 10, rowsCount = Math.max(1, Math.floor(rect.h / pitch)), columns = Math.floor(rect.w / pitch) + 1, digits = '0123456789abcdef';
    ctx.font = mono(8.5, 500); ctx.textAlign = 'center'; ctx.textBaseline = 'alphabetic';
    for (let column = 0; column < columns; column++) {
      const x = rect.x + column * pitch, key = Math.round(x / pitch), head = grain(key, 7, seed) % (rowsCount + 4);
      for (let row = 0; row < rowsCount; row++) {
        const y = rect.y + row * pitch, g = grain(key, row + Math.floor(rect.y / pitch), seed), behind = head - row;
        const f = fade(fading, x + pitch / 2, y + pitch / 2, rect);
        const strength = behind === 0 ? 1 : behind > 0 && behind < 5 ? 0.62 - behind * 0.11 : g % 3 === 0 ? 0.14 : 0;
        if (strength <= 0 || f <= 0.02) continue;
        ctx.fillStyle = alpha(behind === 0 ? c.text : c.accent, strength * f * brightness);
        ctx.fillText(digits[g % 16], x + 1 + pitch / 2, y + 8.5 * 0.952);
      }
    }
  };
  const dashed = (y: number) => {
    ctx.beginPath(); ctx.moveTo(left, y); ctx.lineTo(right, y); ctx.setLineDash([3, 3]); ctx.lineWidth = 1; ctx.strokeStyle = c.rule; ctx.stroke(); ctx.setLineDash([]);
  };
  const line = (x1: number, y1: number, x2: number, y2: number, colour: string) => {
    ctx.beginPath(); ctx.moveTo(x1, y1); ctx.lineTo(x2, y2); ctx.lineWidth = 1; ctx.strokeStyle = colour; ctx.stroke();
  };

  // The window's own background to every edge, lit faintly from the top left; its title bar is the band at the top.
  ctx.fillStyle = c.window; ctx.fillRect(0, 0, WIDTH, height);
  const glow = ctx.createRadialGradient(WIDTH * 0.15, 90, 0, WIDTH * 0.15, 90, WIDTH * 0.9);
  glow.addColorStop(0, alpha(c.accent, dark ? 0.16 : 0.12)); glow.addColorStop(1, alpha(c.accent, 0));
  ctx.fillStyle = glow; ctx.fillRect(0, 0, WIDTH, height);
  ctx.fillStyle = c.bar; ctx.fillRect(0, 0, WIDTH, TOP_BAND);
  line(0, TOP_BAND - 0.5, WIDTH, TOP_BAND - 0.5, c.rule);
  drawMasthead(ctx, p, project.name, { ink: c.text, dateInk: c.accent, font: (s, w) => mono(s, w), name: 14.5, date: 12.5, left, right, weight: 700 });
  rain({ x: 0, y: height - BOTTOM_BAND + 8, w: WIDTH, h: BOTTOM_BAND - 8 }, 'up');

  // The command that printed this, then the title.
  let y = L.title + 4;
  const flag = ({ today: 'today', yesterday: 'yesterday', '24h': '24h', '7d': '7d', '30d': '30d', week: 'week', month: 'month', all: 'all' } as Record<string, string>)[d.rangeKey] ?? d.rangeKey;
  middleText(ctx, '❯', left, y + 8, 14, mono(12.5, 700), c.accent);
  middleText(ctx, 'starmeter usage --range ' + flag, left + 16, y + 8, width - 16, mono(12, 500), c.muted);
  text(ctx, p.title, { x: left, y: L.title + 22, w: width, h: 28 }, ui(22, 700), c.text);
  y = L.hero + 4;

  // The estimate, set like output, with the cursor after it and rain fading in beside it from the window's edge.
  rain({ x: WIDTH - 140, y: y, w: 140, h: 104 }, 'left', 0.55);
  text(ctx, '// ' + p.costLabel, { x: left, y, w: width, h: 15 }, mono(11), c.comment);
  const size = fitAmount(ctx, p.cost, width * 0.84, 64, 24, (s) => mono(s, 700), -0.02), baseline = L.hero + HERO_BASELINE;
  const amountWidth = drawAmount(ctx, p.cost, left - 1, baseline, size, (s) => mono(s, 700), c.text, c.accent, -0.02);
  ctx.fillStyle = alpha(c.accent, 0.85);
  ctx.fillRect(left + amountWidth + 6, baseline - size * 0.7 - 1, size * 0.32, size * 0.7 + 3);
  y = L.stats + 2;

  // Three figures in a ruled table.
  const table: Box = { x: left, y, w: width, h: 56 }, column = width / 3;
  ctx.fillStyle = c.bar; roundRect(ctx, table, 7); ctx.fill();
  ctx.strokeStyle = c.rule; ctx.lineWidth = 1; roundRect(ctx, { x: table.x + 0.5, y: table.y + 0.5, w: table.w - 1, h: table.h - 1 }, 6.5); ctx.stroke();
  figures(p).forEach(([name, value], i) => {
    const cx = table.x + i * column;
    if (i > 0) line(cx, table.y, cx, table.y + table.h, c.rule);
    text(ctx, '▸ ' + name, { x: cx + 12, y: table.y + 9, w: column - 18, h: 14 }, mono(10, 500), c.muted);
    const measured = measure(ctx, value, mono(18, 600)), fitted = measured <= column - 22 ? 18 : Math.max(11, Math.floor((18 * (column - 22)) / measured));
    text(ctx, value, { x: cx + 12, y: table.y + 27, w: column - 18, h: 24 }, mono(fitted, 600), c.text);
  });

  // The trend on an oscilloscope grid: the total glows, each listed tool a thin line in its own colour.
  const chart = p.chart;
  if (chart && L.chart !== null) {
    y = L.chart;
    text(ctx, '// ' + chart.label, { x: left, y, w: width / 2, h: 15 }, mono(10.5), c.comment);
    if (chart.note) text(ctx, chart.note, { x: left + width / 2, y, w: width / 2, h: 15 }, mono(10), c.comment, 'right');
    const plot = { x: left, y: y + 30, w: width, h: ROWS.chart - 66 }, bottom = plot.y + plot.h;
    ctx.setLineDash([1, 3]); ctx.lineWidth = 0.75; ctx.strokeStyle = c.rule;
    for (let f = 0; f <= 1.0001; f += 0.2) { ctx.beginPath(); ctx.moveTo(plot.x + f * plot.w, plot.y); ctx.lineTo(plot.x + f * plot.w, bottom); ctx.stroke(); }
    for (const f of [0, 0.5, 1]) {
      const yy = bottom - f * plot.h;
      ctx.setLineDash(f > 0 ? [1, 3] : []); ctx.lineWidth = f > 0 ? 0.75 : 1; ctx.strokeStyle = f > 0 ? c.rule : alpha(c.muted, 0.35);
      ctx.beginPath(); ctx.moveTo(plot.x, yy); ctx.lineTo(plot.x + plot.w, yy); ctx.stroke();
    }
    ctx.setLineDash([]);
    const curves = smoothCurves([...chart.series.map((s) => s.values), chart.rest], chart.positions, plot, chart.ceiling), total = curves[curves.length - 1];
    ctx.beginPath(); tracePath(ctx, total); ctx.lineTo(plot.x + plot.w, bottom); ctx.lineTo(plot.x, bottom); ctx.closePath();
    const fill = ctx.createLinearGradient(0, plot.y, 0, bottom);
    fill.addColorStop(0, alpha(c.accent, dark ? 0.28 : 0.18)); fill.addColorStop(1, alpha(c.accent, 0));
    ctx.fillStyle = fill; ctx.fill();
    const stroke = (curve: typeof total, colour: string, isTotal: boolean) => {
      ctx.save();
      shadow(ctx, alpha(colour, isTotal ? (dark ? 0.8 : 0.45) : dark ? 0.5 : 0), isTotal ? 4 : 2);
      ctx.beginPath(); tracePath(ctx, curve);
      ctx.lineWidth = isTotal ? 2.2 : 1.2; ctx.lineCap = 'round'; ctx.lineJoin = 'round'; ctx.strokeStyle = colour; ctx.stroke();
      ctx.restore();
    };
    chart.series.forEach((s, i) => stroke(curves[i], alpha(accent(s.id, dark), 0.9), false));
    stroke(total, c.accent, true);
    for (const f of [0.5, 1]) {
      const value = chart.axis[f === 1 ? 2 : 1], yy = bottom - f * plot.h;
      ctx.fillStyle = alpha(c.window, 0.85); ctx.fillRect(left, yy - 13, measure(ctx, value, mono(9)) + 5, 12);
      text(ctx, value, { x: left + 1, y: yy - 13, w: 80, h: 12 }, mono(9), c.comment);
    }
    const end = total.end;
    ctx.fillStyle = alpha(c.accent, 0.22); ctx.beginPath(); ctx.arc(end[0], end[1], 7, 0, Math.PI * 2); ctx.fill();
    ctx.fillStyle = c.accent; ctx.beginPath(); ctx.arc(end[0], end[1], 3, 0, Math.PI * 2); ctx.fill();
    chart.ticks.forEach((tick, i) => {
      const at = tickBox(i, chart.ticks.length, plot.x + tick.at * plot.w, left, right, 80);
      text(ctx, tick.label, { x: at.x, y: bottom + 7, w: 80, h: 13 }, mono(9.5), c.comment, at.align);
    });
  }

  // Each tool as a listing: its mark, name and amount; tokens and share, or its tag; then its first models as a tree
  // with cell bars. 其他工具 lists the tools folded into it instead.
  const orange = systemColor('orange', dark), noteFont = mono(9.5, 500);
  p.tools.forEach((t, index) => {
    const colour = t.id === 'other' ? c.comment : accent(t.id, dark), top = L.tools[index];
    dashed(top - ROWS.gap / 2);
    ctx.fillStyle = alpha(colour, t.tag ? 0.45 : 1); ctx.beginPath(); ctx.arc(left + 4, top + 18, 4, 0, Math.PI * 2); ctx.fill();
    const nameFont = ui(14.5, 600), costFont = mono(14.5, 600), costWidth = measure(ctx, t.cost, costFont), mark = p.marks.tools.get(t.id), markRoom = mark ? 21 : 0;
    if (mark) drawMark(ctx, mark, { x: left + 16, y: top + 10.5, w: 15, h: 15 }, c.text);
    middleText(ctx, t.name, left + 16 + markRoom, top + 18, width - 16 - markRoom - costWidth - 12, nameFont, c.text);
    middleText(ctx, t.cost, right - costWidth - 2, top + 18, costWidth + 2, costFont, c.text, 'right');
    const figuresLine = [t.tokens, t.share].filter(Boolean).join(' · '), lineFont = mono(10.5, 500);
    let tagWidth = 0;
    if (t.tag) {
      tagWidth = measure(ctx, t.tag, noteFont) + 14;
      const box: Box = { x: right - tagWidth, y: top + 30, w: tagWidth, h: 16 };
      ctx.strokeStyle = alpha(c.muted, 0.45); ctx.lineWidth = 1; roundRect(ctx, { x: box.x + 0.5, y: box.y + 0.5, w: box.w - 1, h: box.h - 1 }, 3.5); ctx.stroke();
      middleText(ctx, t.tag, box.x + 7, box.y + 8, box.w - 14, noteFont, c.muted, 'center');
    }
    const room = width - 16 - tagWidth - (tagWidth ? 10 : 0), used = Math.min(room, measure(ctx, figuresLine, lineFont));
    middleText(ctx, figuresLine, left + 16, top + 38, room, lineFont, c.muted);
    if (t.unpriced && used + 60 < room) middleText(ctx, '! ' + t.unpriced, left + 16 + used + 10, top + 38, room - used - 10, mono(10, 500), orange);
    let my = top + ROWS.head;
    const MODEL = ROWS.model, mid = (row: number) => row + MODEL / 2;
    if (t.members.length) drawMembers(ctx, p, t, left + 16, mid(my), width - 16, mono(11), c.text, c.text, 13);
    // Models hang off a drawn tree: one stem, a tick per model.
    if (t.models.length) {
      const stem = left + 20, branch = alpha(c.comment, 0.7);
      line(stem, my + 1, stem, mid(my) + (t.models.length - 1) * MODEL, branch);
      t.models.forEach((_, i) => { const at = mid(my) + i * MODEL; line(stem, at, stem + 10, at, branch); });
    }
    // Columns sized to this tool's longest model name and amount; the cells take what is left.
    const modelFont = mono(11), figureFont = mono(11, 500);
    const nameColumn = Math.min(170, Math.max(0, ...t.models.map((m) => measure(ctx, m.name, modelFont) + (m.note ? measure(ctx, m.note, noteFont) + 6 : 0))) + 12);
    const amountColumn = Math.max(50, Math.max(0, ...t.models.map((m) => measure(ctx, m.amount, figureFont))) + 4);
    const barLeft = left + 40 + nameColumn, slots = Math.max(4, Math.min(16, Math.floor((right - 44 - amountColumn - 10 - barLeft) / 6.5)));
    for (const m of t.models) {
      let nameRoom = nameColumn - 8;
      if (m.note) {
        const w = Math.min(nameRoom * 0.55, measure(ctx, m.note, noteFont)), nw = Math.min(nameRoom - w - 6, measure(ctx, m.name, modelFont));
        middleText(ctx, m.note, left + 38 + nw + 6, mid(my), w + 2, noteFont, orange);
        nameRoom = nw;
      }
      middleText(ctx, m.name, left + 38, mid(my), nameRoom + 1, modelFont, m.other ? c.muted : c.text);
      const lit = m.ratio > 0 ? Math.max(1, Math.round(m.ratio * slots)) : 0;
      for (let k = 0; k < slots; k++) {
        ctx.fillStyle = k < lit ? alpha(m.other ? c.comment : colour, t.tag ? 0.6 : 0.95) : c.rule;
        roundRect(ctx, { x: barLeft + k * 6.5, y: mid(my) - 4, w: 5, h: 8 }, 1); ctx.fill();
      }
      middleText(ctx, m.amount, right - 44 - amountColumn, mid(my), amountColumn, figureFont, c.text, 'right');
      middleText(ctx, m.share, right - 40, mid(my), 40, mono(10.5), c.muted, 'right');
      my += MODEL;
    }
  });
  // The session waits at a prompt, then the signature.
  dashed(L.foot - ROWS.gap / 2);
  middleText(ctx, '❯', left, L.foot + 14, 14, mono(12.5, 700), c.accent);
  ctx.fillStyle = alpha(c.accent, 0.85); ctx.fillRect(left + 16, L.foot + 7, 7.5, 14);
  drawSignature(ctx, p, project, L.sign + 20, { name: ui(14, 600), small: (s) => mono(s), size: 10.5, ink: c.text, muted: c.muted });
  return canvas;
}
