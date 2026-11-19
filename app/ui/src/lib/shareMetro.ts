// 地铁: a metro map. The estimate and figures on a yellow ticket (by night, yellow print on a dark ticket), the hour by
// hour as yellow bars on a night-teal timetable, then a trunk line branching into one coloured line per tool with a
// station for each model, and the signature on the teal platform at the foot. Each section rises over the last with
// rounded shoulders.
import { ROUNDED } from './measure';
import { canvasFor, drawMark, measure, middleText, text, WIDTH } from './shareCommon';
import { drawAmount, drawBars, drawMasthead, drawMembers, drawSignature, figures, fitAmount, HERO_BASELINE, layout, poster, posterFont, ROWS, slotCentre, tickBox, SHARE_TOOLS } from './sharePoster';
import type { Dashboard, Project } from './types';

const M = 24, INNER = WIDTH - M * 2;
const palettes = {
  light: { ticket: '#F6C52B', ticketInk: '#12302F', depot: '#103F3E', bars: '#F6C52B', depotInk: '#F7F1E3', depotMuted: 'rgba(247, 241, 227, 0.75)',
    map: '#F7F1E3', ink: '#12302F', faded: 'rgba(18, 48, 47, 0.62)', trunk: '#103F3E', station: '#FFFFFF', warn: '#C2410C' },
  dark: { ticket: '#1E2225', ticketInk: '#F6C52B', depot: '#0B2928', bars: '#F6C52B', depotInk: '#EDE7D8', depotMuted: 'rgba(237, 231, 216, 0.7)',
    map: '#141918', ink: '#EDE7D8', faded: 'rgba(237, 231, 216, 0.58)', trunk: '#3C8A86', station: '#141918', warn: '#F0915A' },
};
const r = (size: number, weight = 800) => posterFont(size, weight, ROUNDED);
const TRUNK = 36, BRANCH = 90, LABEL = 116, SHOULDER = 18;

/** Line colours, saturated like a transit map (lighter by night, to read on the dark map); tools without one take a
 *  steady blue. */
const lines: Record<string, [string, string]> = {
  claude: ['#D8432A', '#F06B50'], codex: ['#7A3EA8', '#B07CE0'], magpie: ['#13907A', '#3CC4A8'], opencode: ['#D83A6B', '#F0648F'],
  qoder: ['#2E9A4E', '#52C774'], gemini: ['#1F77C9', '#5AA6EE'], cursor: ['#3C4654', '#9AA6B6'], workbuddy: ['#E0791E', '#F49C4E'],
  'deepseek-harness': ['#3A59D8', '#7088F2'], opencodex: ['#2A7CE0', '#62A3F2'], other: ['#8A8F8C', '#8E9592'],
};
const lineOf = (id: string, dark: boolean) => (lines[id] ?? ['#2F6FD0', '#6A9BEC'])[dark ? 1 : 0];

