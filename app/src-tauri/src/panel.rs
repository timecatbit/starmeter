//! The panel window: where it opens, how it closes, its size, pinning and its look.
//!
//! It opens under the star (the tray icon on Windows and Linux; the pointer in SwiftBar's panel mode) and activates,
//! so its glass and controls draw as active from the first frame. Esc and the star close it and hand the focus back to
//! the app used before; a click elsewhere closes it and leaves the focus where it went. Pinned, it floats above
//! ordinary windows and stays open when something else is clicked.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Theme, WebviewWindow};

use crate::data;
#[cfg(target_os = "macos")]
use crate::macos;

pub const PANEL: &str = "panel";

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
    modules: AtomicBool,
    /// Where the star is, in logical screen coordinates (macOS: AppKit's, origin bottom left).
    anchor: Mutex<(f64, f64)>,
    size: Mutex<(f64, f64)>,
    /// The latest frame animation; an older one stops when a newer starts.
    generation: AtomicU64,
    hidden_at: Mutex<Option<Instant>>,
}

impl Panel {
    pub fn new(mode: Mode) -> Self {
        Panel {
            mode,
            pinned: AtomicBool::new(false),
            held: AtomicBool::new(false),
            sharing: AtomicBool::new(false),
            modules: AtomicBool::new(false),
            anchor: Mutex::new((0.0, 0.0)),
            size: Mutex::new((360.0, 520.0)),
            generation: AtomicU64::new(0),
            hidden_at: Mutex::new(None),
        }
    }
}

fn window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window(PANEL)
}

