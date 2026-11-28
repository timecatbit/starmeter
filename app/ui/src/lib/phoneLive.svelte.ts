// 网页版's live view: what the AI on the computer is doing right now. Someone looks at their phone because they are
// away from the computer (or it is running a long job) and want to know whether it is working, how hard, what it has
// spent and when the turn will be done. This asks the computer (/api/live) every 2 seconds while there is work, every
// 5 while it rests, never while the page is out of sight, and makes of the readings:
//
//  - the samples the pinned panel's live chart draws (LiveFeed: each tool's tokens and output, the cost), kept five
//    minutes, for the output rate and its curve;
//  - the sessions running, each with its turn's clock (the turn's start in the computer's seconds, moved onto this
//    phone's clock), its tokens and cost so far and the tools it has called;
//  - what happened, newest first: a tool's request landing, a tool called, a turn begun or done, the work starting;
//  - working or resting: a session running, or tokens added in the last 40 seconds, is working; waking is resting
//    turning to working.
//
// The figures are the panel's (Summary's tools as the live chart counts them); the sessions are 会话's rows.
import { backend, onPhone } from './backend';
import { currency, legendName, money, summaryPart, tokens as tokenText } from './format';
import { english, localize, source, t } from './i18n.svelte';
import { GAP, LiveFeed, RATE_TAU, readingDashboard } from './live';
import type { Dashboard, Entry, LiveAnswer } from './types';

/** Tokens added this recently still count as working, between requests of a turn that has no session to say so. */
const QUIET = 40_000;
const REST_EVERY = 2_500;
// Readings further apart than GAP (live.ts) left a gap: what changed in it happened at moments nobody saw, so it is
// neither this moment's speed, nor requests landing now, nor a turn just done.
/** An amount worth saying: one that money() doesn't write as 0.00 in the computer's currency. */
const worthSaying = (usd: number) => currency.convert(usd) >= 0.005;
/** How much time the readings must cover before the speed means anything: until then it is —, not 0. */
const READY = 4_000;
/** The curve's span: the last minute, as the pinned panel's (the user: 就1分钟就好; it had grown to five). */
export const LIVE_SPAN = 60_000;

export interface LiveSession {
  key: string;
  /** The tool module ID this session belongs to (claude, codex, gemini, …). */
  tool?: string;
  title: string;
  image?: string;
  template?: boolean;
  symbol?: string;
  running: boolean;
  /** The model, effort and context as 会话 writes them, without the state at its head. */
  about: string;
  /** The model and effort alone (the context is shown as its meter, `context`): the panel's 实时 and 网页版 alike. */
  model: string;
  /** How full the context is (0–1), when 会话 says. */
  context: number | null;
  /** When this turn began on this phone's clock; `exact` when the computer said to the second (else to the minute). */
  began: number | null;
  exact: boolean;
  /** This turn's tokens and cost as 会话 writes them. */
  turnTokens: string | null;
  turnCost: string | null;
  requests: number | null;
  /** The tools called this turn, the most first. */
  tools: [string, number][];
  /** Each tool's last call seen, on this phone's clock (its chip lights). */
  called: Record<string, number>;
  /** What the session cost in today's figures. */
  value: string;
  /** Idle for how long, as 会话 says it (空闲 6 分钟), for a resting page. */
  idle: string | null;
}

export interface LiveEvent {
  id: number;
  at: number;
  kind: 'usage' | 'tool' | 'start' | 'done' | 'wake' | 'rest';
  /** The tool's id for its colour (usage), or none. */
  tool: string | null;
  who: string;
  what: string;
}

export interface Landing { id: string; tokens: number; at: number }

const IMAGE_TOOL: Record<string, string> = {
  'claude.svg': 'claude',
  'openai.svg': 'codex',
  'gemini.svg': 'gemini',
  'cursor.svg': 'cursor',
  'opencode.svg': 'opencode',
  'qoder.svg': 'qoder',
  'workbuddy.svg': 'workbuddy',
  'deepseek.svg': 'deepseek_harness',
  'magpie.svg': 'magpie',
};

