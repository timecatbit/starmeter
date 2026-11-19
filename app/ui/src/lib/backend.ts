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
import type { ActionReceipt, Dashboard, ShellInfo, Snapshot } from './types';

export const inTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

type Fixtures = typeof import('./fixtureBackend');
let fixtures: Promise<Fixtures> | null = null;
// Only the dev server (and the tests on it) carries the fixtures; a release build serves the app alone.
const browser = () => (fixtures ??= import.meta.env.DEV ? import('./fixtureBackend') : Promise.reject(new Error('no backend outside the app')));

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
  /** The page has drawn what the panel opened on; a window kept clear until then shows. */
  shown: () => (inTauri ? invoke<void>('panel_shown') : quiet()),
  /** Theme (system, light, dark) and background (glass or solid) for the window and the system material under it. */
  /** Returns whether the system material shows; when it doesn't, the page paints its solid background. */
  setLook: (theme: string, glass: boolean): Promise<boolean> => (inTauri ? invoke<boolean>('set_look', { theme, glass }) : Promise.resolve(glass)),
  /** A web page comes to the front; an app opens to update itself; a swiftbar:// command runs without taking focus. */
  openLink: (href: string) => (inTauri ? invoke<void>('open_link', { href }) : Promise.resolve(void window.open(href, '_blank'))),
  /** Holds the panel open while a native menu or the share sheet has the pointer. */
  holdOpen: (held: boolean) => (inTauri ? invoke<void>('hold_open', { held }) : quiet()),
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
