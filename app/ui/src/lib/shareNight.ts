// 夜空: the logo's night sky as a poster. A violet dusk with stars and a moon, the figures in an italic serif, the day's
// curve glowing over a still lake, and each tool's models as lit bars.
import { smoothCurves, tracePath } from './chart';
import { accent, alpha } from './colors';
import { canvasFor, drawMark, measure, text, WIDTH } from './shareCommon';
import { drawStar, poster, posterFont, SERIF, seeded, type PosterTool } from './sharePoster';
import type { Dashboard, Project } from './types';

const M = 28, INNER = WIDTH - M * 2;
const cream = '#FBEBD8', lavender = '#CDB9E6', dim = 'rgba(205, 185, 230, 0.62)', peach = '#F4C69C';
const serif = (size: number, italic = true, weight = 400) => posterFont(size, weight, SERIF, italic);
const MODEL = 44;

const blockHeight = (t: PosterTool) => 70 + Math.max(1, t.models.length) * MODEL + 26;

/** A tool's colour, lifted toward the moonlight so it glows on the dark sky. */
function moonlit(id: string): string {
  const m = accent(id, true).match(/\d+(\.\d+)?/g)?.map(Number) ?? [200, 200, 230];
  const lift = (v: number) => Math.round(v + (255 - v) * 0.28);
  return `rgb(${lift(m[0])}, ${lift(m[1])}, ${lift(m[2])})`;
}