function sessionTool(e: Entry): string | undefined {
  if (e.tool) return e.tool;
  const file = e.image?.split('/').at(-1);
  return file ? IMAGE_TOOL[file] : undefined;
}

/** A 会话 row read into a live session, by its Chinese wording (the row as the engine wrote it). */
function readSession(
  e: Entry,
  turns: Record<string, number>,
  skew: number,
  before: LiveSession | undefined,
  now: number,
  gap = false,
  first = false,
  pendingTurn = false,
): LiveSession {
  const said = source(e), detail = said.detail ?? '';
  const running = e.statusLight === 'new';
  const shownParts = (e.detail ?? '').split(' · ');
  const about = shownParts.slice(1).join(' · ');
  const context = /上下文 (\d+)%/.exec(detail);
  const turn = running && pendingTurn ? undefined : e.children.find((c) => c.detail !== undefined);
  const turnSaid = turn ? source(turn) : null;
  const labelParts = (turn?.label ?? '').split(' · ');
  const tools = [...(turnSaid?.detail ?? '').matchAll(/([^\s·×]+) ×(\d+)/g)].map((m) => [m[1], Number(m[2])] as [string, number]);
  const requests = /(\d+) 次请求/.exec(turnSaid?.detail ?? '');
  // The turn's start: the computer's second when it says, else the minute the turn's line begins with, on the
  // computer's clock (this phone's moved by the difference), the day before when that minute is still to come there;
  // else, running, the minutes the session says it has run (运行中 12 分钟). A turn begun before the logs the computer
  // reads (its line says 更早开始) has none of these: its clock says 运行中 rather than nothing (it vanished).
  const key = e.key ?? e.label;
  let began: number | null = null, exact = false;
  if (turns[key] !== undefined) { began = turns[key] * 1000 - skew; exact = true; }
  else {
    const hm = /^(\d{1,2}):(\d{2})/.exec(turnSaid?.label ?? '');
    const minutes = running ? /^运行中 (\d+) 分钟/.exec(detail) : null;
    if (hm) {
      const there = now + skew, d = new Date(there);
      d.setHours(Number(hm[1]), Number(hm[2]), 0, 0);
      if (d.getTime() > there + 60_000) d.setDate(d.getDate() - 1);
      began = d.getTime() - skew;
    } else if (minutes) {
      began = now - Number(minutes[1]) * 60_000;
    }
  }
  const called = { ...(before?.called ?? {}) };
  for (const [name, count] of tools) {
    const was = before?.tools.find(([n]) => n === name)?.[1] ?? 0;
    if ((before || (!first && running)) && !gap && count > was) called[name] = now;
  }
  return {
    key, tool: sessionTool(e), title: e.label, image: e.image, template: e.template, symbol: e.symbol, running, about,
    model: shownParts.slice(1).filter((part) => !/%/.test(part)).slice(0, 2).join(' · '), context: context ? Number(context[1]) / 100 : null,
    began, exact, turnTokens: labelParts.length >= 3 ? labelParts[labelParts.length - 2] : null, turnCost: labelParts.length >= 3 ? labelParts[labelParts.length - 1] : null,
    requests: requests ? Number(requests[1]) : (running && pendingTurn ? 0 : null), tools: [...tools].sort((a, b) => b[1] - a[1]), called, value: e.value ?? '',
    idle: !running && detail.startsWith('空闲') ? shownParts[0] : null,
  };
}

/** Summary's tools as the live line counts them: what a gateway logged of a tool is the gateway's. */
function counted(d: Dashboard) {
  return new Map(d.providers.filter((p) => p.available).map((p) => [p.id, { part: summaryPart(p), p }] as const)
    .filter(([, v]) => v.part !== null).map(([id, v]) => [id, { name: legendName(v.p), tokens: v.part!.tokens, cost: v.part!.cost ?? 0 }]));
}

/** Today's estimate as the monitor writes it: Summary's tools' costs, none (—) when no tool it counts has one. */
export function estimate(d: Dashboard): number | null {
  const parts = [...counted(d).values()];
  return d.providers.some((p) => p.available && summaryPart(p)?.cost != null) ? parts.reduce((s, p) => s + p.cost, 0) : null;
}

