// 在手机上看's two looks on the phone: 手机版 (PhoneStage.svelte, made for a phone's screen, portrait or landscape) and
// 电脑版 (the computer's homepage as it is). The phone picks, at the foot of either page, where the choice is seen; the
// computer's QR page stays about connecting, and nothing goes back to the computer (the phone only reads).
//
// The choice is kept in the address's fragment (…&v=panel, so a reload keeps it), in the page's storage, and in a
// cookie: a cookie belongs to the computer's address whatever the port, and the phone server takes a new port each
// time it starts, so a phone that chose 电脑版 still has it after 停止 and 重新开始 on the same network.
import { onPhone } from './backend';

export type PhoneLook = 'stage' | 'panel';
const NAME = 'starmeter-phone-look';
const known = (value: string | null | undefined): value is PhoneLook => value === 'stage' || value === 'panel';

function stored(): PhoneLook {
  try {
    const asked = new URLSearchParams(location.hash.slice(1)).get('v');
    if (known(asked)) return asked;
    const cookie = document.cookie.split('; ').find((pair) => pair.startsWith(NAME + '='))?.slice(NAME.length + 1);
    if (known(cookie)) return cookie;
    const kept = localStorage.getItem(NAME);
    if (known(kept)) return kept;
  } catch { /* nothing kept: the phone's own look */ }
  return 'stage';
}

/** On the computer there is only the panel. */
export const phoneLook = $state({ current: (onPhone ? stored() : 'panel') as PhoneLook });

export function choosePhoneLook(next: PhoneLook) {
  phoneLook.current = next;
  try {
    const fragment = new URLSearchParams(location.hash.slice(1));
    fragment.set('v', next);
    history.replaceState(history.state, '', '#' + fragment.toString());
  } catch { /* the address keeps the look it opened with */ }
  try { document.cookie = `${NAME}=${next}; max-age=31536000; path=/; samesite=strict`; } catch { /* no cookies */ }
  try { localStorage.setItem(NAME, next); } catch { /* no storage */ }
}
