// 地铁: a metro map. The figures on a yellow ticket, then a trunk line branching into one coloured line per tool with a
// station for each model, and the hour by hour as yellow bars on the night-teal depot below.
import { canvasFor, drawMark, measure, text, WIDTH } from './shareCommon';
import { drawBars, drawStar, poster, posterFont, slotCentre, type PosterTool } from './sharePoster';
import { ROUNDED } from './measure';
import type { Dashboard, Project } from './types';

const M = 24, INNER = WIDTH - M * 2;
const yellow = '#F6C52B', teal = '#103F3E', cream = '#F7F1E3', ink = '#12302F';
const r = (size: number, weight = 800) => posterFont(size, weight, ROUNDED);
const TRUNK = 36, BRANCH = 96, LABEL = 126, STATION = 40;

/** Line colours, saturated like a transit map; tools without one take a steady blue. */
const lines: Record<string, string> = {
  claude: '#D8432A', codex: '#7A3EA8', magpie: '#13907A', opencode: '#D83A6B', qoder: '#2E9A4E', gemini: '#1F77C9',
  cursor: '#3C4654', workbuddy: '#E0791E', 'deepseek-harness': '#3A59D8', opencodex: '#2A7CE0',
};
const lineOf = (id: string) => lines[id] ?? '#2F6FD0';
const blockHeight = (t: PosterTool) => 64 + Math.max(1, t.models.length) * STATION + 14;

