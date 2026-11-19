//! macOS pieces Tauri has no API for: the system glass under the panel, and handing the focus back to the app
//! that had it before the panel opened.

use std::cell::RefCell;
use std::ptr;

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2::{class, msg_send};
use objc2_foundation::NSRect;
use tauri::WebviewWindow;

const CORNER_RADIUS: f64 = 13.0;

thread_local! {
    /// The app that was in front when the panel opened.
    static PREVIOUS: RefCell<Option<Retained<AnyObject>>> = const { RefCell::new(None) };
}

/// NSPopUpMenuWindowLevel: above ordinary windows, like the 5.x panel, so it shows in front even when macOS
/// does not let an app that is not active take the focus.
const POPUP_MENU_LEVEL: isize = 101;
/// canJoinAllSpaces | transient | ignoresCycle | fullScreenAuxiliary: on every Space and over full-screen apps,
/// and left out of Cmd-` and Mission Control.
const PANEL_BEHAVIOR: usize = 1 | 1 << 3 | 1 << 6 | 1 << 8;

/// Sets the panel up as the 5.x panel was: a pop-up level window on every Space, with the system glass (macOS
/// 26's regular glass, an earlier system's popover material) under the web view, whose page stays transparent.
pub fn configure_panel(panel: &WebviewWindow) {
    let Ok(window) = panel.ns_window() else { return };
    unsafe {
        let window = window.cast::<AnyObject>();
        let _: () = msg_send![window, setLevel: POPUP_MENU_LEVEL];
        let _: () = msg_send![window, setCollectionBehavior: PANEL_BEHAVIOR];
        let _: () = msg_send![window, setHidesOnDeactivate: false];
        let content: *mut AnyObject = msg_send![window, contentView];
        let bounds: NSRect = msg_send![content, bounds];
        let effect: *mut AnyObject = match AnyClass::get(c"NSGlassEffectView") {
            Some(glass) => {
                let effect: *mut AnyObject = msg_send![glass, alloc];
                let effect: *mut AnyObject = msg_send![effect, initWithFrame: bounds];
                // NSGlassEffectViewStyleRegular
                let _: () = msg_send![effect, setStyle: 0isize];
                let _: () = msg_send![effect, setCornerRadius: CORNER_RADIUS];
                effect
            }
            None => {
                let effect: *mut AnyObject = msg_send![class!(NSVisualEffectView), alloc];
                let effect: *mut AnyObject = msg_send![effect, initWithFrame: bounds];
                // Popover material, blended with what is behind the window, always active.
                let _: () = msg_send![effect, setMaterial: 6isize];
                let _: () = msg_send![effect, setBlendingMode: 0isize];
                let _: () = msg_send![effect, setState: 1isize];
                let _: () = msg_send![effect, setWantsLayer: true];
                let layer: *mut AnyObject = msg_send![effect, layer];
                let _: () = msg_send![layer, setCornerRadius: CORNER_RADIUS];
                let _: () = msg_send![layer, setMasksToBounds: true];
                effect
            }
        };
        // Width and height follow the window.
        let _: () = msg_send![effect, setAutoresizingMask: 18usize];
        // NSWindowBelow: under the web view.
        let _: () =
            msg_send![content, addSubview: effect, positioned: -1isize, relativeTo: ptr::null_mut::<AnyObject>()];
    }
}

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

/// Closed by Esc or the star: the app used before gets the focus back.
pub fn restore_focus() {
    let Some(app) = PREVIOUS.with(|previous| previous.borrow_mut().take()) else { return };
    unsafe {
        let shared: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let responds: bool = msg_send![shared, respondsToSelector: objc2::sel!(yieldActivationToApplication:)];
        if responds {
            let _: () = msg_send![shared, yieldActivationToApplication: &*app];
            let _: bool = msg_send![&*app, activate];
        } else {
            let _: bool = msg_send![&*app, activateWithOptions: 0usize];
        }
    }
}

/// Closed by a click on another app: that app already has the focus.
pub fn forget_focus() {
    PREVIOUS.with(|previous| *previous.borrow_mut() = None);
}
