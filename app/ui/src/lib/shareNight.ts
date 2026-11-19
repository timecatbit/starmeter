// 夜空: the logo's night sky as a poster. A violet dusk with stars and a moon, the estimate glowing in an italic serif,
// the day's curve over a still lake, and each tool's first models as lit bars.
import { smoothCurves, tracePath } from './chart';
import { accent, alpha } from './colors';
import { canvasFor, drawMark, measure, middleText, text, TOP_BAND, WIDTH } from './shareCommon';
import { drawAmount, drawMasthead, drawMembers, drawSignature, figures, fitAmount, poster, posterFont, SERIF, seeded, tickBox, type PosterTool } from './sharePoster';
import type { Dashboard, Project } from './types';

const M = 28, INNER = WIDTH - M * 2;
const cream = '#FBEBD8', lavender = 'rgb(205, 185, 230)', dim = 'rgba(205, 185, 230, 0.66)', peach = '#F4C69C', ember = '#FFB27A';
const serif = (size: number, italic = true, weight = 400) => posterFont(size, weight, SERIF, italic);
const HEAD = 32, SUB = 22, MODEL = 28, AFTER = 14, CHART = 176;

const blockHeight = (t: PosterTool) => HEAD + SUB + (t.members.length ? 1 : t.models.length) * MODEL + AFTER;

/** A tool's colour, lifted toward the moonlight so it glows on the dark sky. */
function moonlit(id: string): string {
  const m = accent(id, true).match(/\d+(\.\d+)?/g)?.map(Number) ?? [200, 200, 230];
  const lift = (v: number) => Math.round(v + (255 - v) * 0.28);
  return `rgb(${lift(m[0])}, ${lift(m[1])}, ${lift(m[2])})`;
}

