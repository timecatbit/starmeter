//! The panel window: where it opens, how it closes, its size, pinning and its look.
//!
//! It opens under the star (the tray icon on Windows and Linux; the pointer in SwiftBar's panel mode) and activates,
//! so its glass and controls draw as active from the first frame. Esc and the star close it and hand the focus back to
//! the app used before; a click elsewhere closes it and leaves the focus where it went. Pinned, it floats above
//! ordinary windows and stays open when something else is clicked; its top strip then moves it, its edges resize it
//! (no smaller than 360 × 280, no wider than 720), and unpinning brings it back under the star.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Theme, WebviewWindow};

use crate::data;
#[cfg(target_os = "macos")]
use crate::macos;

pub const PANEL: &str = "panel";

/// A pinned panel's limits at its edges: the pages are laid out for 360pt and need room for the live chart (pinning
/// starts compact, at about that height).
const MIN_SIZE: (f64, f64) = (360.0, 220.0);
/** Wider leaves the pages' rows and charts with more air than content. */
const MAX_WIDTH: f64 = 480.0;

#[derive(Clone, Copy, PartialEq)]
pub enum Mode {
    /// The app's own star (macOS) or tray icon (Windows, Linux).
    Standalone,
    /// Launched by the SwiftBar plugin, which owns the star: no icon of its own.
    SwiftBar,
}

pub struct Panel {
    pub mode: Mode,
    pinned: AtomicBool,
    /// A native menu or the share sheet has the pointer: a click outside is theirs, not a reason to close.
    held: AtomicBool,
    sharing: AtomicBool,
    /// Which share menu is the latest: a closing one's late word is for its own round, not a newer menu's (macOS,
    /// whose share menu it is).
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    share_round: AtomicU64,
    modules: AtomicBool,
    /// Where the star is, in logical screen coordinates (macOS: AppKit's, origin bottom left).
    anchor: Mutex<(f64, f64)>,
    size: Mutex<(f64, f64)>,
    /// The latest frame animation; an older one stops when a newer starts.
    generation: AtomicU64,
    hidden_at: Mutex<Option<Instant>>,
    /// Pinned, the user moved the panel: it stays where they put it.
    moved: AtomicBool,
    /// Pinned, the user resized it at its edges: the pages take that size.
    sized: AtomicBool,
    resizable: AtomicBool,
    /// An opening waiting for the page to draw before the window shows (its number), 0 when none.
    revealing: AtomicU64,
    openings: AtomicU64,
    /// One of the page's menus (a dropdown) is open, as the page says.
    menu: AtomicBool,
    /// Hidden with a menu open: the window still holds that last drawing, so the next opening waits for the page.
    stale: AtomicBool,
    /// When the shell last set the window's size, to tell the user's resizing from it (Windows, Linux).
    #[cfg(not(target_os = "macos"))]
    sized_at: Mutex<Instant>,
    /// Pinned and moved: the left edge the user chose, and where (and when) the shell last put it.
    left: Mutex<Left>,
    /// The least height the page on screen asks of a pinned panel (the homepage: down to its heatmap).
    min_height: Mutex<f64>,
}

#[derive(Default)]
struct Left {
    chosen: Option<f64>,
    placed: Option<(f64, Instant)>,
}

impl Panel {
    pub fn new(mode: Mode) -> Self {
        Panel {
            mode,
            pinned: AtomicBool::new(false),
            held: AtomicBool::new(false),
            sharing: AtomicBool::new(false),
            share_round: AtomicU64::new(0),
            modules: AtomicBool::new(false),
            anchor: Mutex::new((0.0, 0.0)),
            size: Mutex::new((360.0, 520.0)),
            generation: AtomicU64::new(0),
            hidden_at: Mutex::new(None),
            moved: AtomicBool::new(false),
            sized: AtomicBool::new(false),
            resizable: AtomicBool::new(false),
            revealing: AtomicU64::new(0),
            openings: AtomicU64::new(0),
            menu: AtomicBool::new(false),
            stale: AtomicBool::new(false),
            #[cfg(not(target_os = "macos"))]
            sized_at: Mutex::new(Instant::now()),
            left: Mutex::new(Left::default()),
            min_height: Mutex::new(MIN_SIZE.1),
        }
    }
}

fn window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window(PANEL)
}

pub fn visible(app: &AppHandle) -> bool {
    window(app).is_some_and(|w| w.is_visible().unwrap_or(false))
}