/** "4 分 12 秒", "38 秒", "1 小时 23 分": how long a turn took; by the minute only, "4 分钟", "1 小时 23 分钟". */
export function duration(ms: number, exact = true): string {
  const seconds = Math.max(0, Math.round(ms / 1000)), minutes = Math.floor(seconds / 60), hours = Math.floor(minutes / 60);
  if (!exact) return hours ? t('{h} 小时 {m} 分钟', { h: hours, m: minutes % 60 }) : t('{n} 分钟', { n: Math.max(1, minutes) });
  if (hours) return t('{h} 小时 {m} 分', { h: hours, m: minutes % 60 });
  return minutes ? t('{m} 分 {s} 秒', { m: minutes, s: seconds % 60 }) : t('{s} 秒', { s: seconds });
}

/** A turn's clock as both 实时s write it, in pieces: m:ss (h:mm:ss) to the second when the computer gave its start, else
 *  its minutes (4 分钟, 1 小时 23 分钟), else 运行中 (a turn begun before the logs read: never an empty space). Each
 *  piece a figure (the figures' rounded face) or words (the text's). */
export function turnClock(began: number | null, exact: boolean, now: number): { text: string; figure: boolean }[] {
  if (began === null) return [{ text: t('运行中'), figure: false }];
  const seconds = Math.max(0, Math.floor((now - began) / 1000));
  if (!exact) {
    return duration(seconds * 1000, false).split(/(\d+)/).map((piece) => piece.trim()).filter(Boolean)
      .map((text) => ({ text, figure: /^\d+$/.test(text) }));
  }
  const pad = (n: number) => String(n).padStart(2, '0');
  const h = Math.floor(seconds / 3600), m = Math.floor((seconds % 3600) / 60), s = seconds % 60;
  return [{ text: h ? `${h}:${pad(m)}:${pad(s)}` : `${m}:${pad(s)}`, figure: true }];
}

const KEPT_STATE = 'starmeter.liveState';
/** Where a page starts before its first answer (a flash of 休息中 on a reload while the work went on): at work or resting
 *  as the computer said when it gave the page (phone.rs writes it in), else as this page last saw it within the
 *  minute (kept in its storage), else not known (a neutral 正在读取…). */
function firstState(): { state: 'working' | 'resting'; running: number; known: boolean } {
  const given = typeof window === 'undefined' ? undefined
    : (window as unknown as { STARMETER_LIVE?: { working?: unknown; running?: unknown } }).STARMETER_LIVE;
  if (typeof given?.working === 'boolean') {
    return { state: given.working ? 'working' : 'resting', running: typeof given.running === 'number' ? given.running : 0, known: true };
  }
  try {
    const kept = JSON.parse(localStorage.getItem(KEPT_STATE) ?? 'null') as { state?: string; running?: number; at?: number } | null;
    if (kept && (kept.state === 'working' || kept.state === 'resting') && typeof kept.at === 'number' && Math.abs(Date.now() - kept.at) < 60_000) {
      return { state: kept.state, running: kept.running ?? 0, known: true };
    }
  } catch { /* nothing kept */ }
  return { state: 'resting', running: 0, known: false };
}

