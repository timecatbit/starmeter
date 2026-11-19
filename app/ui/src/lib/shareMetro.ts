// 地铁: a metro map. The estimate and figures on a yellow ticket, the hour by hour as yellow bars on a night-teal
// timetable, then a trunk line branching into one coloured line per tool with a station for each model, and the
// signature on the teal platform at the foot. Each section rises over the last with rounded shoulders.
import { ROUNDED } from './measure';
import { BOTTOM_BAND, canvasFor, drawMark, measure, middleText, text, TOP_BAND, WIDTH } from './shareCommon';
import { drawAmount, drawBars, drawMasthead, drawMembers, drawSignature, figures, fitAmount, poster, posterFont, slotCentre, tickBox, type PosterTool } from './sharePoster';
import type { Dashboard, Project } from './types';

const M = 24, INNER = WIDTH - M * 2;
const yellow = '#F6C52B', teal = '#103F3E', cream = '#F7F1E3', ink = '#12302F', faded = 'rgba(18, 48, 47, 0.62)', orange = '#C2410C';
const r = (size: number, weight = 800) => posterFont(size, weight, ROUNDED);
const TRUNK = 36, BRANCH = 90, LABEL = 116, HEAD = 52, STATION = 26, AFTER = 10, SHOULDER = 22, CHART = 196;

/** Line colours, saturated like a transit map; tools without one take a steady blue. */
const lines: Record<string, string> = {
  claude: '#D8432A', codex: '#7A3EA8', magpie: '#13907A', opencode: '#D83A6B', qoder: '#2E9A4E', gemini: '#1F77C9',
  cursor: '#3C4654', workbuddy: '#E0791E', 'deepseek-harness': '#3A59D8', opencodex: '#2A7CE0', other: '#8A8F8C',
};
const lineOf = (id: string) => lines[id] ?? '#2F6FD0';
const blockHeight = (t: PosterTool) => HEAD + (t.members.length ? 1 : Math.max(1, t.models.length)) * STATION + AFTER;