/// The core relaxes its pace while the panel is closed; SwiftBar's plugin reads it from panel-state.json.
fn publish_open(app: &AppHandle, open: bool) {
    crate::engine::PANEL_OPEN.store(open, Ordering::SeqCst);
    if let Some(data) = app.try_state::<crate::data::Data>()
        && let crate::data::Data::SwiftBar(files) = &*data
    {
        files.publish_visibility(open);
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Opened {
    modules: bool,
    max_height: f64,
}

/// The tallest the panel may grow under the star before it scrolls; never under the least a page asks for (a screen
/// that reports no height for a moment, asleep or being switched, is no reason for a panel of nothing).
fn max_height(app: &AppHandle) -> f64 {
    let anchor = *app.state::<Panel>().anchor.lock().expect("the anchor lock");
    #[cfg(target_os = "macos")]
    let tallest = macos::max_height(anchor);
    #[cfg(not(target_os = "macos"))]
    let tallest = work_area(app, anchor).map(|area| area.3 - 16.0).unwrap_or(800.0);
    tallest.max(80.0)
}

/// The working area (without taskbar or menu bar) of the monitor under a point, logical: x, y, width, height.
#[cfg(not(target_os = "macos"))]
fn work_area(app: &AppHandle, point: (f64, f64)) -> Option<(f64, f64, f64, f64)> {
    let monitor =
        app.monitor_from_point(point.0, point.1).ok().flatten().or_else(|| app.primary_monitor().ok().flatten())?;
    let scale = monitor.scale_factor();
    let area = monitor.work_area();
    Some((
        area.position.x as f64 / scale,
        area.position.y as f64 / scale,
        area.size.width as f64 / scale,
        area.size.height as f64 / scale,
    ))
}

/// The user moved or resized the pinned panel: it keeps its place instead of going back under the star.
/// A moved panel's left edge for a page: where the user put it. A wider page (the module grid) slides it left to stay
/// on its screen, and the narrower pages after it go back to the user's place instead of staying where the wide one
/// left them. The window standing elsewhere than the shell put it, once any animation is over, means the user moved it.
fn chosen_left(state: &Panel, now: f64) -> f64 {
    let mut left = state.left.lock().expect("the left lock");
    let moved = match left.placed {
        Some((x, at)) => at.elapsed() > Duration::from_millis(400) && (x - now).abs() > 0.5,
        None => true,
    };
    if moved || left.chosen.is_none() {
        left.chosen = Some(now);
    }
    left.chosen.unwrap_or(now)
}

fn placed_left(state: &Panel, x: f64) {
    state.left.lock().expect("the left lock").placed = Some((x, Instant::now()));
}

fn free(state: &Panel) -> bool {
    state.moved.load(Ordering::SeqCst) || state.sized.load(Ordering::SeqCst)
}

/// Puts the panel at its size under the anchor (or, moved by the user, where its top left is), at once or easing there
/// over 0.3 s.
fn place(app: &AppHandle, animate: bool) {
    let Some(window) = window(app) else { return };
    let state = app.state::<Panel>();
    let anchor = *state.anchor.lock().expect("the anchor lock");
    let (width, height) = *state.size.lock().expect("the size lock");
    let generation = state.generation.fetch_add(1, Ordering::SeqCst) + 1;
    // A window still waiting to show takes its size at once.
    let animate = animate && state.revealing.load(Ordering::SeqCst) == 0;
    #[cfg(target_os = "macos")]
    {
        let current = macos::frame(&window);
        let target = match current.filter(|_| free(&state)) {
            Some(current) => {
                let mut chosen = current;
                chosen.origin.x = chosen_left(&state, current.origin.x);
                let target = macos::free_frame(chosen, width, height);
                placed_left(&state, target.origin.x);
                target
            }
            None => macos::panel_frame(anchor, width, height),
        };
        let start = if animate { current } else { None };
        // Skip animating sub-2px no-op adjustments; animate width and height smoothly in lockstep with CSS pageIn (320ms).
        let instant = start.is_some_and(|s| {
            (s.size.width - target.size.width).abs() < 2.0
                && (s.size.height - target.size.height).abs() < 2.0
                && (s.origin.x - target.origin.x).abs() < 2.0
                && (s.origin.y - target.origin.y).abs() < 2.0
        });
        let Some(start) = start.filter(|_| !instant && visible(app)) else {
            let _ = app.run_on_main_thread(move || macos::set_frame(&window, target, true));
            return;
        };
        animate_frame(app.clone(), generation, move |t, last| {
            let mix = |a: f64, b: f64| a + (b - a) * t;
            let top = mix(start.origin.y + start.size.height, target.origin.y + target.size.height);
            let (w, h) = (mix(start.size.width, target.size.width), mix(start.size.height, target.size.height));
            let frame = objc2_foundation::NSRect::new(
                objc2_foundation::NSPoint::new(mix(start.origin.x, target.origin.x), top - h),
                objc2_foundation::NSSize::new(w, h),
            );
            let window = window.clone();
            move || macos::set_frame(&window, frame, last)
        });
    }
    #[cfg(not(target_os = "macos"))]
    {
        use tauri::{LogicalPosition, LogicalSize};
        let current =
            window.outer_size().ok().zip(window.scale_factor().ok()).map(|(size, scale)| size.to_logical::<f64>(scale));
        let start =
            window.outer_position().ok().zip(window.scale_factor().ok()).map(|(p, scale)| p.to_logical::<f64>(scale));
        let (x, y, height) = match start.filter(|_| free(&state)) {
            // Moved by the user: the top left stays, the bottom no lower than its screen's working area.
            Some(start) => {
                let Some(area) = work_area(app, (start.x + 1.0, start.y + 1.0)) else { return };
                let x = chosen_left(&state, start.x).min(area.0 + area.2 - width).max(area.0);
                placed_left(&state, x);
                let y = start.y.max(area.1);
                (x, y, height.min(area.1 + area.3 - y).max(MIN_SIZE.1))
            }
            None => {
                let Some(area) = work_area(app, anchor) else { return };
                let height = height.min(area.3 - 16.0);
                let x = (anchor.0 - width / 2.0).max(area.0 + 8.0).min(area.0 + area.2 - width - 8.0);
                // A taskbar at the bottom (Windows) puts the panel above the tray icon; a top bar (Linux) below it.
                let below = anchor.1 < area.1 + area.3 / 2.0;
                (x, if below { area.1 + 4.0 } else { area.1 + area.3 - height - 8.0 }, height)
            }
        };
        let stamp = app.clone();
        let mark = move || *stamp.state::<Panel>().sized_at.lock().expect("the sized lock") = Instant::now();
        let (Some(size), Some(start), true) = (current, start, animate && visible(app)) else {
            mark();
            let _ = window.set_size(LogicalSize::new(width, height));
            let _ = window.set_position(LogicalPosition::new(x, y));
            return;
        };
        animate_frame(app.clone(), generation, move |t, _| {
            let mix = |a: f64, b: f64| a + (b - a) * t;
            let (w, h, px, py) = (mix(size.width, width), mix(size.height, height), mix(start.x, x), mix(start.y, y));
            let window = window.clone();
            let mark = mark.clone();
            move || {
                mark();
                let _ = window.set_size(LogicalSize::new(w, h));
                let _ = window.set_position(LogicalPosition::new(px, py));
            }
        });
    }
}

/// Steps a frame animation at 60 frames a second on the panel's curve (cubic-bezier 0.22, 1, 0.36, 1).
/// Eases the window to its new frame over 0.3 s. One step waits on the main thread at a time, and each takes the time
/// as it runs: a busy main thread skips steps instead of playing a queue of them late, all at once.
fn animate_frame<F, G>(app: AppHandle, generation: u64, step: F)
where
    F: Fn(f64, bool) -> G + Send + Sync + 'static,
    G: FnOnce() + Send + 'static,
{
    std::thread::spawn(move || {
        let start = Instant::now();
        let duration = Duration::from_millis(300);
        let step = std::sync::Arc::new(step);
        let waiting = std::sync::Arc::new(AtomicBool::new(false));
        loop {
            if app.state::<Panel>().generation.load(Ordering::SeqCst) != generation {
                return;
            }
            let done = start.elapsed() >= duration;
            if done || !waiting.swap(true, Ordering::SeqCst) {
                let (step, waiting, current) = (step.clone(), waiting.clone(), app.clone());
                let _ = app.run_on_main_thread(move || {
                    if current.state::<Panel>().generation.load(Ordering::SeqCst) == generation {
                        let t = (start.elapsed().as_secs_f64() / duration.as_secs_f64()).min(1.0);
                        (step(page_curve(t), t >= 1.0))();
                    }
                    waiting.store(false, Ordering::SeqCst);
                });
            }
            if done {
                return;
            }
            std::thread::sleep(Duration::from_millis(16));
        }
    });
}

fn page_curve(x: f64) -> f64 {
    let (x1, y1, x2, y2) = (0.22, 1.0, 0.36, 1.0);
    let sample = |a: f64, b: f64, t: f64| ((1.0 - 3.0 * b + 3.0 * a) * t + (3.0 * b - 6.0 * a)) * t * t + 3.0 * a * t;
    let slope =
        |a: f64, b: f64, t: f64| 3.0 * (1.0 - 3.0 * b + 3.0 * a) * t * t + 2.0 * (3.0 * b - 6.0 * a) * t + 3.0 * a;
    let mut t = x;
    for _ in 0..8 {
        let error = sample(x1, x2, t) - x;
        let d = slope(x1, x2, t);
        if error.abs() < 1e-5 || d.abs() < 1e-6 {
            break;
        }
        t -= error / d;
    }
    sample(y1, y2, t.clamp(0.0, 1.0))
}

/// Opens the panel under an anchor: the homepage, or the module list (the star's right-click 主页模块…).
///
/// A closed panel has gone back to its homepage, but the window shows what its page last drew until the page draws
/// again; opening on the module list, the window stays clear until the page says it has drawn it (or a moment
/// passes), so the homepage never flashes first. So it does when the panel was hidden with a menu open: the page
/// folds the menu as the panel goes, but the window had last drawn it open, and it flashed over the homepage.
pub fn open(app: &AppHandle, anchor: (f64, f64), modules: bool) {
    let Some(panel) = window(app) else { return };
    let state = app.state::<Panel>();
    *state.anchor.lock().expect("the anchor lock") = anchor;
    state.modules.store(modules, Ordering::SeqCst);
    let opening = state.openings.fetch_add(1, Ordering::SeqCst) + 1;
    let stale = state.stale.swap(false, Ordering::SeqCst);
    #[cfg(not(target_os = "macos"))]
    let _ = stale;
    #[cfg(target_os = "macos")]
    if (modules || stale) && !visible(app) {
        state.revealing.store(opening, Ordering::SeqCst);
        let clear = panel.clone();
        let _ = macos::on_main(app, move || macos::set_alpha(&clear, 0.0));
        let handle = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(400));
            reveal(&handle, opening);
        });
    }
    #[cfg(not(target_os = "macos"))]
    let _ = opening;
    #[cfg(target_os = "macos")]
    {
        macos::remember_focus();
        // Opened with no field typed into yet: at its own level, whatever a late word from before said (R12-7).
        let level = panel.clone();
        let _ = macos::on_main(app, move || macos::set_typing(&level, false));
    }
    place(app, false);
    let _ = panel.show();
    let _ = panel.set_focus();
    #[cfg(target_os = "macos")]
    {
        macos::activate();
        // Once the page has drawn into the shown window, the shadow is taken from its rounded shape.
        let window = panel.clone();
        let handle = app.clone();
        std::thread::spawn(move || {
            for delay in [60, 240] {
                std::thread::sleep(Duration::from_millis(delay));
                let window = window.clone();
                let _ = handle.run_on_main_thread(move || macos::reshadow(&window));
            }
        });
    }
    publish_open(app, true);
    let _ = app.emit("panel-opened", Opened { modules, max_height: max_height(app) });
}

