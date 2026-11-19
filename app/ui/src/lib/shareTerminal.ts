// The terminal style: the terminal is the whole picture (no desk around it: chat apps shrink a picture to fit, and a
// frame only made the text smaller), mostly in a monospaced face, with hex rain above, below and beside the estimate.
import { chartPositions, smoothCurves, tracePath } from './chart';
import { accent, alpha, systemColor } from './colors';
import { axisTokens, currency, headline, modelAmount, modelName, share, statsForms, tokenLine, tokens } from './format';
import { font, MONO } from './measure';
import {
  BOTTOM_BAND, canvasFor, clientLines, countedRows, crownSeed, dollars, drawBrand, drawHeatmap, drawMark, fade, grain, heatmapGridHeight, legendNameOf, loadMarks,
  measure, middleText, priceLine, roundRect, sharedRows, shownHeatmap, shownRows, tag, text, title, TOP_BAND, WIDTH, type Box, type Fade,
} from './shareCommon';
import type { Dashboard, Project, ProviderUsage } from './types';

const MARGIN = 0, PAD = 26, BAR = 34, CHART = 170, MODEL = 20, LEGEND = 18;
const CLIENT_WIDTH = WIDTH - MARGIN * 2 - PAD * 2 - 16;
const mono = (size: number, weight = 400) => font(size, weight, MONO);
const ui = (size: number, weight: number) => font(size, weight);
const CLIENT_FONT = mono(10.5);

/** What the picture covers, written as dates: today, a 24-hour window, or first to last day. */
export function span(d: Dashboard): string {
  const numbers = (value: string) => value.split(/[-T:]/).map(Number).filter((n) => !Number.isNaN(n));
  if (d.rangeKey === 'today' || d.period?.length !== 2) return d.date;
  const a = numbers(d.period[0]), b = numbers(d.period[1]);
  if (a.length < 3 || b.length !== a.length) return d.date;
  const p = (n: number) => String(n).padStart(2, '0');
  if (a.length >= 5) return `${p(a[1])}-${p(a[2])} ${p(a[3])}:${p(a[4])} → ${p(b[1])}-${p(b[2])} ${p(b[3])}:${p(b[4])}`;
  if (a[0] !== b[0]) return `${a[0]}-${p(a[1])}-${p(a[2])} → ${b[0]}-${p(b[1])}-${p(b[2])}`;
  return a[1] === b[1] && a[2] === b[2] ? `${a[0]}-${p(a[1])}-${p(a[2])}` : `${a[0]}-${p(a[1])}-${p(a[2])} → ${p(b[1])}-${p(b[2])}`;
}

/** Shares as a row of cells: each tool gets cells in proportion, at least one if it used anything. */
function cells(shares: number[], count: number): number[] {
  const result = shares.map((s) => Math.max(s > 0 ? 1 : 0, Math.floor(s * count)));
  const order = shares.map((_, i) => i).sort((a, b) => (shares[b] * count - Math.floor(shares[b] * count)) - (shares[a] * count - Math.floor(shares[a] * count)));
  for (let i = 0; result.reduce((x, y) => x + y, 0) < count && order.length; i++) result[order[i % order.length]]++;
  while (result.reduce((x, y) => x + y, 0) > count) result[result.indexOf(Math.max(...result))]--;
  return result;
}

