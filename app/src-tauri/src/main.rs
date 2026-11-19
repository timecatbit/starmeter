//! Starmeter's app: the star in the menu bar (the tray on Windows and Linux) and the panel it opens. Everything
//! the panel shows comes from starmeter-core; this crate only owns the window and the system around it.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(target_os = "macos")]
mod macos;

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, WebviewWindow, WindowEvent};
use tauri_plugin_positioner::{Position, WindowExt};

const PANEL: &str = "panel";

#[tauri::command]
fn version() -> &'static str {
    starmeter_core::VERSION
}

/// Esc or a click on the star: close the panel and give the focus back to the app used before it.
#[tauri::command]
fn close_panel(app: AppHandle) {
    if let Some(panel) = app.get_webview_window(PANEL) {
        let _ = panel.hide();
    }
    #[cfg(target_os = "macos")]
    macos::restore_focus();
}

fn open_panel(panel: &WebviewWindow, near_star: bool) {
    #[cfg(target_os = "macos")]
    macos::remember_focus();
    let _ = panel.move_window(if near_star { Position::TrayBottomCenter } else { Position::TopRight });
    let _ = panel.show();
    let _ = panel.set_focus();
}

fn toggle(app: &AppHandle) {
    let Some(panel) = app.get_webview_window(PANEL) else { return };
    if panel.is_visible().unwrap_or(false) {
        close_panel(app.clone());
    } else {
        open_panel(&panel, true);
    }
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_positioner::init())
        .invoke_handler(tauri::generate_handler![version, close_panel])
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            let quit = MenuItem::with_id(app, "quit", "退出 Starmeter", true, None::<&str>)?;
            TrayIconBuilder::with_id("star")
                .icon(tauri::include_image!("icons/tray.png"))
                .icon_as_template(true)
                .tooltip("Starmeter")
                .menu(&Menu::with_items(app, &[&quit])?)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| {
                    if event.id() == "quit" {
                        app.exit(0);
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    tauri_plugin_positioner::on_tray_event(tray.app_handle(), &event);
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left, button_state: MouseButtonState::Up, ..
                    } = event
                    {
                        toggle(tray.app_handle());
                    }
                })
                .build(app)?;
            let panel = app.get_webview_window(PANEL).expect("the panel window is configured");
            #[cfg(target_os = "macos")]
            macos::configure_panel(&panel);
            // For checks without a click: STARMETER_SHOW_PANEL=1 opens the panel at launch.
            if std::env::var_os("STARMETER_SHOW_PANEL").is_some() {
                open_panel(&panel, false);
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // A click outside closes the panel; the app clicked keeps the focus.
            if let WindowEvent::Focused(false) = event
                && window.label() == PANEL
                && window.is_visible().unwrap_or(false)
            {
                let _ = window.hide();
                #[cfg(target_os = "macos")]
                macos::forget_focus();
            }
        })
        .run(tauri::generate_context!())
        .expect("Starmeter could not start");
}
