//! macOS pieces Tauri has no API for: the system glass under the panel, the panel's level and frame, handing the focus
//! back, the confirmation alert, the share menu, a picture on the pasteboard, the menu bar figure's font and the user
//! notification permission. All of them run on the main thread.

use std::cell::{Cell, RefCell};
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{OnceLock, mpsc};

use block2::{Block, RcBlock};
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Bool, NSObject, Sel};
use objc2::{ClassType, class, define_class, msg_send, sel};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use tauri::{AppHandle, Runtime, WebviewWindow};

const CORNER_RADIUS: f64 = 13.0;

// UNUserNotificationCenter lives in its own framework; nothing else links it.
#[link(name = "UserNotifications", kind = "framework")]
unsafe extern "C" {}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    /// Whether a mouse button is down now, from any thread (state 0: the session's, button 0: the left one).
    fn CGEventSourceButtonState(state: i32, button: u32) -> bool;
}

thread_local! {
    /// The app that was in front when the panel opened.
    static PREVIOUS: RefCell<Option<Retained<AnyObject>>> = const { RefCell::new(None) };
    /// The system's glass under the web view (macOS 26 and later).
    static GLASS: RefCell<Option<Retained<AnyObject>>> = const { RefCell::new(None) };
    /// The classic frosted blur under the web view (磨砂; also 玻璃 before macOS 26).
    static FROST: RefCell<Option<Retained<AnyObject>>> = const { RefCell::new(None) };
}

/// NSPopUpMenuWindowLevel: above ordinary windows, like the 5.x panel, so it shows in front even when macOS does not
/// let an app that is not active take the focus. Pinned, it floats above ordinary windows instead.
const POPUP_MENU_LEVEL: isize = 101;
const FLOATING_LEVEL: isize = 3;
/// canJoinAllSpaces | transient | ignoresCycle | fullScreenAuxiliary: on every Space and over full-screen apps,
/// and left out of Cmd-` and Mission Control.
const PANEL_BEHAVIOR: usize = 1 | 1 << 3 | 1 << 6 | 1 << 8;

/// A string for AppKit.
fn ns_string(text: &str) -> Retained<AnyObject> {
    let bytes = std::ffi::CString::new(text.replace('\0', "")).unwrap_or_default();
    unsafe { msg_send![class!(NSString), stringWithUTF8String: bytes.as_ptr()] }
}

/// Sets the panel up as the 5.x panel was: a pop-up level window on every Space, with the system glass and a frosted
/// blur under the web view (one shows, or neither for a solid colour the page paints), whose page stays transparent.
pub fn configure_panel(panel: &WebviewWindow) {
    let Ok(window) = panel.ns_window() else { return };
    unsafe {
        let window = window.cast::<AnyObject>();
        let _: () = msg_send![window, setLevel: POPUP_MENU_LEVEL];
        let _: () = msg_send![window, setCollectionBehavior: PANEL_BEHAVIOR];
        let _: () = msg_send![window, setHidesOnDeactivate: false];
        let clear: *mut AnyObject = msg_send![class!(NSColor), clearColor];
        let _: () = msg_send![window, setBackgroundColor: clear];
        let _: () = msg_send![window, setOpaque: false];
        let content: *mut AnyObject = msg_send![window, contentView];
        // The window's shape is what its content leaves opaque; clipping the content to the panel's corners keeps the
        // corners clear even while the web view paints its first frame, so the system shadow and edge follow them.
        let _: () = msg_send![content, setWantsLayer: true];
        let content_layer: *mut AnyObject = msg_send![content, layer];
        if !content_layer.is_null() {
            let _: () = msg_send![content_layer, setCornerRadius: CORNER_RADIUS];
            let _: () = msg_send![content_layer, setMasksToBounds: true];
        }
        let bounds: NSRect = msg_send![content, bounds];
        // Under the web view, its size following the window's (NSWindowBelow; width and height sizable), hidden until
        // the look picks it.
        let place = |view: *mut AnyObject| {
            let _: () = msg_send![view, setAutoresizingMask: 18usize];
            let _: () = msg_send![view, setHidden: true];
            let _: () =
                msg_send![content, addSubview: view, positioned: -1isize, relativeTo: ptr::null_mut::<AnyObject>()];
            Retained::retain(view)
        };
        // Popover material, blended with what is behind the window, always active: the classic frost.
        let frost: *mut AnyObject = msg_send![class!(NSVisualEffectView), alloc];
        let frost: *mut AnyObject = msg_send![frost, initWithFrame: bounds];
        let _: () = msg_send![frost, setMaterial: 6isize];
        let _: () = msg_send![frost, setBlendingMode: 0isize];
        let _: () = msg_send![frost, setState: 1isize];
        let _: () = msg_send![frost, setWantsLayer: true];
        let layer: *mut AnyObject = msg_send![frost, layer];
        let _: () = msg_send![layer, setCornerRadius: CORNER_RADIUS];
        let _: () = msg_send![layer, setMasksToBounds: true];
        let frost = place(frost);
        FROST.with(|view| *view.borrow_mut() = frost);
        if let Some(glass) = AnyClass::get(c"NSGlassEffectView") {
            let effect: *mut AnyObject = msg_send![glass, alloc];
            let effect: *mut AnyObject = msg_send![effect, initWithFrame: bounds];
            // NSGlassEffectViewStyleRegular
            let _: () = msg_send![effect, setStyle: 0isize];
            let _: () = msg_send![effect, setCornerRadius: CORNER_RADIUS];
            let effect = place(effect);
            GLASS.with(|view| *view.borrow_mut() = effect);
        }
    }
}

