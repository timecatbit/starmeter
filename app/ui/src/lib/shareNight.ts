// 夜空: the logo's sky as a poster. By night a violet dusk with stars and a gold moon; by day a pale dawn with a white
// moon still up. The estimate glows in an italic serif, the day's curve lies over a still lake, and each tool's first
// models are lit bars.
import { smoothCurves, tracePath } from './chart';
import { accent, alpha } from './colors';
import { blurFilter, canvasFor, drawMark, measure, middleText, shadow, text, WIDTH } from './shareCommon';
import { drawAmount, drawMasthead, drawMembers, drawSignature, figures, fitAmount, HERO_BASELINE, layout, poster, posterFont, ROWS, SERIF, seeded, tickBox } from './sharePoster';
import type { Dashboard, Project } from './types';

const M = 28, INNER = WIDTH - M * 2;
const serif = (size: number, italic = true, weight = 400) => posterFont(size, weight, SERIF, italic);

const palettes = {
  dark: {
    sky: ['#241B3D', '#3B2959', '#2E2149', '#16112A'], star: [255, 244, 228], stars: 150, starStrength: 1,
    halo: 'rgba(255, 210, 170, 0.3)', disc: ['#FFF1DC', '#E3B892'], seas: 'rgba(176, 120, 96, 0.22)', cloud: 'rgba(226, 150, 160, 0.28)',
    text: '#FBEBD8', soft: 'rgb(205, 185, 230)', dim: 'rgba(205, 185, 230, 0.66)', curve: '#F4C69C', curveGlow: 'rgba(255, 190, 140, 0.8)',
    fill: 'rgba(244, 198, 156, 0.32)', end: '#FFF3E4', heroGlow: 'rgba(255, 196, 150, 0.5)', warn: '#FFB27A', rule: 'rgba(205, 185, 230, 0.28)',
    hills: '#1E1631', lake: 'rgba(90, 64, 120, 0.55)', light: [255, 200, 150], track: 'rgba(255, 255, 255, 0.09)',
  },
  light: {
    sky: ['#E7E0F4', '#F4E2EC', '#FAE8DE', '#F1ECF6'], star: [128, 104, 178], stars: 70, starStrength: 0.45,
    halo: 'rgba(255, 255, 255, 0.75)', disc: ['#FFFFFF', '#E9E1F2'], seas: 'rgba(150, 130, 190, 0.16)', cloud: 'rgba(255, 255, 255, 0.85)',
    text: '#2B2145', soft: 'rgb(104, 82, 156)', dim: 'rgba(43, 33, 69, 0.6)', curve: '#DE7357', curveGlow: 'rgba(222, 115, 87, 0.45)',
    fill: 'rgba(222, 115, 87, 0.2)', end: '#DE7357', heroGlow: 'rgba(222, 115, 87, 0.28)', warn: '#C2410C', rule: 'rgba(104, 82, 156, 0.25)',
    hills: 'rgba(120, 96, 168, 0.3)', lake: 'rgba(170, 150, 210, 0.35)', light: [222, 115, 87], track: 'rgba(43, 33, 69, 0.08)',
  },
};

/** A tool's colour: by night lifted toward the moonlight so it glows on the dark sky, by day as it is. */
function lit(id: string, dark: boolean): string {
  if (!dark) return accent(id, false);
  const m = accent(id, true).match(/\d+(\.\d+)?/g)?.map(Number) ?? [200, 200, 230];
  const lift = (v: number) => Math.round(v + (255 - v) * 0.28);
  return `rgb(${lift(m[0])}, ${lift(m[1])}, ${lift(m[2])})`;
}

