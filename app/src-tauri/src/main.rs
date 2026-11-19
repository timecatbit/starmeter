//! Starmeter's app: the star in the menu bar (the tray on Windows and Linux) and the panel it opens. Everything the
//! panel shows comes through data.rs; this crate owns the window and the system around it.
//!
//! Two ways to run: standalone, with its own star, login item and notifications; or as the SwiftBar plugin's panel
//! (`--swiftbar-panel`, or 5.x's `--snapshot … --plugin …`), where SwiftBar owns the star and each click on it
//! launches or reopens this app, which then has no icon of its own.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod data;
mod fixtures;
mod host;
#[cfg(target_os = "macos")]
mod macos;
mod panel;

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, RunEvent, WindowEvent};
use tauri_plugin_autostart::ManagerExt;

use panel::{Mode, PANEL};

const REPOSITORY: &str = "https://github.com/timecatbit/starmeter";

/// What a launch (or a second launch, passed on by the running app) asks for.
struct Request {
    mode: Mode,
    modules: bool,
    show: bool,
}

fn request(args: &[String]) -> Request {
    let has = |flag: &str| args.iter().any(|a| a == flag);
    let swiftbar = has("--swiftbar-panel") || (has("--snapshot") && has("--plugin"));
    Request {
        mode: if swiftbar { Mode::SwiftBar } else { Mode::Standalone },
        modules: has("--modules"),
        show: swiftbar || has("--show-panel") || std::env::var_os("STARMETER_SHOW_PANEL").is_some(),
    }
}

/// Where the panel opens without a star click: under the pointer (SwiftBar's star is where the user clicked), or at
/// the top right for a launch check.
fn pointer_anchor(app: &AppHandle) -> (f64, f64) {
    #[cfg(target_os = "macos")]
    {
        let _ = app;
        macos::mouse_location()
    }
    #[cfg(not(target_os = "macos"))]
    {
        app.cursor_position()
            .ok()
            .zip(app.primary_monitor().ok().flatten())
            .map(|(p, m)| (p.x / m.scale_factor(), p.y / m.scale_factor()))
            .unwrap_or((0.0, 0.0))
    }
}

/// The star's position: its middle, logical (macOS: AppKit's coordinates, which share x with the tray's).
fn star_anchor(app: &AppHandle, rect: Option<tauri::Rect>) -> (f64, f64) {
    let rect = rect.or_else(|| app.tray_by_id("star").and_then(|tray| tray.rect().ok().flatten()));
    let scale = app.primary_monitor().ok().flatten().map(|m| m.scale_factor()).unwrap_or(1.0);
    let Some(rect) = rect else { return pointer_anchor(app) };
    let position = rect.position.to_logical::<f64>(scale);
    let size = rect.size.to_logical::<f64>(scale);
    // Right after launch the status item isn't laid out yet; a launch check opens at the top right instead.
    if size.width < 1.0 || size.height < 1.0 {
        #[cfg(target_os = "macos")]
        return macos::top_right();
        #[cfg(not(target_os = "macos"))]
        return pointer_anchor(app);
    }
    #[cfg(target_os = "macos")]
    {
        // The star's middle, on the screen whose menu bar holds it.
        macos::from_top_left(position.x + size.width / 2.0, position.y + size.height / 2.0)
    }
    #[cfg(not(target_os = "macos"))]
    {
        (position.x + size.width / 2.0, position.y + size.height / 2.0)
    }
}

fn handle_menu(app: &AppHandle, id: &str) {
    match id {
        "modules" => panel::open(app, star_anchor(app, None), true),
        "detect" => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                data::run(&app, vec!["detect-modules".into()]).await;
            });
        }
        "login" => {
            let launcher = app.autolaunch();
            let enabled = launcher.is_enabled().unwrap_or(false);
            let result = if enabled { launcher.disable() } else { launcher.enable() };
            if result.is_err() {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    host::alert(
                        &app,
                        "操作未完成",
                        "请将 Starmeter 拖到「应用程序」后重试，或检查系统设置中的登录项。",
                    )
                    .await;
                });
            }
        }
        "notices" => host::notification_settings(app),
        "changelog" => open_page(app, &format!("{REPOSITORY}/blob/main/CHANGELOG.md")),
        "repository" => open_page(app, REPOSITORY),
        "quit" => app.exit(0),
        _ => {}
    }
}

fn open_page(app: &AppHandle, url: &str) {
    host::open_link(app.clone(), url.to_owned());
}