/// 实色, 磨砂 or 玻璃 (glass falls back to the frost before macOS 26). A solid colour, or the system's Reduce
/// Transparency, shows neither: the web page paints it. Returns whether a material shows.
pub fn set_material(material: &str) -> bool {
    let reduce = reduce_transparency();
    let has_glass = GLASS.with(|view| view.borrow().is_some());
    let glass = !reduce && material == "glass" && has_glass;
    let frost = !reduce && (material == "frosted" || (material == "glass" && !has_glass));
    let show = |slot: &RefCell<Option<Retained<AnyObject>>>, shown: bool| {
        if let Some(view) = slot.borrow().as_ref() {
            unsafe {
                let _: () = msg_send![&**view, setHidden: !shown];
            }
        }
    };
    GLASS.with(|view| show(view, glass));
    FROST.with(|view| show(view, frost));
    glass || frost
}

fn reduce_transparency() -> bool {
    unsafe {
        let workspace: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
        let reduce: Bool = msg_send![workspace, accessibilityDisplayShouldReduceTransparency];
        reduce.as_bool()
    }
}

/// A save dialog is up: the panel drops to the ordinary level so the dialog shows above it, then goes back up.
pub fn step_aside(panel: &WebviewWindow, aside: bool, pinned: bool) {
    let Ok(window) = panel.ns_window() else { return };
    let level = if aside {
        0
    } else if pinned {
        FLOATING_LEVEL
    } else {
        POPUP_MENU_LEVEL
    };
    unsafe {
        let _: () = msg_send![window.cast::<AnyObject>(), setLevel: level];
    }
}

pub fn set_pinned(panel: &WebviewWindow, pinned: bool) {
    PINNED.store(pinned, Ordering::SeqCst);
    let Ok(window) = panel.ns_window() else { return };
    unsafe {
        let _: () =
            msg_send![window.cast::<AnyObject>(), setLevel: if pinned { FLOATING_LEVEL } else { POPUP_MENU_LEVEL }];
    }
}

// MARK: Moving the pinned panel

/// Pinned, the panel answers the first click as a floating tool palette does.
static PINNED: AtomicBool = AtomicBool::new(false);
/// The web view's own acceptsFirstMouse:, asked while the panel is not pinned.
static FIRST_MOUSE: OnceLock<usize> = OnceLock::new();

thread_local! {
    /// The last left press on the panel, and the monitor that keeps it.
    static PRESS: RefCell<Option<Retained<AnyObject>>> = const { RefCell::new(None) };
    static PRESS_MONITOR: RefCell<Option<Retained<AnyObject>>> = const { RefCell::new(None) };
    /// The app has had the release of that press.
    static RELEASED: Cell<bool> = const { Cell::new(true) };
}

unsafe extern "C-unwind" fn accepts_first_mouse(view: *mut AnyObject, selector: Sel, event: *mut AnyObject) -> Bool {
    if PINNED.load(Ordering::SeqCst) {
        return Bool::YES;
    }
    match FIRST_MOUSE.get() {
        Some(&original) => unsafe {
            let original: unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject) -> Bool =
                std::mem::transmute(original);
            original(view, selector, event)
        },
        None => Bool::NO,
    }
}

/// A pinned panel is mostly not the key window (the user works in another app beside it), and a web view lets the
/// first click on such a window only bring it forward: a press on the top strip moved nothing until a second one.
/// Pinned, the web view takes the first click as well, as a floating palette does; unpinned it answers as before.
/// The press on the panel is kept too, for `drag` to move the window from, and whether its release came.
pub fn take_first_clicks(panel: &WebviewWindow) {
    let _ = panel.with_webview(|webview| unsafe {
        let view = webview.inner() as *mut AnyObject;
        if view.is_null() {
            return;
        }
        let class = objc2::ffi::object_getClass(view) as *mut AnyClass;
        let selector = sel!(acceptsFirstMouse:);
        let method = objc2::ffi::class_getInstanceMethod(class, selector);
        if method.is_null() {
            return;
        }
        let Some(original) = objc2::ffi::method_getImplementation(method) else { return };
        if FIRST_MOUSE.set(original as usize).is_err() {
            return;
        }
        let ours: objc2::runtime::Imp = std::mem::transmute(
            accepts_first_mouse as unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject) -> Bool,
        );
        // On the web view's own class: added when it inherits WKWebView's, replaced when it has its own.
        let types = objc2::ffi::method_getTypeEncoding(method);
        if !objc2::ffi::class_addMethod(class, selector, ours, types).as_bool() {
            objc2::ffi::method_setImplementation(method, ours);
        }
    });
    let Ok(window) = panel.ns_window() else { return };
    let window = window as usize;
    // NSEventMaskLeftMouseDown and Up: every left press and release in the app, passed on unchanged; a press is kept
    // when it is on the panel.
    let block = RcBlock::new(move |event: *mut AnyObject| -> *mut AnyObject {
        unsafe {
            let kind: usize = msg_send![event, type];
            let on: *mut AnyObject = msg_send![event, window];
            if kind == 2 {
                RELEASED.set(true);
            } else if on as usize == window {
                PRESS.with(|press| *press.borrow_mut() = Retained::retain(event));
                RELEASED.set(false);
            }
        }
        event
    });
    unsafe {
        let monitor: *mut AnyObject = msg_send![class!(NSEvent), addLocalMonitorForEventsMatchingMask: (1u64 << 1) | (1u64 << 2), handler: &*block];
        PRESS_MONITOR.with(|kept| *kept.borrow_mut() = Retained::retain(monitor));
    }
}

