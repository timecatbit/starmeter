// The panel's data on a phone (在手机上看): the computer's app serves this page and, behind the key in the address's
// fragment (#k=…, never sent by the browser on its own), the homepage's figures. Read-only: a range picked here is the
// phone's own (the computer keeps its own), and nothing else is done from the phone. The figures are asked for again
// every 15 seconds while the page is in view.
import type { ActionReceipt, Dashboard, Snapshot } from './types';

const key = new URLSearchParams(location.hash.slice(1)).get('k') ?? '';
let range: string | null = null;
const listeners = new Set<() => void>();
const tell = () => listeners.forEach((changed) => changed());

async function get<T>(path: string): Promise<T> {
  const answer = await fetch(path, { headers: { 'X-Starmeter-Key': key }, cache: 'no-store' });
  if (!answer.ok) throw new Error(answer.status === 403 ? '链接已失效' : `HTTP ${answer.status}`);
  return answer.json() as Promise<T>;
}

export const snapshot = () => get<Snapshot>('/api/snapshot' + (range ? `?range=${encodeURIComponent(range)}` : ''));
export const dashboard = (asked: string) => get<Dashboard>(`/api/dashboard?range=${encodeURIComponent(asked)}`);

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