/// The page has drawn what the opening asked for: the window shows.
fn reveal(app: &AppHandle, opening: u64) {
    let state = app.state::<Panel>();
    // Only the opening that asked, once: a later page drawing for an older opening changes nothing.
    let mine = opening != 0 && state.revealing.compare_exchange(opening, 0, Ordering::SeqCst, Ordering::SeqCst).is_ok();
    #[cfg(target_os = "macos")]
    if mine && let Some(window) = window(app) {
        let _ = app.run_on_main_thread(move || {
            macos::set_alpha(&window, 1.0);
            macos::reshadow(&window);
        });
    }
    #[cfg(not(target_os = "macos"))]
    let _ = mine;
}

/// Esc or the star (return_focus): the app used before gets the focus back. A click elsewhere: it keeps it.
pub fn close(app: &AppHandle, return_focus: bool) {
    let Some(panel) = window(app) else { return };
    let state = app.state::<Panel>();
    let was_visible = panel.is_visible().unwrap_or(false);
    state.sharing.store(false, Ordering::SeqCst);
    state.held.store(false, Ordering::SeqCst);
    set_pinned_state(app, false);
    // A question still waiting under a row is answered as cancelled.
    crate::ask::cancel_all();
    // No field keeps the focus away: the panel opens again at its own level, the page saying so if one takes it back.
    #[cfg(target_os = "macos")]
    {
        let window = panel.clone();
        let _ = app.run_on_main_thread(move || macos::set_typing(&window, false));
    }
    let _ = panel.hide();
    if was_visible {
        state.stale.store(state.menu.swap(false, Ordering::SeqCst), Ordering::SeqCst);
        *state.hidden_at.lock().expect("the hidden lock") = Some(Instant::now());
        publish_open(app, false);
        let _ = app.emit("panel-closed", ());
    }
    if return_focus {
        return_focus_now(app);
    }
}