export async function drawMetroPicture(d: Dashboard, project: Project, dark: boolean, tools = SHARE_TOOLS): Promise<HTMLCanvasElement> {
  const c = dark ? palettes.dark : palettes.light;
  const p = await poster(d, c.ink, tools), L = layout(p), height = L.height;
  const { canvas, ctx } = canvasFor(height);
  const section = (top: number, colour: string) => { ctx.fillStyle = colour; ctx.beginPath(); ctx.roundRect(0, top, WIDTH, height - top, [24, 24, 0, 0]); ctx.fill(); };

  // The ticket: masthead, title, the estimate, the figures.
  const ink = c.ticketInk;
  ctx.fillStyle = c.ticket; ctx.fillRect(0, 0, WIDTH, height);
  drawMasthead(ctx, p, project.name, { ink, font: r, name: 19, date: 16, left: M, right: WIDTH - M, weight: 800 });
  text(ctx, p.title, { x: M, y: L.title + 8, w: INNER, h: 36 }, r(30, 900), ink);
  text(ctx, p.costLabel, { x: M, y: L.hero + 4, w: INNER, h: 16 }, r(13, 800), ink);
  const hero = fitAmount(ctx, p.cost, INNER, 88, 40, (s) => r(s, 900), -0.03);
  drawAmount(ctx, p.cost, M - 2, L.hero + HERO_BASELINE, hero, (s) => r(s, 900), ink, ink, -0.03);
  const column = INNER / 3, y = L.stats + 2;
  figures(p).forEach(([name, value], i) => {
    const x = M + i * column + (i ? 16 : 0), w = column - (i ? 16 : 0) - 14;
    if (i) { ctx.fillStyle = ink; ctx.beginPath(); ctx.roundRect(M + i * column, y + 4, 3, 48, 1.5); ctx.fill(); }
    text(ctx, name, { x, y, w, h: 17 }, r(13, 800), ink);
    let size = 26;
    while (size > 16 && measure(ctx, value, r(size, 900)) > w) size -= 1;
    text(ctx, value, { x, y: y + 22, w, h: 32 }, r(size, 900), ink);
  });

  // The timetable: the hour by hour on night teal.
  const chart = p.chart;
  if (chart && L.chart !== null) {
    section(L.chart - SHOULDER, c.depot);
    const top = L.chart + 6;
    text(ctx, chart.label, { x: M, y: top, w: INNER / 2, h: 24 }, r(19, 900), c.depotInk);
    if (chart.note) text(ctx, chart.note, { x: M + INNER / 2, y: top + 4, w: INNER / 2, h: 16 }, r(12.5, 700), c.depotMuted, 'right');
    const plot = { x: M + 2, y: top + 36, w: INNER - 4, h: 70 };
    drawBars(ctx, chart, plot, c.bars, 0.24, 2);
    ctx.fillStyle = c.depotInk; ctx.fillRect(plot.x - 2, plot.y + plot.h + 2, plot.w + 4, 2.5);
    chart.ticks.forEach((tick, i) => {
      const cx = slotCentre(chart, plot, tick.index), at = tickBox(i, chart.ticks.length, cx, M, WIDTH - M, 90);
      ctx.fillRect(cx - 1.2, plot.y + plot.h + 2, 2.5, 7);
      text(ctx, tick.label, { x: at.x, y: plot.y + plot.h + 13, w: 90, h: 18 }, r(15, 900), c.depotInk, at.align);
    });
  }

  // The map: a trunk from a terminus, each tool's line leaving it for its own stations.
  const mapTop = (L.chart !== null ? L.chart + ROWS.chart : L.stats + ROWS.stats) - SHOULDER;
  section(mapTop, c.map);
  if (p.tools.length) {
    ctx.strokeStyle = c.trunk; ctx.lineWidth = 7; ctx.lineCap = 'round';
    ctx.beginPath(); ctx.moveTo(TRUNK, mapTop + 12); ctx.lineTo(TRUNK, L.tools.at(-1)! + 2); ctx.stroke();
    ctx.fillStyle = c.map; ctx.lineWidth = 5; ctx.beginPath(); ctx.arc(TRUNK, mapTop + 10, 7, 0, Math.PI * 2); ctx.fill(); ctx.stroke();
  }
  p.tools.forEach((t, i) => {
    const at = L.tools[i], colour = lineOf(t.id, dark), level = at + 16, rows = t.members.length ? 1 : Math.max(1, t.models.length);
    const station = (row: number) => at + ROWS.head + row * ROWS.model + ROWS.model / 2;
    ctx.strokeStyle = colour; ctx.lineWidth = 7; ctx.lineCap = 'round'; ctx.lineJoin = 'round';
    ctx.beginPath(); ctx.moveTo(TRUNK, at); ctx.lineTo(TRUNK + 16, level); ctx.lineTo(BRANCH, level); ctx.lineTo(BRANCH, station(rows - 1)); ctx.stroke();
    // The tool's mark at its interchange, then its name and amount in its line's colour.
    const mark = p.marks.tools.get(t.id);
    ctx.fillStyle = c.station; ctx.beginPath(); ctx.arc(BRANCH, level, 13.5, 0, Math.PI * 2); ctx.fill();
    ctx.lineWidth = 4; ctx.strokeStyle = colour; ctx.stroke();
    if (mark) drawMark(ctx, mark, { x: BRANCH - 8, y: level - 8, w: 16, h: 16 }, c.ink);
    const costWidth = Math.min(160, measure(ctx, t.cost, r(21, 900)) + 4);
    middleText(ctx, t.name, LABEL, level, WIDTH - M - LABEL - costWidth - 10, r(22, 900), colour);
    middleText(ctx, t.cost, WIDTH - M - costWidth, level, costWidth, r(21, 900), colour, 'right');
    const line = [t.tokens, t.share, t.tag].filter(Boolean).join(' · '), room = WIDTH - M - LABEL, used = Math.min(room, measure(ctx, line, r(12.5, 700)));
    middleText(ctx, line, LABEL, at + 37, room, r(12.5, 700), c.ink);
    if (t.unpriced && used + 80 < room) middleText(ctx, '· ' + t.unpriced, LABEL + used + 5, at + 37, room - used - 5, r(12, 800), c.warn);
    const stop = (sy: number) => { ctx.fillStyle = c.station; ctx.strokeStyle = colour; ctx.lineWidth = 3.5; ctx.beginPath(); ctx.arc(BRANCH, sy, 6.5, 0, Math.PI * 2); ctx.fill(); ctx.stroke(); };
    if (t.members.length) { stop(station(0)); drawMembers(ctx, p, t, LABEL, station(0), room, r(14, 800), c.ink, c.ink, 14); }
    t.models.forEach((m, row) => {
      const sy = station(row);
      stop(sy);
      const nameRoom = room - 134, nameWidth = Math.min(nameRoom, measure(ctx, m.name, r(14, 800)));
      middleText(ctx, m.name, LABEL, sy, nameRoom, r(14, 800), m.other ? c.faded : c.ink);
      if (m.note && nameWidth + 30 < nameRoom) middleText(ctx, m.note, LABEL + nameWidth + 6, sy, nameRoom - nameWidth - 6, r(11, 800), c.warn);
      middleText(ctx, m.amount, WIDTH - M - 134, sy, 88, r(14, 800), c.ink, 'right');
      middleText(ctx, m.share, WIDTH - M - 44, sy, 44, r(14, 800), c.ink, 'right');
    });
  });

  // The platform: our signature on teal.
  section(L.foot + 6, c.depot);
  drawSignature(ctx, p, project, L.sign + 20, { name: r(17, 900), small: (s) => r(s, 700), size: 12, ink: c.depotInk, muted: c.depotMuted });
  return canvas;
}
