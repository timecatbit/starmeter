// 在手机上看, as the panel knows it: whether 同一个网络 and the public link are on, where a phone can open them, and for
// how long.
// The server lives in the shell; this asks after it when the panel or the page opens (a green dot on the homepage's
// 在手机上看 while a way of it is on), and while the page is on screen.
import { shell, type PhoneView } from './backend';

const STOPPED = 'starmeter.phone-stopped';
const kept = (key: string): boolean => { try { return localStorage.getItem(key) === '1'; } catch { return false; } };
const keep = (key: string, on: boolean) => { try { if (on) localStorage.setItem(key, '1'); else localStorage.removeItem(key); } catch { /* not kept */ } };

/** The view as last read, and when: the minutes left count down from then. `stopped`: 同一个网络's 停止 pressed, and
 *  nothing has started it since: the page opened again leaves it off until 重新开始 (用户：停了要手动打开), kept as the
 *  app starts again too. `reach`: the page shown last, shown again when 在手机上看 is opened again. */
export const phone = $state({ view: null as PhoneView | null, at: 0, stopped: kept(STOPPED), reach: 'local' as 'local' | 'public' });

export function setPhone(view: PhoneView | null) {
  phone.view = view;
  phone.at = Date.now();
}

/** 同一个网络's 停止 pressed (true), or it started by hand (false). */
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
