// 在手机上看, as the panel knows it: whether the phone server is on, where a phone can open it, and for how long.
// The server lives in the shell; this asks after it when the module grid or the page opens, and while the page is on
// screen.
import { shell, type PhoneView } from './backend';
import type { Entry } from './types';

/** The view as last read, and when: the minutes left count down from then. */
export const phone = $state({ view: null as PhoneView | null, at: 0 });

export function setPhone(view: PhoneView | null) {
  phone.view = view;
  phone.at = Date.now();
}

export async function phoneStatus() {
  setPhone(await shell.phoneStatus().catch(() => null));
  return phone.view;
}

/** Seconds left now; 0 once the hour is up. */
export function secondsLeft(): number {
  return phone.view ? Math.max(0, phone.view.secondsLeft - (Date.now() - phone.at) / 1000) : 0;
}

/** 在手机上看 in the module grid, a cell of its own beside the settings: the panel's own, like 语言. */
export function phoneRow(): Entry {
  const left = secondsLeft();
  return { label: '在手机上看', symbol: 'iphone', detail: left > 0 ? `开着 · 还剩 ${Math.max(1, Math.round(left / 60))} 分钟` : '扫码在手机浏览器里看',
    action: ['show-phone'], children: [] };
}