/// Moves the panel with the pointer from the press that asked (the page's top strip), while the button is held. The
/// press is the one AppKit saw, not whatever event is current when the page's request comes in (a drag, or the
/// release after a quick one), which left the window where it was. Released already: nothing to move. False when no
/// press was seen, for the caller's own way.
pub fn drag(panel: &WebviewWindow) -> bool {
    let Ok(window) = panel.ns_window() else { return false };
    let Some(press) = PRESS.with(|press| press.borrow().clone()) else { return false };
    unsafe {
        let held: usize = msg_send![class!(NSEvent), pressedMouseButtons];
        if held & 1 == 0 {
            return true;
        }
        let _: () = msg_send![window.cast::<AnyObject>(), performWindowDragWithEvent: &*press];
    }
    // The window server runs the drag and keeps its release, so WebKit still counts the button held and makes the
    // next press a pointer move, without a pointerdown: the corner, a menu or the strip took the second press after a
    // move. Once the button is up (and the app has had a moment to hear the release itself), the web view gets it.
    let (panel, dragged) = (panel.clone(), Retained::as_ptr(&press) as usize);
    std::thread::spawn(move || {
        while unsafe { CGEventSourceButtonState(0, 0) } {
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
        let target = panel.clone();
        let _ = panel.run_on_main_thread(move || release(&target, dragged));
    });
    true
}

/// Sends the panel the release of the press it was dragged from, where that press was in the window (the window moved
/// with the pointer, so the pointer is there still), unless the app had it or a new press came since.
fn release(panel: &WebviewWindow, dragged: usize) {
    let Some(press) = PRESS.with(|press| press.borrow().clone()) else { return };
    if Retained::as_ptr(&press) as usize != dragged || RELEASED.replace(true) {
        return;
    }
    let Ok(window) = panel.ns_window() else { return };
    unsafe {
        let location: NSPoint = msg_send![&*press, locationInWindow];
        let number: isize = msg_send![&*press, windowNumber];
        let clicks: isize = msg_send![&*press, clickCount];
        if let Some(up) = left_mouse_up(location, number, clicks) {
            let _: () = msg_send![window.cast::<AnyObject>(), sendEvent: &*up];
        }
    }
}

/// A left release at a point of a window, now.
fn left_mouse_up(location: NSPoint, window_number: isize, clicks: isize) -> Option<Retained<AnyObject>> {
    unsafe {
        let process: *mut AnyObject = msg_send![class!(NSProcessInfo), processInfo];
        let now: f64 = msg_send![process, systemUptime];
        // NSEventTypeLeftMouseUp.
        let up: *mut AnyObject = msg_send![class!(NSEvent), mouseEventWithType: 2usize, location: location,
            modifierFlags: 0usize, timestamp: now, windowNumber: window_number, context: ptr::null_mut::<AnyObject>(),
            eventNumber: 0isize, clickCount: clicks, pressure: 0.0f32];
        Retained::retain(up)
    }
}

// MARK: Where the panel goes

/// The pointer, in AppKit's screen coordinates (origin at the bottom left of the main screen).
pub fn mouse_location() -> (f64, f64) {
    unsafe {
        let point: NSPoint = msg_send![class!(NSEvent), mouseLocation];
        (point.x, point.y)
    }
}

/// A point in top-left screen coordinates (Tauri's, logical) in AppKit's, whose origin is the main screen's bottom left.
pub fn from_top_left(x: f64, y: f64) -> (f64, f64) {
    unsafe {
        let screens: Retained<AnyObject> = msg_send![class!(NSScreen), screens];
        let count: usize = msg_send![&*screens, count];
        if count == 0 {
            return (x, y);
        }
        let main: *mut AnyObject = msg_send![&*screens, objectAtIndex: 0usize];
        let frame: NSRect = msg_send![main, frame];
        (x, frame.size.height - y)
    }
}

/// The main screen's top right, for a panel opened at launch before the star has a place.
pub fn top_right() -> (f64, f64) {
    unsafe {
        let screen: *mut AnyObject = msg_send![class!(NSScreen), mainScreen];
        if screen.is_null() {
            return (0.0, 0.0);
        }
        let frame: NSRect = msg_send![screen, visibleFrame];
        (frame.origin.x + frame.size.width - 1.0, frame.origin.y + frame.size.height - 1.0)
    }
}

/// The visible frame (below the menu bar, above the Dock) of the screen under a point, as x, y, width, height.
fn visible_frame(x: f64, y: f64) -> NSRect {
    unsafe {
        let screens: Retained<AnyObject> = msg_send![class!(NSScreen), screens];
        let count: usize = msg_send![&*screens, count];
        let mut fallback: Option<NSRect> = None;
        for index in 0..count {
            let screen: *mut AnyObject = msg_send![&*screens, objectAtIndex: index];
            let frame: NSRect = msg_send![screen, frame];
            let visible: NSRect = msg_send![screen, visibleFrame];
            fallback.get_or_insert(visible);
            let inside = x >= frame.origin.x && x < frame.origin.x + frame.size.width;
            if inside && y >= frame.origin.y && y <= frame.origin.y + frame.size.height {
                return visible;
            }
        }
        fallback.unwrap_or(NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1440.0, 900.0)))
    }
}

