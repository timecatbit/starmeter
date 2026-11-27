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
import type { ActionReceipt, Dashboard, LiveAnswer, LiveReading, Question, ShellInfo, Snapshot } from './types';

export const inTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
/** On a phone (在手机上看): the computer's app served this page, with its key in the address's fragment. */
export const onPhone = !inTauri && typeof location !== 'undefined' && /(^#|&)k=[0-9a-f]{32}/.test(location.hash);
/** Served by the phone server (outside the app, not the dev server) without a key that opens it: a link cut short, or
 *  typed in by hand (a Tailscale address with the port alone). The page says so instead of an empty panel; ?keyless
 *  shows it on the dev server. */
export const keyless = !inTauri && !onPhone && typeof location !== 'undefined'
  && (!import.meta.env.DEV || new URLSearchParams(location.search).has('keyless'));

type Fixtures = typeof import('./fixtureBackend');
let fixtures: Promise<Fixtures> | null = null;
// A phone reads the computer's figures; only the dev server (and the tests on it) carries the fixtures; a release
// build opened anywhere else has nothing to show.
const browser = () => (fixtures ??= onPhone ? import('./phoneBackend')
  : import.meta.env.DEV ? import('./fixtureBackend') : Promise.reject(new Error('no backend outside the app')));

/** A window event standing in for one of the app's (in a browser, where there is no app): its detail to `then`. */
function windowEvent(name: string, then: (detail: { modules?: boolean; maxHeight?: number } | null) => void): () => void {
  if (typeof window === 'undefined') return () => {};
  const listener = (event: Event) => then((event as CustomEvent).detail ?? null);
  window.addEventListener(name, listener);
  return () => window.removeEventListener(name, listener);
}

export const backend = {
  snapshot: (): Promise<Snapshot> => (inTauri ? invoke('snapshot') : browser().then((f) => f.snapshot())),
  dashboard: (range: string): Promise<Dashboard> => (inTauri ? invoke('dashboard', { range }) : browser().then((f) => f.dashboard(range))),
  action: (action: string[]): Promise<ActionReceipt> => (inTauri ? invoke('action', { action }) : browser().then((f) => f.action(action))),
  /** 网页版's live view (on a phone, what the computer read a moment ago), and the pinned panel's 正在运行 and 动态. */
  live: (history = false): Promise<LiveAnswer> => (inTauri ? invoke('live_now', { history }) : onPhone ? import('./phoneBackend').then((f) => f.live(history)) : browser().then((f) => f.live(history))),
  /** The computer's last minutes of live readings only, at once (nothing read again): the panel pinned draws its live
   *  line from them before its first tick. */
  recent: (): Promise<{ at: number; history: LiveReading[] }> =>
    (inTauri ? invoke('recent_readings') : browser().then((f) => f.recent())),
  onSnapshot(changed: () => void): () => void {
    if (inTauri) {
      const stop = listen('snapshot-changed', changed);
      return () => void stop.then((f) => f());
    }
    let off = () => {};
    // Outside the app with nothing behind the page (a release build opened elsewhere), there is nothing to hear.
    void browser().then((f) => (off = f.onChange(changed))).catch(() => {});
    return () => off();
  },
};

const quiet = () => Promise.resolve();

const localGuide = () => { try { return localStorage.getItem('starmeter.guide') === '1'; } catch { return false; } };

/** Where a phone can open the homepage, and how long the public link has left (0 without one). 同一个网络 and the public
 *  link are on and off on their own: `localOpen` says the first is on (its addresses, when this computer has them). */
export interface PhoneView {
  localOpen?: boolean; local: string | null; tailnet?: string | null; wireguard?: string | null; public: string | null; secondsLeft: number;
  /** 自有域名's link (https://meter.example.com/#k=…) while it is on: the user's own proxy hands it on to this server. */
  domain?: string | null;
  /** The server's port: 自有域名's proxy forwards to 127.0.0.1 on it. */
  port?: number;
  /** The public link seen to open from outside before it was shown; false when the wait for it ran out (PHONE-11). */
  publicChecked?: boolean;
  /** A public link is being opened right now (e.g. startup resume or started before leaving the page, R10-8). */
  publicOpening?: boolean;
}
/** 自有域名 as set, for its page: its address, the port the user's proxy forwards to, this computer's network address. */
export interface DomainSetup { domain: string | null; port: number; lan: string | null }
/** How 自有域名's address answered this computer: this server through it, something else (`status`), or nothing (`reason`). */
export interface DomainCheck { opens: boolean; status: number | null; reason: string | null }

/** The browser's stand-in for the phone server, for the tests and the dev server: 同一个网络, the public link and
 *  自有域名, each on and off on its own, the server on while any is; what is on kept in the page's storage, on again as
 *  the page loads again, as the app keeps it in the settings folder and turns it on again as it starts (phone.rs). */
const browserPhone = (() => {
  const KEPT = 'starmeter.test-phone';
  type Kept = { local?: boolean; open?: boolean; domain?: string | null; domainOn?: boolean; port?: number };
  const kept = (() => { try { return JSON.parse(localStorage.getItem(KEPT) ?? '{}') as Kept; } catch { return {} as Kept; } })();
  let local = !!kept.local, open = !!kept.open, domain = kept.domain ?? null, domainOn = !!kept.domainOn && !!domain;
  let port = kept.port ?? 51234;
  const keep = () => { try { localStorage.setItem(KEPT, JSON.stringify({ local, open, domain, domainOn, port })); } catch { /* not kept */ } };
  const asked = (name: string) => new URLSearchParams(location.search).has(name);
  // ?tailnet: this computer on a tailnet too.
  const tailnet = () => (asked('tailnet') ? `http://100.76.55.73:${port}/#k=0123456789abcdef0123456789abcdef` : null);
  // ?wireguard: a WireGuard tunnel up too.
  const wireguard = () => (asked('wireguard') ? `http://10.0.0.2:${port}/#k=0123456789abcdef0123456789abcdef` : null);
  const view = (): PhoneView | null => (local || open || domainOn ? { localOpen: local, local: local ? `http://192.168.1.20:${port}/#k=0123456789abcdef0123456789abcdef` : null,
    tailnet: local ? tailnet() : null,
    wireguard: local ? wireguard() : null,
    domain: domainOn ? `${domain}/#k=abcdefabcdefabcdefabcdefabcdefab` : null, port,
    public: open ? 'https://quiet-river-1234.trycloudflare.com/#k=fedcba9876543210fedcba9876543210' : null, secondsLeft: open ? 3540 : 0,
    // ?unchecked: the wait for the public link to open ran out.
    publicChecked: !asked('unchecked') } : null);
  /** The address as the shell takes it (phone.rs origin_of). */
  const origin = (typed: string) => {
    const text = typed.trim();
    if (!text) throw '填写你的域名，比如 meter.example.com';
    let url: URL;
    try { url = new URL(text.includes('://') ? text : `https://${text}`); } catch { throw '域名的格式不对：只填域名，比如 meter.example.com'; }
    if (!['http:', 'https:'].includes(url.protocol) || !url.hostname || url.username || url.password || url.search || url.hash
      || !['', '/'].includes(url.pathname)) throw '域名的格式不对：只填域名，比如 meter.example.com';
    return url.origin;
  };
  return {
    view, start: () => { local = true; keep(); return view()!; }, stop: () => { local = false; keep(); return view(); },
    open: () => { open = true; keep(); return view()!; }, close: () => { open = false; keep(); return view(); },
    domainSetup: (): DomainSetup => ({ domain, port, lan: '192.168.1.20' }),
    // ?port-taken: the port asked for is another app's.
    domainStart: (typed: string, asked_port: number) => {
      const given = origin(typed);
      if (asked_port < 1024 || asked_port > 65535) throw '端口填 1024 到 65535 之间的数';
      if (asked('port-taken')) throw `端口 ${asked_port} 被别的程序占用`;
      domain = given; port = asked_port; domainOn = true; keep(); return view()!;
    },
    domainStop: () => { domainOn = false; keep(); return view(); },
    // ?domain-502, ?domain-redirect, ?domain-unreachable: how the user's proxy answers.
    domainCheck: (): DomainCheck | null => (!domainOn ? null
      : asked('domain-502') ? { opens: false, status: 502, reason: null }
        : asked('domain-redirect') ? { opens: false, status: 308, reason: null }
          : asked('domain-unreachable') ? { opens: false, status: null, reason: 'timed out' }
            : { opens: true, status: null, reason: null }),
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
      modules: new URLSearchParams(location.search).has('modules'), visible: !new URLSearchParams(location.search).has('hidden') })),
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
  /** The first-run tour: shown once on this computer, whichever panel (the app's or SwiftBar's) showed it. In a
   *  browser, the page's own storage stands in. */
  guideShown: (): Promise<boolean> => (inTauri ? invoke<boolean>('guide_shown').catch(() => false) : Promise.resolve(localGuide())),
  markGuideShown: () => (inTauri ? invoke<void>('mark_guide_shown').catch(() => {}) : quiet().then(() => { try { localStorage.setItem('starmeter.guide', '1'); } catch { /* shown again */ } })),
  /** The panel's look ('standard' or 'sketch'), for a phone (在手机上看) to follow. */
  setStyle: (look: string) => (inTauri ? invoke<void>('set_style', { style: look }).catch(() => {}) : quiet()),
  /** The panel's language ('zh' or 'en'), for the shell's own words: the star's right-click menu. */
  setLanguage: (language: string) => (inTauri ? invoke<void>('set_language', { language }).catch(() => {}) : quiet()),
  /** 在手机上看: 同一个网络 turned on and off (phoneStart, phoneStop), the public link through Cloudflare opened and
   *  closed (phonePublic, phonePrivate), each leaving the other as it is; and where they stand. In a browser, made-up
   *  addresses for the tests. */
  phoneStart: (): Promise<PhoneView> => (inTauri ? invoke<PhoneView>('phone_start') : Promise.resolve(browserPhone.start())),
  phoneStatus: (): Promise<PhoneView | null> => (inTauri ? invoke<PhoneView | null>('phone_status') : Promise.resolve(browserPhone.view())),
  phoneStop: (): Promise<PhoneView | null> => (inTauri ? invoke<PhoneView | null>('phone_stop') : Promise.resolve(browserPhone.stop())),
  /** 任何网络 through the user's own domain (自有域名): as set (phoneDomain), on at an address and port, which are kept
   *  (phoneDomainStart), off (phoneDomainStop), and whether its address opens this server from here (phoneDomainCheck). */
  phoneDomain: (): Promise<DomainSetup | null> => (inTauri ? invoke<DomainSetup | null>('phone_domain') : Promise.resolve(browserPhone.domainSetup())),
  phoneDomainStart: (domain: string, port: number): Promise<PhoneView> =>
    (inTauri ? invoke<PhoneView>('phone_domain_start', { domain, port }) : Promise.resolve().then(() => browserPhone.domainStart(domain, port))),
  phoneDomainStop: (): Promise<PhoneView | null> => (inTauri ? invoke<PhoneView | null>('phone_domain_stop') : Promise.resolve(browserPhone.domainStop())),
  phoneDomainCheck: (): Promise<DomainCheck | null> => (inTauri ? invoke<DomainCheck | null>('phone_domain_check')
    : new Promise((done) => setTimeout(() => done(browserPhone.domainCheck()), 400))),
  phonePublic: (): Promise<PhoneView> => (inTauri ? invoke<PhoneView>('phone_public')
    // ?tunnel-fails: the public link fails as on a network Cloudflare can't be reached from.
    : new URLSearchParams(location.search).has('tunnel-fails') ? Promise.reject('Cloudflare 这次没有连上，稍后再试') : Promise.resolve(browserPhone.open())),
  phonePrivate: (): Promise<PhoneView | null> => (inTauri ? invoke<PhoneView | null>('phone_private') : Promise.resolve(browserPhone.close())),
  /** Whether the public link needs cloudflared downloaded first. */
  tunnelReady: (): Promise<boolean> => (inTauri ? invoke<boolean>('tunnel_ready') : Promise.resolve(!new URLSearchParams(location.search).has('no-cloudflared'))),
  /** The page has drawn what the panel opened on; a window kept clear until then shows. */
  shown: () => (inTauri ? invoke<void>('panel_shown') : quiet()),
  /** One of the page's menus opened or folded: hidden with one open, the next opening waits for the page to draw. */
  menuOpen: (open: boolean) => (inTauri ? invoke<void>('panel_menu', { open }) : quiet()),
  /** A field on the page took the focus or let it go: on a Mac the panel floats meanwhile, so the input method's candidate
   *  window shows above it. */
  typing: (typing: boolean) => (inTauri ? invoke<void>('panel_typing', { typing }) : quiet()),
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
    // In a browser, the same as a window event (the tests open and close the panel so).
    if (!inTauri) return windowEvent('panel-opened', (detail) => opened(!!detail?.modules, detail?.maxHeight ?? 0));
    const stop = listen<{ modules: boolean; maxHeight: number }>('panel-opened', (event) => opened(event.payload.modules, event.payload.maxHeight));
    return () => void stop.then((f) => f());
  },
  onClosed(closed: () => void): () => void {
    if (!inTauri) return windowEvent('panel-closed', () => closed());
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