export async function drawNightPicture(d: Dashboard, project: Project, dark: boolean): Promise<HTMLCanvasElement> {
  const c = dark ? palettes.dark : palettes.light;
  const p = await poster(d, c.text), L = layout(p), height = L.height;
  const measuring = document.createElement('canvas').getContext('2d')!;
  const hero = fitAmount(measuring, p.cost, INNER - 10, 92, 44, (s) => serif(s));
  const { canvas, ctx } = canvasFor(height);
  const random = seeded([...d.date].reduce((s, ch) => s + ch.charCodeAt(0), 0));
  const [sr, sg, sb] = c.star;

  // The sky, top to bottom.
  const sky = ctx.createLinearGradient(0, 0, 0, height);
  c.sky.forEach((colour, i) => sky.addColorStop([0, 0.18, 0.36, 1][i], colour));
  ctx.fillStyle = sky; ctx.fillRect(0, 0, WIDTH, height);
  // Stars, a few with a glint.
  for (let i = 0; i < c.stars; i++) {
    const x = random() * WIDTH, y = random() * height * 0.97, r = 0.35 + random() * 1.05, a = (0.25 + random() * 0.6) * c.starStrength;
    ctx.fillStyle = `rgba(${sr}, ${sg}, ${sb}, ${a})`; ctx.beginPath(); ctx.arc(x, y, r, 0, Math.PI * 2); ctx.fill();
    if (r > 1.25 && random() > 0.4) {
      ctx.strokeStyle = `rgba(${sr}, ${sg}, ${sb}, ${a * 0.55})`; ctx.lineWidth = 0.6;
      ctx.beginPath(); ctx.moveTo(x - 4, y); ctx.lineTo(x + 4, y); ctx.moveTo(x, y - 4); ctx.lineTo(x, y + 4); ctx.stroke();
    }
  }
  // The moon beside the title: a halo, the disc, its seas, a cloud drifting past.
  const moon = { x: WIDTH - 66, y: 104, r: 28 };
  const halo = ctx.createRadialGradient(moon.x, moon.y, moon.r * 0.6, moon.x, moon.y, moon.r * 3.2);
  halo.addColorStop(0, c.halo); halo.addColorStop(1, alpha(c.halo, 0));
  ctx.fillStyle = halo; ctx.fillRect(0, 0, WIDTH, moon.y + moon.r * 3.2);
  const disc = ctx.createRadialGradient(moon.x - 10, moon.y - 10, 4, moon.x, moon.y, moon.r);
  disc.addColorStop(0, c.disc[0]); disc.addColorStop(1, c.disc[1]);
  ctx.fillStyle = disc; ctx.beginPath(); ctx.arc(moon.x, moon.y, moon.r, 0, Math.PI * 2); ctx.fill();
  ctx.save(); ctx.beginPath(); ctx.arc(moon.x, moon.y, moon.r, 0, Math.PI * 2); ctx.clip();
  for (const [dx, dy, r] of [[-11, -7, 8], [9, 5, 6], [-4, 14, 4.5], [14, -12, 3.5], [-18, 9, 3.5], [4, -18, 3]]) {
    ctx.fillStyle = c.seas; ctx.beginPath(); ctx.arc(moon.x + dx, moon.y + dy, r, 0, Math.PI * 2); ctx.fill();
  }
  ctx.restore();
  ctx.save(); ctx.filter = blurFilter(4.5);
  ctx.fillStyle = c.cloud; ctx.beginPath(); ctx.ellipse(moon.x + 6, moon.y + 24, 44, 7, -0.1, 0, Math.PI * 2); ctx.fill();
  ctx.restore();

  // Masthead, the title, then the estimate glowing under it.
  drawMasthead(ctx, p, project.name, { ink: c.text, font: (s) => serif(s), name: 19, date: 16, left: M, right: WIDTH - M, weight: 400 });
  text(ctx, p.title, { x: M, y: L.title + 8, w: INNER - 80, h: 40 }, serif(34, false, 500), c.text);
  text(ctx, p.costLabel, { x: M, y: L.hero + 4, w: INNER, h: 18 }, serif(15, false), c.soft);
  const baseline = L.hero + HERO_BASELINE;
  ctx.save(); shadow(ctx, c.heroGlow, 13);
  drawAmount(ctx, p.cost, M - 2, baseline, hero, (s) => serif(s), c.text, c.soft);
  ctx.restore();
  let y = L.stats + 4;

  // Three figures, centred between faint rules.
  const column = INNER / 3;
  figures(p).forEach(([name, value], i) => {
    const x = M + i * column;
    if (i) { ctx.fillStyle = c.rule; ctx.fillRect(x, y + 4, 1, 52); }
    text(ctx, name, { x, y, w: column, h: 17 }, serif(13.5, false), c.dim, 'center');
    let size = 28;
    while (size > 16 && measure(ctx, value, serif(size, false, 500)) > column - 12) size -= 1;
    text(ctx, value, { x, y: y + 22, w: column, h: 34 }, serif(size, false, 500), c.text, 'center');
  });

  // The curve over the horizon, glowing; the light lies on the lake under it.
  const chart = p.chart;
  if (chart && L.chart !== null) {
    y = L.chart;
    text(ctx, chart.label, { x: M, y, w: INNER / 2, h: 18 }, serif(15), c.soft);
    if (chart.note) text(ctx, chart.note, { x: M + INNER / 2, y: y + 1, w: INNER / 2, h: 17 }, serif(13.5), c.dim, 'right');
    const plot = { x: M + 4, y: y + 30, w: INNER - 8, h: 92 }, bottom = plot.y + plot.h;
    const curve = smoothCurves([chart.values], chart.positions, plot, chart.ceiling).at(-1)!;
    ctx.save();
    ctx.beginPath(); tracePath(ctx, curve); ctx.lineTo(plot.x + plot.w, bottom); ctx.lineTo(plot.x, bottom); ctx.closePath();
    const fill = ctx.createLinearGradient(0, plot.y, 0, bottom);
    fill.addColorStop(0, c.fill); fill.addColorStop(1, alpha(c.fill, 0));
    ctx.fillStyle = fill; ctx.fill();
    shadow(ctx, c.curveGlow, 6);
    ctx.beginPath(); tracePath(ctx, curve); ctx.strokeStyle = c.curve; ctx.lineWidth = 2.2; ctx.lineCap = 'round'; ctx.lineJoin = 'round'; ctx.stroke();
    const end = curve.end;
    ctx.fillStyle = c.end; ctx.beginPath(); ctx.arc(end[0], end[1], 4.5, 0, Math.PI * 2); ctx.fill();
    ctx.restore();
    // Far hills on the horizon, then the lake holding the light.
    const horizon = bottom + 2;
    ctx.fillStyle = c.hills;
    ctx.beginPath(); ctx.moveTo(0, horizon);
    for (let x = 0; x <= WIDTH; x += 20) ctx.lineTo(x, horizon - 6 - 5 * Math.sin(x / 47) - 3 * Math.sin(x / 19 + 1));
    ctx.lineTo(WIDTH, horizon + 3); ctx.lineTo(0, horizon + 3); ctx.closePath(); ctx.fill();
    const lake = ctx.createLinearGradient(0, horizon, 0, horizon + 70);
    lake.addColorStop(0, c.lake); lake.addColorStop(1, alpha(c.lake, 0));
    ctx.fillStyle = lake; ctx.fillRect(0, horizon + 3, WIDTH, 70);
    const [lr, lg, lb] = c.light;
    for (let i = 0; i < 8; i++) {
      const w = 60 - i * 6, ly = horizon + 30 + i * 5, lx = end[0] - w / 2 + Math.sin(i * 1.7) * 5;
      ctx.fillStyle = `rgba(${lr}, ${lg}, ${lb}, ${0.36 - i * 0.04})`; ctx.fillRect(Math.min(WIDTH - w - 4, lx), ly, w, 1.3);
    }
    chart.ticks.forEach((tick, i) => {
      const at = tickBox(i, chart.ticks.length, plot.x + tick.at * plot.w, M, WIDTH - M, 90);
      text(ctx, tick.label, { x: at.x, y: horizon + 8, w: 90, h: 17 }, serif(14), c.text, at.align);
    });
  }

  // Each tool: its mark in a small halo, name and amount in the serif, a line of figures, then its models as lit bars.
  p.tools.forEach((t, index) => {
    y = L.tools[index];
    const colour = t.id === 'other' ? c.soft : lit(t.id, dark), mark = p.marks.tools.get(t.id);
    if (mark) {
      const glow = ctx.createRadialGradient(M + 11, y + 16, 2, M + 11, y + 16, 20);
      glow.addColorStop(0, alpha(colour, dark ? 0.45 : 0.28)); glow.addColorStop(1, alpha(colour, 0));
      ctx.fillStyle = glow; ctx.fillRect(M - 10, y - 5, 44, 44);
      drawMark(ctx, mark, { x: M + 2, y: y + 7, w: 18, h: 18 }, c.text);
    }
    const costWidth = Math.min(170, measure(ctx, t.cost, serif(22)) + 4);
    middleText(ctx, t.name, M + 32, y + 16, INNER - 32 - costWidth - 10, serif(24), c.soft);
    middleText(ctx, t.cost, WIDTH - M - costWidth, y + 16, costWidth, serif(22), c.text, 'right');
    y += 29;
    const line = [t.tokens, t.share, t.tag].filter(Boolean).join('  ·  '), lineWidth = Math.min(INNER - 32, measure(ctx, line, serif(13.5, false)));
    middleText(ctx, line, M + 32, y + 8, INNER - 32, serif(13.5, false), c.dim);
    if (t.unpriced && lineWidth + 80 < INNER - 32) middleText(ctx, '·  ' + t.unpriced, M + 32 + lineWidth + 8, y + 8, INNER - 32 - lineWidth - 8, serif(13, true), c.warn);
    y = L.tools[index] + ROWS.head;
    if (t.members.length) { drawMembers(ctx, p, t, M + 32, y + 10, INNER - 32, serif(15, false), c.text, c.text, 14); y += ROWS.model; }
    for (const m of t.models) {
      const nameRoom = INNER - 32 - 150, nameWidth = Math.min(nameRoom, measure(ctx, m.name, serif(14, false)));
      middleText(ctx, m.name, M + 32, y + 8, nameRoom, serif(14, false), m.other ? c.dim : c.text);
      if (m.note && nameWidth + 30 < nameRoom) middleText(ctx, m.note, M + 32 + nameWidth + 8, y + 8, nameRoom - nameWidth - 8, serif(12), c.warn);
      middleText(ctx, m.amount, WIDTH - M - 150, y + 8, 96, serif(14, false), c.text, 'right');
      middleText(ctx, m.share, WIDTH - M - 50, y + 8, 50, serif(14, false), c.text, 'right');
      const track = { x: M + 32, y: y + 16.5, w: INNER - 32, h: 2.5 };
      ctx.fillStyle = c.track; ctx.beginPath(); ctx.roundRect(track.x, track.y, track.w, track.h, 1.5); ctx.fill();
      const bar = m.other ? c.soft : colour;
      ctx.save(); shadow(ctx, alpha(bar, dark ? 1 : 0.5), dark ? 4 : 2);
      const fill = ctx.createLinearGradient(track.x, 0, track.x + track.w * m.ratio, 0);
      fill.addColorStop(0, alpha(bar, 0.7)); fill.addColorStop(1, bar);
      ctx.fillStyle = m.other ? alpha(bar, 0.6) : fill; ctx.beginPath(); ctx.roundRect(track.x, track.y, Math.max(3, track.w * m.ratio), track.h, 1.5); ctx.fill();
      ctx.restore();
      y += ROWS.model;
    }
  });

  // Signature.
  // A shooting star across the end of the sky, then the signature.
  const streak = ctx.createLinearGradient(M + 60, L.foot + 22, WIDTH - M - 60, L.foot + 6);
  streak.addColorStop(0, alpha(c.soft, 0)); streak.addColorStop(1, alpha(c.soft, 0.9));
  ctx.strokeStyle = streak; ctx.lineWidth = 1.4; ctx.lineCap = 'round';
  ctx.beginPath(); ctx.moveTo(M + 60, L.foot + 24); ctx.quadraticCurveTo(WIDTH / 2, L.foot + 10, WIDTH - M - 60, L.foot + 6); ctx.stroke();
  ctx.fillStyle = c.text; ctx.beginPath(); ctx.arc(WIDTH - M - 60, L.foot + 6, 1.8, 0, Math.PI * 2); ctx.fill();
  drawSignature(ctx, p, project, L.sign + 20, { name: serif(18), small: (s) => serif(s, false), size: 13, ink: c.text, muted: c.dim });
  return canvas;
}
