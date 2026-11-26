// 在手机上看, as the panel knows it: whether the phone server is on, where a phone can open it, and for how long.
// The server lives in the shell; this asks after it when the module grid or the page opens, and while the page is on
// screen.
import { shell, type PhoneView } from './backend';
import type { Entry } from './types';

const STOPPED = 'starmeter.phone-stopped';
const kept = (key: string): boolean => { try { return localStorage.getItem(key) === '1'; } catch { return false; } };
const keep = (key: string, on: boolean) => { try { if (on) localStorage.setItem(key, '1'); else localStorage.removeItem(key); } catch { /* not kept */ } };

/** The view as last read, and when: the minutes left count down from then. `stopped`: 停止 pressed, and nothing has
 *  started it since: the page opened again leaves it off until 重新开始 (用户：停了要手动打开), kept as the app starts
 *  again too. */
export const phone = $state({ view: null as PhoneView | null, at: 0, stopped: kept(STOPPED) });

export function setPhone(view: PhoneView | null) {
  phone.view = view;
  phone.at = Date.now();
}

/** 停止 pressed (true), or the server started by hand (false). */
export function keepStopped(stopped: boolean) {
  phone.stopped = stopped;
  keep(STOPPED, stopped);
}

export async function phoneStatus() {
  setPhone(await shell.phoneStatus().catch(() => null));
  return phone.view;
}

/** The public link's seconds left now; 0 without one or once its hour is up. On the same network there is no limit. */
export function secondsLeft(): number {
  return phone.view ? Math.max(0, phone.view.secondsLeft - (Date.now() - phone.at) / 1000) : 0;
}

/** 在手机上看 in the module grid, a cell of its own beside the settings: the panel's own, like 语言. While it is on, a
 *  green dot after its name, as a module that is set up has. */
export function phoneRow(): Entry {
  return { label: '在手机上看', symbol: 'iphone', ...(phone.view ? { statusLight: 'ok' as const } : {}), action: ['show-phone'], children: [] };
}
