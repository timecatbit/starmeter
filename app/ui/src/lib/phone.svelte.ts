// 在手机上看, as the panel knows it: whether the phone server is on, where a phone can open it, and for how long.
// The server lives in the shell; this asks after it when the page opens and while it is on screen.
import { shell, type PhoneView } from './backend';
import type { Entry } from './types';

export const phone = $state({ view: null as PhoneView | null });

export async function phoneStatus() {
  phone.view = await shell.phoneStatus().catch(() => null);
  return phone.view;
}

/** The 在手机上看 row under 显示 in 外观与设置, the panel's own like 语言. */
export function phoneRow(): Entry {
  const minutes = phone.view ? Math.max(1, Math.round(phone.view.secondsLeft / 60)) : 0;
  return { label: '在手机上看', symbol: 'iphone', value: phone.view ? `开着 · 还剩 ${minutes} 分钟` : '未开启',
    detail: '扫二维码，在手机浏览器里看主页；只读', action: ['show-phone'], children: [] };
}
