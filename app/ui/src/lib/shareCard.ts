// The card style: the card is the whole picture (no surround: chat apps shrink a picture to fit, and a frame only made
// the figures smaller), lit by the leading tools' colours, with halftone dots beside the estimate and under the
// signature. It follows light and dark.
import { smoothCurves, tracePath } from './chart';
import { accent, alpha, label as labelColour, systemColor } from './colors';
import { font, ROUNDED } from './measure';
import { BOTTOM_BAND, canvasFor, crownSeed, drawMark, drawSymbol, fade, loadSymbols, measure, middleText, roundRect, text, WIDTH, type Box, type Fade } from './shareCommon';
import { drawAmount, drawMasthead, drawMembers, drawSignature, figures, fitAmount, HERO_BASELINE, layout, poster, ROWS, tickBox, toolHeight, SHARE_TOOLS } from './sharePoster';
import type { Dashboard, Project } from './types';

const INSET = 28;
const rounded = (size: number, weight = 600) => font(size, weight, ROUNDED);

export async function drawCardPicture(d: Dashboard, project: Project, dark: boolean, tools = SHARE_TOOLS): Promise<HTMLCanvasElement> {
  const label = labelColour(dark, 0.85), secondary = labelColour(dark, dark ? 0.55 : 0.5), tertiary = labelColour(dark, dark ? 0.3 : 0.3);
  const p = await poster(d, label, tools), L = layout(p), height = L.height;
  const width = WIDTH - INSET * 2, left = INSET, right = left + width;
  const { canvas, ctx } = canvasFor(height);
  const teal = dark ? 'rgb(110, 201, 191)' : 'rgb(38, 120, 117)';
  const surface = dark ? 'rgb(26, 27, 29)' : 'rgb(255, 255, 255)';
  const quiet = labelColour(dark, dark ? 0.055 : 0.035);
  const symbols = await loadSymbols([['square.stack.3d.up.fill', teal], ['arrow.up.arrow.down', teal], ['bolt.fill', teal]]);
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

  // The card's own surface to every edge, lit softly by the colours of the tools that cost the most.
  ctx.fillStyle = surface; ctx.fillRect(0, 0, WIDTH, height);
  const glows = [teal, ...(p.chart?.series ?? []).map((s) => [s.id, s.values.reduce((a, b) => a + b, 0)] as const).sort((a, b) => b[1] - a[1]).slice(0, 2).map(([id]) => accent(id, dark))];
  const spots: [number, number, number][] = [[0.08, 0.03, 0.95], [1, 0.22, 0.8], [0.1, 0.78, 0.9]];
  glows.forEach((colour, i) => {
    const [fx, fy, fr] = spots[i], cx = WIDTH * fx, cy = height * fy, r = WIDTH * fr;
    const gradient = ctx.createRadialGradient(cx, cy, 0, cx, cy, r);
    gradient.addColorStop(0, alpha(colour, dark ? 0.2 : 0.12)); gradient.addColorStop(1, alpha(colour, 0));
    ctx.fillStyle = gradient; ctx.fillRect(0, 0, WIDTH, height);
  });
  halftone({ x: 0, y: height - BOTTOM_BAND + 8, w: WIDTH, h: BOTTOM_BAND - 8 }, 'up', teal, dark ? 0.6 : 0.5);

  // Masthead, then the title.
  drawMasthead(ctx, p, project.name, { ink: label, dateInk: teal, font: (s, w) => font(s, Math.min(700, w)), name: 16, date: 13.5, left, right, weight: 700 });
  text(ctx, p.title, { x: left, y: L.title + 10, w: width, h: 30 }, font(24, 700), label);

  // Hero: the estimate, its sign small against the figure, with dots fading in from the card's right edge beside it.
  halftone({ x: WIDTH - 170, y: L.hero + 4, w: 170, h: 104 }, 'left', teal, dark ? 0.34 : 0.26);
  text(ctx, p.costLabel, { x: left, y: L.hero + 4, w: width, h: 16 }, font(12, 500), secondary);
  const hero = fitAmount(ctx, p.cost, width - 30, 80, 30, (s) => rounded(s, 700), -0.012);
  drawAmount(ctx, p.cost, left - 2, L.hero + HERO_BASELINE, hero, (s) => rounded(s, 700), label, secondary, -0.012);

  // Three figures, each with its own small symbol. The cache hit rate is weighted by each tool's tokens.
  const tileWidth = (width - 20) / 3, icons = ['square.stack.3d.up.fill', 'arrow.up.arrow.down', 'bolt.fill'];
  figures(p).forEach(([name, value], i) => {
    const tile: Box = { x: left + i * (tileWidth + 10), y: L.stats + 2, w: tileWidth, h: 62 };
    ctx.fillStyle = quiet; roundRect(ctx, tile, 16); ctx.fill();
    drawSymbol(ctx, symbols.get(icons[i]), tile.x + 19, tile.y + 18, 11);
    text(ctx, name, { x: tile.x + 30, y: tile.y + 10.5, w: tile.w - 40, h: 15 }, font(10.5, 500), secondary);
    const full = measure(ctx, value, rounded(20)), size = full <= tile.w - 26 ? 20 : Math.max(11, Math.floor((20 * (tile.w - 26)) / full));
    text(ctx, value, { x: tile.x + 13, y: tile.y + 29, w: tile.w - 24, h: 26 }, rounded(size), label);
  });

  // Chart: the total as a soft area, each listed tool as a thin line in its own colour, on the panel's scale.
  const chart = p.chart;
  if (chart && L.chart !== null) {
    const y = L.chart;
    text(ctx, chart.label, { x: left, y, w: width / 2, h: 15 }, font(11, 600), secondary);
    if (chart.note) text(ctx, chart.note, { x: left + width / 2, y: y + 1, w: width / 2, h: 14 }, font(10, 500), tertiary, 'right');
    const plot = { x: left, y: y + 32, w: width, h: ROWS.chart - 70 }, bottom = plot.y + plot.h, axisFont = rounded(9.5, 500);
    for (const f of [0, 0.5, 1]) {
      const yy = bottom - f * plot.h;
      ctx.beginPath(); ctx.moveTo(plot.x, yy); ctx.lineTo(plot.x + plot.w, yy);
      if (f > 0) { ctx.setLineDash([2, 3]); ctx.lineWidth = 0.75; ctx.strokeStyle = labelColour(dark, dark ? 0.12 : 0.09); }
      else { ctx.setLineDash([]); ctx.lineWidth = 1; ctx.strokeStyle = labelColour(dark, dark ? 0.16 : 0.1); }
      ctx.stroke();
    }
    ctx.setLineDash([]);
    const curves = smoothCurves([...chart.series.map((s) => s.values), chart.rest], chart.positions, plot, chart.ceiling), total = curves[curves.length - 1];
    ctx.beginPath(); tracePath(ctx, total); ctx.lineTo(plot.x + plot.w, bottom); ctx.lineTo(plot.x, bottom); ctx.closePath();
    const fill = ctx.createLinearGradient(0, plot.y, 0, bottom);
    fill.addColorStop(0, alpha(teal, dark ? 0.34 : 0.22)); fill.addColorStop(1, alpha(teal, 0));
    ctx.fillStyle = fill; ctx.fill();
    ctx.lineCap = 'round'; ctx.lineJoin = 'round';
    chart.series.forEach((s, i) => { ctx.beginPath(); tracePath(ctx, curves[i]); ctx.lineWidth = 1.3; ctx.strokeStyle = alpha(accent(s.id, dark), 0.85); ctx.stroke(); });
    ctx.beginPath(); tracePath(ctx, total); ctx.lineWidth = 2.4; ctx.strokeStyle = teal; ctx.stroke();
    for (const f of [0.5, 1]) {
      const value = chart.axis[f === 1 ? 2 : 1], yy = bottom - f * plot.h;
      ctx.fillStyle = alpha(surface, 0.85); roundRect(ctx, { x: left - 3, y: yy - 14, w: measure(ctx, value, axisFont) + 6, h: 13 }, 3); ctx.fill();
      text(ctx, value, { x: left, y: yy - 14, w: 80, h: 12 }, axisFont, tertiary);
    }
    const end = total.end;
    ctx.fillStyle = alpha(teal, 0.18); ctx.beginPath(); ctx.arc(end[0], end[1], 7, 0, Math.PI * 2); ctx.fill();
    ctx.fillStyle = surface; ctx.strokeStyle = teal; ctx.lineWidth = 2;
    ctx.beginPath(); ctx.arc(end[0], end[1], 3.5, 0, Math.PI * 2); ctx.fill(); ctx.stroke();
    chart.ticks.forEach((tick, i) => {
      const at = tickBox(i, chart.ticks.length, plot.x + tick.at * plot.w, left, right, 80);
      text(ctx, tick.label, { x: at.x, y: bottom + 8, w: 80, h: 13 }, rounded(9.5, 500), tertiary, at.align);
    });
  }

  // Each tool on its own tile: its mark, name and amount; its tokens and share, or the tag that explains them; then
  // its first models. 其他工具 lists the tools folded into it instead.
  p.tools.forEach((t, index) => {
    const top = L.tools[index], other = t.id === 'other', c = other ? secondary : accent(t.id, dark), block: Box = { x: left, y: top, w: width, h: toolHeight(t) };
    ctx.fillStyle = quiet; roundRect(ctx, block, 16); ctx.fill();
    const il = block.x + 14, ir = block.x + block.w - 14, tl = il + 12, tw = ir - tl;
    ctx.fillStyle = alpha(c, t.tag ? 0.45 : 1); roundRect(ctx, { x: il, y: top + 8.5, w: 4, h: 15 }, 2); ctx.fill();
    const nameFont = font(14.5, 600), amountFont = rounded(16.5), amountWidth = measure(ctx, t.cost, amountFont), mark = p.marks.tools.get(t.id), markRoom = mark ? 22 : 0;
    if (mark) drawMark(ctx, mark, { x: tl, y: top + 8, w: 16, h: 16 }, label);
    middleText(ctx, t.name, tl + markRoom, top + 16, tw - amountWidth - 12 - markRoom, nameFont, label);
    middleText(ctx, t.cost, ir - amountWidth - 2, top + 16, amountWidth + 2, amountFont, label, 'right');
    // Tokens and share; a tag at the right, read whole; an unpriced count in orange after the figures.
    const lineFont = font(11.5, 500), tagFont = font(9.5, 500), line = [t.tokens, t.share].filter(Boolean).join(' · ');
    let tagWidth = 0;
    if (t.tag) {
      tagWidth = measure(ctx, t.tag, tagFont) + 16;
      const capsule: Box = { x: ir - tagWidth, y: top + 27.5, w: tagWidth, h: 17 };
      ctx.fillStyle = labelColour(dark, dark ? 0.09 : 0.06); roundRect(ctx, capsule, 8.5); ctx.fill();
      middleText(ctx, t.tag, capsule.x + 8, capsule.y + 8.5, capsule.w - 16, tagFont, secondary, 'center');
    }
    const room = tw - tagWidth - (tagWidth ? 10 : 0), lineWidth = Math.min(room, measure(ctx, line, lineFont));
    middleText(ctx, line, tl, top + 36, room, lineFont, secondary);
    if (t.unpriced && lineWidth + 60 < room) middleText(ctx, '· ' + t.unpriced, tl + lineWidth + 5, top + 36, room - lineWidth - 5, font(10.5, 600), systemColor('orange', dark));
    let my = top + ROWS.head;
    if (other) drawMembers(ctx, p, t, tl, my + ROWS.model / 2, tw, font(11.5), label, label, 13);
    for (const m of t.models) {
      const middle = my + ROWS.model / 2, nameRoom = tw - 196, noteFont = font(10, 500), nameWidth = Math.min(nameRoom, measure(ctx, m.name, font(11.5)));
      middleText(ctx, m.name, tl, middle, nameRoom, font(11.5), m.other ? secondary : label);
      if (m.note && nameWidth + 30 < nameRoom) middleText(ctx, m.note, tl + nameWidth + 6, middle, nameRoom - nameWidth - 6, noteFont, systemColor('orange', dark));
      const track: Box = { x: ir - 184, y: middle - 2, w: 56, h: 4 };
      ctx.fillStyle = labelColour(dark, dark ? 0.08 : 0.06); roundRect(ctx, track, 2); ctx.fill();
      ctx.fillStyle = m.other ? secondary : c; roundRect(ctx, { x: track.x, y: track.y, w: Math.max(4, track.w * m.ratio), h: 4 }, 2); ctx.fill();
      middleText(ctx, m.amount, ir - 120, middle, 72, rounded(11.5, 500), label, 'right');
      middleText(ctx, m.share, ir - 42, middle, 42, rounded(11, 500), secondary, 'right');
      my += ROWS.model;
    }
  });

  // Signature: the QR code, the app icon, our name and where to find us.
  drawSignature(ctx, p, project, L.sign + 20, { name: font(14, 600), small: (s) => font(s), ink: label, muted: secondary });
  return canvas;
}