export class PhoneLive {
  feed = new LiveFeed();
  private start0 = firstState();
  /** Whether the page knows yet if the AI is at work: from its first answer, or told before it (firstState). */
  known = $state(this.start0.known);
  /** The first answer taken: until then the sessions and what happened are still to come (not "none"). */
  answered = $state(false);
  /** Before the first answer, how many sessions the computer said were running. */
  runningBefore = this.start0.running;
  /** Bumped at each reading: what draws from the feed reads it. */
  version = $state(0);
  dashboard = $state<Dashboard | null>(null);
  sessions = $state<LiveSession[]>([]);
  /** The localized 会话 entry from the latest live tick, so 网页版's 会话 tab stays as fresh as 正在运行 for today. */
  sessionsEntry = $state<Entry | null>(null);
  events = $state<LiveEvent[]>([]);
  state = $state<'working' | 'resting'>(this.start0.state);
  /** When the page last woke (resting to working), on this phone's clock; 0 never. */
  woke = $state(0);
  /** A turn just done: said at the top for a moment. */
  done = $state<{ title: string; took: string; cost: string | null; at: number } | null>(null);
  /** When the work was last seen (tokens added, or a session's idle time), on this phone's clock. */
  lastActive = $state<number | null>(null);
  /** The last reading failed: the computer out of reach. */
  trouble = $state(false);
  /** When the computer was last read, on this phone's clock; 0 not yet. */
  readAt = $state(0);
  /** User-chosen live refresh cadence in seconds (`pinInterval`, 1–5s), synced from `/api/live` or the desktop panel. */
  interval = $state(2);
  /** Requests landed at the last reading, for the curve to drop in. */
  landings = $state<Landing[]>([]);
  /** Today's estimate and what the last reading added to it. */
  cost = $state<number | null>(null);
  costAdded = $state<{ amount: number; at: number } | null>(null);

  private skew = 0;
  private timer: ReturnType<typeof setTimeout> | undefined;
  private stopped = true;
  private lastGrowth = 0;
  private restingSince = 0;
  private before: Map<string, { name: string; tokens: number; cost: number }> | null = null;
  private beforeDay: string | null = null;
  /** The range the last figures were for: today on the app; SwiftBar's panel sends whatever the computer shows. */
  private beforeRange: string | null = null;
  /** When the last reading was taken, on this phone's clock. */
  private lastTake = 0;
  private uiTag: string | null = null;
  /** The computer's moment of the last reading taken (its seconds): the minutes given again with a later answer are
   *  new from there on. Not this phone's clock less the skew, which moves with each request's time on the way (STAGE-18:
   *  a slower answer made readings already taken look new, and they were taken again). */
  private lastAt = 0;
  private lastKept = '';
  private lastKeptAt = 0;
  private nextId = 1;
  private visible = () => typeof document === 'undefined' || document.visibilityState === 'visible';
  /** The next reading asks for the computer's last minutes too: the first, and the first after being away (out of
   *  sight, the panel closed), so what happened meanwhile comes in at its moments rather than as a gap. */
  private wantHistory = true;
  private onVisible = () => { if (this.visible() && !this.stopped) { this.wantHistory = true; clearTimeout(this.timer); void this.loop(); } };

  constructor() {
    this.feed.history = LIVE_SPAN + LiveFeed.window + 10_000;
    // Match the pinned panel's LiveFeed (spread = 0) so the web stage and pinned panel draw identical curves; SwiftBar's
    // 30s plugin cadence is handled via a matching 30s decay constant (answer.slow) instead of retroactive smearing.
    this.feed.spread = 0;
  }

  start() {
    if (!this.stopped) return;
    this.stopped = false;
    this.wantHistory = true;
    document.addEventListener('visibilitychange', this.onVisible);
    void this.loop();
  }

  stop() {
    this.stopped = true;
    clearTimeout(this.timer);
    document.removeEventListener('visibilitychange', this.onVisible);
  }

  /** One loop at a time: a loop begun (the page back in sight, 网页版 back on screen) ends any before it, so a reading
   *  two loops waited for is taken once (taken twice, the second found nothing new and the requests that landed never
   *  dropped onto the curve). */
  private round = 0;

  /** Updates the polling cadence in seconds (from the user's `pinInterval` preference, clamped to 1–5s). */
  setIntervalSec(seconds: number) {
    if (!Number.isFinite(seconds)) return;
    const clamped = Math.min(5, Math.max(1, Math.round(seconds)));
    if (clamped !== this.interval) this.interval = clamped;
  }

  private async loop() {
    if (this.stopped) return;
    const round = ++this.round;
    clearTimeout(this.timer);
    if (this.visible()) await this.read(round);
    if (this.stopped || round !== this.round) return;
    clearTimeout(this.timer);
    // Out of sight, nothing is asked until the page comes back (onVisible).
    const busyMs = this.interval * 1000;
    const restMs = Math.max(REST_EVERY, busyMs);
    if (this.visible()) this.timer = setTimeout(() => void this.loop(), this.state === 'working' ? busyMs : restMs);
  }

