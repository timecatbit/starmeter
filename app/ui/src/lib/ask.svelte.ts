// The question the core is asking (or the notice it gave) while an action runs, and the action it belongs to: the
// row running that action shows it, so a monthly fee is typed where 订阅月费 is, a Key deleted where it is listed.
import { shell } from './backend';
import { toggle } from './detail';
import type { Entry, Question } from './types';

class AskState {
  current = $state<{ question: Question; action: string[] | null } | null>(null);
  /** How many rows are showing it now; with none (a question from the homepage or the grid) the panel shows a sheet. */
  placed = $state(0);
}
export const ask = new AskState();

let noticeTimer: ReturnType<typeof setTimeout> | undefined;

export function show(question: Question, action: string[] | null) {
  clearTimeout(noticeTimer);
  ask.current = { question, action };
  // The SwiftBar plugin waits even on a notice: it is let go at once, and the notice stays on screen.
  if (question.kind === 'notice' && typeof question.id === 'string') void shell.answer(question.id, null);
  // A notice stays a moment after its action has finished, then goes.
  if (question.kind === 'notice') noticeTimer = setTimeout(() => { if (ask.current?.question === question) ask.current = null; }, 8000);
}

export function answer(value: unknown) {
  const now = ask.current;
  if (!now) return;
  ask.current = null;
  if (now.question.kind !== 'notice') void shell.answer(now.question.id, value);
}

/** The row asking: its action is the one running (a switch matches whatever side it is turning to). */
export function asks(entry: Entry): boolean {
  const action = ask.current?.action, mine = entry.action;
  if (!action || !mine) return false;
  const same = (a: string[], b: string[]) => a.length === b.length && a.every((x, i) => x === b[i]);
  return same(action, mine) || (toggle(entry) && same(action.slice(0, -1), mine.slice(0, -1)));
}