pub fn visible(app: &AppHandle) -> bool {
    window(app).is_some_and(|w| w.is_visible().unwrap_or(false))
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Opened {
    modules: bool,
    max_height: f64,
}

/// The tallest the panel may grow under the star before it scrolls.
fn max_height(app: &AppHandle) -> f64 {
    let anchor = *app.state::<Panel>().anchor.lock().expect("the anchor lock");
    #[cfg(target_os = "macos")]
    {
        macos::max_height(anchor)
    }
    #[cfg(not(target_os = "macos"))]
    {
        work_area(app, anchor).map(|area| area.3 - 16.0).unwrap_or(800.0)
    }
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

/// Puts the panel at its size under the anchor, at once or easing there over 0.3 s.
fn place(app: &AppHandle, animate: bool) {
    let Some(window) = window(app) else { return };
    let state = app.state::<Panel>();
    let anchor = *state.anchor.lock().expect("the anchor lock");
    let (width, height) = *state.size.lock().expect("the size lock");
    let generation = state.generation.fetch_add(1, Ordering::SeqCst) + 1;
    #[cfg(target_os = "macos")]
    {
        let target = macos::panel_frame(anchor, width, height);
        let start = if animate { macos::frame(&window) } else { None };
        let Some(start) = start.filter(|_| visible(app)) else {
            let _ = app.run_on_main_thread(move || macos::set_frame(&window, target));
            return;
        };
        animate_frame(app.clone(), generation, move |t| {
            let mix = |a: f64, b: f64| a + (b - a) * t;
            let top = mix(start.origin.y + start.size.height, target.origin.y + target.size.height);
            let (w, h) = (mix(start.size.width, target.size.width), mix(start.size.height, target.size.height));
            let frame = objc2_foundation::NSRect::new(
                objc2_foundation::NSPoint::new(mix(start.origin.x, target.origin.x), top - h),
                objc2_foundation::NSSize::new(w, h),
            );
            let window = window.clone();
            move || macos::set_frame(&window, frame)
        });
    }
    #[cfg(not(target_os = "macos"))]
    {
        use tauri::{LogicalPosition, LogicalSize};
        let Some(area) = work_area(app, anchor) else { return };
        let height = height.min(area.3 - 16.0);
        let x = (anchor.0 - width / 2.0).max(area.0 + 8.0).min(area.0 + area.2 - width - 8.0);
        // A taskbar at the bottom (Windows) puts the panel above the tray icon; a top bar (Linux) below it.
        let below = anchor.1 < area.1 + area.3 / 2.0;
        let y = if below { area.1 + 4.0 } else { area.1 + area.3 - height - 8.0 };
        let current =
            window.outer_size().ok().zip(window.scale_factor().ok()).map(|(size, scale)| size.to_logical::<f64>(scale));
        let start =
            window.outer_position().ok().zip(window.scale_factor().ok()).map(|(p, scale)| p.to_logical::<f64>(scale));
        let (Some(size), Some(start), true) = (current, start, animate && visible(app)) else {
            let _ = window.set_size(LogicalSize::new(width, height));
            let _ = window.set_position(LogicalPosition::new(x, y));
            return;
        };
        animate_frame(app.clone(), generation, move |t| {
            let mix = |a: f64, b: f64| a + (b - a) * t;
            let (w, h, px, py) = (mix(size.width, width), mix(size.height, height), mix(start.x, x), mix(start.y, y));
            let window = window.clone();
            move || {
                let _ = window.set_size(LogicalSize::new(w, h));
                let _ = window.set_position(LogicalPosition::new(px, py));
            }
        });
    }
}

/// Steps a frame animation at 60 frames a second on the panel's curve (cubic-bezier 0.22, 1, 0.36, 1).
fn animate_frame<F, G>(app: AppHandle, generation: u64, step: F)
where
    F: Fn(f64) -> G + Send + 'static,
    G: FnOnce() + Send + 'static,
{
    std::thread::spawn(move || {
        let start = Instant::now();
        let duration = Duration::from_millis(300);
        loop {
            if app.state::<Panel>().generation.load(Ordering::SeqCst) != generation {
                return;
            }
            let t = (start.elapsed().as_secs_f64() / duration.as_secs_f64()).min(1.0);
            let _ = app.run_on_main_thread(step(page_curve(t)));
            if t >= 1.0 {
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
pub fn open(app: &AppHandle, anchor: (f64, f64), modules: bool) {
    let Some(panel) = window(app) else { return };
    let state = app.state::<Panel>();
    *state.anchor.lock().expect("the anchor lock") = anchor;
    state.modules.store(modules, Ordering::SeqCst);
    #[cfg(target_os = "macos")]
    macos::remember_focus();
    place(app, false);
    let _ = panel.show();
    let _ = panel.set_focus();
    #[cfg(target_os = "macos")]
    macos::activate();
    let _ = app.emit("panel-opened", Opened { modules, max_height: max_height(app) });
}

/// Esc or the star (return_focus): the app used before gets the focus back. A click elsewhere: it keeps it.
pub fn close(app: &AppHandle, return_focus: bool) {
    let Some(panel) = window(app) else { return };
    let state = app.state::<Panel>();
    let was_visible = panel.is_visible().unwrap_or(false);
    state.sharing.store(false, Ordering::SeqCst);
    state.held.store(false, Ordering::SeqCst);
    set_pinned_state(app, false);
    let _ = panel.hide();
    if was_visible {
        *state.hidden_at.lock().expect("the hidden lock") = Some(Instant::now());
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
    #[cfg(target_os = "macos")]
    macos::forget_focus();
    close(app, false);
}

pub fn focused(app: &AppHandle) {
    app.state::<Panel>().sharing.store(false, Ordering::SeqCst);
}

pub fn hold_for_share(app: &AppHandle) {
    app.state::<Panel>().sharing.store(true, Ordering::SeqCst);
}

fn set_pinned_state(app: &AppHandle, pinned: bool) {
    let state = app.state::<Panel>();
    if state.pinned.swap(pinned, Ordering::SeqCst) == pinned {
        return;
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
        let _ = tray.set_title(if text.is_empty() { None } else { Some(text.as_str()) });
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
    }
}

#[tauri::command]
pub fn close_panel(app: AppHandle, return_focus: bool) {
    close(&app, return_focus);
}

#[tauri::command]
pub fn resize_panel(app: AppHandle, width: f64, height: f64, animate: bool) {
    let state = app.state::<Panel>();
    let size = (width.clamp(240.0, 900.0), height.clamp(80.0, max_height(&app)).round());
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
    set_pinned_state(&app, pinned);
}

#[tauri::command]
pub fn hold_open(app: AppHandle, held: bool) {
    app.state::<Panel>().held.store(held, Ordering::SeqCst);
}

/// 跟随系统 / 浅色 / 深色 for the window (and the web page's light or dark), and glass or a solid background. Returns
/// whether the system material shows; when it doesn't, the page paints the solid background.
#[tauri::command]
pub fn set_look(app: AppHandle, theme: String, glass: bool) -> bool {
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
        macos::on_main(&app, move || macos::set_glass(glass)).unwrap_or(false)
    }
    #[cfg(windows)]
    {
        use tauri::window::{Effect, EffectsBuilder};
        let effects = glass.then(|| EffectsBuilder::new().effect(Effect::Mica).build());
        window.set_effects(effects).is_ok() && glass
    }
    #[cfg(all(not(target_os = "macos"), not(windows)))]
    {
        let _ = glass;
        false
    }
}