  private async read(round: number) {
    let answer: LiveAnswer;
    // The first reading asks for the computer's last minutes too (a page opened or reloaded while the work goes on), and
    // the first after being away.
    try { answer = await backend.live(this.wantHistory); } catch { if (round === this.round) this.trouble = true; return; }
    if (this.stopped || round !== this.round) return;
    this.trouble = false;
    this.wantHistory = false;
    this.take(answer, answer.received ?? Date.now());
  }

  /** A reading taken at `now` (this phone's clock). `replay`: one of the computer's last minutes, given with a page's
   *  first reading, taken as it was then: its moments, its 动态; no banner, no waking, nothing landing now. */
  take(answer: LiveAnswer, now: number, replay = false) {
    if (!replay && answer.ui) {
      if (this.uiTag !== null && this.uiTag !== answer.ui && typeof window !== 'undefined' && onPhone) {
        this.stop();
        window.location.reload();
        return;
      }
      this.uiTag = answer.ui;
    }
    if (!replay && answer.interval !== undefined) this.setIntervalSec(answer.interval);
    if (!replay && answer.slow !== undefined) this.feed.rateTau = answer.slow ? 30_000 : RATE_TAU;
    if (Number.isFinite(answer.at)) this.skew = answer.at * 1000 - now;
    // Quiet and resting are judged on the computer's clock (the readings' own moments), not this phone's.
    const moment = Number.isFinite(answer.at) ? answer.at * 1000 : now;
    if (!replay && answer.history?.length) this.replay(answer);
    if (Number.isFinite(answer.at)) this.lastAt = answer.at;
    const gap = this.lastTake > 0 && now - this.lastTake > GAP;
    this.lastTake = now;
    this.readAt = now;
    const shown = localize({ entries: answer.sessions ? [answer.sessions] : [], dashboard: answer.dashboard });
    const dashboard = shown.dashboard ?? null, entry = shown.entries[0] ?? null;
    const events: LiveEvent[] = [];
    const event = (kind: LiveEvent['kind'], who: string, what: string, tool: string | null = null) =>
      events.push({ id: this.nextId++, at: now, kind, tool, who, what });

    // What each tool added since the last reading of the same day: a request (or a few) landing.
    const landings: Landing[] = [];
    if (dashboard) {
      currency.unit = dashboard.currency ?? null;
      const now_ = counted(dashboard), day = dashboard.date;
      // Only figures for the same day and range compare: the computer picking 近 30 天 on SwiftBar's panel is no usage.
      const before = this.before && this.beforeDay === day && this.beforeRange === dashboard.rangeKey ? this.before : null;
      if (before && !gap) {
        for (const [id, part] of now_) {
          const was = before.get(id);
          const added = was ? part.tokens - was.tokens : 0;
          if (added > 0.5) {
            landings.push({ id, tokens: added, at: now });
            const spent = part.cost - (was?.cost ?? part.cost);
            event('usage', part.name, t('+{n} Token', { n: tokenText(added) }) + (worthSaying(spent) ? ' · ' + money(spent) : ''), id);
          }
        }
      }
      const grew = !!before && [...now_].some(([id, part]) => part.tokens > (before.get(id)?.tokens ?? 0) + 0.5);
      this.before = now_;
      this.beforeDay = day;
      this.beforeRange = dashboard.rangeKey;
      this.feed.add(dashboard, now, gap);
      const costs = [...now_.values()].map((p) => p.cost), total = costs.reduce((a, b) => a + b, 0);
      if (!replay && before && !gap && this.cost !== null && worthSaying(total - this.cost)) this.costAdded = { amount: total - this.cost, at: now };
      // Work went on in the gap, when exactly unknown: the last activity is what the sessions say of themselves.
      if (gap && grew) this.lastActive = null;
      this.cost = estimate(dashboard);
      this.dashboard = dashboard;
    }
    this.landings = replay ? [] : landings;

    // The sessions: each turn begun, its tools called, each turn done.
    const rows = (entry?.children ?? []).filter((c) => c.statusLight !== undefined);
    const known = new Map(this.sessions.map((s) => [s.key, s]));
    const first = this.events.length === 0 && this.sessions.length === 0 && !this.dashboardSeen;
    const firstSessions = !this.sessionsSeen;
    const pendingSet = new Set(answer.pending ?? []);
    const sessions = rows.map((row) => {
      const key = row.key ?? row.label;
      return readSession(row, answer.turns ?? {}, this.skew, known.get(key), now, gap, firstSessions, pendingSet.has(key));
    });
    let finished = false;
    for (const s of sessions) {
      const was = known.get(s.key);
      // Turns begun or done in a gap went at moments unknown: no event at this one, no time made up for them.
      if (gap) continue;
      if (!was) {
        if (!firstSessions && s.running) {
          event('start', s.title, t('开始新一轮'));
          if (s.tools.length) event('tool', s.title, t('调用 {tools}', { tools: s.tools.map(([name]) => name).join(english() ? ', ' : '、') }));
        }
        continue;
      }
      if (was.running && !s.running) {
        finished = true;
        const took = s.began !== null ? duration(now - s.began, s.exact) : '';
        event('done', s.title, took ? t('这一轮完成 · {d}', { d: took }) : t('这一轮完成'));
        if (!replay) this.done = { title: s.title, took, cost: s.turnCost, at: now };
      } else if (!was.running && s.running) {
        event('start', s.title, t('开始新一轮'));
      } else if (s.running) {
        const fresh = s.tools.filter(([name, count]) => count > (was.tools.find(([n]) => n === name)?.[1] ?? 0));
        if (fresh.length) event('tool', s.title, t('调用 {tools}', { tools: fresh.map(([name]) => name).join(english() ? ', ' : '、') }));
      }
    }
    this.sessions = sessions;
    if (entry !== null) this.sessionsSeen = true;
    if (!replay) this.sessionsEntry = entry;
    // When tracked sessions stop, zero out ONLY each stopped tool's live curve (never wipe the total curve, so an
    // active gateway like Magpie or another tool keeps its full rate in the total: LIVE-MIX-1). If a short turn's final
    // tokens landed in this same tick, offset the stop marker by 800ms so the spike is visible before gliding down.
    const runningToolsNow = new Set(sessions.filter((s) => s.running && s.tool).map((s) => s.tool!));
    if (!gap && this.prevRunningTools.size > 0) {
      const landedIds = new Set(landings.map((l) => l.id));
      for (const id of this.prevRunningTools) {
        if (!runningToolsNow.has(id)) this.feed.markStopped(landedIds.has(id) ? now + 800 : now, id);
      }
    }
    this.prevRunningTools = runningToolsNow;
    // Only let token growth keep the stage in `working` for QUIET (40s) when the tool has NO tracked session (e.g.
    // gateways or tools with 会话 off). Tracked sessions report their own exact start/end, and finishing a tracked
    // session must never clear an untracked gateway's active `lastGrowth` (LIVE-MIX-1).
    const trackedTools = new Set(sessions.map((s) => s.tool).filter((id): id is string => !!id));
    if (landings.length) {
      this.lastActive = now;
      if (landings.some((l) => !trackedTools.has(l.id))) this.lastGrowth = moment;
    }
    if (sessions.length > 0 && trackedTools.size === 0 && sessions.every((s) => !s.running) && finished) {
      this.lastGrowth = 0;
    }
    // Active sessions and newly finished turns advance lastActive too, so 开始休息 is never stamped before 这一轮完成 (R10-4).
    if (sessions.some((s) => s.running) || finished) this.lastActive = now;
    // Last seen working: the most recent idle session's own word for it, if nothing has been seen here yet.
    if (this.lastActive === null) {
      const minutes = sessions.map((s) => /空闲 (\d+) 分钟/.exec(source(rows.find((r) => (r.key ?? r.label) === s.key)!).detail ?? '')?.[1])
        .filter((m): m is string => !!m).map(Number);
      if (minutes.length) this.lastActive = now - Math.min(...minutes) * 60_000;
    }

    // Working or resting; resting for a while and now working is waking.
    const working = sessions.some((s) => s.running) || (this.lastGrowth > 0 && moment - this.lastGrowth < QUIET);
    if (working && this.state === 'resting' && !first && !gap && this.restingSince && moment - this.restingSince >= 6_000) {
      if (!replay) this.woke = now;
      const who = sessions.find((s) => s.running)?.title ?? (landings[0] ? this.before?.get(landings[0].id)?.name : null) ?? 'AI';
      event('wake', who, t('开始工作'));
    }
    if (!working && this.state === 'working' && !first && !gap) {
      this.restingSince = moment;
      // Resting is in 动态 too, at the last activity (it is told a while after, once nothing has come for QUIET), so the
      // feed says how long it has stood still and what came before (用户：正在运行和动态呢？).
      const who = sessions.find((s) => !s.running)?.title ?? 'AI';
      events.push({ id: this.nextId++, at: this.lastActive ?? now, kind: 'rest', tool: null, who, what: t('开始休息') });
    }
    if (!working && !this.restingSince) this.restingSince = moment;
    this.state = working ? 'working' : 'resting';
    if (working) this.restingSince = 0;
    if (!replay) {
      this.known = true;
      this.answered = true;
      // Throttle localStorage writes: only write when state/running changed or >= 15s elapsed (PERF-4).
      const runningCount = sessions.filter((s) => s.running).length;
      const sig = `${this.state}:${runningCount}`;
      const stamp = Date.now();
      if (sig !== this.lastKept || stamp - this.lastKeptAt >= 15_000) {
        this.lastKept = sig;
        this.lastKeptAt = stamp;
        try { localStorage.setItem(KEPT_STATE, JSON.stringify({ state: this.state, running: runningCount, at: stamp })); } catch { /* this load only */ }
      }
    }
    this.dashboardSeen = true;

    // Newest first: the wake before what woke it, and a turn done before its last tools.
    const order: Record<LiveEvent['kind'], number> = { rest: 0, wake: 0, done: 1, start: 2, tool: 3, usage: 4 };
    events.sort((a, b) => order[a.kind] - order[b.kind]);
    if (events.length) this.events = [...events, ...this.events].slice(0, 40);
    this.version++;
  }

