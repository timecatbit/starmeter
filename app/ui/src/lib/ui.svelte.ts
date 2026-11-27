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
  /** A field on the page has the focus (the domain, a key, a name…): the shell is told, see watchTyping. */
  typing = $state(false);
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
  shown = $state(!inTauri && !(typeof location !== 'undefined' && new URLSearchParams(location.search).has('hidden')));

  constructor() {
    darkQuery?.addEventListener('change', (e) => (this.systemDark = e.matches));
    motionQuery?.addEventListener('change', (e) => (this.reduceMotion = e.matches));
  }
}

export const panel = new PanelState();

/** A key pressed while an input method is still composing (拼音 before it is picked): Return puts the letters in as typed
 *  and Esc drops them, neither meant for the page. WebKit ends the composition before the Return's keydown, which then
 *  only says so by its key code, 229. */
export function composing(event: KeyboardEvent) {
  return event.isComposing || event.keyCode === 229;
}

/** Whether an element takes typed text: a field or a text area that isn't read-only, or editable content. */
function takesText(element: Element | null) {
  if (element instanceof HTMLTextAreaElement) return !element.readOnly && !element.disabled;
  if (element instanceof HTMLInputElement) {
    return !element.readOnly && !element.disabled
      && !['button', 'checkbox', 'color', 'file', 'hidden', 'image', 'radio', 'range', 'reset', 'submit'].includes(element.type);
  }
  return element instanceof HTMLElement && element.isContentEditable;
}

/** Follows whether a field on the page is being typed into: the input method's candidate window (简体拼音's 联想框) sits
 *  below the unpinned panel's level, and showed under the panel; while a field has the focus, the shell lowers the
 *  panel to the floating level. A move from one field to the next is one stretch of typing. Returns the stop. */
export function watchTyping() {
  let timer: ReturnType<typeof setTimeout> | undefined;
  let again = false;
  let field: Element | null = null;
  // A field taken off the page with the focus in it (the domain's once its link is on, a question answered) says
  // nothing as it goes: while one is typed into, the page's changes are watched for it.
  const gone = new MutationObserver(() => { if (field && !field.isConnected) check(); });
  const check = (event?: Event) => {
    clearTimeout(timer);
    // The panel coming back: the shell set the level back as it closed, so it hears where things stand again.
    if (event?.type === 'focus' && event.target === window) again = true;
    // After the focus has settled: between leaving a field and entering the next, the page's own body has it.
    timer = setTimeout(() => {
      const typing = document.hasFocus() && takesText(document.activeElement);
      field = typing ? document.activeElement : null;
      if (typing) gone.observe(document.body, { childList: true, subtree: true });
      else gone.disconnect();
      if (typing === panel.typing && !again) return;
      again = false;
      panel.typing = typing;
      void shell.typing(typing);
    });
  };
  document.addEventListener('focusin', check);
  document.addEventListener('focusout', check);
  window.addEventListener('focus', check);
  window.addEventListener('blur', check);
  return () => {
    clearTimeout(timer);
    gone.disconnect();
    document.removeEventListener('focusin', check);
    document.removeEventListener('focusout', check);
    window.removeEventListener('focus', check);
    window.removeEventListener('blur', check);
  };
}

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

