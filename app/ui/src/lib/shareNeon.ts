// 霓虹: a neon sign on a brick wall. By night the tubes glow white-hot with coloured halos on dark brick; by day they
// are coloured glass on whitewashed brick. The title and the estimate are a sign in a tube frame, the three figures
// and the trend are lit in their own colours, and each tool is lettered in its own colour with its models as short
// lit tubes.
import { smoothCurves, tracePath } from './chart';
import { accent, alpha } from './colors';
import { ROUNDED } from './measure';
import { canvasFor, drawMark, grain, measure, middleText, shadow, text, WIDTH } from './shareCommon';
import { drawAmount, drawMasthead, drawMembers, drawSignature, figures, fitAmount, HERO_BASELINE, layout, poster, posterFont, ROWS, tickBox, SHARE_TOOLS } from './sharePoster';
import type { Dashboard, Project } from './types';

const M = 28, INNER = WIDTH - M * 2;
const r = (size: number, weight = 700) => posterFont(size, weight, ROUNDED);

const palettes = {
  dark: {
    wall: 'rgb(22, 18, 27)', mortar: 'rgba(255, 255, 255, 0.045)', brick: 'rgba(255, 255, 255, 0.03)', text: 'rgb(246, 238, 248)', soft: 'rgba(246, 238, 248, 0.62)',
    pink: 'rgb(255, 79, 163)', cyan: 'rgb(62, 240, 255)', yellow: 'rgb(255, 216, 74)', green: 'rgb(88, 242, 154)', violet: 'rgb(181, 123, 255)',
    warn: 'rgb(255, 170, 110)', other: 'rgb(200, 190, 214)', track: 'rgba(255, 255, 255, 0.08)', glow: 1,
  },
  light: {
    wall: 'rgb(238, 234, 229)', mortar: 'rgba(60, 40, 30, 0.07)', brick: 'rgba(60, 40, 30, 0.025)', text: 'rgb(38, 33, 43)', soft: 'rgba(38, 33, 43, 0.62)',
    pink: 'rgb(226, 34, 126)', cyan: 'rgb(0, 150, 178)', yellow: 'rgb(205, 150, 0)', green: 'rgb(18, 150, 86)', violet: 'rgb(120, 64, 220)',
    warn: 'rgb(194, 65, 12)', other: 'rgb(112, 102, 124)', track: 'rgba(38, 33, 43, 0.08)', glow: 0.45,
  },
};

/** A colour taken toward white (the hot core of a lit tube), or toward black when the amount is negative (glass
 *  that must read on a pale wall). */
function core(colour: string, amount: number): string {
  const [red, green, blue] = colour.match(/\d+(\.\d+)?/g)!.map(Number);
  const lift = (v: number) => Math.round(amount >= 0 ? v + (255 - v) * amount : v * (1 + amount));
  return `rgb(${lift(red)}, ${lift(green)}, ${lift(blue)})`;
}