fn return_focus_now(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    {
        let _ = app.run_on_main_thread(macos::restore_focus);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = app;
}

/// After a dialog an action showed with the panel out of the way.
pub fn return_focus(app: &AppHandle) {
    return_focus_now(app);
}

/// The star clicked: open, or close when open. A click that itself closed the panel (it took the focus away a
/// moment ago) doesn't reopen it.
pub fn toggle(app: &AppHandle, anchor: (f64, f64), modules: bool) {
    let state = app.state::<Panel>();
    if visible(app) {
        if state.modules.load(Ordering::SeqCst) == modules {
            close(app, true);
        } else {
            open(app, anchor, modules);
        }
        return;
    }
    let recent =
        state.hidden_at.lock().expect("the hidden lock").is_some_and(|at| at.elapsed() < Duration::from_millis(250));
    if !recent {
        open(app, anchor, modules);
    }
}

/// The panel lost the focus: a click elsewhere closes it, unless it is pinned or a menu or the share sheet is up.
pub fn blurred(app: &AppHandle) {
    let state = app.state::<Panel>();
    if !visible(app)
        || state.pinned.load(Ordering::SeqCst)
        || state.held.load(Ordering::SeqCst)
        || state.sharing.load(Ordering::SeqCst)
    {
        return;
    }
    // A click on the star itself: the star toggles the panel (SwiftBar by relaunching the app), as 5.x let it.
    #[cfg(target_os = "macos")]
    if macos::pointer_on_star(*state.anchor.lock().expect("the anchor lock")) {
        return;
    }
    #[cfg(target_os = "macos")]
    macos::forget_focus();
    close(app, false);
}

pub fn focused(app: &AppHandle) {
    app.state::<Panel>().sharing.store(false, Ordering::SeqCst);
}

/// A save dialog is up: the panel stays open while the dialog has the focus, and sits below it (pinned or not, the panel
/// floats above ordinary windows); afterwards it is back where it was, with the focus.
pub fn step_aside(app: &AppHandle, aside: bool) {
    let state = app.state::<Panel>();
    state.held.store(aside, Ordering::SeqCst);
    let Some(window) = window(app) else { return };
    #[cfg(target_os = "macos")]
    {
        let w = window.clone();
        let _ = macos::on_main(app, move || macos::step_aside(&w, aside));
    }
    #[cfg(not(target_os = "macos"))]
    if state.pinned.load(Ordering::SeqCst) {
        let _ = window.set_always_on_top(!aside);
    }
    if !aside {
        let _ = window.set_focus();
    }
}

/// The share sheet (macOS) draws in another process; its clicks must not close the panel. Says which round of sharing
/// this is, for share_closed and share_dropped.
#[cfg(target_os = "macos")]
pub fn hold_for_share(app: &AppHandle) -> u64 {
    let state = app.state::<Panel>();
    let round = state.share_round.fetch_add(1, Ordering::SeqCst) + 1;
    state.sharing.store(true, Ordering::SeqCst);
    round
}

/// The share menu of this round never came up (no window to show it on): the panel is back to closing on a click
/// elsewhere at once.
#[cfg(target_os = "macos")]
pub fn share_dropped(app: &AppHandle, round: u64) {
    let state = app.state::<Panel>();
    if state.share_round.load(Ordering::SeqCst) == round {
        state.sharing.store(false, Ordering::SeqCst);
    }
}

/// The share menu closed (picked: a service was chosen). A click on another app while it was up took the focus without
/// closing the panel (blurred stood aside for the menu) and the panel would stay up, floating, until clicked again: a
/// moment later, once the focus has settled, the panel closes as a click elsewhere closes it. Dismissed, that is when
/// the panel no longer has the focus; with a service picked, only when another app has it (a service may show a
/// window of this app's own, such as AirDrop's, which takes the focus from the panel and needs it to stay).
/// Only for its own round: a menu opened again within the moment is the newer round's to settle.
#[cfg(target_os = "macos")]
pub fn share_closed(app: &AppHandle, picked: bool, round: u64) {
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || {
            let state = handle.state::<Panel>();
            if state.share_round.load(Ordering::SeqCst) != round || !state.sharing.swap(false, Ordering::SeqCst) {
                return;
            }
            let focused = window(&handle).is_some_and(|panel| panel.is_focused().unwrap_or(false));
            let away = if picked { !crate::macos::app_active() } else { !focused };
            if away {
                blurred(&handle);
            }
        });
    });
}

