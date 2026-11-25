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
import { backend } from './backend';
import { currency, legendName, money, summaryPart, tokens as tokenText } from './format';
import { english, localize, source, t } from './i18n.svelte';
import { LiveFeed } from './live';
import type { Dashboard, Entry, LiveAnswer } from './types';

/** Tokens added this recently still count as working, between requests of a turn that has no session to say so. */
const QUIET = 40_000;
const BUSY_EVERY = 2_000, REST_EVERY = 5_000;
/** Readings further apart than this left a gap (the page out of sight, the computer out of reach): what changed in it
 *  happened at moments nobody saw, so it is neither this moment's speed, nor requests landing now, nor a turn just done. */
const GAP = 15_000;
/** An amount worth saying: one that money() doesn't write as 0.00 in the computer's currency. */
const worthSaying = (usd: number) => currency.convert(usd) >= 0.005;
/** How much time the readings must cover before the speed means anything: until then it is —, not 0. */
const READY = 20_000;
/** The curve's span. */
export const LIVE_SPAN = 5 * 60_000;

export interface LiveSession {
  key: string;
  title: string;
  image?: string;
  template?: boolean;
  symbol?: string;
  running: boolean;
  /** The model, effort and context as 会话 writes them, without the state at its head. */
  about: string;
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
  kind: 'usage' | 'tool' | 'start' | 'done' | 'wake';
  /** The tool's id for its colour (usage), or none. */
  tool: string | null;
  who: string;
  what: string;
}

export interface Landing { id: string; tokens: number; at: number }

