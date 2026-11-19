// The one door between the panel and everything outside it. Two halves:
//
//  - Data (the core): `snapshot`, `dashboard` and `action`. Inside the app these are the Tauri commands of the same
//    names in app/src-tauri/src/data.rs, which documents the contract: starmeter-core in the standalone app, the
//    plugin's snapshot and action files in SwiftBar's panel, the fixtures with STARMETER_FIXTURE. The JSON is the
//    engine's PanelSnapshot and Dashboard (types.ts). `onSnapshot` fires when the data changed
//    in the background (a live tick, a file the core watches), and the panel then asks for a fresh snapshot.
//  - Shell (the window and the system): closing, resizing and pinning the panel, its look, links, the share picture.
//
// Opened in a plain browser (the Vite dev server, the Playwright tests) there is no Tauri: the data comes from the
// same fixtures through fixtureBackend.ts, and the shell calls do nothing.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { ActionReceipt, Dashboard, Question, ShellInfo, Snapshot } from './types';

export const inTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
/** On a phone (在手机上看): the computer's app served this page, with its key in the address's fragment. */
export const onPhone = !inTauri && typeof location !== 'undefined' && /(^#|&)k=[0-9a-f]{32}/.test(location.hash);

type Fixtures = typeof import('./fixtureBackend');
let fixtures: Promise<Fixtures> | null = null;
// A phone reads the computer's figures; only the dev server (and the tests on it) carries the fixtures; a release
// build opened anywhere else has nothing to show.
const browser = () => (fixtures ??= onPhone ? import('./phoneBackend')
  : import.meta.env.DEV ? import('./fixtureBackend') : Promise.reject(new Error('no backend outside the app')));

export const backend = {
  snapshot: (): Promise<Snapshot> => (inTauri ? invoke('snapshot') : browser().then((f) => f.snapshot())),
  dashboard: (range: string): Promise<Dashboard> => (inTauri ? invoke('dashboard', { range }) : browser().then((f) => f.dashboard(range))),
  action: (action: string[]): Promise<ActionReceipt> => (inTauri ? invoke('action', { action }) : browser().then((f) => f.action(action))),
  onSnapshot(changed: () => void): () => void {
    if (inTauri) {
      const stop = listen('snapshot-changed', changed);
      return () => void stop.then((f) => f());
    }
    let off = () => {};
    void browser().then((f) => (off = f.onChange(changed)));
    return () => off();
  },
};

const quiet = () => Promise.resolve();

/** Where a phone can open the homepage, and for how much longer. */
export interface PhoneView { local: string | null; public: string | null; secondsLeft: number }
/** The browser's stand-in for the phone server, for the tests and the dev server. */
const browserPhone = (() => {
  let on = false, open = false;
  const view = (): PhoneView | null => (on ? { local: 'http://192.168.1.20:51234/#k=0123456789abcdef0123456789abcdef',
    public: open ? 'https://quiet-river-1234.trycloudflare.com/#k=0123456789abcdef0123456789abcdef' : null, secondsLeft: 3540 } : null);
  return {
    view, start: () => { on = true; return view()!; }, stop: () => { on = false; open = false; },
    open: () => { open = true; return view()!; }, close: () => { open = false; return view(); },
  };
})();
const base64 = (bytes: Uint8Array) => {
  let text = '';
  for (let i = 0; i < bytes.length; i += 0x8000) text += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(text);
};

export const shell = {
  info: (): Promise<ShellInfo> => (inTauri ? invoke('shell_info')
    : Promise.resolve({ platform: 'browser', mode: 'browser', version: '6.0.0', maxHeight: Math.max(400, window.innerHeight - 16),
      modules: new URLSearchParams(location.search).has('modules') })),
  /** Esc or the star hand the focus back to the app used before; a click elsewhere leaves it where it went. */
  close: (returnFocus = true) => (inTauri ? invoke<void>('close_panel', { returnFocus }) : quiet()),
  /** The panel's size; the top stays under the star (or, moved by the user, where they put it). Animated, it eases
   *  there over 0.3 s as the page slides. Resizable: a pinned panel's edges resize it (not on the module grid). */
  resize: (width: number, height: number, animate: boolean, resizable = false) =>
    (inTauri ? invoke<void>('resize_panel', { width, height, animate, resizable }) : quiet()),
  setPinned: (pinned: boolean) => (inTauri ? invoke<void>('set_pinned', { pinned }) : quiet()),
  /** Pinned: moves the window with the pointer, from a page's top strip. */
  drag: () => (inTauri ? invoke<void>('drag_panel') : quiet()),
  /** Pinned: the corner handle's size, the top left kept; the size applied comes back through onResized. */
  sizeTo: (width: number, height: number) => (inTauri ? invoke<void>('size_panel', { width, height })
    : quiet().then(() => void window.dispatchEvent(new CustomEvent('panel-resized', { detail: { width, height } })))),
  /** The least height a pinned panel keeps at its edges, for the page on screen. */
  setMinHeight: (height: number) => (inTauri ? invoke<void>('set_min_height', { height }) : quiet()),
  /** Registers the global shortcut by its name ('none' clears it); false when the system or another app holds it.
   *  In a browser, ?shortcut-taken makes every choice taken, for tests. */
  setShortcut: (choice: string) => (inTauri ? invoke<boolean>('set_shortcut', { choice })
    : Promise.resolve(choice === 'none' || !new URLSearchParams(location.search).has('shortcut-taken'))),
  /** 在手机上看: the read-only server for a phone, started (or its hour started again), asked after, and stopped; the
   *  public link through Cloudflare opened and closed. In a browser, made-up addresses for the tests. */
  phoneStart: (): Promise<PhoneView> => (inTauri ? invoke<PhoneView>('phone_start') : Promise.resolve(browserPhone.start())),
  phoneStatus: (): Promise<PhoneView | null> => (inTauri ? invoke<PhoneView | null>('phone_status') : Promise.resolve(browserPhone.view())),
  phoneStop: () => (inTauri ? invoke<void>('phone_stop') : quiet().then(() => browserPhone.stop())),
  phonePublic: (): Promise<PhoneView> => (inTauri ? invoke<PhoneView>('phone_public') : Promise.resolve(browserPhone.open())),
  phonePrivate: (): Promise<PhoneView | null> => (inTauri ? invoke<PhoneView | null>('phone_private') : Promise.resolve(browserPhone.close())),
  /** Whether the public link needs cloudflared downloaded first. */
  tunnelReady: (): Promise<boolean> => (inTauri ? invoke<boolean>('tunnel_ready') : Promise.resolve(!new URLSearchParams(location.search).has('no-cloudflared'))),
  /** The page has drawn what the panel opened on; a window kept clear until then shows. */
  shown: () => (inTauri ? invoke<void>('panel_shown') : quiet()),
  /** Theme (system, light, dark) and background (solid, frosted or glass) for the window and the material under it.
   *  Returns whether a material shows; when it doesn't, the page paints its solid background. */
  setLook: (theme: string, material: string): Promise<boolean> =>
    (inTauri ? invoke<boolean>('set_look', { theme, material }) : Promise.resolve(material !== 'solid')),
  /** A web page comes to the front; an app opens to update itself; a swiftbar:// command runs without taking focus. */
  openLink: (href: string) => (inTauri ? invoke<void>('open_link', { href }) : Promise.resolve(void window.open(href, '_blank'))),
  /** Holds the panel open while a native menu or the share sheet has the pointer. */
  holdOpen: (held: boolean) => (inTauri ? invoke<void>('hold_open', { held }) : quiet()),
  /** One of the header's symbols as macOS draws it (base64 PNG at 3 pixels a point); null off a Mac or outside the app. */
  systemSymbol: (name: string, points: number): Promise<string | null> =>
    (inTauri ? invoke<string | null>('system_symbol', { name, points }).catch(() => null) : Promise.resolve(null)),
  copyPicture: (png: Uint8Array): Promise<boolean> => (inTauri ? invoke('copy_picture', { png: base64(png) }) : Promise.resolve(true)),
  /** Saves the picture where the user picks in the system's save dialog (the desktop at first); true once written. */
  savePicture: (png: Uint8Array, name: string): Promise<boolean> => (inTauri ? invoke<boolean>('save_picture', { png: base64(png), name }) : Promise.resolve(true)),
  /** The system share menu at a button (macOS); elsewhere a save dialog. */
  sharePicture: (png: Uint8Array, name: string, rect: { x: number; y: number; width: number; height: number }) =>
    (inTauri ? invoke<void>('share_picture', { png: base64(png), name, ...rect }) : quiet()),
  onOpened(opened: (modules: boolean, maxHeight: number) => void): () => void {
    if (!inTauri) return () => {};
    const stop = listen<{ modules: boolean; maxHeight: number }>('panel-opened', (event) => opened(event.payload.modules, event.payload.maxHeight));
    return () => void stop.then((f) => f());
  },
  onClosed(closed: () => void): () => void {
    if (!inTauri) return () => {};
    const stop = listen('panel-closed', closed);
    return () => void stop.then((f) => f());
  },
  /** The core asks (a confirmation, a line of text, one of two choices) or tells (what went wrong) while an action runs;
   *  the panel answers under the row that asked. In a browser, a test sends the same as a window event. */
  onAsk(asked: (question: Question) => void): () => void {
    if (!inTauri) {
      const listener = (event: Event) => asked((event as CustomEvent<Question>).detail);
      window.addEventListener('panel-ask', listener);
      return () => window.removeEventListener('panel-ask', listener);
    }
    const stop = listen<Question>('panel-ask', (event) => asked(event.payload));
    return () => void stop.then((f) => f());
  },
  /** The answer to a question: true or false, the text typed, the option's value, or null when cancelled. */
  answer: (id: number | string, answer: unknown) => (inTauri ? invoke<void>('answer_question', { id, answer })
    : quiet().then(() => void window.dispatchEvent(new CustomEvent('panel-answer', { detail: { id, answer } })))),
  /** The pointer came over the panel (true, again as it moves) or left it (false), heard even when the panel does not
   *  have the focus (macOS). In a browser, a test sends the same as a window event. */
  onHover(hovered: (inside: boolean) => void): () => void {
    if (!inTauri) {
      const listener = (event: Event) => hovered((event as CustomEvent<boolean>).detail);
      window.addEventListener('panel-hover', listener);
      return () => window.removeEventListener('panel-hover', listener);
    }
    const stop = listen<boolean>('panel-hover', (event) => hovered(event.payload));
    return () => void stop.then((f) => f());
  },
  /** The user resized the pinned panel at its edges. In a browser, a test sends the same as a window event. */
  onResized(resized: (width: number, height: number) => void): () => void {
    if (!inTauri) {
      const listener = (event: Event) => { const size = (event as CustomEvent<{ width: number; height: number }>).detail; resized(size.width, size.height); };
      window.addEventListener('panel-resized', listener);
      return () => window.removeEventListener('panel-resized', listener);
    }
    const stop = listen<{ width: number; height: number }>('panel-resized', (event) => resized(event.payload.width, event.payload.height));
    return () => void stop.then((f) => f());
  },
};