export async function drawMetroPicture(d: Dashboard, project: Project): Promise<HTMLCanvasElement> {
  const p = await poster(d, ink);
  const measuring = document.createElement('canvas').getContext('2d')!;
  const hero = fitAmount(measuring, p.cost, INNER, 84, 40, (s) => r(s, 900), -0.03);
  const ticket = TOP_BAND + 11 + 46 + 20 + Math.round(hero * 0.78) + 20 + 62 + 14 + SHOULDER;
  const depot = p.chart ? CHART : 0;
  const map = p.tools.length ? 30 + p.tools.reduce((s, t) => s + blockHeight(t), 0) + 10 + SHOULDER : SHOULDER;
  const height = ticket - SHOULDER + depot + map + 4 + 46 + BOTTOM_BAND;
  const { canvas, ctx } = canvasFor(height);
  const section = (top: number, colour: string) => { ctx.fillStyle = colour; ctx.beginPath(); ctx.roundRect(0, top, WIDTH, height - top, [24, 24, 0, 0]); ctx.fill(); };

  // The ticket: masthead, title, the estimate, the figures.
  ctx.fillStyle = yellow; ctx.fillRect(0, 0, WIDTH, height);
  drawMasthead(ctx, p, project.name, { ink, font: r, name: 19, date: 16, left: M, right: WIDTH - M, weight: 800 });
  let y = TOP_BAND + 11;
  text(ctx, p.title, { x: M, y, w: INNER, h: 36 }, r(30, 900), ink);
  y += 46;
  text(ctx, p.costLabel, { x: M, y, w: INNER, h: 16 }, r(13, 800), ink);
  y += 20;
  const baseline = y + Math.round(hero * 0.78);
  drawAmount(ctx, p.cost, M - 2, baseline, hero, (s) => r(s, 900), ink, ink, -0.03);
  y = baseline + 20;
  const column = INNER / 3;
  figures(p).forEach(([name, value], i) => {
    const x = M + i * column + (i ? 16 : 0), w = column - (i ? 16 : 0) - 14;
    if (i) { ctx.fillStyle = ink; ctx.beginPath(); ctx.roundRect(M + i * column, y + 4, 3, 50, 1.5); ctx.fill(); }
    text(ctx, name, { x, y, w, h: 17 }, r(13, 800), ink);
    let size = 26;
    while (size > 16 && measure(ctx, value, r(size, 900)) > w) size -= 1;
    text(ctx, value, { x, y: y + 22, w, h: 32 }, r(size, 900), ink);
  });

  // The timetable: the hour by hour on night teal.
  let top = ticket - SHOULDER;
  const chart = p.chart;
  if (chart) {
    section(top, teal);
    y = top + 24;
    text(ctx, chart.label, { x: M, y, w: INNER / 2, h: 24 }, r(19, 900), cream);
    if (chart.note) text(ctx, chart.note, { x: M + INNER / 2, y: y + 4, w: INNER / 2, h: 16 }, r(12.5, 700), 'rgba(247, 241, 227, 0.75)', 'right');
    const plot = { x: M + 2, y: y + 40, w: INNER - 4, h: 80 };
    drawBars(ctx, chart, plot, yellow, 0.24, 2);
    ctx.fillStyle = cream; ctx.fillRect(plot.x - 2, plot.y + plot.h + 2, plot.w + 4, 2.5);
    chart.ticks.forEach((tick, i) => {
      const cx = slotCentre(chart, plot, tick.index), at = tickBox(i, chart.ticks.length, cx, M, WIDTH - M, 90);
      ctx.fillRect(cx - 1.2, plot.y + plot.h + 2, 2.5, 7);
      text(ctx, tick.label, { x: at.x, y: plot.y + plot.h + 13, w: 90, h: 18 }, r(15, 900), cream, at.align);
    });
    top += depot;
  }

  // The map: a trunk from a terminus, each tool's line leaving it for its own stations.
  if (p.tools.length) section(top, cream);
  y = top + 30;
  const branches = p.tools.map((t) => { const at = y; y += blockHeight(t); return at; });
  if (p.tools.length) {
    ctx.strokeStyle = teal; ctx.lineWidth = 7; ctx.lineCap = 'round';
    ctx.beginPath(); ctx.moveTo(TRUNK, top + 26); ctx.lineTo(TRUNK, branches.at(-1)! + 6); ctx.stroke();
    ctx.fillStyle = cream; ctx.lineWidth = 6; ctx.beginPath(); ctx.arc(TRUNK, top + 22, 10, 0, Math.PI * 2); ctx.fill(); ctx.stroke();
  }
  p.tools.forEach((t, i) => {
    const at = branches[i], colour = lineOf(t.id), level = at + 18, rows = t.members.length ? 1 : Math.max(1, t.models.length);
    const lastStation = at + HEAD + (rows - 1) * STATION + 10;
    ctx.strokeStyle = colour; ctx.lineWidth = 7; ctx.lineCap = 'round'; ctx.lineJoin = 'round';
    ctx.beginPath(); ctx.moveTo(TRUNK, at - 14); ctx.lineTo(TRUNK + 26, level); ctx.lineTo(BRANCH, level); ctx.lineTo(BRANCH, lastStation); ctx.stroke();
    // The tool's mark at its interchange, then its name and amount in its line's colour.
    const mark = p.marks.tools.get(t.id);
    ctx.fillStyle = '#FFFFFF'; ctx.beginPath(); ctx.arc(BRANCH, level, 13.5, 0, Math.PI * 2); ctx.fill();
    ctx.lineWidth = 4; ctx.strokeStyle = colour; ctx.stroke();
    if (mark) drawMark(ctx, mark, { x: BRANCH - 8, y: level - 8, w: 16, h: 16 }, ink);
    const costWidth = Math.min(160, measure(ctx, t.cost, r(21, 900)) + 4);
    middleText(ctx, t.name, LABEL, level, WIDTH - M - LABEL - costWidth - 10, r(22, 900), colour);
    middleText(ctx, t.cost, WIDTH - M - costWidth, level, costWidth, r(21, 900), colour, 'right');
    const line = [t.tokens, t.share, t.tag].filter(Boolean).join(' · '), room = WIDTH - M - LABEL, used = Math.min(room, measure(ctx, line, r(12.5, 700)));
    middleText(ctx, line, LABEL, level + 20, room, r(12.5, 700), ink);
    if (t.unpriced && used + 80 < room) middleText(ctx, '· ' + t.unpriced, LABEL + used + 5, level + 20, room - used - 5, r(12, 800), orange);
    let sy = at + HEAD + 10;
    const stop = () => { ctx.fillStyle = '#FFFFFF'; ctx.strokeStyle = colour; ctx.lineWidth = 4; ctx.beginPath(); ctx.arc(BRANCH, sy, 8, 0, Math.PI * 2); ctx.fill(); ctx.stroke(); };
    if (t.members.length) { stop(); drawMembers(ctx, p, t, LABEL, sy, room, r(14.5, 800), ink, ink, 14); }
    if (!t.members.length && !t.models.length) { stop(); middleText(ctx, '此时段暂无模型调用', LABEL, sy, room, r(14, 700), faded); }
    for (const m of t.models) {
      stop();
      const nameRoom = room - 134, nameWidth = Math.min(nameRoom, measure(ctx, m.name, r(14.5, 800)));
      middleText(ctx, m.name, LABEL, sy, nameRoom, r(14.5, 800), m.other ? faded : ink);
      if (m.note && nameWidth + 30 < nameRoom) middleText(ctx, m.note, LABEL + nameWidth + 6, sy, nameRoom - nameWidth - 6, r(11, 800), orange);
      middleText(ctx, m.amount, WIDTH - M - 134, sy, 88, r(14, 800), ink, 'right');
      middleText(ctx, m.share, WIDTH - M - 44, sy, 44, r(14, 800), ink, 'right');
      sy += STATION;
    }
  });

  // The platform: our signature on teal.
  top += map;
  section(top - SHOULDER, teal);
  drawSignature(ctx, p, project, top + 4, { name: r(17, 900), url: r(13, 700), ink: cream, muted: 'rgba(247, 241, 227, 0.78)' });
  return canvas;
}