/// The pointer is on the star, in the menu bar above the panel's anchor, with no right button down: that click toggles
/// the panel itself (SwiftBar relaunches the app), so losing the focus to it is no reason to close first.
pub fn pointer_on_star(anchor: (f64, f64)) -> bool {
    let (x, y) = mouse_location();
    let visible = visible_frame(x, y);
    let buttons: usize = unsafe { msg_send![class!(NSEvent), pressedMouseButtons] };
    buttons & 2 == 0 && (x - anchor.0).abs() <= 24.0 && y >= visible.origin.y + visible.size.height
}

/// The most the panel may grow on the screen under the anchor before it scrolls.
pub fn max_height(anchor: (f64, f64)) -> f64 {
    visible_frame(anchor.0, anchor.1).size.height - 16.0
}

/// Under the star as the 5.x panel sat: its left edge 22pt left of the star, kept 4pt inside the screen, its top 4pt
/// below the menu bar.
pub fn panel_frame(anchor: (f64, f64), width: f64, height: f64) -> NSRect {
    let visible = visible_frame(anchor.0, anchor.1);
    let height = height.min(visible.size.height - 16.0);
    let x = (anchor.0 - 22.0).max(visible.origin.x + 4.0).min(visible.origin.x + visible.size.width - width - 4.0);
    let top = visible.origin.y + visible.size.height - 4.0;
    NSRect::new(NSPoint::new(x, top - height), NSSize::new(width, height))
}

/// Where a panel the user moved goes at a new size: its top left stays, the window kept on its screen and its bottom
/// above the Dock (it scrolls rather than run off the screen).
pub fn free_frame(current: NSRect, width: f64, height: f64) -> NSRect {
    let visible =
        visible_frame(current.origin.x + current.size.width / 2.0, current.origin.y + current.size.height / 2.0);
    let top = (current.origin.y + current.size.height).min(visible.origin.y + visible.size.height);
    let height = height.min(top - visible.origin.y - 4.0).max(80.0);
    let x = current.origin.x.min(visible.origin.x + visible.size.width - width).max(visible.origin.x);
    NSRect::new(NSPoint::new(x, top - height), NSSize::new(width, height))
}

/// The window's opacity: clear while the page draws an opening, then shown.
pub fn set_alpha(panel: &WebviewWindow, alpha: f64) {
    let Ok(window) = panel.ns_window() else { return };
    unsafe {
        let _: () = msg_send![window.cast::<AnyObject>(), setAlphaValue: alpha];
    }
}

/// The user is dragging one of the window's edges.
pub fn in_live_resize(panel: &WebviewWindow) -> bool {
    let Ok(window) = panel.ns_window() else { return false };
    unsafe {
        let live: Bool = msg_send![window.cast::<AnyObject>(), inLiveResize];
        live.as_bool()
    }
}

/// The window's size while the user drags an edge; None when the shell changed it.
pub fn live_resize_size(panel: &WebviewWindow) -> Option<(f64, f64)> {
    if !in_live_resize(panel) {
        return None;
    }
    frame(panel).map(|frame| (frame.size.width, frame.size.height))
}

/// Sets the window's frame. Within an animation (`last` false) the page draws at the next display cycle and the shadow
/// waits: redrawing at once and working the shadow out again at every step made the resize stutter. The last step
/// draws and settles the shadow.
pub fn set_frame(panel: &WebviewWindow, frame: NSRect, last: bool) {
    let Ok(window) = panel.ns_window() else { return };
    unsafe {
        let _: () = msg_send![window.cast::<AnyObject>(), setFrame: frame, display: last];
        if last {
            let _: () = msg_send![window.cast::<AnyObject>(), invalidateShadow];
        }
    }
}

/// The system draws a borderless window's shadow and edge from the shape it had when last asked; asked again once the
/// panel has drawn, they follow its rounded corners instead of the square it had before.
pub fn reshadow(panel: &WebviewWindow) {
    let Ok(window) = panel.ns_window() else { return };
    unsafe {
        let _: () = msg_send![window.cast::<AnyObject>(), invalidateShadow];
    }
}

pub fn frame(panel: &WebviewWindow) -> Option<NSRect> {
    let window = panel.ns_window().ok()?;
    unsafe { Some(msg_send![window.cast::<AnyObject>(), frame]) }
}

// MARK: The pointer over the panel

/// Told when the pointer is over the panel (true, again as it moves) or leaves it (false).
type HoverHandler = Box<dyn Fn(bool) + Send + Sync>;
static HOVER_HANDLER: OnceLock<HoverHandler> = OnceLock::new();

thread_local! {
    /// The tracking area's owner: the area holds it weakly.
    static HOVER: RefCell<Option<Retained<HoverTracker>>> = const { RefCell::new(None) };
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "StarmeterHoverTracker"]
    struct HoverTracker;

    impl HoverTracker {
        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, _event: *mut AnyObject) {
            if let Some(handler) = HOVER_HANDLER.get() {
                handler(true);
            }
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: *mut AnyObject) {
            if let Some(handler) = HOVER_HANDLER.get() {
                handler(false);
            }
        }
    }
);