export async function drawNightPicture(d: Dashboard, project: Project): Promise<HTMLCanvasElement> {
  const p = await poster(d, cream);
  const measuring = document.createElement('canvas').getContext('2d')!;
  const hero = fitAmount(measuring, p.cost, INNER - 10, 86, 44, (s) => serif(s));
  const height = TOP_BAND + 11 + 50 + 22 + Math.round(hero * 0.8) + 24 + 76 + (p.chart ? CHART : 30) + p.tools.reduce((s, t) => s + blockHeight(t), 0) + 20 + 46 + 34;
  const { canvas, ctx } = canvasFor(height);
  const random = seeded([...d.date].reduce((s, c) => s + c.charCodeAt(0), 0));

  // Dusk to night.
  const sky = ctx.createLinearGradient(0, 0, 0, height);
  sky.addColorStop(0, '#241B3D'); sky.addColorStop(0.18, '#3B2959'); sky.addColorStop(0.36, '#2E2149'); sky.addColorStop(1, '#16112A');
  ctx.fillStyle = sky; ctx.fillRect(0, 0, WIDTH, height);
  // Stars, a few with a glint.
  for (let i = 0; i < 150; i++) {
    const x = random() * WIDTH, y = random() * height * 0.97, r = 0.35 + random() * 1.05, a = 0.25 + random() * 0.6;
    ctx.fillStyle = `rgba(255, 244, 228, ${a})`; ctx.beginPath(); ctx.arc(x, y, r, 0, Math.PI * 2); ctx.fill();
    if (r > 1.25 && random() > 0.4) {
      ctx.strokeStyle = `rgba(255, 244, 228, ${a * 0.55})`; ctx.lineWidth = 0.6;
      ctx.beginPath(); ctx.moveTo(x - 4, y); ctx.lineTo(x + 4, y); ctx.moveTo(x, y - 4); ctx.lineTo(x, y + 4); ctx.stroke();
    }
  }
  // The moon beside the title: a halo, the disc, its seas, a cloud drifting past.
  const moon = { x: WIDTH - 66, y: 104, r: 28 };
  const halo = ctx.createRadialGradient(moon.x, moon.y, moon.r * 0.6, moon.x, moon.y, moon.r * 3.2);
  halo.addColorStop(0, 'rgba(255, 210, 170, 0.3)'); halo.addColorStop(1, 'rgba(255, 210, 170, 0)');
  ctx.fillStyle = halo; ctx.fillRect(0, 0, WIDTH, moon.y + moon.r * 3.2);
  const disc = ctx.createRadialGradient(moon.x - 10, moon.y - 10, 4, moon.x, moon.y, moon.r);
  disc.addColorStop(0, '#FFF1DC'); disc.addColorStop(1, '#E3B892');
  ctx.fillStyle = disc; ctx.beginPath(); ctx.arc(moon.x, moon.y, moon.r, 0, Math.PI * 2); ctx.fill();
  ctx.save(); ctx.beginPath(); ctx.arc(moon.x, moon.y, moon.r, 0, Math.PI * 2); ctx.clip();
  for (const [dx, dy, r] of [[-11, -7, 8], [9, 5, 6], [-4, 14, 4.5], [14, -12, 3.5], [-18, 9, 3.5], [4, -18, 3]]) {
    ctx.fillStyle = 'rgba(176, 120, 96, 0.22)'; ctx.beginPath(); ctx.arc(moon.x + dx, moon.y + dy, r, 0, Math.PI * 2); ctx.fill();
  }
  ctx.restore();
  ctx.save(); ctx.filter = 'blur(9px)';
  ctx.fillStyle = 'rgba(226, 150, 160, 0.28)'; ctx.beginPath(); ctx.ellipse(moon.x + 6, moon.y + 24, 44, 7, -0.1, 0, Math.PI * 2); ctx.fill();
  ctx.restore();

  // Masthead, the title, then the estimate glowing under it.
  drawMasthead(ctx, p, project.name, { ink: cream, font: (s) => serif(s), name: 19, date: 16, left: M, right: WIDTH - M, weight: 400 });
  let y = TOP_BAND + 11;
  text(ctx, p.title, { x: M, y, w: INNER - 80, h: 40 }, serif(34, false, 500), cream);
  y += 50;
  text(ctx, p.costLabel, { x: M, y, w: INNER, h: 18 }, serif(15, false), lavender);
  y += 22;
  const baseline = y + Math.round(hero * 0.8);
  ctx.save(); ctx.shadowColor = 'rgba(255, 196, 150, 0.5)'; ctx.shadowBlur = 26;
  drawAmount(ctx, p.cost, M - 2, baseline, hero, (s) => serif(s), cream, lavender);
  ctx.restore();
  y = baseline + 24;

  // Three figures, centred between faint rules.
  const column = INNER / 3;
  figures(p).forEach(([name, value], i) => {
    const x = M + i * column;
    if (i) { ctx.fillStyle = 'rgba(205, 185, 230, 0.28)'; ctx.fillRect(x, y + 4, 1, 52); }
    text(ctx, name, { x, y, w: column, h: 17 }, serif(13.5, false), dim, 'center');
    let size = 28;
    while (size > 16 && measure(ctx, value, serif(size, false, 500)) > column - 12) size -= 1;
    text(ctx, value, { x, y: y + 22, w: column, h: 34 }, serif(size, false, 500), cream, 'center');
  });
  y += 76;

  // The curve over the horizon, glowing; the moon's light lies on the lake under it.
  const chart = p.chart;
  if (chart) {
    text(ctx, chart.label, { x: M, y, w: INNER / 2, h: 18 }, serif(15), lavender);
    if (chart.note) text(ctx, chart.note, { x: M + INNER / 2, y: y + 1, w: INNER / 2, h: 17 }, serif(13.5), dim, 'right');
    const plot = { x: M + 4, y: y + 30, w: INNER - 8, h: 92 }, bottom = plot.y + plot.h;
    const curve = smoothCurves([chart.values], chart.positions, plot, chart.ceiling).at(-1)!;
    ctx.save();
    ctx.beginPath(); tracePath(ctx, curve); ctx.lineTo(plot.x + plot.w, bottom); ctx.lineTo(plot.x, bottom); ctx.closePath();
    const fill = ctx.createLinearGradient(0, plot.y, 0, bottom);
    fill.addColorStop(0, 'rgba(244, 198, 156, 0.32)'); fill.addColorStop(1, 'rgba(244, 198, 156, 0)');
    ctx.fillStyle = fill; ctx.fill();
    ctx.shadowColor = 'rgba(255, 190, 140, 0.8)'; ctx.shadowBlur = 12;
    ctx.beginPath(); tracePath(ctx, curve); ctx.strokeStyle = peach; ctx.lineWidth = 2.2; ctx.lineCap = 'round'; ctx.lineJoin = 'round'; ctx.stroke();
    const end = curve.end;
    ctx.fillStyle = '#FFF3E4'; ctx.beginPath(); ctx.arc(end[0], end[1], 4.5, 0, Math.PI * 2); ctx.fill();
    ctx.restore();
    // Far hills on the horizon, then the lake holding the light.
    const horizon = bottom + 2;
    ctx.fillStyle = '#1E1631';
    ctx.beginPath(); ctx.moveTo(0, horizon);
    for (let x = 0; x <= WIDTH; x += 20) ctx.lineTo(x, horizon - 6 - 5 * Math.sin(x / 47) - 3 * Math.sin(x / 19 + 1));
    ctx.lineTo(WIDTH, horizon + 3); ctx.lineTo(0, horizon + 3); ctx.closePath(); ctx.fill();
    const lake = ctx.createLinearGradient(0, horizon, 0, horizon + 70);
    lake.addColorStop(0, 'rgba(90, 64, 120, 0.55)'); lake.addColorStop(1, 'rgba(22, 17, 42, 0)');
    ctx.fillStyle = lake; ctx.fillRect(0, horizon + 3, WIDTH, 70);
    for (let i = 0; i < 8; i++) {
      const w = 60 - i * 6, ly = horizon + 30 + i * 5, lx = end[0] - w / 2 + Math.sin(i * 1.7) * 5;
      ctx.fillStyle = `rgba(255, 200, 150, ${0.36 - i * 0.04})`; ctx.fillRect(Math.min(WIDTH - w - 4, lx), ly, w, 1.3);
    }
    chart.ticks.forEach((tick, i) => {
      const at = tickBox(i, chart.ticks.length, plot.x + tick.at * plot.w, M, WIDTH - M, 90);
      text(ctx, tick.label, { x: at.x, y: horizon + 8, w: 90, h: 17 }, serif(14), cream, at.align);
    });
    y += CHART;
  } else y += 30;

  // Each tool: its mark in a small halo, name and amount in the serif, a line of figures, then its models as lit bars.
  for (const t of p.tools) {
    const colour = t.id === 'other' ? lavender : moonlit(t.id), mark = p.marks.tools.get(t.id);
    if (mark) {
      const glow = ctx.createRadialGradient(M + 11, y + 15, 2, M + 11, y + 15, 20);
      glow.addColorStop(0, alpha(colour, 0.45)); glow.addColorStop(1, alpha(colour, 0));
      ctx.fillStyle = glow; ctx.fillRect(M - 10, y - 6, 44, 44);
      drawMark(ctx, mark, { x: M + 2, y: y + 6, w: 18, h: 18 }, cream);
    }
    const costWidth = Math.min(170, measure(ctx, t.cost, serif(22)) + 4);
    middleText(ctx, t.name, M + 32, y + 15, INNER - 32 - costWidth - 10, serif(24), lavender);
    middleText(ctx, t.cost, WIDTH - M - costWidth, y + 15, costWidth, serif(22), cream, 'right');
    y += HEAD;
    const line = [t.tokens, t.share, t.tag].filter(Boolean).join('  ·  '), lineWidth = Math.min(INNER - 32, measure(ctx, line, serif(13.5, false)));
    middleText(ctx, line, M + 32, y + 8, INNER - 32, serif(13.5, false), dim);
    if (t.unpriced && lineWidth + 80 < INNER - 32) middleText(ctx, '·  ' + t.unpriced, M + 32 + lineWidth + 8, y + 8, INNER - 32 - lineWidth - 8, serif(13, true), ember);
    y += SUB;
    if (t.members.length) { drawMembers(ctx, p, t, M + 32, y + 10, INNER - 32, serif(15, false), cream, cream, 14); y += MODEL; }
    for (const m of t.models) {
      const nameRoom = INNER - 32 - 150, nameWidth = Math.min(nameRoom, measure(ctx, m.name, serif(15, false)));
      middleText(ctx, m.name, M + 32, y + 8, nameRoom, serif(15, false), m.other ? dim : cream);
      if (m.note && nameWidth + 30 < nameRoom) middleText(ctx, m.note, M + 32 + nameWidth + 8, y + 8, nameRoom - nameWidth - 8, serif(12), ember);
      middleText(ctx, m.amount, WIDTH - M - 150, y + 8, 96, serif(15, false), cream, 'right');
      middleText(ctx, m.share, WIDTH - M - 50, y + 8, 50, serif(15, false), cream, 'right');
      const track = { x: M + 32, y: y + 20, w: INNER - 32, h: 3 };
      ctx.fillStyle = 'rgba(255, 255, 255, 0.09)'; ctx.beginPath(); ctx.roundRect(track.x, track.y, track.w, track.h, 1.5); ctx.fill();
      const lit = m.other ? lavender : colour;
      ctx.save(); ctx.shadowColor = lit; ctx.shadowBlur = 8;
      const fill = ctx.createLinearGradient(track.x, 0, track.x + track.w * m.ratio, 0);
      fill.addColorStop(0, alpha(lit, 0.7)); fill.addColorStop(1, lit);
      ctx.fillStyle = m.other ? alpha(lavender, 0.6) : fill; ctx.beginPath(); ctx.roundRect(track.x, track.y, Math.max(3, track.w * m.ratio), track.h, 1.5); ctx.fill();
      ctx.restore();
      y += MODEL;
    }
    y += AFTER;
  }

  // Signature.
  drawSignature(ctx, p, project, y + 20, { name: serif(18), url: serif(13.5, false), ink: cream, muted: dim });
  return canvas;
}