/** A 会话 row read into a live session, by its Chinese wording (the row as the engine wrote it). */
function readSession(e: Entry, turns: Record<string, number>, skew: number, before: LiveSession | undefined, now: number, gap = false): LiveSession {
  const said = source(e), detail = said.detail ?? '';
  const running = e.statusLight === 'new';
  const shownParts = (e.detail ?? '').split(' · ');
  const about = shownParts.slice(1).join(' · ');
  const context = /上下文 (\d+)%/.exec(detail);
  const turn = e.children.find((c) => c.detail !== undefined);
  const turnSaid = turn ? source(turn) : null;
  const labelParts = (turn?.label ?? '').split(' · ');
  const tools = [...(turnSaid?.detail ?? '').matchAll(/([^\s·×]+) ×(\d+)/g)].map((m) => [m[1], Number(m[2])] as [string, number]);
  const requests = /(\d+) 次请求/.exec(turnSaid?.detail ?? '');
  // The turn's start: the computer's second when it says, else the minute the turn's line begins with, on the
  // computer's clock (this phone's moved by the difference), the day before when that minute is still to come there.
  const key = e.key ?? e.label;
  let began: number | null = null, exact = false;
  if (turns[key] !== undefined) { began = turns[key] * 1000 - skew; exact = true; }
  else {
    const hm = /^(\d{1,2}):(\d{2})/.exec(turnSaid?.label ?? '');
    if (hm) {
      const there = now + skew, d = new Date(there);
      d.setHours(Number(hm[1]), Number(hm[2]), 0, 0);
      if (d.getTime() > there + 60_000) d.setDate(d.getDate() - 1);
      began = d.getTime() - skew;
    }
  }
  const called = { ...(before?.called ?? {}) };
  for (const [name, count] of tools) {
    const was = before?.tools.find(([n]) => n === name)?.[1] ?? 0;
    if (before && !gap && count > was) called[name] = now;
  }
  return {
    key, title: e.label, image: e.image, template: e.template, symbol: e.symbol, running, about, context: context ? Number(context[1]) / 100 : null,
    began, exact, turnTokens: labelParts.length >= 3 ? labelParts[labelParts.length - 2] : null, turnCost: labelParts.length >= 3 ? labelParts[labelParts.length - 1] : null,
    requests: requests ? Number(requests[1]) : null, tools: [...tools].sort((a, b) => b[1] - a[1]), called, value: e.value ?? '',
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

/** "4 分 12 秒", "38 秒": how long a turn took. */
export function duration(ms: number, exact = true): string {
  const seconds = Math.max(0, Math.round(ms / 1000)), minutes = Math.floor(seconds / 60);
  if (!exact) return t('{n} 分钟', { n: Math.max(1, minutes) });
  return minutes ? t('{m} 分 {s} 秒', { m: minutes, s: seconds % 60 }) : t('{s} 秒', { s: seconds });
}

export class PhoneLive {
  feed = new LiveFeed();
  /** Bumped at each reading: what draws from the feed reads it. */
  version = $state(0);
  dashboard = $state<Dashboard | null>(null);
  sessions = $state<LiveSession[]>([]);
  events = $state<LiveEvent[]>([]);
  state = $state<'working' | 'resting'>('resting');
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
  private nextId = 1;
  private visible = () => typeof document === 'undefined' || document.visibilityState === 'visible';
  private onVisible = () => { if (this.visible() && !this.stopped) { clearTimeout(this.timer); void this.loop(); } };

  constructor() { this.feed.history = LIVE_SPAN + LiveFeed.window + 10_000; }

  start() {
    if (!this.stopped) return;
    this.stopped = false;
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

  private async loop() {
    if (this.stopped) return;
    const round = ++this.round;
    clearTimeout(this.timer);
    if (this.visible()) await this.read(round);
    if (this.stopped || round !== this.round) return;
    clearTimeout(this.timer);
    // Out of sight, nothing is asked until the page comes back (onVisible).
    if (this.visible()) this.timer = setTimeout(() => void this.loop(), this.state === 'working' ? BUSY_EVERY : REST_EVERY);
  }

  private async read(round: number) {
    let answer: LiveAnswer;
    // The first reading asks for the computer's last minutes too (a page opened or reloaded while the work goes on).
    try { answer = await backend.live(this.feed.samples.length === 0); } catch { if (round === this.round) this.trouble = true; return; }
    if (this.stopped || round !== this.round) return;
    this.trouble = false;
    this.take(answer, Date.now());
  }

  /** A reading taken at `now` (this phone's clock). */
  take(answer: LiveAnswer, now: number) {
    if (Number.isFinite(answer.at)) this.skew = answer.at * 1000 - now;
    // Quiet and resting are judged on the computer's clock (the readings' own moments), not this phone's.
    const moment = Number.isFinite(answer.at) ? answer.at * 1000 : now;
    if (answer.history?.length && !this.feed.samples.length) this.seed(answer.history, moment);
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
      if (before && !gap && this.cost !== null && worthSaying(total - this.cost)) this.costAdded = { amount: total - this.cost, at: now };
      // Work went on in the gap, when exactly unknown: the last activity is what the sessions say of themselves.
      if (gap && grew) this.lastActive = null;
      this.cost = estimate(dashboard);
      this.dashboard = dashboard;
    }
    if (landings.length) { this.lastGrowth = moment; this.lastActive = now; }
    this.landings = landings;

    // The sessions: each turn begun, its tools called, each turn done.
    const rows = (entry?.children ?? []).filter((c) => c.statusLight !== undefined);
    const known = new Map(this.sessions.map((s) => [s.key, s]));
    const first = this.events.length === 0 && this.sessions.length === 0 && !this.dashboardSeen;
    const sessions = rows.map((row) => readSession(row, answer.turns ?? {}, this.skew, known.get(row.key ?? row.label), now, gap));
    for (const s of sessions) {
      const was = known.get(s.key);
      // Turns begun or done in a gap went at moments unknown: no event at this one, no time made up for them.
      if (!was || gap) continue;
      if (was.running && !s.running) {
        const took = s.began !== null ? duration(now - s.began, s.exact) : '';
        event('done', s.title, took ? t('这一轮完成 · {d}', { d: took }) : t('这一轮完成'));
        this.done = { title: s.title, took, cost: s.turnCost, at: now };
      } else if (!was.running && s.running) {
        event('start', s.title, t('开始新一轮'));
      } else if (s.running) {
        const fresh = s.tools.filter(([name, count]) => count > (was.tools.find(([n]) => n === name)?.[1] ?? 0));
        if (fresh.length) event('tool', s.title, t('调用 {tools}', { tools: fresh.map(([name]) => name).join(english() ? ', ' : '、') }));
      }
    }
    this.sessions = sessions;
    // Last seen working: the most recent idle session's own word for it, if nothing has been seen here yet.
    if (this.lastActive === null) {
      const minutes = sessions.map((s) => /空闲 (\d+) 分钟/.exec(source(rows.find((r) => (r.key ?? r.label) === s.key)!).detail ?? '')?.[1])
        .filter((m): m is string => !!m).map(Number);
      if (minutes.length) this.lastActive = now - Math.min(...minutes) * 60_000;
    }

    // Working or resting; resting for a while and now working is waking.
    const working = sessions.some((s) => s.running) || (this.lastGrowth > 0 && moment - this.lastGrowth < QUIET);
    if (working && this.state === 'resting' && !first && !gap && this.restingSince && moment - this.restingSince >= 6_000) {
      this.woke = now;
      const who = sessions.find((s) => s.running)?.title ?? (landings[0] ? this.before?.get(landings[0].id)?.name : null) ?? 'AI';
      event('wake', who, t('开始工作'));
    }
    if (!working && this.state === 'working') this.restingSince = moment;
    if (!working && !this.restingSince) this.restingSince = moment;
    this.state = working ? 'working' : 'resting';
    if (working) this.restingSince = 0;
    this.dashboardSeen = true;

    // Newest first: the wake before what woke it, and a turn done before its last tools.
    const order: Record<LiveEvent['kind'], number> = { wake: 0, done: 1, start: 2, tool: 3, usage: 4 };
    events.sort((a, b) => order[a.kind] - order[b.kind]);
    if (events.length) this.events = [...events, ...this.events].slice(0, 40);
    this.version++;
  }

  private dashboardSeen = false;

  /** The computer's last minutes, given with the first reading: drawn on the line as they were, on this phone's clock.
   *  A tool that grew in the last of them is still working (QUIET), before any reading here has seen it grow. */
  private seed(history: NonNullable<LiveAnswer['history']>, moment: number) {
    let before: number | null = null;
    for (const reading of history) {
      const at = reading.at * 1000;
      const shown = { ...reading, dates: [], providers: reading.providers.map((p) => ({ history: [], ...p })) } as unknown as Dashboard;
      this.feed.add(shown, at - this.skew);
      const total = shown.providers.filter((p) => p.available).reduce((s, p) => s + (summaryPart(p)?.tokens ?? 0), 0);
      if (before !== null && total > before + 0.5 && moment - at < QUIET) this.lastGrowth = at;
      before = total;
    }
    if (this.lastGrowth) this.lastActive = this.lastGrowth - this.skew;
  }

  /** The readings cover enough time for the speed to mean something. */
  ready(): boolean {
    const samples = this.feed.samples;
    return samples.length > 1 && samples[samples.length - 1].at - samples[0].at >= READY;
  }

  /** Output tokens a second over the last minute, as the pinned panel reads it; none until the readings cover enough. */
  rate(now = Date.now()): number | null { return this.ready() ? this.feed.rate(now) : null; }
}