fn set_pinned_state(app: &AppHandle, pinned: bool) {
    let state = app.state::<Panel>();
    if state.pinned.swap(pinned, Ordering::SeqCst) == pinned {
        return;
    }
    // Unpinned, the panel belongs under the star again, at its pages' own size.
    if !pinned {
        state.moved.store(false, Ordering::SeqCst);
        state.sized.store(false, Ordering::SeqCst);
        *state.left.lock().expect("the left lock") = Left::default();
        set_resizable(app, false);
    }
    #[cfg(target_os = "macos")]
    if let Some(window) = window(app) {
        let _ = app.run_on_main_thread(move || macos::set_pinned(&window, pinned));
    }
    #[cfg(not(target_os = "macos"))]
    if let Some(window) = window(app) {
        let _ = window.set_always_on_top(pinned);
    }
}

/// The star's figure (menuBar preference): today's estimate or the tightest window beside the mark, in even digits;
/// Windows puts it in the tray's tooltip.
pub fn update_star(app: &AppHandle) {
    let Some(tray) = app.tray_by_id("star") else { return };
    let text = data::menu_bar_text(app);
    #[cfg(target_os = "macos")]
    {
        // An empty title, not none: tray-icon leaves the button's old title in place when given none, so a figure
        // turned off (只显示星星) stayed beside the star.
        let _ = tray.set_title(Some(text.as_str()));
        let _ = tray.with_inner_tray_icon(|inner| {
            if let Some(item) = inner.ns_status_item() {
                use objc2::msg_send;
                use objc2::runtime::AnyObject;
                let item: *const AnyObject = (&*item as *const _ as *const AnyObject).cast();
                let button: *mut AnyObject = unsafe { msg_send![item, button] };
                macos::monospaced_title(button);
            }
        });
    }
    #[cfg(not(target_os = "macos"))]
    {
        let tip = if text.is_empty() { "Starmeter".to_owned() } else { format!("Starmeter · {text}") };
        let _ = tray.set_tooltip(Some(tip));
    }
}

// MARK: Commands for the UI

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellInfo {
    platform: &'static str,
    mode: &'static str,
    version: &'static str,
    max_height: f64,
    modules: bool,
    /** The window on screen as the page asks: hidden at the app's start (as at login), the page doesn't take itself as
     * opened (STAGE-11: it did, and 实时 read the logs in the background until the panel was opened and closed). */
    visible: bool,
}