const rgb = (r: number, g: number, b: number, a = 1) => `rgba(${Math.round(r * 255)}, ${Math.round(g * 255)}, ${Math.round(b * 255)}, ${a})`;
const palettes = {
  dark: { base: rgb(0.04, 0.055, 0.07), dots: rgb(1, 1, 1, 0.07), window: rgb(0.06, 0.08, 0.1), bar: rgb(0.09, 0.11, 0.14), text: rgb(0.9, 0.93, 0.95),
    muted: rgb(0.62, 0.66, 0.7), comment: rgb(0.45, 0.5, 0.55), accent: rgb(0.25, 0.85, 0.72), rule: rgb(1, 1, 1, 0.1) },
  light: { base: rgb(0.95, 0.96, 0.97), dots: rgb(0, 0, 0, 0.1), window: rgb(1, 1, 1), bar: rgb(0.95, 0.96, 0.97), text: rgb(0.11, 0.13, 0.15),
    muted: rgb(0.36, 0.4, 0.44), comment: rgb(0.52, 0.56, 0.6), accent: rgb(0.02, 0.54, 0.46), rule: rgb(0, 0, 0, 0.09) },
};

export async function drawTerminalPicture(d: Dashboard, project: Project, dark: boolean): Promise<HTMLCanvasElement> {
  const measureContext = document.createElement('canvas').getContext('2d')!;
  const rows = shownRows(d), counted = countedRows(d), shared = sharedRows(d), heatmap = shownHeatmap(d);
  const blockHeight = (p: ProviderUsage) => 90 + (priceLine(p) ? 17 : 0) + clientLines(measureContext, p, CLIENT_WIDTH, CLIENT_FONT).length * 17 + Math.max(1, p.models.length) * MODEL;
  const inner = WIDTH - MARGIN * 2 - PAD * 2;
  const heatmapHeight = heatmap ? 24 + heatmapGridHeight(heatmap, inner) + 24 : 0;
  const windowHeight = BAR + PAD * 2 + 32 + 46 + 80 + 80 + (d.dates.length > 1 ? CHART : 0) + heatmapHeight
    + (shared.length ? 56 + Math.ceil(shared.length / 3) * LEGEND : 0) + rows.reduce((s, p) => s + blockHeight(p), 0) + 26;
  const height = TOP_BAND + windowHeight + 58 + BOTTOM_BAND;
  const { canvas, ctx } = canvasFor(height);
  const c = dark ? palettes.dark : palettes.light;
  const marks = await loadMarks(rows, c.text);
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
  const dashed = (y: number, from: number, to: number) => {
    ctx.beginPath(); ctx.moveTo(from, y); ctx.lineTo(to, y); ctx.setLineDash([3, 3]); ctx.lineWidth = 1; ctx.strokeStyle = c.rule; ctx.stroke(); ctx.setLineDash([]);
  };
  const line = (x1: number, y1: number, x2: number, y2: number, colour: string) => {
    ctx.beginPath(); ctx.moveTo(x1, y1); ctx.lineTo(x2, y2); ctx.lineWidth = 1; ctx.strokeStyle = colour; ctx.stroke();
  };
  const prompt = (command: string, x: number, middle: number, cursor: boolean) => {
    middleText(ctx, '❯', x, middle, 14, mono(12.5, 700), c.accent);
    middleText(ctx, command, x + 16, middle, WIDTH, mono(12, 500), c.text);
    if (cursor) {
      const after = x + 16 + measure(ctx, command, mono(12, 500)) + (command ? 6 : 0);
      ctx.fillStyle = alpha(c.accent, 0.85); ctx.fillRect(after, middle - 7, 7.5, 14);
    }
  };

  // The window's own background to every edge, lit faintly from the top left.
  ctx.fillStyle = c.window; ctx.fillRect(0, 0, WIDTH, height);
  const glow = ctx.createRadialGradient(WIDTH * 0.15, 60, 0, WIDTH * 0.15, 60, WIDTH * 0.9);
  glow.addColorStop(0, alpha(c.accent, dark ? 0.16 : 0.12)); glow.addColorStop(1, alpha(c.accent, 0));
  ctx.fillStyle = glow; ctx.fillRect(0, 0, WIDTH, height);
  rain({ x: 0, y: 0, w: WIDTH, h: TOP_BAND - 8 }, 'down');
  rain({ x: 0, y: height - BOTTOM_BAND + 8, w: WIDTH, h: BOTTOM_BAND - 8 }, 'up');

  // The window: a title bar with its three lights, then the session.
  const win: Box = { x: MARGIN, y: TOP_BAND, w: WIDTH - MARGIN * 2, h: windowHeight };
  ctx.fillStyle = c.bar; ctx.fillRect(win.x, win.y, win.w, BAR);
  line(win.x, win.y, win.x + win.w, win.y, c.rule);
  line(win.x, win.y + BAR, win.x + win.w, win.y + BAR, c.rule);
  [rgb(1, 0.37, 0.34), rgb(1, 0.74, 0.18), rgb(0.16, 0.78, 0.25)].forEach((light, i) => {
    ctx.fillStyle = light; ctx.beginPath(); ctx.arc(win.x + PAD + i * 18 + 6, win.y + BAR / 2, 6, 0, Math.PI * 2); ctx.fill();
  });
  middleText(ctx, 'starmeter — usage', win.x, win.y + BAR / 2, win.w, mono(11, 500), c.muted, 'center');
  const left = win.x + PAD, width = win.w - PAD * 2, right = left + width;
  let y = win.y + BAR + PAD;
  const flag = ({ today: 'today', '24h': '24h', '7d': '7d', '30d': '30d', week: 'week', month: 'month', all: 'all' } as Record<string, string>)[d.rangeKey] ?? d.rangeKey;
  prompt('starmeter usage --range ' + flag, left, y + 9, false);
  y += 32;

  // Title, and the dates it covers in a bracket-like tag.
  const when = span(d), spanFont = mono(10.5, 600), tagWidth = Math.min(width * 0.62, measure(ctx, when, spanFont) + 18);
  const box: Box = { x: right - tagWidth, y, w: tagWidth, h: 24 };
  ctx.fillStyle = alpha(c.accent, dark ? 0.12 : 0.08); roundRect(ctx, box, 5); ctx.fill();
  ctx.strokeStyle = alpha(c.accent, 0.4); ctx.lineWidth = 1; roundRect(ctx, { x: box.x + 0.5, y: box.y + 0.5, w: box.w - 1, h: box.h - 1 }, 4.5); ctx.stroke();
  middleText(ctx, when, box.x + 9, box.y + 12, box.w - 14, spanFont, c.accent);
  const heading = title(d), room = box.x - left - 10, full = measure(ctx, heading, ui(19, 700));
  middleText(ctx, heading, left, box.y + 12, room + 2, ui(full <= room ? 19 : Math.max(15, Math.floor((19 * room) / full)), 700), c.text);
  y += 46;

  // The estimate, set like output, with the cursor after it and rain fading in beside it from the window's edge.
  ctx.save(); roundRect(ctx, win, 12); ctx.clip();
  rain({ x: win.x + win.w - 140, y: y - 4, w: 140, h: 84 }, 'left', 0.55);
  ctx.restore();
  const costs = counted.map((p) => p.cost).filter((x): x is number => x !== null), partial = counted.some((p) => p.cost === null || p.unpriced > 0);
  text(ctx, partial ? '// 已知 API 等值估算' : '// API 等值估算', { x: left, y, w: width, h: 15 }, mono(11), c.comment);
  const total = costs.length ? currency.symbol + dollars(currency.convert(costs.reduce((a, b) => a + b, 0))) : '—';
  const size = Math.min(42, Math.floor((42 * width * 0.86) / Math.max(1, measure(ctx, total, mono(42, 700))))), heroTop = y + 18;
  ctx.font = mono(size, 700); ctx.textAlign = 'left'; ctx.textBaseline = 'alphabetic'; ctx.letterSpacing = '-1px';
  const baseline = heroTop + size * 0.952;
  let x = left - 1;
  for (const [index, char] of [...total].entries()) {
    ctx.fillStyle = index === 0 && total.startsWith(currency.symbol) ? c.accent : c.text;
    ctx.fillText(char, x, baseline);
    x += ctx.measureText(char).width - 1;
  }
  ctx.letterSpacing = '0px';
  ctx.fillStyle = alpha(c.accent, 0.85);
  ctx.fillRect(x + 5, baseline - size * 0.7 - 1, size * 0.32, size * 0.7 + 3);
  y += 80;

  // Three figures in a ruled table.
  const cached = counted.filter((p) => p.cache !== null), weight = cached.reduce((s, p) => s + p.tokens, 0);
  const hit = weight > 0 ? ((cached.reduce((s, p) => s + (p.cache ?? 0) * p.tokens, 0) / weight) * 100).toFixed(1) + '%' : '—';
  const figures: [string, string][] = [['Token', tokens(counted.reduce((s, p) => s + p.tokens, 0))], ['请求', counted.reduce((s, p) => s + p.requests, 0).toLocaleString('en-US')], ['缓存命中', hit]];
  const table: Box = { x: left, y, w: width, h: 58 }, column = width / 3;
  ctx.fillStyle = c.bar; roundRect(ctx, table, 7); ctx.fill();
  ctx.strokeStyle = c.rule; roundRect(ctx, { x: table.x + 0.5, y: table.y + 0.5, w: table.w - 1, h: table.h - 1 }, 6.5); ctx.stroke();
  figures.forEach(([name, value], i) => {
    const cx = table.x + i * column;
    if (i > 0) line(cx, table.y, cx, table.y + table.h, c.rule);
    text(ctx, '▸ ' + name, { x: cx + 12, y: table.y + 10, w: column - 18, h: 14 }, mono(10, 500), c.muted);
    const measured = measure(ctx, value, mono(18, 600)), fitted = measured <= column - 22 ? 18 : Math.max(11, Math.floor((18 * (column - 22)) / measured));
    text(ctx, value, { x: cx + 12, y: table.y + 28, w: column - 18, h: 24 }, mono(fitted, 600), c.text);
  });
  y += 80;

  // The trend on an oscilloscope grid: the total glows, each tool a thin line in its own colour.
  const n = d.dates.length;
  if (n > 1) {
    text(ctx, '// 用量趋势 · ' + d.chartUnit + ' Token', { x: left, y, w: width, h: 15 }, mono(10.5), c.comment);
    const series = counted.map((p) => Array.from({ length: n }, (_, i) => Math.max(0, p.history[i] ?? 0)));
    const observed = Math.max(Math.max(0, ...Array.from({ length: n }, (_, i) => series.reduce((s, row) => s + row[i], 0))), 1);
    const unit = Math.pow(10, Math.floor(Math.log10(observed))), ceiling = (Math.ceil(((observed * 1.08) / unit) * 2) / 2) * unit;
    const plot = { x: left, y: y + 34, w: width, h: 104 }, bottom = plot.y + plot.h;
    ctx.setLineDash([1, 3]); ctx.lineWidth = 0.75; ctx.strokeStyle = c.rule;
    for (let f = 0; f <= 1.0001; f += 0.2) { ctx.beginPath(); ctx.moveTo(plot.x + f * plot.w, plot.y); ctx.lineTo(plot.x + f * plot.w, bottom); ctx.stroke(); }
    for (const f of [0, 0.5, 1]) {
      const yy = bottom - f * plot.h;
      ctx.setLineDash(f > 0 ? [1, 3] : []); ctx.lineWidth = f > 0 ? 0.75 : 1; ctx.strokeStyle = f > 0 ? c.rule : alpha(c.muted, 0.35);
      ctx.beginPath(); ctx.moveTo(plot.x, yy); ctx.lineTo(plot.x + plot.w, yy); ctx.stroke();
    }
    ctx.setLineDash([]);
    const positions = chartPositions(d), curves = smoothCurves(series, positions, plot, ceiling), totalCurve = curves[curves.length - 1];
    ctx.beginPath(); tracePath(ctx, totalCurve); ctx.lineTo(plot.x + plot.w, bottom); ctx.lineTo(plot.x, bottom); ctx.closePath();
    const fill = ctx.createLinearGradient(0, plot.y, 0, bottom);
    fill.addColorStop(0, alpha(c.accent, dark ? 0.28 : 0.18)); fill.addColorStop(1, alpha(c.accent, 0));
    ctx.fillStyle = fill; ctx.fill();
    curves.forEach((curve, i) => {
      const isTotal = i === curves.length - 1, colour = isTotal ? c.accent : alpha(accent(counted[i].id, dark), 0.9);
      ctx.save();
      ctx.shadowColor = alpha(colour, isTotal ? (dark ? 0.8 : 0.45) : dark ? 0.5 : 0); ctx.shadowBlur = isTotal ? 8 : 4;
      ctx.beginPath(); tracePath(ctx, curve);
      ctx.lineWidth = isTotal ? 2.2 : 1.2; ctx.lineCap = 'round'; ctx.lineJoin = 'round'; ctx.strokeStyle = colour; ctx.stroke();
      ctx.restore();
    });
    for (const f of [0.5, 1]) {
      const value = axisTokens(ceiling * f), yy = bottom - f * plot.h;
      ctx.fillStyle = alpha(c.window, 0.85); ctx.fillRect(left, yy - 13, measure(ctx, value, mono(9)) + 5, 12);
      text(ctx, value, { x: left + 1, y: yy - 13, w: 80, h: 12 }, mono(9), c.comment);
    }
    const end = totalCurve.end;
    ctx.fillStyle = alpha(c.accent, 0.22); ctx.beginPath(); ctx.arc(end[0], end[1], 7, 0, Math.PI * 2); ctx.fill();
    ctx.fillStyle = c.accent; ctx.beginPath(); ctx.arc(end[0], end[1], 3, 0, Math.PI * 2); ctx.fill();
    let middle: number | null = null;
    for (let i = 1; i < n - 1; i++) if (middle === null || Math.abs(positions[i] - 0.5) < Math.abs(positions[middle] - 0.5)) middle = i;
    for (const i of [0, ...(middle !== null ? [middle] : []), n - 1]) {
      const px = plot.x + positions[i] * plot.w, align = i === 0 ? 'left' : i === n - 1 ? 'right' : 'center';
      text(ctx, d.dates[i], { x: i === 0 ? px : i === n - 1 ? px - 80 : px - 40, y: bottom + 7, w: 80, h: 13 }, mono(9.5), c.comment, align);
    }
    y += CHART;
  }

  // The usage heatmap under its comment, the unit at the right.
  if (heatmap) {
    text(ctx, '// 用量热力图' + (heatmap.streak ? ' · ' + heatmap.streak : ''), { x: left, y, w: width - 90, h: 15 }, mono(10.5), c.comment);
    text(ctx, heatmap.hint, { x: left, y: y + 1, w: width, h: 14 }, mono(9.5), c.comment, 'right');
    drawHeatmap(ctx, heatmap, left, y + 24, width, [c.rule, ...[0.3, 0.52, 0.76, 1].map((a) => alpha(c.accent, a))]);
    y += heatmapHeight;
  }

  // Share of tokens as a row of cells, then a key naming each colour.
  const sum = Math.max(counted.reduce((s, p) => s + p.tokens, 0), 1);
  if (shared.length) {
    text(ctx, '// Token 占比', { x: left, y, w: width, h: 15 }, mono(10.5), c.comment);
    const count = 40, gap = 2, cell = (width - gap * (count - 1)) / count;
    let index = 0;
    cells(shared.map((p) => p.tokens / sum), count).forEach((lit, i) => {
      for (let k = 0; k < lit; k++) { ctx.fillStyle = accent(shared[i].id, dark); roundRect(ctx, { x: left + index * (cell + gap), y: y + 26, w: cell, h: 10 }, 1.5); ctx.fill(); index++; }
    });
    const key = ui(11, 500), shareFont = mono(10.5, 500), legendColumn = width / 3;
    shared.forEach((p, i) => {
      const lx = left + (i % 3) * legendColumn, ky = y + 48 + Math.floor(i / 3) * LEGEND, percent = share(p.tokens / sum);
      ctx.fillStyle = accent(p.id, dark); roundRect(ctx, { x: lx, y: ky + 3.5, w: 8, h: 8 }, 2); ctx.fill();
      const name = legendNameOf(p), nameWidth = Math.min(measure(ctx, name, key), legendColumn - measure(ctx, percent, shareFont) - 24);
      middleText(ctx, name, lx + 13, ky + 7.5, nameWidth, key, c.text);
      middleText(ctx, percent, lx + 18 + nameWidth, ky + 7.5, legendColumn - nameWidth - 18, shareFont, c.muted);
    });
    y += 56 + Math.ceil(shared.length / 3) * LEGEND;
  }

  // Each tool as a listing: cost, share and stats, then its models as a tree with cell bars.
  for (const p of rows) {
    const colour = accent(p.id, dark), inSummary = p.summaryIncluded !== false, top = y;
    dashed(top, left, right);
    ctx.fillStyle = alpha(colour, inSummary ? 1 : 0.4); ctx.beginPath(); ctx.arc(left + 4, top + 24, 4, 0, Math.PI * 2); ctx.fill();
    const nameFont = ui(14.5, 600), cost = p.cost !== null ? currency.symbol + dollars(currency.convert(p.cost)) : headline(p), costFont = mono(14.5, 600);
    const costWidth = measure(ctx, cost, costFont), mark = marks.tools.get(p.id), markRoom = mark ? 21 : 0;
    if (mark) drawMark(ctx, mark, { x: left + 16, y: top + 16.5, w: 15, h: 15 }, c.text);
    const nameWidth = Math.min(width * 0.45, measure(ctx, p.name, nameFont));
    middleText(ctx, p.name, left + 16 + markRoom, top + 24, Math.min(nameWidth + 2, width - 16 - markRoom - costWidth - 12), nameFont, c.text);
    middleText(ctx, cost, right - costWidth - 2, top + 24, costWidth + 2, costFont, c.text, 'right');
    const part = inSummary ? (p.counted?.tokens ?? p.tokens) : 0, figuresLine = tokenLine(p) + (part > 0 ? ' · 占 ' + share(part / sum) : ''), lineFont = mono(10.5, 500);
    let noteWidth = 0;
    const noteFont = mono(9.5, 500), note = tag(p);
    if (note) {
      noteWidth = Math.min(width - 16 - measure(ctx, figuresLine, lineFont) - 10, measure(ctx, note, noteFont) + 14);
      const noteBox: Box = { x: right - noteWidth, y: top + 39.5, w: noteWidth, h: 16 };
      ctx.strokeStyle = alpha(c.muted, 0.45); ctx.lineWidth = 1; roundRect(ctx, { x: noteBox.x + 0.5, y: noteBox.y + 0.5, w: noteBox.w - 1, h: noteBox.h - 1 }, 3.5); ctx.stroke();
      middleText(ctx, note, noteBox.x + 7, noteBox.y + 8, noteBox.w - 14, noteFont, c.muted, 'center');
    }
    text(ctx, figuresLine, { x: left + 16, y: top + 40, w: width - 16 - noteWidth - (noteWidth > 0 ? 10 : 0), h: 15 }, lineFont, c.muted);
    text(ctx, statsForms(p)[0], { x: left + 16, y: top + 57, w: width - 16, h: 15 }, mono(10.5), c.comment);
    let my = top + 80;
    const price = priceLine(p);
    if (price) { text(ctx, '! ' + price, { x: left + 16, y: my - 6, w: width - 16, h: 15 }, mono(10.5, 500), systemColor('orange', dark)); my += 17; }
    for (const client of clientLines(ctx, p, CLIENT_WIDTH, CLIENT_FONT)) { text(ctx, client, { x: left + 16, y: my - 6, w: width - 16, h: 15 }, CLIENT_FONT, c.comment); my += 17; }
    if (!p.models.length) text(ctx, '此时段暂无模型调用', { x: left + 16, y: my, w: width - 16, h: 15 }, mono(11), c.comment);
    // Models hang off a drawn tree: one stem, a tick per model.
    if (p.models.length) {
      const stem = left + 20, branch = alpha(c.comment, 0.7);
      line(stem, my + 1, stem, my + 8 + (p.models.length - 1) * MODEL, branch);
      p.models.forEach((_, i) => { const mid = my + 8 + i * MODEL; line(stem, mid, stem + 10, mid, branch); });
    }
    // Columns sized to this tool's longest model name and amount; the cells take what is left.
    const modelFont = mono(11), figureFont = mono(11, 500);
    const noted = (m: ProviderUsage['models'][number]) => measure(ctx, modelName(m.name), modelFont) + (m.priceNote ? measure(ctx, m.priceNote, noteFont) + 6 : 0);
    const nameColumn = Math.min(170, Math.max(0, ...p.models.map(noted)) + 12);
    const amountColumn = Math.max(50, Math.max(0, ...p.models.map((m) => measure(ctx, modelAmount(m), figureFont))) + 4);
    const barLeft = left + 40 + nameColumn, barRoom = right - 44 - amountColumn - 10 - barLeft, slots = Math.max(4, Math.min(16, Math.floor(barRoom / 6.5)));
    for (const m of p.models) {
      let nameRoom = nameColumn - 8;
      if (m.priceNote) {
        const w = Math.min(nameRoom * 0.55, measure(ctx, m.priceNote, noteFont)), nw = Math.min(nameRoom - w - 6, measure(ctx, modelName(m.name), modelFont));
        middleText(ctx, m.priceNote, left + 38 + nw + 6, my + 8, w + 2, noteFont, systemColor('orange', dark));
        nameRoom = nw;
      }
      middleText(ctx, modelName(m.name), left + 38, my + 8, nameRoom + 1, modelFont, c.text);
      const lit = m.share > 0 ? Math.max(1, Math.round(Math.min(1, m.share) * slots)) : 0;
      for (let k = 0; k < slots; k++) {
        ctx.fillStyle = k < lit ? alpha(colour, inSummary ? 0.95 : 0.5) : c.rule;
        roundRect(ctx, { x: barLeft + k * 6.5, y: my + 4, w: 5, h: 8 }, 1); ctx.fill();
      }
      middleText(ctx, modelAmount(m), right - 44 - amountColumn, my + 8, amountColumn, figureFont, c.text, 'right');
      middleText(ctx, share(m.share), right - 40, my + 8, 40, mono(10.5), c.muted, 'right');
      my += MODEL;
    }
    y += blockHeight(p);
  }

  // The session waits at a prompt.
  dashed(y, left, right);
  prompt('', left, y + 16, true);
  // Signature under the window: the mark, the name and where to find it.
  const footer = win.y + win.h + 18, signFont = ui(13.5, 600), signWidth = measure(ctx, project.name, signFont), rowWidth = 20 + 8 + signWidth;
  drawBrand(ctx, marks.brand, { x: WIDTH / 2 - rowWidth / 2, y: footer, w: 20, h: 20 });
  middleText(ctx, project.name, WIDTH / 2 - rowWidth / 2 + 28, footer + 10, signWidth + 2, signFont, c.text);
  text(ctx, project.url, { x: 0, y: footer + 26, w: WIDTH, h: 14 }, mono(10), c.comment, 'center');
  return canvas;
}