  private dashboardSeen = false;
  private sessionsSeen = false;
  private prevRunningTools = new Set<string>();

  /** The computer's last minutes, given with a page's first reading: each taken as it was then (`take` with `replay`),
   *  on this phone's clock, so the line, the speed, the last minute and 动态 are as they were before the page opened or
   *  reloaded. A reading's tools are the answer's own with that moment's figures; 会话 as it last stood. */
  private replay(answer: LiveAnswer) {
    const template = answer.dashboard;
    if (!template) return;
    let sessions: Entry | null = null;
    for (const reading of answer.history ?? []) {
      if (!(reading.at < answer.at)) continue;
      if (reading.sessions) {
        const rows = reading.sessions.map((row) => ({ ...row, children: (row.children ?? []).map((turn) => ({ ...turn, children: [] })) }));
        sessions = { label: '会话', children: rows } as unknown as Entry;
      }
      // Only what came after the last reading taken here (back in sight, the panel opened again: the minutes missed).
      if (reading.at <= this.lastAt) continue;
      const at = reading.at * 1000 - this.skew;
      this.take({ at: reading.at, dashboard: readingDashboard(template, reading), sessions, turns: {} }, at, true);
    }
  }

  /** The readings since the last gap cover enough time for the speed to mean something: readings either side of a
   *  gap (the page away, the computer out of reach, the phone server off) are no run of readings (STAGE-3). */
  ready(): boolean {
    const samples = this.feed.samples;
    if (samples.length < 2) return false;
    let from = samples.length - 1;
    while (from > 0 && samples[from].at - samples[from - 1].at <= GAP) from--;
    return samples[samples.length - 1].at - samples[from].at >= READY;
  }

  /** Output tokens a second over the last minute, as the pinned panel reads it; none until the readings cover enough. */
  rate(now = Date.now()): number | null { return this.ready() ? this.feed.rate(now) : null; }
}