#[tauri::command]
pub fn shell_info(app: AppHandle) -> ShellInfo {
    let state = app.state::<Panel>();
    ShellInfo {
        platform: if cfg!(target_os = "macos") {
            "macos"
        } else if cfg!(windows) {
            "windows"
        } else {
            "linux"
        },
        mode: if state.mode == Mode::SwiftBar { "swiftbar" } else { "standalone" },
        version: starmeter_core::VERSION,
        max_height: max_height(&app),
        modules: state.modules.load(Ordering::SeqCst),
        visible: visible(&app),
    }
}

#[tauri::command]
pub fn close_panel(app: AppHandle, return_focus: bool) {
    close(&app, return_focus);
}

/// Pinned, the edges resize the panel within its limits; otherwise, and on the module grid, the pages set its size.
fn set_resizable(app: &AppHandle, resizable: bool) {
    use tauri::LogicalSize;
    let state = app.state::<Panel>();
    if state.resizable.swap(resizable, Ordering::SeqCst) == resizable {
        return;
    }
    let Some(window) = window(app) else { return };
    #[cfg(not(target_os = "macos"))]
    {
        *state.sized_at.lock().expect("the sized lock") = Instant::now();
    }
    let _ = window.set_resizable(resizable);
    if resizable {
        let least = *state.min_height.lock().expect("the min height lock");
        let _ = window.set_min_size(Some(LogicalSize::new(MIN_SIZE.0, least)));
        let _ = window.set_max_size(Some(LogicalSize::new(MAX_WIDTH, 10_000.0)));
    } else {
        let _ = window.set_min_size(None::<LogicalSize<f64>>);
        let _ = window.set_max_size(None::<LogicalSize<f64>>);
    }
}

#[derive(Serialize, Clone)]
struct Size {
    width: f64,
    height: f64,
}

/// The window changed size. When the user did it at an edge, the pages take that size from now on (until unpinned).
pub fn resized(app: &AppHandle) {
    let state = app.state::<Panel>();
    if !state.resizable.load(Ordering::SeqCst) {
        return;
    }
    let Some(window) = window(app) else { return };
    #[cfg(target_os = "macos")]
    let size = macos::on_main(app, {
        let window = window.clone();
        move || macos::live_resize_size(&window)
    })
    .flatten();
    #[cfg(not(target_os = "macos"))]
    let size = {
        let settled = state.sized_at.lock().expect("the sized lock").elapsed() > Duration::from_millis(300);
        let size = window
            .inner_size()
            .ok()
            .zip(window.scale_factor().ok())
            .map(|(size, scale)| size.to_logical::<f64>(scale))
            .map(|size| (size.width, size.height));
        size.filter(|_| settled)
    };
    let Some((width, height)) = size else { return };
    let size = (width.round(), height.round());
    state.sized.store(true, Ordering::SeqCst);
    // A frame animation in flight would pull against the edge.
    state.generation.fetch_add(1, Ordering::SeqCst);
    {
        let mut current = state.size.lock().expect("the size lock");
        if *current == size {
            return;
        }
        *current = size;
    }
    #[cfg(target_os = "macos")]
    {
        // The shadow redrawn at most every 100 ms while an edge is dragged (PERF-8), and once more after the last
        // change, so the size the drag ends at always has its own shadow (one skipped left the shadow of a size before).
        const EVERY: Duration = Duration::from_millis(100);
        static LAST_SHADOW: Mutex<Option<Instant>> = Mutex::new(None);
        static TRAILING: AtomicBool = AtomicBool::new(false);
        let wait = {
            let mut last = LAST_SHADOW.lock().expect("the shadow lock");
            match last.and_then(|at| EVERY.checked_sub(at.elapsed())) {
                Some(left) => Some(left),
                None => {
                    *last = Some(Instant::now());
                    None
                }
            }
        };
        match wait {
            None => {
                let _ = app.run_on_main_thread(move || macos::reshadow(&window));
            }
            Some(wait) if !TRAILING.swap(true, Ordering::SeqCst) => {
                let app = app.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(wait);
                    TRAILING.store(false, Ordering::SeqCst);
                    *LAST_SHADOW.lock().expect("the shadow lock") = Some(Instant::now());
                    let _ = app.run_on_main_thread(move || macos::reshadow(&window));
                });
            }
            Some(_) => {}
        }
    }
    let _ = app.emit("panel-resized", Size { width: size.0, height: size.1 });
}

#[tauri::command]
pub fn resize_panel(app: AppHandle, width: f64, height: f64, animate: bool, resizable: Option<bool>) {
    let state = app.state::<Panel>();
    set_resizable(&app, resizable.unwrap_or(false) && state.pinned.load(Ordering::SeqCst));
    // While the user drags an edge, their size wins; the page's answer to it arrives as this same size.
    #[cfg(target_os = "macos")]
    if let Some(window) = window(&app)
        && macos::on_main(&app, move || macos::in_live_resize(&window)).unwrap_or(false)
    {
        return;
    }
    let limit = if free(&state) { 10_000.0 } else { max_height(&app) };
    let size = (width.clamp(240.0, 900.0), height.clamp(80.0, limit.max(80.0)).round());
    {
        let mut current = state.size.lock().expect("the size lock");
        if *current == size {
            return;
        }
        *current = size;
    }
    place(&app, animate);
}