export async function drawMetroPicture(d: Dashboard, project: Project): Promise<HTMLCanvasElement> {
  const p = await poster(d, ink);
  const chart = p.values.length > 1;
  const top = 300, mapTop = top + 24;
  const mapHeight = 70 + p.tools.reduce((s, t) => s + blockHeight(t), 0);
  const depot = chart ? 250 : 110;
  const height = mapTop + mapHeight + 20 + depot;
  const { canvas, ctx } = canvasFor(height);

  // The ticket: yellow, the body coming up under it with rounded shoulders.
  ctx.fillStyle = yellow; ctx.fillRect(0, 0, WIDTH, top + 40);
  ctx.fillStyle = cream; ctx.beginPath(); ctx.roundRect(0, top, WIDTH, height - top, [24, 24, 0, 0]); ctx.fill();

  drawStar(ctx, p.star, M, 33, 20, ink);
  text(ctx, project.name, { x: M + 27, y: 30, w: 200, h: 26 }, r(22, 800), ink);
  text(ctx, p.date, { x: WIDTH / 2, y: 32, w: WIDTH / 2 - M, h: 22 }, r(19, 800), ink, 'right');
  text(ctx, p.title, { x: M, y: 70, w: INNER, h: 40 }, r(34, 900), ink);
  const unitFont = r(32, 900), unitWidth = measure(ctx, 'Token', unitFont);
  let size = 88;
  while (size > 40 && measure(ctx, p.tokens, r(size, 900)) > INNER - unitWidth - 12) size -= 2;
  ctx.font = r(size, 900); ctx.fillStyle = ink; ctx.textAlign = 'left'; ctx.textBaseline = 'alphabetic';
  ctx.letterSpacing = `${-size * 0.03}px`;
  const baseline = 116 + size * 0.8;
  ctx.fillText(p.tokens, M - 2, baseline);
  const figureWidth = ctx.measureText(p.tokens).width;
  ctx.letterSpacing = '0px';
  ctx.font = unitFont; ctx.fillText('Token', M + figureWidth + 10, baseline);
  const column = INNER / 3, fy = baseline + 22;
  [[p.costLabel, p.cost], ['请求数', p.requests], ['缓存命中率', p.cache]].forEach(([name, value], i) => {
    const x = M + i * column + (i ? 16 : 0), w = column - (i ? 16 : 0) - 4;
    if (i) { ctx.fillStyle = ink; ctx.beginPath(); ctx.roundRect(M + i * column, fy + 4, 3, 54, 1.5); ctx.fill(); }
    text(ctx, name, { x, y: fy, w, h: 18 }, r(14, 800), ink);
    let vs = 32;
    while (vs > 18 && measure(ctx, value, r(vs, 900)) > w) vs -= 1;
    text(ctx, value, { x, y: fy + 22, w, h: 38 }, r(vs, 900), ink);
  });

  // The map: a trunk from a terminus, each tool's line leaving it for its own stations.
  let y = mapTop;
  text(ctx, '工具与模型', { x: M, y, w: INNER, h: 44 }, r(38, 900), ink);
  y += 70;
  const branches = p.tools.map((t) => { const at = y; y += blockHeight(t); return at; });
  if (p.tools.length) {
    ctx.strokeStyle = teal; ctx.lineWidth = 7; ctx.lineCap = 'round';
    ctx.beginPath(); ctx.moveTo(TRUNK, mapTop + 64); ctx.lineTo(TRUNK, branches.at(-1)! + 6); ctx.stroke();
    ctx.fillStyle = cream; ctx.lineWidth = 6; ctx.beginPath(); ctx.arc(TRUNK, mapTop + 58, 12, 0, Math.PI * 2); ctx.fill(); ctx.stroke();
  }
  p.tools.forEach((t, i) => {
    const at = branches[i], colour = lineOf(t.p.id), level = at + 20;
    const lastStation = at + 64 + (Math.max(1, t.models.length) - 1) * STATION + 12;
    ctx.strokeStyle = colour; ctx.lineWidth = 7; ctx.lineCap = 'round'; ctx.lineJoin = 'round';
    ctx.beginPath(); ctx.moveTo(TRUNK, at - 14); ctx.lineTo(TRUNK + 28, level); ctx.lineTo(BRANCH, level); ctx.lineTo(BRANCH, lastStation); ctx.stroke();
    // The tool's mark at its interchange, then its name and cost in its line's colour.
    const mark = p.marks.tools.get(t.p.id);
    ctx.fillStyle = '#FFFFFF'; ctx.beginPath(); ctx.arc(BRANCH, level, 15, 0, Math.PI * 2); ctx.fill();
    ctx.lineWidth = 4; ctx.strokeStyle = colour; ctx.stroke();
    if (mark) drawMark(ctx, mark, { x: BRANCH - 9, y: level - 9, w: 18, h: 18 }, ink);
    const costWidth = Math.min(160, measure(ctx, t.cost, r(26, 900)) + 4);
    text(ctx, t.name, { x: LABEL, y: level - 18, w: WIDTH - M - LABEL - costWidth - 10, h: 34 }, r(28, 900), colour);
    text(ctx, t.cost, { x: WIDTH - M - costWidth, y: level - 15, w: costWidth, h: 30 }, r(26, 900), colour, 'right');
    text(ctx, [t.tokens, t.note].filter(Boolean).join(' · '), { x: LABEL, y: level + 18, w: WIDTH - M - LABEL, h: 18 }, r(14, 700), ink);
    let sy = at + 64 + 12;
    if (!t.models.length) {
      text(ctx, '此时段暂无模型调用', { x: LABEL, y: sy - 9, w: WIDTH - M - LABEL, h: 18 }, r(15, 700), ink);
    }
    for (const m of t.models) {
      ctx.fillStyle = '#FFFFFF'; ctx.strokeStyle = colour; ctx.lineWidth = 4.5;
      ctx.beginPath(); ctx.arc(BRANCH, sy, 9.5, 0, Math.PI * 2); ctx.fill(); ctx.stroke();
      text(ctx, m.name, { x: LABEL, y: sy - 11, w: WIDTH - M - LABEL - 142, h: 22 }, r(17, 800), ink);
      text(ctx, m.amount, { x: WIDTH - M - 142, y: sy - 10, w: 92, h: 20 }, r(16, 800), ink, 'right');
      text(ctx, m.share, { x: WIDTH - M - 46, y: sy - 10, w: 46, h: 20 }, r(16, 800), ink, 'right');
      sy += STATION;
    }
  });

  // The depot: the hour by hour on night teal.
  const base = height - depot;
  ctx.fillStyle = teal; ctx.fillRect(0, base, WIDTH, depot);
  let by = base + 24;
  if (chart) {
    text(ctx, p.chartLabel, { x: M, y: by, w: INNER, h: 34 }, r(30, 900), cream);
    const plot = { x: M + 2, y: by + 50, w: INNER - 4, h: 92 };
    drawBars(ctx, p, plot, yellow, 0.24, 2);
    ctx.fillStyle = cream; ctx.fillRect(plot.x - 2, plot.y + plot.h + 2, plot.w + 4, 2.5);
    p.ticks.forEach((tick, i) => {
      const cx = slotCentre(p, plot, tick.index), last = i === p.ticks.length - 1 && i > 0;
      ctx.fillRect(cx - 1.2, plot.y + plot.h + 2, 2.5, 8);
      text(ctx, tick.label, { x: last ? WIDTH - M - 90 : i === 0 ? M : cx - 45, y: plot.y + plot.h + 14, w: 90, h: 22 }, r(19, 900), cream,
        last ? 'right' : i === 0 ? 'left' : 'center');
    });
    by = plot.y + plot.h + 50;
  }
  text(ctx, project.url.replace(/^https:\/\//, ''), { x: M, y: by, w: INNER, h: 18 }, r(15, 700), 'rgba(247, 241, 227, 0.8)');
  return canvas;
}