/// The web view hears the pointer only while the panel has the focus; a pinned panel mostly does not, and its grabber
/// and resize corner should still show when the pointer comes over it. A tracking area that is always active tells.
pub fn track_hover(panel: &WebviewWindow, handler: impl Fn(bool) + Send + Sync + 'static) {
    let _ = HOVER_HANDLER.set(Box::new(handler));
    let Ok(window) = panel.ns_window() else { return };
    unsafe {
        let content: *mut AnyObject = msg_send![window.cast::<AnyObject>(), contentView];
        if content.is_null() {
            return;
        }
        let owner: Retained<HoverTracker> = msg_send![HoverTracker::class(), new];
        // NSTrackingMouseEnteredAndExited | NSTrackingActiveAlways | NSTrackingInVisibleRect: in and out only. With the
        // pointer's every move as well, AppKit set the cursor again at each one, which with a custom pointer (colour or
        // size in Accessibility) draws it anew with a blur on the main thread, and the panel's animations dropped frames.
        let options: usize = 0x01 | 0x80 | 0x200;
        let area: *mut AnyObject = msg_send![class!(NSTrackingArea), alloc];
        let area: *mut AnyObject = msg_send![area, initWithRect: NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(0.0, 0.0)),
            options: options, owner: &*owner, userInfo: ptr::null_mut::<AnyObject>()];
        if area.is_null() {
            return;
        }
        let _: () = msg_send![content, addTrackingArea: area];
        let _: () = msg_send![area, release];
        HOVER.with(|hover| *hover.borrow_mut() = Some(owner));
    }
}

// MARK: The star

/// What a click on the star does: true for the menu (a right click, or a click with Control held), false for the panel.
type StarHandler = Box<dyn Fn(bool) + Send + Sync>;
static STAR_HANDLER: OnceLock<StarHandler> = OnceLock::new();

thread_local! {
    /// The star's status item and the target its button reports clicks to (a control holds its target weakly).
    static STAR: RefCell<Option<(Retained<AnyObject>, Retained<StarTarget>)>> = const { RefCell::new(None) };
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "StarmeterStarTarget"]
    struct StarTarget;

    impl StarTarget {
        #[unsafe(method(starClicked:))]
        fn star_clicked(&self, _sender: *mut AnyObject) {
            // NSEventTypeRightMouseUp is 4; NSEventModifierFlagControl is 1 << 18.
            let secondary = unsafe {
                let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
                let event: *mut AnyObject = msg_send![app, currentEvent];
                if event.is_null() {
                    false
                } else {
                    let kind: usize = msg_send![event, type];
                    let flags: usize = msg_send![event, modifierFlags];
                    kind == 4 || flags & (1 << 18) != 0
                }
            };
            if let Some(handler) = STAR_HANDLER.get() {
                handler(secondary);
            }
        }
    }
);

/// Takes the star's clicks from the tray library. With a menu attached, a status item opens it on any click (newer
/// macOS gives the click to the item before the library's overlay sees it), so the menu stays detached: the button
/// reports left and right clicks to us, a left click toggles the panel, and the menu is attached only to show it.
pub fn take_over_star(item: usize, handler: impl Fn(bool) + Send + Sync + 'static) {
    let _ = STAR_HANDLER.set(Box::new(handler));
    unsafe {
        let Some(item) = Retained::retain(item as *mut AnyObject) else { return };
        let _: () = msg_send![&*item, setMenu: ptr::null_mut::<AnyObject>()];
        let button: *mut AnyObject = msg_send![&*item, button];
        if button.is_null() {
            return;
        }
        // The library's overlay would otherwise also catch clicks where macOS still hands them to it.
        let subviews: Retained<AnyObject> = msg_send![button, subviews];
        let count: usize = msg_send![&*subviews, count];
        for index in (0..count).rev() {
            let view: *mut AnyObject = msg_send![&*subviews, objectAtIndex: index];
            if !view.is_null() && (*view).class().name() == c"TaoTrayTarget" {
                let _: () = msg_send![view, removeFromSuperview];
            }
        }
        let target: Retained<StarTarget> = msg_send![StarTarget::class(), new];
        let _: () = msg_send![button, setTarget: &*target];
        let _: () = msg_send![button, setAction: sel!(starClicked:)];
        // NSEventMaskLeftMouseUp | NSEventMaskRightMouseUp
        let _: isize = msg_send![button, sendActionOn: (1usize << 2) | (1usize << 4)];
        STAR.with(|star| *star.borrow_mut() = Some((item, target)));
    }
}

/// Opens the menu the tray library just attached, then detaches it again so the next left click opens the panel.
pub fn pop_star_menu() {
    let Some(item) = STAR.with(|star| star.borrow().as_ref().map(|(item, _)| item.clone())) else { return };
    unsafe {
        let button: *mut AnyObject = msg_send![&*item, button];
        if !button.is_null() {
            // Tracks the menu until it closes.
            let _: () = msg_send![button, performClick: ptr::null_mut::<AnyObject>()];
        }
        let _: () = msg_send![&*item, setMenu: ptr::null_mut::<AnyObject>()];
    }
}

// MARK: Focus

pub fn remember_focus() {
    unsafe {
        let workspace: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
        let front: Option<Retained<AnyObject>> = msg_send![workspace, frontmostApplication];
        let front = front.filter(|app| {
            let pid: i32 = msg_send![&**app, processIdentifier];
            pid as u32 != std::process::id()
        });
        PREVIOUS.with(|previous| *previous.borrow_mut() = front);
    }
}

/// Glass and controls draw their inactive, greyer style until the app is active, so the panel activates on open.
pub fn activate() {
    unsafe {
        let shared: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let _: () = msg_send![shared, activateIgnoringOtherApps: true];
    }
}