#[tauri::command]
pub fn set_pinned(app: AppHandle, pinned: bool) {
    let was_free = free(&app.state::<Panel>());
    set_pinned_state(&app, pinned);
    // Unpinned where the user put it, it eases back under the star (the page then asks for its own size).
    if !pinned && was_free {
        place(&app, true);
    }
}

/// Pinned, the corner handle sets the size (within the limits, the top left kept, on its screen); the size the window
/// took comes back as the user's.
#[tauri::command]
pub fn size_panel(app: AppHandle, width: f64, height: f64) {
    let state = app.state::<Panel>();
    if !state.pinned.load(Ordering::SeqCst) || !state.resizable.load(Ordering::SeqCst) {
        return;
    }
    let Some(window) = window(&app) else { return };
    let least = *state.min_height.lock().expect("the min height lock");
    let asked = (width.clamp(MIN_SIZE.0, MAX_WIDTH).round(), height.max(least).round());
    #[cfg(target_os = "macos")]
    let size = macos::frame(&window).map_or(asked, |current| {
        let frame = macos::free_frame(current, asked.0, asked.1);
        (frame.size.width.round(), frame.size.height.round())
    });
    // As macOS keeps a frame on its screen: no taller than from the window's top to the bottom of its working area
    // (place keeps the window there), so the page lays out for the size the window has, its corner on screen.
    #[cfg(not(target_os = "macos"))]
    let size = {
        let top =
            window.outer_position().ok().zip(window.scale_factor().ok()).map(|(p, scale)| p.to_logical::<f64>(scale));
        match top.and_then(|top| work_area(&app, (top.x + 1.0, top.y + 1.0)).map(|area| (top, area))) {
            Some((top, area)) => {
                let room = (area.1 + area.3 - top.y.max(area.1)).max(MIN_SIZE.1);
                (asked.0.min(area.2.max(MIN_SIZE.0)).round(), asked.1.min(room).round())
            }
            None => asked,
        }
    };
    state.sized.store(true, Ordering::SeqCst);
    {
        let mut current = state.size.lock().expect("the size lock");
        if *current == size {
            return;
        }
        *current = size;
    }
    place(&app, false);
    let _ = app.emit("panel-resized", Size { width: size.0, height: size.1 });
}

/// Pinned, a page's top strip moves the window with the pointer.
#[tauri::command]
pub fn drag_panel(app: AppHandle) {
    let state = app.state::<Panel>();
    let Some(window) = window(&app).filter(|_| state.pinned.load(Ordering::SeqCst)) else { return };
    state.moved.store(true, Ordering::SeqCst);
    state.generation.fetch_add(1, Ordering::SeqCst);
    #[cfg(target_os = "macos")]
    {
        let panel = window.clone();
        let _ = app.run_on_main_thread(move || {
            if !macos::drag(&panel) {
                let _ = panel.start_dragging();
            }
        });
    }
    #[cfg(not(target_os = "macos"))]
    let _ = window.start_dragging();
}

/// The page has drawn the opening's page at its size.
#[tauri::command]
pub fn panel_shown(app: AppHandle) {
    let opening = app.state::<Panel>().revealing.load(Ordering::SeqCst);
    reveal(&app, opening);
}

/// The page opened or folded one of its menus.
#[tauri::command]
pub fn panel_menu(app: AppHandle, open: bool) {
    app.state::<Panel>().menu.store(open, Ordering::SeqCst);
}

