// Shared session-list and activity-feed helpers for both PanelLive (pinned panel) and StageActivity (web stage), so
// both views show identical running/last-active headings, timestamps, and turn summaries.
import { t } from './i18n.svelte';
import type { LiveSession } from './phoneLive.svelte';

/** Newest twelve events shown in 动态, none folded away. */
export const LIVE_EVENTS_SHOWN = 12;

const pad2 = (n: number) => String(n).padStart(2, '0');

/** Local wall clock as `HH:MM:SS` for the activity feed. */
export function liveClock(at: number): string {
  const d = new Date(at);
  return `${pad2(d.getHours())}:${pad2(d.getMinutes())}:${pad2(d.getSeconds())}`;
}

/** Derives running sessions, displayed cards (running, else most recent idle), and the matching section heading. */
export function liveSessionView(sessions: LiveSession[], answered: boolean): {
  running: LiveSession[];
  shown: LiveSession[];
  heading: string;
} {
  const running = sessions.filter((s) => s.running);
  const latest = sessions.find((s) => !s.running) ?? null;
  const shown = running.length ? running : latest ? [latest] : [];
  const heading = !running.length && answered && shown.length ? '最近完成' : '正在运行';
  return { running, shown, heading };
}

/** Formats a session's turn token/cost summary (`本轮 ...` / `上一轮 ...`), or null if no tokens yet. */
export function liveTurnText(session: LiveSession): string | null {
  if (!session.turnTokens) return null;
  return t(session.running ? '本轮' : '上一轮') + ' ' + session.turnTokens + (session.turnCost ? ' · ' + session.turnCost : '');
}