export async function drawNeonPicture(d: Dashboard, project: Project, dark: boolean, tools = SHARE_TOOLS): Promise<HTMLCanvasElement> {
  const c = dark ? palettes.dark : palettes.light;
  const p = await poster(d, c.text, tools), L = layout(p), height = L.height;
  const { canvas, ctx } = canvasFor(height);
  const hero = fitAmount(ctx, p.cost, INNER - 40, 74, 32, (s) => r(s, 800));
  const frame = { x: M - 8, y: L.title + 4, w: INNER + 16, h: L.stats - 8 - L.title - 4 };
  /** Draws once with a wide halo, once with a tight one, then the core: a lit tube. By day, only a faint halo. */
  const lit = (colour: string, paint: (fill: string) => void, strength = 1) => {
    ctx.save();
    if (dark) {
      shadow(ctx, alpha(colour, 0.9), 9 * strength); paint(colour);
      shadow(ctx, colour, 3 * strength); paint(colour);
      ctx.restore(); ctx.save(); paint(core(colour, 0.55));
    } else {
      shadow(ctx, alpha(colour, c.glow), 4 * strength); paint(colour);
    }
    ctx.restore();
  };

  // The wall: brick courses to every edge, a few bricks darker or lighter, and the signs' light pooling on it.
  ctx.fillStyle = c.wall; ctx.fillRect(0, 0, WIDTH, height);
  const course = 15, brick = 46;
  for (let row = 0, y = 0; y < height; row++, y += course) {
    const offset = row % 2 ? brick / 2 : 0;
    for (let x = -offset; x < WIDTH; x += brick) {
      if (grain(row, Math.round(x), 5) % 5 === 0) { ctx.fillStyle = c.brick; ctx.fillRect(x + 1, y + 1, brick - 2, course - 2); }
      ctx.fillStyle = c.mortar; ctx.fillRect(x, y, 1.2, course);
    }
    ctx.fillStyle = c.mortar; ctx.fillRect(0, y, WIDTH, 1.2);
  }
  for (const [x, y, radius, colour] of [[WIDTH * 0.25, frame.y + frame.h * 0.45, 260, c.pink], [WIDTH * 0.8, frame.y + frame.h * 0.7, 220, c.cyan]] as const) {
    const pool = ctx.createRadialGradient(x, y, 0, x, y, radius);
    pool.addColorStop(0, alpha(colour, dark ? 0.16 : 0.08)); pool.addColorStop(1, alpha(colour, 0));
    ctx.fillStyle = pool; ctx.fillRect(0, 0, WIDTH, height);
  }

  drawMasthead(ctx, p, project.name, { ink: c.text, font: r, name: 18, date: 15, left: M, right: WIDTH - M, weight: 800 });

  // The sign: a violet tube frame around the title and the estimate.
  lit(c.violet, (colour) => { ctx.lineWidth = 2.6; ctx.strokeStyle = colour; ctx.beginPath(); ctx.roundRect(frame.x, frame.y, frame.w, frame.h, 18); ctx.stroke(); });
  lit(c.pink, (fill) => text(ctx, p.title, { x: M + 10, y: L.title + 20, w: INNER - 20, h: 32 }, r(28, 800), fill));
  text(ctx, p.costLabel, { x: M + 10, y: L.hero + 6, w: INNER - 20, h: 17 }, r(13, 600), c.soft);
  lit(c.cyan, (fill) => drawAmount(ctx, p.cost, M + 8, L.hero + HERO_BASELINE - 6, hero, (s) => r(s, 800), fill, fill), 1.2);
  let y = L.stats + 4;

  // Three figures, each lit in its own colour.
  const column = INNER / 3, colours = [c.yellow, c.green, c.violet];
  figures(p).forEach(([name, value], i) => {
    const x = M + i * column;
    text(ctx, name, { x, y, w: column, h: 16 }, r(12.5, 600), c.soft, 'center');
    let size = 26;
    while (size > 14 && measure(ctx, value, r(size, 800)) > column - 14) size -= 1;
    lit(colours[i], (fill) => text(ctx, value, { x, y: y + 22, w: column, h: 32 }, r(size, 800), fill, 'center'), 0.8);
  });

  // The trend as a pink tube over a faint glow.
  const chart = p.chart;
  if (chart && L.chart !== null) {
    y = L.chart;
    text(ctx, chart.label, { x: M, y, w: INNER / 2, h: 16 }, r(13, 700), c.text);
    if (chart.note) text(ctx, chart.note, { x: M + INNER / 2, y, w: INNER / 2, h: 16 }, r(12, 600), c.soft, 'right');
    const plot = { x: M + 2, y: y + 30, w: INNER - 4, h: 96 }, bottom = plot.y + plot.h;
    const curve = smoothCurves([chart.values], chart.positions, plot, chart.ceiling).at(-1)!;
    ctx.beginPath(); tracePath(ctx, curve); ctx.lineTo(plot.x + plot.w, bottom); ctx.lineTo(plot.x, bottom); ctx.closePath();
    const fill = ctx.createLinearGradient(0, plot.y, 0, bottom);
    fill.addColorStop(0, alpha(c.pink, dark ? 0.22 : 0.12)); fill.addColorStop(1, alpha(c.pink, 0));
    ctx.fillStyle = fill; ctx.fill();
    ctx.fillStyle = c.track; ctx.fillRect(plot.x, bottom, plot.w, 1);
    ctx.lineCap = 'round'; ctx.lineJoin = 'round';
    lit(c.pink, (colour) => { ctx.beginPath(); tracePath(ctx, curve); ctx.lineWidth = 2.4; ctx.strokeStyle = colour; ctx.stroke(); });
    lit(c.pink, (colour) => { ctx.fillStyle = colour; ctx.beginPath(); ctx.arc(curve.end[0], curve.end[1], 4, 0, Math.PI * 2); ctx.fill(); });
    chart.ticks.forEach((tick, i) => {
      const at = tickBox(i, chart.ticks.length, plot.x + tick.at * plot.w, M, WIDTH - M, 80);
      text(ctx, tick.label, { x: at.x, y: bottom + 8, w: 80, h: 14 }, r(11.5, 600), c.soft, at.align);
    });
  }

  // Each tool lettered in its own colour, its amount and figures, then its models with short lit tubes.
  p.tools.forEach((tool, index) => {
    y = L.tools[index];
    const colour = tool.id === 'other' ? c.other : dark ? core(accent(tool.id, true), 0.2) : core(accent(tool.id, false), -0.18);
    const mark = p.marks.tools.get(tool.id), indent = mark ? 26 : 0, costWidth = measure(ctx, tool.cost, r(18, 800));
    if (mark) drawMark(ctx, mark, { x: M, y: y + 7, w: 18, h: 18 }, c.text);
    lit(colour, (fill) => middleText(ctx, tool.name, M + indent, y + 16, INNER - indent - costWidth - 12, r(19, 800), fill), 0.6);
    middleText(ctx, tool.cost, WIDTH - M - costWidth - 2, y + 16, costWidth + 2, r(18, 800), c.text, 'right');
    y += 29;
    const line = [tool.tokens, tool.share, tool.tag].filter(Boolean).join(' · '), used = Math.min(INNER - indent, measure(ctx, line, r(12, 600)));
    middleText(ctx, line, M + indent, y + 8, INNER - indent, r(12, 600), c.soft);
    if (tool.unpriced && used + 70 < INNER - indent) middleText(ctx, '· ' + tool.unpriced, M + indent + used + 6, y + 8, INNER - indent - used - 6, r(12, 700), c.warn);
    y = L.tools[index] + ROWS.head;
    if (tool.members.length) { drawMembers(ctx, p, tool, M + indent, y + 10.5, INNER - indent, r(13.5, 600), c.text, c.text, 13); y += ROWS.model; }
    for (const m of tool.models) {
      const nameRoom = INNER - indent - 180, nameWidth = Math.min(nameRoom, measure(ctx, m.name, r(13.5, 600)));
      middleText(ctx, m.name, M + indent, y + 10.5, nameRoom, r(13.5, 600), m.other ? c.soft : c.text);
      if (m.note && nameWidth + 30 < nameRoom) middleText(ctx, m.note, M + indent + nameWidth + 6, y + 10.5, nameRoom - nameWidth - 6, r(11, 700), c.warn);
      const tube = { x: WIDTH - M - 172, y: y + 9, w: 60 };
      ctx.fillStyle = c.track; ctx.beginPath(); ctx.roundRect(tube.x, tube.y, tube.w, 3, 1.5); ctx.fill();
      lit(m.other ? c.other : colour, (fill) => { ctx.fillStyle = fill; ctx.beginPath(); ctx.roundRect(tube.x, tube.y, Math.max(3, tube.w * m.ratio), 3, 1.5); ctx.fill(); }, 0.5);
      middleText(ctx, m.amount, WIDTH - M - 104, y + 10.5, 62, r(13.5, 700), c.text, 'right');
      middleText(ctx, m.share, WIDTH - M - 40, y + 10.5, 40, r(13, 600), c.soft, 'right');
      y += ROWS.model;
    }
  });

  drawSignature(ctx, p, project, L.sign + 20, { name: r(16, 800), small: (s) => r(s, 600), size: 12, ink: c.text, muted: c.soft });
  return canvas;
}