export async function drawNightPicture(d: Dashboard, project: Project): Promise<HTMLCanvasElement> {
  const p = await poster(d, cream);
  const chart = p.values.length > 1;
  const sceneHeight = chart ? 560 : 420;
  const height = sceneHeight + 120 + 70 + p.tools.reduce((s, t) => s + blockHeight(t), 0) + 80;
  const { canvas, ctx } = canvasFor(height);
  const random = seeded([...d.date].reduce((s, c) => s + c.charCodeAt(0), 0));

  // Dusk to night.
  const sky = ctx.createLinearGradient(0, 0, 0, height);
  sky.addColorStop(0, '#241B3D'); sky.addColorStop(0.18, '#3B2959'); sky.addColorStop(0.34, '#2E2149'); sky.addColorStop(1, '#16112A');
  ctx.fillStyle = sky; ctx.fillRect(0, 0, WIDTH, height);
  // Stars, a few with a glint.
  for (let i = 0; i < 170; i++) {
    const x = random() * WIDTH, y = random() * height * 0.95, r = 0.35 + random() * 1.05, a = 0.25 + random() * 0.65;
    ctx.fillStyle = `rgba(255, 244, 228, ${a})`; ctx.beginPath(); ctx.arc(x, y, r, 0, Math.PI * 2); ctx.fill();
    if (r > 1.25 && random() > 0.4) {
      ctx.strokeStyle = `rgba(255, 244, 228, ${a * 0.55})`; ctx.lineWidth = 0.6;
      ctx.beginPath(); ctx.moveTo(x - 4, y); ctx.lineTo(x + 4, y); ctx.moveTo(x, y - 4); ctx.lineTo(x, y + 4); ctx.stroke();
    }
  }
  // The moon: a halo, the disc, its seas.
  const moon = { x: WIDTH - 74, y: 92, r: 34 };
  const halo = ctx.createRadialGradient(moon.x, moon.y, moon.r * 0.6, moon.x, moon.y, moon.r * 3.2);
  halo.addColorStop(0, 'rgba(255, 210, 170, 0.32)'); halo.addColorStop(1, 'rgba(255, 210, 170, 0)');
  ctx.fillStyle = halo; ctx.fillRect(0, 0, WIDTH, sceneHeight);
  const disc = ctx.createRadialGradient(moon.x - 12, moon.y - 12, 4, moon.x, moon.y, moon.r);
  disc.addColorStop(0, '#FFF1DC'); disc.addColorStop(1, '#E3B892');
  ctx.fillStyle = disc; ctx.beginPath(); ctx.arc(moon.x, moon.y, moon.r, 0, Math.PI * 2); ctx.fill();
  ctx.save(); ctx.beginPath(); ctx.arc(moon.x, moon.y, moon.r, 0, Math.PI * 2); ctx.clip();
  for (const [dx, dy, r] of [[-12, -8, 9], [10, 6, 7], [-4, 16, 5], [16, -14, 4], [-20, 10, 4], [4, -20, 3]]) {
    ctx.fillStyle = 'rgba(176, 120, 96, 0.22)'; ctx.beginPath(); ctx.arc(moon.x + dx, moon.y + dy, r, 0, Math.PI * 2); ctx.fill();
  }
  ctx.restore();
  // Soft clouds drifting past the moon and over the horizon.
  ctx.save(); ctx.filter = 'blur(10px)';
  for (const [x, y, w, h, colour] of [[WIDTH - 130, 132, 170, 20, 'rgba(226, 150, 160, 0.3)'], [WIDTH - 40, 150, 140, 14, 'rgba(240, 170, 150, 0.26)']] as const) {
    ctx.fillStyle = colour; ctx.beginPath(); ctx.ellipse(x, y, w / 2, h / 2, -0.08, 0, Math.PI * 2); ctx.fill();
  }
  ctx.restore();

  // Masthead.
  drawStar(ctx, p.star, M, 34, 18, cream);
  text(ctx, project.name, { x: M + 25, y: 31, w: 200, h: 24 }, serif(21), cream);
  text(ctx, p.date, { x: WIDTH / 2, y: 33, w: WIDTH / 2 - M, h: 22 }, serif(18), cream, 'right');
  text(ctx, p.title, { x: M, y: 76, w: INNER - 110, h: 50 }, serif(44, false, 500), cream);
  // The figure, glowing.
  let size = 92;
  while (size > 44 && measure(ctx, p.tokens, serif(size)) > INNER - 20) size -= 2;
  ctx.save(); ctx.shadowColor = 'rgba(255, 196, 150, 0.5)'; ctx.shadowBlur = 26;
  text(ctx, p.tokens, { x: M - 2, y: 138, w: INNER + 10, h: size * 1.1 }, serif(size), cream);
  ctx.restore();
  text(ctx, 'Token', { x: M + 8, y: 138 + size * 0.98, w: 200, h: 46 }, serif(40), lavender);

  let y = 138 + size * 0.98 + 66;
  if (chart) {
    text(ctx, p.chartLabel.replace(' ', '  '), { x: M, y, w: INNER, h: 20 }, serif(16), lavender);
    const plot = { x: M + 4, y: y + 34, w: INNER - 8, h: 104 };
    const curve = smoothCurves([p.values], p.positions, plot, p.ceiling).at(-1)!;
    const bottom = plot.y + plot.h;
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
    ctx.fillStyle = 'rgba(244, 198, 156, 0.35)'; ctx.fillRect(plot.x, bottom, plot.w, 1);
    p.ticks.forEach((tick, i) => {
      const x = plot.x + tick.at * plot.w, last = i === p.ticks.length - 1 && i > 0;
      text(ctx, tick.label, { x: last ? x - 90 : i === 0 ? x : x - 45, y: bottom + 12, w: 90, h: 18 }, serif(15), cream, last ? 'right' : i === 0 ? 'left' : 'center');
    });
    y = bottom + 46;
  }

  // The horizon: far hills, then a lake holding the moon's light.
  const horizon = y + 6;
  ctx.fillStyle = '#1E1631';
  ctx.beginPath(); ctx.moveTo(0, horizon);
  for (let x = 0; x <= WIDTH; x += 20) ctx.lineTo(x, horizon - 10 - 8 * Math.sin(x / 47) - 5 * Math.sin(x / 19 + 1));
  ctx.lineTo(WIDTH, horizon + 4); ctx.lineTo(0, horizon + 4); ctx.closePath(); ctx.fill();
  const lake = ctx.createLinearGradient(0, horizon, 0, horizon + 90);
  lake.addColorStop(0, 'rgba(90, 64, 120, 0.55)'); lake.addColorStop(1, 'rgba(22, 17, 42, 0)');
  ctx.fillStyle = lake; ctx.fillRect(0, horizon + 4, WIDTH, 90);
  for (let i = 0; i < 9; i++) {
    const w = 70 - i * 6, ly = horizon + 10 + i * 6;
    ctx.fillStyle = `rgba(255, 200, 150, ${0.42 - i * 0.04})`; ctx.fillRect(moon.x - w / 2 + Math.sin(i * 1.7) * 6, ly, w, 1.4);
  }
  y = horizon + 70;

  // Three figures, centred between faint rules.
  const column = INNER / 3;
  [[p.cost, p.costLabel], [p.requests, '请求数'], [p.cache, '缓存命中率']].forEach(([value, name], i) => {
    const x = M + i * column;
    if (i) { ctx.fillStyle = 'rgba(205, 185, 230, 0.28)'; ctx.fillRect(x, y + 4, 1, 64); }
    let vs = 34;
    while (vs > 18 && measure(ctx, value, serif(vs, false, 500)) > column - 12) vs -= 1;
    text(ctx, value, { x, y, w: column, h: 40 }, serif(vs, false, 500), cream, 'center');
    text(ctx, name, { x, y: y + 46, w: column, h: 18 }, serif(14, false), dim, 'center');
  });
  y += 110;

  // 模型用量, with a shooting star across.
  text(ctx, '模型用量', { x: M, y, w: 220, h: 44 }, serif(38, false, 500), cream);
  const streak = ctx.createLinearGradient(M + 170, y + 30, M + 330, y + 6);
  streak.addColorStop(0, 'rgba(255, 210, 170, 0)'); streak.addColorStop(1, 'rgba(255, 226, 196, 0.9)');
  ctx.strokeStyle = streak; ctx.lineWidth = 1.6; ctx.lineCap = 'round';
  ctx.beginPath(); ctx.moveTo(M + 170, y + 32); ctx.quadraticCurveTo(M + 250, y + 12, M + 330, y + 8); ctx.stroke();
  ctx.fillStyle = '#FFF3E4'; ctx.beginPath(); ctx.arc(M + 330, y + 8, 1.8, 0, Math.PI * 2); ctx.fill();
  y += 70;

  for (const t of p.tools) {
    const colour = moonlit(t.p.id), mark = p.marks.tools.get(t.p.id);
    // The tool's own mark in a small halo, its name and cost in the serif.
    const glow = ctx.createRadialGradient(M + 14, y + 20, 2, M + 14, y + 20, 22);
    glow.addColorStop(0, alpha(colour, 0.45)); glow.addColorStop(1, alpha(colour, 0));
    ctx.fillStyle = glow; ctx.fillRect(M - 10, y - 4, 48, 48);
    if (mark) drawMark(ctx, mark, { x: M + 3, y: y + 9, w: 22, h: 22 }, cream);
    const costWidth = Math.min(170, measure(ctx, t.cost, serif(30)) + 4);
    text(ctx, t.name, { x: M + 40, y, w: INNER - 40 - costWidth - 10, h: 40 }, serif(34), lavender);
    text(ctx, t.cost, { x: WIDTH - M - costWidth, y: y + 2, w: costWidth, h: 36 }, serif(30), cream, 'right');
    text(ctx, [t.tokens, t.note].filter(Boolean).join('  ·  '), { x: M + 40, y: y + 42, w: INNER - 40, h: 18 }, serif(14.5, false), dim);
    let my = y + 70;
    if (!t.models.length) text(ctx, '此时段暂无模型调用', { x: M + 40, y: my + 4, w: INNER, h: 18 }, serif(15, false), dim);
    for (const m of t.models) {
      text(ctx, m.name, { x: M + 40, y: my, w: INNER - 190, h: 22 }, serif(18, false), cream);
      text(ctx, m.amount, { x: WIDTH - M - 160, y: my + 1, w: 100, h: 20 }, serif(17, false), cream, 'right');
      text(ctx, m.share, { x: WIDTH - M - 56, y: my + 1, w: 56, h: 20 }, serif(17, false), cream, 'right');
      const track = { x: M + 40, y: my + 27, w: INNER - 40, h: 5 };
      ctx.fillStyle = 'rgba(255, 255, 255, 0.09)'; ctx.beginPath(); ctx.roundRect(track.x, track.y, track.w, track.h, 2.5); ctx.fill();
      ctx.save(); ctx.shadowColor = colour; ctx.shadowBlur = 9;
      const fill = ctx.createLinearGradient(track.x, 0, track.x + track.w * m.ratio, 0);
      fill.addColorStop(0, alpha(colour, 0.75)); fill.addColorStop(1, colour);
      ctx.fillStyle = fill; ctx.beginPath(); ctx.roundRect(track.x, track.y, Math.max(5, track.w * m.ratio), track.h, 2.5); ctx.fill();
      ctx.restore();
      my += MODEL;
    }
    y += blockHeight(t);
  }

  // Signature.
  const url = project.url.replace(/^https:\/\//, ''), urlFont = serif(15), urlWidth = measure(ctx, url, urlFont) + 26;
  drawStar(ctx, p.star, WIDTH / 2 - urlWidth / 2, height - 62, 16, cream);
  text(ctx, url, { x: WIDTH / 2 - urlWidth / 2 + 24, y: height - 62, w: urlWidth, h: 18 }, urlFont, dim);
  return canvas;
}
