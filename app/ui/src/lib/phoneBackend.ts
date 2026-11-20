// The panel's data on a phone (在手机上看): the computer's app serves this page and, behind the key in the address's
// fragment (#k=…, never sent by the browser on its own), the homepage's figures. Read-only: a range picked here is the
// phone's own (the computer keeps its own), and nothing else is done from the phone. The figures are asked for again
// every 15 seconds while the page is in view.
import type { ActionReceipt, Dashboard, Snapshot } from './types';

const key = new URLSearchParams(location.hash.slice(1)).get('k') ?? '';
let range: string | null = null;
const listeners = new Set<() => void>();
const tell = () => listeners.forEach((changed) => changed());

/** Why the phone has no fresh figures: the key is no longer the server's, or the computer can't be reached. */
export class PhoneProblem extends Error {}

async function get<T>(path: string): Promise<T> {
  let answer: Response;
  try {
    answer = await fetch(path, { headers: { 'X-Starmeter-Key': key }, cache: 'no-store', signal: AbortSignal.timeout(12_000) });
  } catch {
    throw new PhoneProblem('连不上电脑：电脑可能睡着了、换了网络，或者已经停止手机查看');
  }
  if (answer.status === 403) throw new PhoneProblem('链接已失效：在电脑上重新打开“在手机上看”，再扫一次码');
  if (!answer.ok) throw new PhoneProblem(`电脑没有给出数字（HTTP ${answer.status}）`);
  return answer.json() as Promise<T>;
}

/** The reading on its way, if any: another ask for the same one waits for it instead of sending a second. */
const pending = new Map<string, Promise<unknown>>();
function once<T>(path: string): Promise<T> {
  const known = pending.get(path) as Promise<T> | undefined;
  if (known) return known;
  const asked = get<T>(path).finally(() => pending.delete(path));
  pending.set(path, asked);
  return asked;
}

export const snapshot = () => once<Snapshot>('/api/snapshot' + (range ? `?range=${encodeURIComponent(range)}` : ''));
export const dashboard = (asked: string) => once<Dashboard>(`/api/dashboard?range=${encodeURIComponent(asked)}`);

export async function action(asked: string[]): Promise<ActionReceipt> {
  if (asked[0] === 'set-range' && asked[1]) {
    range = asked[1];
    tell();
    return { status: 'done' };
  }
  // The panel's own refreshes are answered by reading again; everything else stays on the computer.
  if (asked[0] === 'refresh-view' || asked[0] === 'refresh-live') return { status: 'done' };
  return { status: 'cancelled' };
}

export const onChange = (changed: () => void) => { listeners.add(changed); return () => listeners.delete(changed); };

setInterval(() => { if (document.visibilityState === 'visible') tell(); }, 15_000);
document.addEventListener('visibilitychange', () => { if (document.visibilityState === 'visible') tell(); });
