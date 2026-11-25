// On a phone, the browser's own bars (the status bar above, Safari's toolbar below) take the page's colours, so the
// page runs on under them instead of stopping at a white band. theme-color is the top's (the status bar in Safari
// before iOS 26, and in Chrome); the page's background behind everything is the top's colour above and the bottom's
// below (Safari 26 tints its bars from the page, and from what is fixed at its edges, which the pages paint too).
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
  root.backgroundImage = `linear-gradient(${top} 0 50%, ${bottom} 50% 100%)`;
  keep({ top, bottom });
}

/** The computer's theme, kept for the next load's first frame, before the computer has answered. */
export function keepTheme(theme: string) { if (kept().theme !== theme) keep({ theme }); }
export function keptTheme(): 'system' | 'light' | 'dark' {
  const theme = kept().theme;
  return theme === 'light' || theme === 'dark' ? theme : 'system';
}