/// Closed by Esc or the star (or a dialog finished): the app used before gets the focus back.
pub fn restore_focus() {
    let Some(app) = PREVIOUS.with(|previous| previous.borrow_mut().take()) else { return };
    unsafe {
        let shared: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let responds: bool = msg_send![shared, respondsToSelector: objc2::sel!(yieldActivationToApplication:)];
        // macOS 14's cooperative activation: this app yields, then asks the other one forward. (NSRunningApplication's
        // plain `activate` is Swift's shorthand; the message itself is activateWithOptions:.)
        if responds {
            let _: () = msg_send![shared, yieldActivationToApplication: &*app];
        }
        let _: bool = msg_send![&*app, activateWithOptions: 0usize];
    }
}

/// Closed by a click on another app: that app already has the focus.
pub fn forget_focus() {
    PREVIOUS.with(|previous| *previous.borrow_mut() = None);
}

// MARK: Dialogs, links, pasteboard, sharing

/// A question with Cancel as the default button (Return), so a change that replaces something never happens by
/// accident; the other button does it.
pub fn confirm(title: &str, message: &str, proceed: &str) -> bool {
    unsafe {
        activate();
        let alert: Retained<AnyObject> = msg_send![class!(NSAlert), new];
        let _: () = msg_send![&*alert, setMessageText: &*ns_string(title)];
        let _: () = msg_send![&*alert, setInformativeText: &*ns_string(message)];
        let _: *mut AnyObject = msg_send![&*alert, addButtonWithTitle: &*ns_string("取消")];
        let _: *mut AnyObject = msg_send![&*alert, addButtonWithTitle: &*ns_string(proceed)];
        // NSAlertFirstButtonReturn is 1000, the second 1001.
        let answer: isize = msg_send![&*alert, runModal];
        answer == 1001
    }
}

/// A question with a text field: the text typed, or None when cancelled. Hidden for a secret (a Key).
pub fn prompt(title: &str, message: &str, value: &str, hidden: bool) -> Option<String> {
    unsafe {
        activate();
        let alert: Retained<AnyObject> = msg_send![class!(NSAlert), new];
        let _: () = msg_send![&*alert, setMessageText: &*ns_string(title)];
        let _: () = msg_send![&*alert, setInformativeText: &*ns_string(message)];
        let _: *mut AnyObject = msg_send![&*alert, addButtonWithTitle: &*ns_string("保存")];
        let _: *mut AnyObject = msg_send![&*alert, addButtonWithTitle: &*ns_string("取消")];
        let class = if hidden { class!(NSSecureTextField) } else { class!(NSTextField) };
        let field: *mut AnyObject = msg_send![class, alloc];
        let field: *mut AnyObject =
            msg_send![field, initWithFrame: NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(300.0, 24.0))];
        let _: () = msg_send![field, setStringValue: &*ns_string(value)];
        let _: () = msg_send![&*alert, setAccessoryView: field];
        let window: *mut AnyObject = msg_send![&*alert, window];
        let _: () = msg_send![window, setInitialFirstResponder: field];
        let answer: isize = msg_send![&*alert, runModal];
        if answer != 1000 {
            return None;
        }
        let text: Retained<AnyObject> = msg_send![field, stringValue];
        let utf8: *const std::ffi::c_char = msg_send![&*text, UTF8String];
        (!utf8.is_null()).then(|| std::ffi::CStr::from_ptr(utf8).to_string_lossy().into_owned())
    }
}

/// One of two choices, or None when cancelled (the default button).
pub fn choose(title: &str, message: &str, first: &str, second: &str) -> Option<usize> {
    unsafe {
        activate();
        let alert: Retained<AnyObject> = msg_send![class!(NSAlert), new];
        let _: () = msg_send![&*alert, setMessageText: &*ns_string(title)];
        let _: () = msg_send![&*alert, setInformativeText: &*ns_string(message)];
        let _: *mut AnyObject = msg_send![&*alert, addButtonWithTitle: &*ns_string("取消")];
        let _: *mut AnyObject = msg_send![&*alert, addButtonWithTitle: &*ns_string(first)];
        let _: *mut AnyObject = msg_send![&*alert, addButtonWithTitle: &*ns_string(second)];
        match msg_send![&*alert, runModal] {
            1001isize => Some(0),
            1002isize => Some(1),
            _ => None,
        }
    }
}

pub fn alert(title: &str, message: &str) {
    unsafe {
        activate();
        let alert: Retained<AnyObject> = msg_send![class!(NSAlert), new];
        let _: () = msg_send![&*alert, setMessageText: &*ns_string(title)];
        let _: () = msg_send![&*alert, setInformativeText: &*ns_string(message)];
        let _: *mut AnyObject = msg_send![&*alert, addButtonWithTitle: &*ns_string("好")];
        let _: isize = msg_send![&*alert, runModal];
    }
}

/// Opens a link: a web page or an app comes to the front, a swiftbar:// command runs without taking the focus.
pub fn open_url(url: &str, activates: bool) -> bool {
    unsafe {
        let url: Option<Retained<AnyObject>> = msg_send![class!(NSURL), URLWithString: &*ns_string(url)];
        let Some(url) = url else { return false };
        let workspace: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
        let configuration: Retained<AnyObject> = msg_send![class!(NSWorkspaceOpenConfiguration), configuration];
        let _: () = msg_send![&*configuration, setActivates: activates];
        let is_file: bool = msg_send![&*url, isFileURL];
        let none = None::<&Block<dyn Fn(*mut AnyObject, *mut AnyObject)>>;
        if is_file {
            let _: () = msg_send![workspace, openApplicationAtURL: &*url, configuration: &*configuration, completionHandler: none];
        } else {
            let _: () = msg_send![workspace, openURL: &*url, configuration: &*configuration, completionHandler: none];
        }
        true
    }
}