/// A field on the page took the focus or let it go. On a Mac the panel floats while it is typed into, below the input
/// method's candidate window, which the pop-up level hid; elsewhere the input method shows above it already.
#[tauri::command]
pub fn panel_typing(app: AppHandle, typing: bool) {
    // A field's focus told late, the panel already closed: nothing typed into, so the level stays as it is (R12-7).
    #[cfg(target_os = "macos")]
    if let Some(window) = window(&app) {
        let _ = app.run_on_main_thread(move || {
            if !typing || window.is_visible().unwrap_or(false) {
                macos::set_typing(&window, typing);
            }
        });
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (app, typing);
}

#[tauri::command]
pub fn hold_open(app: AppHandle, held: bool) {
    app.state::<Panel>().held.store(held, Ordering::SeqCst);
}

/// 跟随系统 / 浅色 / 深色 for the window (and the web page's light or dark), and its background: 实色 (solid), 磨砂
/// (frosted) or 玻璃 (glass). Returns whether a material shows; when it doesn't, the page paints the solid background.
#[tauri::command]
pub fn set_look(app: AppHandle, theme: String, material: String) -> bool {
    let wanted = match theme.as_str() {
        "light" => Some(Theme::Light),
        "dark" => Some(Theme::Dark),
        _ => None,
    };
    app.set_theme(wanted);
    let Some(window) = window(&app) else { return false };
    let _ = window.set_theme(wanted);
    #[cfg(target_os = "macos")]
    {
        macos::on_main(&app, move || macos::set_material(&material)).unwrap_or(false)
    }
    #[cfg(windows)]
    {
        // 磨砂 is Acrylic, 玻璃 is Mica; both need Windows 11, so Windows 10 paints the solid colour.
        use tauri::window::{Effect, EffectsBuilder};
        let effect = match material.as_str() {
            "frosted" => Some(Effect::Acrylic),
            "glass" => Some(Effect::Mica),
            _ => None,
        }
        .filter(|_| mica_supported());
        let shown = effect.is_some();
        let effects = effect.map(|effect| EffectsBuilder::new().effect(effect).build());
        window.set_effects(effects).is_ok() && shown
    }
    #[cfg(all(not(target_os = "macos"), not(windows)))]
    {
        let _ = material;
        false
    }
}

/// Mica needs Windows 11 (build 22000); on Windows 10 the window would be see-through, so the page paints instead.
#[cfg(windows)]
fn mica_supported() -> bool {
    static SUPPORTED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *SUPPORTED.get_or_init(|| {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: no console flashes up.
        std::process::Command::new("reg")
            .args(["query", r"HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion", "/v", "CurrentBuildNumber"])
            .creation_flags(0x0800_0000)
            .output()
            .ok()
            .and_then(|out| {
                String::from_utf8_lossy(&out.stdout).split_whitespace().last().and_then(|n| n.parse::<u32>().ok())
            })
            .is_some_and(|build| build >= 22000)
    })
}

/// The symbols the panel takes from macOS (the header's pin, gear and share; the share page's share, copy, save and
/// done): base64 PNG at 3 pixels a point, or None for any other name. Elsewhere the panel draws its own.
#[cfg(target_os = "macos")]
#[tauri::command]
pub fn system_symbol(app: AppHandle, name: String, points: f64) -> Option<String> {
    use std::collections::HashMap;
    use std::sync::OnceLock;
    const SYMBOLS: [&str; 7] =
        ["pin", "pin.fill", "gearshape", "square.and.arrow.up", "square.and.arrow.down", "doc.on.doc", "checkmark"];
    if !SYMBOLS.contains(&name.as_str()) {
        return None;
    }
    let clamped = points.clamp(8.0, 48.0);
    let key = (name.clone(), (clamped * 10.0).round() as u32);
    type SymbolCache = Mutex<HashMap<(String, u32), Option<String>>>;
    static CACHE: OnceLock<SymbolCache> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(hit) = cache.lock().expect("the symbol cache lock").get(&key).cloned() {
        return hit;
    }
    let rendered = macos::on_main(&app, move || macos::symbol_png(&name, clamped))
        .flatten()
        .map(|png| starmeter_core::dashboard::base64_encode(&png));
    // Only a drawn symbol is kept: one the main thread couldn't draw this time is asked for again next time.
    if rendered.is_some() {
        cache.lock().expect("the symbol cache lock").insert(key, rendered.clone());
    }
    rendered
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
pub fn system_symbol(name: String, points: f64) -> Option<String> {
    let _ = (name, points);
    None
}

#[cfg(test)]
mod left_tests {
    use super::*;

    #[test]
    fn a_wide_page_slides_a_moved_panel_and_the_next_page_puts_it_back() {
        let state = Panel::new(Mode::Standalone);
        // The user's place, then the module grid slides it 228pt left to fit on screen.
        assert_eq!(chosen_left(&state, 1000.0), 1000.0);
        placed_left(&state, 772.0);
        // Back on a narrower page, the window still where the grid left it: the user's place again.
        assert_eq!(chosen_left(&state, 772.0), 1000.0);
        placed_left(&state, 1000.0);
        // The user drags it elsewhere (some time after the shell last placed it): that is their place now.
        state.left.lock().unwrap().placed = Some((1000.0, Instant::now() - Duration::from_secs(2)));
        assert_eq!(chosen_left(&state, 500.0), 500.0);
    }
}

/// The least height the page on screen asks of a pinned panel, kept at its edges too (the homepage: down to its
/// heatmap, so a panel made small still shows the figures, the live chart and the heatmap).
#[tauri::command]
pub fn set_min_height(app: AppHandle, height: f64) {
    use tauri::LogicalSize;
    let state = app.state::<Panel>();
    let height = height.clamp(MIN_SIZE.1, 900.0).round();
    *state.min_height.lock().expect("the min height lock") = height;
    if state.resizable.load(Ordering::SeqCst)
        && let Some(window) = window(&app)
    {
        let _ = window.set_min_size(Some(LogicalSize::new(MIN_SIZE.0, height)));
    }
}

/// The pointer over the panel or off it, heard natively so it reaches a pinned panel that does not have the focus:
/// the page shows the grabber and the resize corner. Moves are passed on at most every 150ms.
#[cfg(target_os = "macos")]
pub fn hovered(app: &AppHandle, inside: bool) {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};
    static LAST: Mutex<Option<Instant>> = Mutex::new(None);
    if inside {
        let mut last = LAST.lock().expect("the hover time lock");
        if last.is_some_and(|at| at.elapsed() < Duration::from_millis(150)) {
            return;
        }
        *last = Some(Instant::now());
    } else {
        *LAST.lock().expect("the hover time lock") = None;
    }
    let _ = app.emit("panel-hover", inside);
}
