// 在手机上看, as the panel knows it: whether 同一个网络 and the public link are on, where a phone can open them, and for
// how long.
// The server lives in the shell; this asks after it when the panel or the page opens (a green dot on the homepage's
// 在手机上看 while a way of it is on), and while the page is on screen.
import { shell, type PhoneView } from './backend';

// Once kept so a 停止 lasted past the app's restart; now both ways start off every time, so it is only cleared.
try { localStorage.removeItem('starmeter.phone-stopped'); } catch { /* nothing kept */ }

/** The view as last read, and when: the minutes left count down from then. Both ways start off, 同一个网络 and the
 *  public link alike, and only their own 开始 starts them (用户：默认，同一网络，任何网络都是停止状态，用户点击开始才开始);
 *  one left on is on again when the app starts again, an update too (the shell keeps it, phone.rs).
 *  `localStopped`, `publicStopped`: that way's 停止 pressed since the app started, so its page says 已停止 rather than
 *  未开始. `reach`: the page shown last, shown again when 在手机上看 is opened again. */
export const phone = $state({ view: null as PhoneView | null, at: 0, localStopped: false, publicStopped: false, reach: 'local' as 'local' | 'public' });

export function setPhone(view: PhoneView | null) {
  phone.view = view;
  phone.at = Date.now();
}

export async function phoneStatus() {
  setPhone(await shell.phoneStatus().catch(() => null));
  return phone.view;
}

/** The public link's seconds left now; 0 without one or once its hour is up. On the same network there is no limit. */
export function secondsLeft(): number {
  return phone.view ? Math.max(0, phone.view.secondsLeft - (Date.now() - phone.at) / 1000) : 0;
}
