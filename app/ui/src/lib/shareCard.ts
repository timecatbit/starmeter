// The card style: an opaque card on a soft wash lit by the leading tools' colours, with halftone dots above, below and
// beside the estimate.
import { chartPositions, smoothCurves, tracePath } from './chart';
import { accent, alpha, heatmapShades, label as labelColour, systemColor } from './colors';
import { axisTokens, currency, headline, modelAmount, modelName, share, statsForms, tokenLine, tokens } from './format';
import { font, ROUNDED, SANS } from './measure';
import {
  BOTTOM_BAND, canvasFor, clientLines, countedRows, crownSeed, dollars, drawBrand, drawHeatmap, drawMark, drawSymbol, fade, heatmapGridHeight, legendNameOf,
  loadMarks, loadSymbols, measure, middleText, priceLine, roundRect, sharedRows, shownHeatmap, shownRows, tag, text, title, TOP_BAND, WIDTH, type Box, type Fade,
} from './shareCommon';
import type { Dashboard, Project, ProviderUsage } from './types';

const MARGIN = 22, INSET = 24, CHART = 150, MODEL = 24, LEGEND = 20;
const CLIENT_WIDTH = WIDTH - MARGIN * 2 - INSET * 2 - 32 - 12, CLIENT_FONT = font(10.5);
const rounded = (size: number, weight: number) => font(size, weight, ROUNDED);

/** What the picture covers: today's or yesterday's date, a 24-hour window with its times, or first to last day. */
export function period(d: Dashboard): string {
  const day = (iso: string) => { const p = iso.split('-').map(Number); return p.length === 3 ? `${p[0]} 年 ${p[1]} 月 ${p[2]} 日` : iso; };
  const numbers = (value: string) => value.split(/[-T:]/).map(Number).filter((n) => !Number.isNaN(n));
  if (d.rangeKey === 'today' || d.period?.length !== 2) return day(d.date);
  const a = numbers(d.period[0]), b = numbers(d.period[1]);
  if (a.length < 3 || b.length !== a.length) return day(d.date);
  const p2 = (n: number) => String(n).padStart(2, '0');
  if (a.length >= 5) return `${a[1]}/${a[2]} ${p2(a[3])}:${p2(a[4])} – ${b[1]}/${b[2]} ${p2(b[3])}:${p2(b[4])}`;
  if (a[0] !== b[0]) return `${a[0]} 年 ${a[1]} 月 ${a[2]} 日 – ${b[0]} 年 ${b[1]} 月 ${b[2]} 日`;
  if (a[1] !== b[1]) return `${a[0]} 年 ${a[1]} 月 ${a[2]} 日 – ${b[1]} 月 ${b[2]} 日`;
  return a[2] !== b[2] ? `${a[0]} 年 ${a[1]} 月 ${a[2]} – ${b[2]} 日` : `${a[0]} 年 ${a[1]} 月 ${a[2]} 日`;
}

