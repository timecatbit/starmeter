// Panel-wide state the components share: light or dark, reduced motion, the header hint bubble, the layout width and
// whether the panel is pinned.
import { inTauri, shell } from './backend';

const media = (query: string) => (typeof matchMedia === 'function' ? matchMedia(query) : null);
const darkQuery = media('(prefers-color-scheme: dark)');
const motionQuery = media('(prefers-reduced-motion: reduce)');

class PanelState {
  /** The theme preference: system, light or dark. */
  theme = $state('system');
  systemDark = $state(darkQuery?.matches ?? false);
  reduceMotion = $state(motionQuery?.matches ?? false);
  dark = $derived(this.theme === 'dark' || (this.theme !== 'light' && this.systemDark));
  /** A native menu (the range picker, the share menu) is open: the panel waits to redraw until it closes. */
  menuOpen = $state(false);
  /** The hint under a header button: its text and the button's box in the panel. */
  hint = $state<{ text: string; x: number; y: number; width: number; height: number } | null>(null);
  /** The width the homepage and the pages lay out at: 360, or what the user made a pinned panel at its edges. */
  width = $state(360);
  /** Pinned: a page's top strip moves the window. */
  pinned = $state(false);
  /** The user has moved or resized a pinned panel: the hint saying how is no longer needed. */
  arranged = $state(false);
  /** The panel on screen (the app's window opened, not yet gone again): what reads live figures for it stops while it
   *  is away. Outside the app (the browser preview, a phone) always. */
  shown = $state(!inTauri);

  constructor() {
    darkQuery?.addEventListener('change', (e) => (this.systemDark = e.matches));
    motionQuery?.addEventListener('change', (e) => (this.reduceMotion = e.matches));
  }
}

export const panel = new PanelState();

let hintTimer: ReturnType<typeof setTimeout> | undefined;
/** A short label under a header button, 350 ms after the pointer settles on it; a click hides it at once. */
export function hoverHint(element: HTMLElement | null, text: string) {
  clearTimeout(hintTimer);
  panel.hint = null;
  if (!element || !text || panel.menuOpen) return;
  hintTimer = setTimeout(() => {
    if (panel.menuOpen) return;
    // In the panel's own coordinates: the browser preview puts the panel at a margin.
    const box = element.getBoundingClientRect(), frame = element.closest('.window')?.getBoundingClientRect() ?? { left: 0, top: 0 };
    panel.hint = { text, x: box.left - frame.left, y: box.top - frame.top, width: box.width, height: box.height };
  }, 350);
}

/** Pinned: moves the window with the pointer. The window server runs the drag, so the page never sees the button come
 *  up; the press ends here instead, or the panel would hold back its redraws (and a control its pressed look) until the
 *  next click. */
export async function moveWindow() {
  panel.arranged = true;
  panel.hint = null;
  try {
    await shell.drag();
  } finally {
    window.dispatchEvent(new Event('pointercancel'));
  }
}

/** A left press on a pinned panel's top strip moves the window. It listens for mousedown, as Tauri's own drag regions
 *  do: WebKit sends one for every press, but no pointerdown while it still counts an earlier press as held (one whose
 *  release a window drag kept). */
export function grabWindow(event: MouseEvent) {
  if (event.button !== 0 || !panel.pinned) return;
  event.preventDefault();
  void moveWindow();
}