/// The star's right-click menu: 主页模块…, 自动检测…, the login item and notifications, 关于 Starmeter, 退出.
fn star_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let version = starmeter_core::VERSION;
    let login = CheckMenuItem::with_id(
        app,
        "login",
        "登录时启动",
        true,
        app.autolaunch().is_enabled().unwrap_or(false),
        None::<&str>,
    )?;
    let notices = MenuItem::with_id(
        app,
        "notices",
        if host::notifications_answered() { "通知设置…" } else { "开启系统通知…" },
        true,
        None::<&str>,
    )?;
    let about = Submenu::with_items(
        app,
        "关于 Starmeter",
        true,
        &[
            &MenuItem::with_id(app, "changelog", format!("版本 {version} · 更新记录"), true, None::<&str>)?,
            &MenuItem::with_id(app, "repository", "作者 minicoke · GitHub 仓库", true, None::<&str>)?,
        ],
    )?;
    Menu::with_items(
        app,
        &[
            &MenuItem::with_id(app, "modules", "主页模块…", true, None::<&str>)?,
            &MenuItem::with_id(app, "detect", "自动检测…", true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &login,
            &notices,
            &about,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "quit", "退出 Starmeter", true, None::<&str>)?,
        ],
    )
}

fn build_star(app: &AppHandle) -> tauri::Result<()> {
    TrayIconBuilder::with_id("star")
        .icon(tauri::include_image!("icons/tray.png"))
        .icon_as_template(true)
        .tooltip("Starmeter")
        .menu(&star_menu(app)?)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| handle_menu(app, event.id().as_ref()))
        .on_tray_icon_event(|tray, event| {
            let app = tray.app_handle();
            match event {
                TrayIconEvent::Click {
                    button: MouseButton::Left, button_state: MouseButtonState::Up, rect, ..
                } => {
                    panel::toggle(app, star_anchor(app, Some(rect)), false);
                }
                // The right-click menu replaces the panel; the menu's own items then decide.
                TrayIconEvent::Click { button: MouseButton::Right, button_state: MouseButtonState::Down, .. } => {
                    panel::close(app, false);
                    if let Ok(menu) = star_menu(app) {
                        let _ = tray.set_menu(Some(menu));
                    }
                }
                _ => {}
            }
        })
        .build(app)?;
    panel::update_star(app);
    Ok(())
}

/// A second launch (SwiftBar clicked again, or the app opened twice): show or hide the panel it asks for.
fn relaunched(app: &AppHandle, args: Vec<String>) {
    let asked = request(&args);
    let anchor =
        if app.state::<panel::Panel>().mode == Mode::SwiftBar { pointer_anchor(app) } else { star_anchor(app, None) };
    panel::toggle(app, anchor, asked.modules);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let launch = request(&args);
    let mode = launch.mode;
    #[allow(unused_mut)]
    let mut autostart = tauri_plugin_autostart::Builder::new();
    #[cfg(target_os = "macos")]
    {
        autostart = autostart.macos_launcher(tauri_plugin_autostart::MacosLauncher::LaunchAgent);
    }
    if cfg!(windows) {
        // The login item keeps 5.x's name, so an upgrade finds and replaces it.
        autostart = autostart.app_name("TokenMonitor");
    }
    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| relaunched(app, args)))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init());
    if mode == Mode::Standalone {
        builder = builder.plugin(autostart.build());
    }
    #[cfg(not(target_os = "macos"))]
    {
        builder = builder.plugin(tauri_plugin_clipboard_manager::init());
    }
    let app = builder
        .manage(panel::Panel::new(mode))
        .manage(data::Data::new())
        .invoke_handler(tauri::generate_handler![
            data::snapshot,
            data::dashboard,
            data::action,
            panel::shell_info,
            panel::close_panel,
            panel::resize_panel,
            panel::set_pinned,
            panel::hold_open,
            panel::set_look,
            host::open_link,
            host::copy_picture,
            host::share_picture,
        ])
        .setup(move |app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            let handle = app.handle().clone();
            if mode == Mode::Standalone {
                build_star(&handle)?;
                host::ask_for_notifications(&handle);
            }
            let window = app.get_webview_window(PANEL).expect("the panel window is configured");
            #[cfg(target_os = "macos")]
            macos::configure_panel(&window);
            let _ = window;
            if launch.show {
                let anchor = if mode == Mode::SwiftBar { pointer_anchor(&handle) } else { star_anchor(&handle, None) };
                panel::open(&handle, anchor, launch.modules);
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() != PANEL {
                return;
            }
            match event {
                // A click outside closes the panel; the app clicked keeps the focus.
                WindowEvent::Focused(false) => panel::blurred(window.app_handle()),
                WindowEvent::Focused(true) => panel::focused(window.app_handle()),
                WindowEvent::CloseRequested { api, .. } => {
                    api.prevent_close();
                    panel::close(window.app_handle(), true);
                }
                _ => {}
            }
        })
        .build(tauri::generate_context!())
        .expect("Starmeter could not start");
    app.run(|app, event| {
        // Opening the app again (Finder, Spotlight, SwiftBar's `open -a`) shows the panel.
        #[cfg(target_os = "macos")]
        if let RunEvent::Reopen { .. } = event {
            relaunched(app, Vec::new());
        }
        if let RunEvent::ExitRequested { api, code: None, .. } = event {
            // Hiding the last window is not quitting: the star stays.
            api.prevent_exit();
        }
        let _ = app;
    });
}