export async function drawCardPicture(d: Dashboard, project: Project, dark: boolean): Promise<HTMLCanvasElement> {
  const measureContext = document.createElement('canvas').getContext('2d')!;
  const rows = shownRows(d), counted = countedRows(d), shared = sharedRows(d), heatmap = shownHeatmap(d);
  const blockHeight = (p: ProviderUsage) => 92 + (priceLine(p) ? 18 : 0) + clientLines(measureContext, p, CLIENT_WIDTH, CLIENT_FONT).length * 18 + Math.max(1, p.models.length) * MODEL;
  const inner = WIDTH - MARGIN * 2 - INSET * 2;
  const heatmapHeight = heatmap ? 24 + heatmapGridHeight(heatmap, inner) + 24 : 0;
  const cardHeight = INSET * 2 + 44 + 88 + 88 + (d.dates.length > 1 ? CHART + 22 : 0) + heatmapHeight + (shared.length ? 60 + Math.ceil(shared.length / 3) * LEGEND : 0)
    + (rows.length ? rows.reduce((s, p) => s + blockHeight(p) + 10, 0) - 10 : 0);
  const height = TOP_BAND + cardHeight + 60 + BOTTOM_BAND;
  const { canvas, ctx } = canvasFor(height);
  const label = labelColour(dark, 0.85), secondary = labelColour(dark, dark ? 0.55 : 0.5), tertiary = labelColour(dark, dark ? 0.25 : 0.26);
  const teal = dark ? 'rgb(110, 201, 191)' : 'rgb(38, 120, 117)';
  const surface = dark ? 'rgb(26, 27, 29)' : 'rgb(255, 255, 255)';
  const quiet = labelColour(dark, dark ? 0.055 : 0.035);
  const marks = await loadMarks(rows, label);
  const symbols = await loadSymbols([['calendar', teal], ['square.stack.3d.up.fill', teal], ['arrow.up.arrow.down', teal], ['bolt.fill', teal]]);
  const seed = crownSeed(d);

  // The card's pattern: soft dots swelling in slow diagonal waves that follow the picture, so every patch is one field.
  const halftone = (rect: Box, fading: Fade, colour: string, strength: number) => {
    const pitch = 9, rowsCount = Math.max(1, Math.floor(rect.h / pitch)), columns = Math.floor(rect.w / pitch) + 2;
    for (let row = 0; row < rowsCount; row++) {
      const y = rect.y + row * pitch + pitch / 2;
      for (let column = 0; column < columns; column++) {
        const x = rect.x + column * pitch + (row % 2 === 0 ? 0 : pitch / 2) - 2, f = fade(fading, x, y, rect);
        const wave = 0.5 + 0.5 * Math.sin(x * 0.05 - y * 0.083 + seed) * (0.65 + 0.35 * Math.cos(x * 0.013 - seed * 0.7));
        const radius = (0.7 + 3.3 * wave) * f;
        if (radius <= 0.5) continue;
        ctx.fillStyle = alpha(colour, strength * (0.35 + 0.65 * f));
        ctx.beginPath(); ctx.arc(x, y, radius, 0, Math.PI * 2); ctx.fill();
      }
    }
  };

  // A soft wash lit by the colours of the tools that used the most.
  ctx.fillStyle = dark ? 'rgb(13, 15, 18)' : 'rgb(242, 245, 247)';
  ctx.fillRect(0, 0, WIDTH, height);
  const glows = [teal, ...[...counted].sort((a, b) => b.tokens - a.tokens).slice(0, 2).map((p) => accent(p.id, dark))];
  const spots: [number, number, number][] = [[0.08, 0.03, 0.95], [1, 0.22, 0.8], [0.1, 0.78, 0.9]];
  glows.forEach((colour, i) => {
    const [fx, fy, fr] = spots[i], cx = WIDTH * fx, cy = height * fy, r = WIDTH * fr;
    const gradient = ctx.createRadialGradient(cx, cy, 0, cx, cy, r);
    gradient.addColorStop(0, alpha(colour, dark ? 0.26 : 0.2)); gradient.addColorStop(1, alpha(colour, 0));
    ctx.fillStyle = gradient; ctx.fillRect(0, 0, WIDTH, height);
  });
  halftone({ x: 0, y: 0, w: WIDTH, h: TOP_BAND - 8 }, 'down', teal, dark ? 0.6 : 0.5);
  halftone({ x: 0, y: height - BOTTOM_BAND + 8, w: WIDTH, h: BOTTOM_BAND - 8 }, 'up', teal, dark ? 0.6 : 0.5);
  const card: Box = { x: MARGIN, y: TOP_BAND, w: WIDTH - MARGIN * 2, h: cardHeight };
  ctx.save();
  ctx.shadowColor = `rgba(0, 0, 0, ${dark ? 0.45 : 0.09})`; ctx.shadowBlur = 36; ctx.shadowOffsetY = 12;
  ctx.fillStyle = surface; roundRect(ctx, card, 26); ctx.fill();
  ctx.restore();
  ctx.strokeStyle = labelColour(dark, dark ? 0.11 : 0.05); ctx.lineWidth = 1;
  roundRect(ctx, { x: card.x + 0.5, y: card.y + 0.5, w: card.w - 1, h: card.h - 1 }, 25.5); ctx.stroke();
  const left = card.x + INSET, width = card.w - INSET * 2, right = left + width;
  let y = card.y + INSET;

  // Header: what and when.
  const day = period(d), dayFont = font(11, 600), pillWidth = Math.min(width * 0.6, measure(ctx, day, dayFont) + 32);
  const pill: Box = { x: right - pillWidth, y, w: pillWidth, h: 26 };
  ctx.fillStyle = alpha(teal, dark ? 0.16 : 0.09); roundRect(ctx, pill, 13); ctx.fill();
  drawSymbol(ctx, symbols.get('calendar'), pill.x + 15, pill.y + 13, 11);
  middleText(ctx, day, pill.x + 24, pill.y + 13, pillWidth - 32, dayFont, teal);
  const heading = title(d), room = pill.x - left - 10, full = measure(ctx, heading, font(19, 700));
  middleText(ctx, heading, left, pill.y + 13, room + 2, font(full <= room ? 19 : Math.max(15, Math.floor((19 * room) / full)), 700), label);
  y += 44;

  // Hero: the estimate, its sign small against the figure, with dots fading in from the card's right edge beside it.
  ctx.save(); roundRect(ctx, card, 26); ctx.clip();
  halftone({ x: card.x + card.w - 170, y: y - 6, w: 170, h: 94 }, 'left', teal, dark ? 0.34 : 0.26);
  ctx.restore();
  const costs = counted.map((p) => p.cost).filter((c): c is number => c !== null), partial = counted.some((p) => p.cost === null || p.unpriced > 0);
  text(ctx, partial ? '已知 API 等值估算' : 'API 等值估算', { x: left, y, w: width, h: 16 }, font(11.5, 500), secondary);
  const baseline = y + 16 + 50 * 0.952;
  ctx.textBaseline = 'alphabetic'; ctx.textAlign = 'left';
  if (costs.length) {
    ctx.font = rounded(26, 600); ctx.fillStyle = secondary;
    ctx.fillText(currency.symbol, left - 2, baseline - 15);
    const signWidth = ctx.measureText(currency.symbol).width;
    ctx.font = rounded(50, 700); ctx.fillStyle = label;
    ctx.letterSpacing = '-0.6px';
    ctx.fillText(dollars(currency.convert(costs.reduce((a, b) => a + b, 0))), left - 2 + signWidth, baseline);
    ctx.letterSpacing = '0px';
  } else {
    ctx.font = rounded(50, 700); ctx.fillStyle = label; ctx.fillText('—', left - 2, baseline);
  }
  y += 88;

  // Three figures, each with its own small symbol. The cache hit rate is weighted by each tool's tokens.
  const cached = counted.filter((p) => p.cache !== null), weight = cached.reduce((s, p) => s + p.tokens, 0);
  const hit = weight > 0 ? ((cached.reduce((s, p) => s + (p.cache ?? 0) * p.tokens, 0) / weight) * 100).toFixed(1) + '%' : '—';
  const tiles: [string, string, string][] = [['square.stack.3d.up.fill', 'Token', tokens(counted.reduce((s, p) => s + p.tokens, 0))],
    ['arrow.up.arrow.down', '请求', counted.reduce((s, p) => s + p.requests, 0).toLocaleString('en-US')], ['bolt.fill', '缓存命中', hit]];
  const tileWidth = (width - 20) / 3;
  tiles.forEach(([icon, name, value], i) => {
    const tile: Box = { x: left + i * (tileWidth + 10), y, w: tileWidth, h: 66 };
    ctx.fillStyle = quiet; roundRect(ctx, tile, 16); ctx.fill();
    drawSymbol(ctx, symbols.get(icon), tile.x + 19, tile.y + 19, 11);
    text(ctx, name, { x: tile.x + 30, y: tile.y + 11.5, w: tile.w - 40, h: 15 }, font(10.5, 500), secondary);
    const fullWidth = measure(ctx, value, rounded(20, 600)), size = fullWidth <= tile.w - 26 ? 20 : Math.max(11, Math.floor((20 * (tile.w - 26)) / fullWidth));
    text(ctx, value, { x: tile.x + 13, y: tile.y + 31, w: tile.w - 24, h: 26 }, rounded(size, 600), label);
  });
  y += 88;

  // Chart: the total as a soft area, each tool as a thin line in its own colour, on the panel's scale.
  const n = d.dates.length;
  if (n > 1) {
    const series = counted.map((p) => Array.from({ length: n }, (_, i) => Math.max(0, p.history[i] ?? 0)));
    const observed = Math.max(Math.max(0, ...Array.from({ length: n }, (_, i) => series.reduce((s, row) => s + row[i], 0))), 1);
    const unit = Math.pow(10, Math.floor(Math.log10(observed))), ceiling = (Math.ceil(((observed * 1.08) / unit) * 2) / 2) * unit;
    const plot = { x: left, y: y + 16, w: width, h: CHART - 46 }, bottom = plot.y + plot.h, axisFont = rounded(9.5, 500);
    text(ctx, d.chartUnit + ' · Token', { x: right - 160, y: y - 2, w: 160, h: 13 }, font(9.5, 500), tertiary, 'right');
    for (const f of [0, 0.5, 1]) {
      const yy = bottom - f * plot.h;
      ctx.beginPath(); ctx.moveTo(plot.x, yy); ctx.lineTo(plot.x + plot.w, yy);
      if (f > 0) { ctx.setLineDash([2, 3]); ctx.lineWidth = 0.75; ctx.strokeStyle = labelColour(dark, dark ? 0.12 : 0.09); }
      else { ctx.setLineDash([]); ctx.lineWidth = 1; ctx.strokeStyle = labelColour(dark, dark ? 0.16 : 0.1); }
      ctx.stroke();
    }
    ctx.setLineDash([]);
    const positions = chartPositions(d), curves = smoothCurves(series, positions, plot, ceiling), total = curves[curves.length - 1];
    ctx.save();
    ctx.beginPath(); tracePath(ctx, total); ctx.lineTo(plot.x + plot.w, bottom); ctx.lineTo(plot.x, bottom); ctx.closePath();
    const fill = ctx.createLinearGradient(0, plot.y, 0, bottom);
    fill.addColorStop(0, alpha(teal, dark ? 0.34 : 0.22)); fill.addColorStop(1, alpha(teal, 0));
    ctx.fillStyle = fill; ctx.fill();
    ctx.restore();
    curves.forEach((curve, i) => {
      const isTotal = i === curves.length - 1;
      ctx.beginPath(); tracePath(ctx, curve);
      ctx.lineWidth = isTotal ? 2.4 : 1.3; ctx.lineCap = 'round'; ctx.lineJoin = 'round';
      ctx.strokeStyle = isTotal ? teal : alpha(accent(counted[i].id, dark), 0.85); ctx.stroke();
    });
    for (const f of [0.5, 1]) {
      const value = axisTokens(ceiling * f), yy = bottom - f * plot.h;
      ctx.fillStyle = alpha(surface, 0.85); roundRect(ctx, { x: left - 3, y: yy - 14, w: measure(ctx, value, axisFont) + 6, h: 13 }, 3); ctx.fill();
      text(ctx, value, { x: left, y: yy - 14, w: 80, h: 12 }, axisFont, tertiary);
    }
    const end = total.end;
    ctx.fillStyle = alpha(teal, 0.18); ctx.beginPath(); ctx.arc(end[0], end[1], 7, 0, Math.PI * 2); ctx.fill();
    ctx.fillStyle = surface; ctx.strokeStyle = teal; ctx.lineWidth = 2;
    ctx.beginPath(); ctx.arc(end[0], end[1], 3.5, 0, Math.PI * 2); ctx.fill(); ctx.stroke();
    let middle: number | null = null;
    for (let i = 1; i < n - 1; i++) if (middle === null || Math.abs(positions[i] - 0.5) < Math.abs(positions[middle] - 0.5)) middle = i;
    for (const i of [0, ...(middle !== null ? [middle] : []), n - 1]) {
      const x = plot.x + positions[i] * plot.w, align = i === 0 ? 'left' : i === n - 1 ? 'right' : 'center';
      text(ctx, d.dates[i], { x: i === 0 ? x : i === n - 1 ? x - 80 : x - 40, y: bottom + 8, w: 80, h: 13 }, rounded(9.5, 500), tertiary, align);
    }
    y += CHART + 22;
  }

  // The usage heatmap under its heading, the unit at the right.
  if (heatmap) {
    text(ctx, '用量热力图' + (heatmap.streak ? ' · ' + heatmap.streak : ''), { x: left, y, w: width - 90, h: 15 }, font(11, 600), secondary);
    text(ctx, heatmap.hint, { x: left, y: y + 1, w: width, h: 14 }, font(10, 500), tertiary, 'right');
    drawHeatmap(ctx, heatmap, left, y + 24, width, heatmapShades(dark));
    y += heatmapHeight;
  }

  // Share of tokens: one bar in the tools' colours, then a key naming each colour.
  const sum = Math.max(counted.reduce((s, p) => s + p.tokens, 0), 1);
  if (shared.length) {
    text(ctx, 'Token 占比', { x: left, y, w: width, h: 15 }, font(11, 600), secondary);
    const bar: Box = { x: left, y: y + 24, w: width, h: 10 };
    ctx.save(); roundRect(ctx, bar, 5); ctx.clip();
    ctx.fillStyle = quiet; ctx.fillRect(bar.x, bar.y, bar.w, bar.h);
    // A small share still shows as a sliver; the largest gives up the room.
    const widths = shared.map((p) => Math.max(10, (bar.w * p.tokens) / sum));
    const widest = widths.indexOf(Math.max(...widths));
    widths[widest] -= widths.reduce((a, b) => a + b, 0) - bar.w;
    let x = bar.x;
    shared.forEach((p, i) => {
      ctx.fillStyle = accent(p.id, dark);
      ctx.fillRect(x, bar.y, Math.max(0, widths[i] - (i < shared.length - 1 ? 2 : 0)), bar.h);
      x += widths[i];
    });
    ctx.restore();
    const column = width / 3, keyFont = font(11, 500), shareFont = rounded(11, 500);
    shared.forEach((p, i) => {
      const x = left + (i % 3) * column, ky = bar.y + bar.h + 14 + Math.floor(i / 3) * LEGEND, percent = share(p.tokens / sum);
      ctx.fillStyle = accent(p.id, dark); ctx.beginPath(); ctx.arc(x + 3.5, ky + 7.5, 3.5, 0, Math.PI * 2); ctx.fill();
      const name = legendNameOf(p), nameWidth = Math.min(measure(ctx, name, keyFont), column - measure(ctx, percent, shareFont) - 24);
      text(ctx, name, { x: x + 12, y: ky, w: nameWidth, h: 15 }, keyFont, label);
      text(ctx, percent, { x: x + 16 + nameWidth, y: ky, w: column - nameWidth - 16, h: 15 }, shareFont, secondary);
    });
    y += 60 + Math.ceil(shared.length / 3) * LEGEND;
  }

  // Each tool on its own tile: what it cost, its share and stats, then every model.
  for (const p of rows) {
    const c = accent(p.id, dark), inSummary = p.summaryIncluded !== false, block: Box = { x: left, y, w: width, h: blockHeight(p) };
    ctx.fillStyle = quiet; roundRect(ctx, block, 18); ctx.fill();
    const il = block.x + 16, ir = block.x + block.w - 16, top = block.y + 16, tl = il + 12, tw = ir - tl;
    ctx.fillStyle = alpha(c, inSummary ? 1 : 0.45); roundRect(ctx, { x: il, y: top + 3, w: 4, h: 15 }, 2); ctx.fill();
    const nameFont = font(14.5, 600), amount = p.cost !== null ? currency.symbol + dollars(currency.convert(p.cost)) : headline(p), amountFont = rounded(17, 600);
    const amountWidth = measure(ctx, amount, amountFont), mark = marks.tools.get(p.id), markRoom = mark ? 22 : 0;
    if (mark) drawMark(ctx, mark, { x: tl, y: top + 1 + 14.5 * 0.6 - 8, w: 16, h: 16 }, label);
    text(ctx, p.name, { x: tl + markRoom, y: top, w: Math.min(tw - amountWidth - 12 - markRoom, measure(ctx, p.name, nameFont) + 2), h: 20 }, nameFont, label);
    text(ctx, amount, { x: ir - amountWidth - 2, y: top - 1, w: amountWidth + 2, h: 22 }, amountFont, label, 'right');
    const part = inSummary ? (p.counted?.tokens ?? p.tokens) : 0, figures = tokenLine(p) + (part > 0 ? ' · 占 ' + share(part / sum) : ''), figureFont = font(11.5, 500);
    // A source a gateway partly counts says so at the right of the token line, under the amount, read whole.
    let noteWidth = 0;
    const note = tag(p);
    if (note) {
      const tagFont = font(9.5, 500);
      noteWidth = Math.min(tw - measure(ctx, figures, figureFont) - 10, measure(ctx, note, tagFont) + 16);
      const capsule: Box = { x: ir - noteWidth, y: top + 23.5, w: noteWidth, h: 17 };
      ctx.fillStyle = labelColour(dark, dark ? 0.09 : 0.06); roundRect(ctx, capsule, 8.5); ctx.fill();
      middleText(ctx, note, capsule.x + 8, capsule.y + 8.5, capsule.w - 16, tagFont, secondary, 'center');
    }
    text(ctx, figures, { x: tl, y: top + 24, w: tw - noteWidth - (noteWidth > 0 ? 10 : 0), h: 16 }, figureFont, secondary);
    text(ctx, statsForms(p)[0], { x: tl, y: top + 42, w: tw, h: 15 }, font(10.5), secondary);
    let my = top + 66;
    const price = priceLine(p);
    if (price) { text(ctx, price, { x: tl, y: my - 6, w: tw, h: 15 }, font(10.5, 500), systemColor('orange', dark)); my += 18; }
    for (const line of clientLines(ctx, p, CLIENT_WIDTH, CLIENT_FONT)) { text(ctx, line, { x: tl, y: my - 6, w: tw, h: 15 }, CLIENT_FONT, secondary); my += 18; }
    if (!p.models.length) text(ctx, '此时段暂无模型调用', { x: tl, y: my, w: tw, h: 15 }, font(11), secondary);
    for (const m of p.models) {
      const modelFont = font(11.5), noteFont = font(10, 500);
      let nameRoom = tw - 140;
      if (m.priceNote) {
        const w = Math.min(nameRoom * 0.55, measure(ctx, m.priceNote, noteFont)), nameWidth = Math.min(nameRoom - w - 6, measure(ctx, modelName(m.name), modelFont));
        text(ctx, m.priceNote, { x: tl + nameWidth + 6, y: my + 1, w: w + 2, h: 15 }, noteFont, systemColor('orange', dark));
        nameRoom = nameWidth;
      }
      text(ctx, modelName(m.name), { x: tl, y: my, w: nameRoom + 2, h: 16 }, modelFont, label);
      text(ctx, modelAmount(m), { x: ir - 134, y: my, w: 86, h: 16 }, rounded(11.5, 500), label, 'right');
      text(ctx, share(m.share), { x: ir - 44, y: my, w: 44, h: 16 }, rounded(11, 500), secondary, 'right');
      const track: Box = { x: tl, y: my + 17, w: tw, h: 3 };
      ctx.fillStyle = labelColour(dark, dark ? 0.08 : 0.06); roundRect(ctx, track, 1.5); ctx.fill();
      const fillWidth = Math.max(3, track.w * Math.min(1, m.share)), gradient = ctx.createLinearGradient(track.x, 0, track.x + fillWidth, 0);
      gradient.addColorStop(0, alpha(c, 0.55)); gradient.addColorStop(1, c);
      ctx.fillStyle = gradient; roundRect(ctx, { x: track.x, y: track.y, w: fillWidth, h: 3 }, 1.5); ctx.fill();
      my += MODEL;
    }
    y += block.h + 10;
  }

  // Signature under the card: the mark, the name and where to find it.
  const footer = card.y + card.h + 20, signFont = font(13.5, 600), signWidth = measure(ctx, project.name, signFont), rowWidth = 20 + 8 + signWidth;
  drawBrand(ctx, marks.brand, { x: WIDTH / 2 - rowWidth / 2, y: footer, w: 20, h: 20 });
  text(ctx, project.name, { x: WIDTH / 2 - rowWidth / 2 + 28, y: footer + 1, w: signWidth + 2, h: 18 }, signFont, label);
  text(ctx, project.url, { x: 0, y: footer + 26, w: WIDTH, h: 15 }, font(10.5), secondary, 'center');
  void SANS;
  return canvas;
}