fn ns_data(bytes: &[u8]) -> Retained<AnyObject> {
    unsafe { msg_send![class!(NSData), dataWithBytes: bytes.as_ptr().cast::<std::ffi::c_void>(), length: bytes.len()] }
}

/// PNG for apps that take it, TIFF for older ones; no file URL, so chat apps paste a picture.
pub fn copy_png(png: &[u8]) -> bool {
    unsafe {
        let data = ns_data(png);
        let item: Retained<AnyObject> = msg_send![class!(NSPasteboardItem), new];
        let png_type = ns_string("public.png");
        let ok: bool = msg_send![&*item, setData: &*data, forType: &*png_type];
        let rep: Option<Retained<AnyObject>> = msg_send![class!(NSBitmapImageRep), imageRepWithData: &*data];
        if let Some(rep) = rep {
            let tiff: Option<Retained<AnyObject>> = msg_send![&*rep, TIFFRepresentation];
            if let Some(tiff) = tiff {
                let _: bool = msg_send![&*item, setData: &*tiff, forType: &*ns_string("public.tiff")];
            }
        }
        let board: *mut AnyObject = msg_send![class!(NSPasteboard), generalPasteboard];
        let _: isize = msg_send![board, clearContents];
        let items: Retained<AnyObject> = msg_send![class!(NSArray), arrayWithObject: &*item];
        let written: bool = msg_send![board, writeObjects: &*items];
        ok && written
    }
}

/// The system share menu for a picture file, at a box in the web view (top-left origin, as the page measures it).
pub fn share_file(panel: &WebviewWindow, path: &std::path::Path, rect: (f64, f64, f64, f64)) {
    let Ok(window) = panel.ns_window() else { return };
    unsafe {
        let window = window.cast::<AnyObject>();
        let view: *mut AnyObject = msg_send![window, contentView];
        let bounds: NSRect = msg_send![view, bounds];
        let flipped: bool = msg_send![view, isFlipped];
        let y = if flipped { rect.1 } else { bounds.size.height - rect.1 - rect.3 };
        let anchor = NSRect::new(NSPoint::new(rect.0, y), NSSize::new(rect.2, rect.3));
        let url: Retained<AnyObject> = msg_send![class!(NSURL), fileURLWithPath: &*ns_string(&path.to_string_lossy())];
        let items: Retained<AnyObject> = msg_send![class!(NSArray), arrayWithObject: &*url];
        // The picker stays alive for as long as its menu may be up; one per share is little to keep.
        let picker: *mut AnyObject = msg_send![class!(NSSharingServicePicker), alloc];
        let picker: *mut AnyObject = msg_send![picker, initWithItems: &*items];
        // Below the button: NSMinYEdge in an unflipped view, NSMaxYEdge in a flipped one.
        let edge: usize = if flipped { 3 } else { 1 };
        let _: () = msg_send![picker, showRelativeToRect: anchor, ofView: view, preferredEdge: edge];
    }
}

/// The menu bar figure in even-width digits, so it doesn't jiggle as it changes.
pub fn monospaced_title(button: *mut AnyObject) {
    if button.is_null() {
        return;
    }
    unsafe {
        let size: f64 = msg_send![class!(NSFont), systemFontSize];
        let font: *mut AnyObject = msg_send![class!(NSFont), monospacedDigitSystemFontOfSize: size, weight: 0.0f64];
        let _: () = msg_send![button, setFont: font];
    }
}

// MARK: Notifications

/// UNUserNotificationCenter only exists for an app bundle; a bare binary (a debug run) has none and would crash.
fn bundled() -> bool {
    unsafe {
        let bundle: *mut AnyObject = msg_send![class!(NSBundle), mainBundle];
        let identifier: Option<Retained<AnyObject>> = msg_send![bundle, bundleIdentifier];
        let path: Retained<AnyObject> = msg_send![bundle, bundlePath];
        let suffix: bool = msg_send![&*path, hasSuffix: &*ns_string(".app")];
        identifier.is_some() && suffix
    }
}

fn center() -> Option<Retained<AnyObject>> {
    if !bundled() {
        return None;
    }
    let class = AnyClass::get(c"UNUserNotificationCenter")?;
    unsafe { msg_send![class, currentNotificationCenter] }
}

/// 0 not asked yet, 1 denied, 2 allowed (3 provisional, 4 ephemeral count as allowed); None outside a bundle.
pub fn notification_status() -> Option<isize> {
    let center = center()?;
    let (send, receive) = mpsc::channel::<isize>();
    let block = RcBlock::new(move |settings: *mut AnyObject| {
        let status: isize = unsafe { msg_send![settings, authorizationStatus] };
        let _ = send.send(status);
    });
    unsafe {
        let _: () = msg_send![&*center, getNotificationSettingsWithCompletionHandler: &*block];
    }
    receive.recv_timeout(std::time::Duration::from_secs(2)).ok()
}

