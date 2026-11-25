// On a phone, the browser's own bars (the status bar above, Safari's toolbar below) take the page's colours, so the
// page runs on under them instead of stopping at a white band. theme-color is the top's (the status bar in Safari
// before iOS 26, and in Chrome); the page's background behind everything is the top's colour above and the bottom's
// below. Safari 26 ignores theme-color and reads a fixed element along each edge (index.html's two samplers, which paint
// nothing): they take the colours here, and are read again when they change (their height 12 for two frames), when
// the phone turns, and when the page comes back.
//
// Safari takes the bars' colour from the first frame it draws: the colours and the computer's theme are kept in the
// page's storage, and on a reload public/first-frame.js paints them before anything else (else the first frame is the
// phone's own theme, its bars stay dark over a light page).

const KEY = 'starmeter.edges';
type Kept = { top?: string; bottom?: string; theme?: string };

function kept(): Kept {
  try { return (JSON.parse(localStorage.getItem(KEY) ?? '{}') as Kept) ?? {}; } catch { return {}; }
}
function keep(change: Kept) {
  try { localStorage.setItem(KEY, JSON.stringify({ ...kept(), ...change })); } catch { /* this load only */ }
}

let painted = '';

export function paintEdges(top: string, bottom: string) {
  if (typeof document === 'undefined' || !top || !bottom) return;
  const key = `${top}|${bottom}`;
  if (key === painted) return;
  painted = key;
  let meta = document.querySelector<HTMLMetaElement>('meta[name="theme-color"]');
  if (!meta) { meta = document.createElement('meta'); meta.name = 'theme-color'; document.head.append(meta); }
  meta.content = top;
  const root = document.documentElement.style;
  root.backgroundColor = bottom;
  // The top's colour all the way up beyond the page, the bottom's all the way down (Safari 26 shows the page's ground
  // in its bars' bands: repeated, the gradient put the bottom's colour under the status bar).
  root.backgroundImage = `linear-gradient(${top} 0 50%, ${bottom} 50% 100%)`;
  root.backgroundSize = '100% 300%';
  root.backgroundPosition = 'center';
  root.backgroundRepeat = 'no-repeat';
  root.setProperty('--bar-top', top);
  root.setProperty('--bar-bottom', bottom);
  document.documentElement.classList.add('phone-page');
  keep({ top, bottom });
  rereadBars();
  watchBars();
}

/** Safari keeps the colour it read: the samplers change height for two frames, and it reads them again. */
function rereadBars() {
  const root = document.documentElement;
  root.classList.add('bar-refresh');
  requestAnimationFrame(() => requestAnimationFrame(() => root.classList.remove('bar-refresh')));
}
let watching = false;
/** Read again when the phone turns and when the page comes back (from another tab, from the app switcher). */
function watchBars() {
  if (watching) return;
  watching = true;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const soon = () => { clearTimeout(timer); timer = setTimeout(rereadBars, 120); };
  addEventListener('resize', soon);
  addEventListener('orientationchange', soon);
  addEventListener('pageshow', rereadBars);
  document.addEventListener('visibilitychange', () => { if (document.visibilityState === 'visible') rereadBars(); });
}

/** The computer's look as the phone server wrote it into this page (phone.rs), when it did: it is the computer's now,
 *  so it goes before anything kept from a visit before and before the address (STAGE-15). */
export function pageLook(): { theme?: string; style?: string } | null {
  return typeof window === 'undefined' ? null : ((window as unknown as { STARMETER_LOOK?: { theme?: string; style?: string } }).STARMETER_LOOK ?? null);
}

/** The computer's theme, kept for the next load's first frame, before the computer has answered. */
export function keepTheme(theme: string) { if (kept().theme !== theme) keep({ theme }); }
export function keptTheme(): 'system' | 'light' | 'dark' {
  const theme = pageLook()?.theme ?? kept().theme;
  return theme === 'light' || theme === 'dark' ? theme : 'system';
}