/// Asks once; the system remembers the answer and shows no second request.
pub fn request_notifications() {
    let Some(center) = center() else { return };
    let block = RcBlock::new(|_granted: Bool, _error: *mut AnyObject| {});
    unsafe {
        // UNAuthorizationOptionSound | UNAuthorizationOptionAlert
        let _: () = msg_send![&*center, requestAuthorizationWithOptions: (1usize << 1) | (1usize << 2), completionHandler: &*block];
    }
}

pub fn notify(subtitle: &str, body: &str) {
    let Some(center) = center() else { return };
    unsafe {
        let content: Retained<AnyObject> = msg_send![class!(UNMutableNotificationContent), new];
        let _: () = msg_send![&*content, setTitle: &*ns_string("Starmeter")];
        let _: () = msg_send![&*content, setSubtitle: &*ns_string(subtitle)];
        let _: () = msg_send![&*content, setBody: &*ns_string(body)];
        let uuid: Retained<AnyObject> = msg_send![class!(NSUUID), UUID];
        let identifier: Retained<AnyObject> = msg_send![&*uuid, UUIDString];
        let request: *mut AnyObject = msg_send![class!(UNNotificationRequest), requestWithIdentifier: &*identifier, content: &*content,
            trigger: ptr::null_mut::<AnyObject>()];
        let _: () = msg_send![&*center, addNotificationRequest: request, withCompletionHandler: None::<&Block<dyn Fn(*mut AnyObject)>>];
    }
}

/// Runs a closure on the main thread and waits for its answer (for an alert from a command's thread).
pub fn on_main<R: Runtime, T: Send + 'static>(
    app: &AppHandle<R>,
    work: impl FnOnce() -> T + Send + 'static,
) -> Option<T> {
    let main: bool = unsafe { msg_send![class!(NSThread), isMainThread] };
    if main {
        return Some(work());
    }
    let (send, receive) = mpsc::channel();
    app.run_on_main_thread(move || {
        let _ = send.send(work());
    })
    .ok()?;
    receive.recv().ok()
}

/// An SF Symbol as macOS itself draws it, at 3 pixels a point: PNG bytes, or None for a name it doesn't know. The
/// panel's own icons can't copy the symbols' outlines (their licence keeps them to Apple platforms, and the panel ships
/// on Windows too); asking the system for them at run time on a Mac is how they are meant to be used. Only the alpha
/// matters: the panel uses it as a mask in the text colour.
pub fn symbol_png(name: &str, points: f64) -> Option<Vec<u8>> {
    let name = std::ffi::CString::new(name).ok()?;
    unsafe {
        let text: *mut AnyObject = msg_send![class!(NSString), stringWithUTF8String: name.as_ptr()];
        if text.is_null() {
            return None;
        }
        let symbol: *mut AnyObject = msg_send![
            class!(NSImage),
            imageWithSystemSymbolName: text,
            accessibilityDescription: ptr::null_mut::<AnyObject>()
        ];
        if symbol.is_null() {
            return None;
        }
        let configuration: *mut AnyObject =
            msg_send![class!(NSImageSymbolConfiguration), configurationWithPointSize: points * 3.0, weight: 0.0f64];
        let sized: *mut AnyObject = msg_send![symbol, imageWithSymbolConfiguration: configuration];
        if sized.is_null() {
            return None;
        }
        let tiff: *mut AnyObject = msg_send![sized, TIFFRepresentation];
        if tiff.is_null() {
            return None;
        }
        let bitmap: *mut AnyObject = msg_send![class!(NSBitmapImageRep), imageRepWithData: tiff];
        if bitmap.is_null() {
            return None;
        }
        let properties: *mut AnyObject = msg_send![class!(NSDictionary), dictionary];
        // NSBitmapImageFileTypePNG
        let png: *mut AnyObject = msg_send![bitmap, representationUsingType: 4usize, properties: properties];
        if png.is_null() {
            return None;
        }
        let length: usize = msg_send![png, length];
        let bytes: *const u8 = msg_send![png, bytes];
        if bytes.is_null() || length == 0 {
            return None;
        }
        Some(std::slice::from_raw_parts(bytes, length).to_vec())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn draws_the_header_symbols_and_nothing_for_an_unknown_name() {
        for name in
            ["pin", "pin.fill", "gearshape", "square.and.arrow.up", "square.and.arrow.down", "doc.on.doc", "checkmark"]
        {
            let png = super::symbol_png(name, 15.0).unwrap_or_else(|| panic!("{name} is drawn"));
            assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n", "{name} is a PNG");
            if let Ok(folder) = std::env::var("STARMETER_SYMBOLS_OUT") {
                std::fs::write(format!("{folder}/{name}.png"), &png).unwrap();
            }
        }
        assert!(super::symbol_png("no.such.symbol.anywhere", 15.0).is_none());
    }

    /// The release handed to the web view after a window drag is a left mouse-up where the press was, built with the
    /// argument types AppKit declares (objc2 checks them in debug builds).
    #[test]
    fn the_release_after_a_drag_is_a_left_mouse_up_where_the_press_was() {
        let up = super::left_mouse_up(objc2_foundation::NSPoint::new(120.0, 290.0), 0, 1).expect("an event");
        unsafe {
            let kind: usize = objc2::msg_send![&*up, type];
            let at: objc2_foundation::NSPoint = objc2::msg_send![&*up, locationInWindow];
            let clicks: isize = objc2::msg_send![&*up, clickCount];
            assert_eq!((kind, at.x, at.y, clicks), (2, 120.0, 290.0, 1));
        }
    }
}
