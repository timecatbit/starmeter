//! Starmeter's app: the star in the menu bar (the tray on Windows and Linux) and the panel it opens. Everything the
//! panel shows comes through data.rs; this crate owns the window and the system around it.
//!
//! Two ways to run: standalone, with its own star, login item and notifications; or as the SwiftBar plugin's panel
//! (`--swiftbar-panel`, or 5.x's `--snapshot … --plugin …`), where SwiftBar owns the star and each click on it
//! launches or reopens this app, which then has no icon of its own.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod ask;
mod data;
mod engine;
mod fixtures;
mod guide;
mod host;
mod language;
#[cfg(target_os = "macos")]
mod macos;
mod panel;
mod phone;
mod shortcut;
mod style;
mod swiftbar;
mod tunnel;
#[cfg(windows)]
mod win;

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

/// The SwiftBar panel's build keeps 5.x's panel id, so installed plugins (and their self-updates) find it.
const PANEL_IDENTIFIER: &str = "cc.opencodex.swiftbar.panel";

fn request(args: &[String], identifier: &str) -> Request {
    let has = |flag: &str| args.iter().any(|a| a == flag);
    let swiftbar = has("--swiftbar-panel") || (has("--snapshot") && has("--plugin")) || identifier == PANEL_IDENTIFIER;
    Request {
        mode: if swiftbar { Mode::SwiftBar } else { Mode::Standalone },
        modules: has("--modules"),
        show: swiftbar || has("--show-panel") || std::env::var_os("STARMETER_SHOW_PANEL").is_some(),
    }
}

/// Whether this launch opens the panel. On Linux the first launch does too: the tray there (AppIndicator) hands no
/// click to the app, and a desktop may show no tray at all, so a first launch from the app menu showed nothing, the
/// star's menu at most. It opens on the tour instead; once the tour has been shown, a launch (the login item's
/// included) stays in the tray.
fn opens_at_launch(launch: &Request, linux: bool, tour_shown: impl FnOnce() -> bool) -> bool {
    launch.show || (linux && launch.mode == Mode::Standalone && !tour_shown())
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
        // Linux: by the pointer, where the menu was (the tray there doesn't say where the star is).
        "open" => panel::open(app, star_anchor(app, None), false),
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
            // The tick as it now is, failed or not: a menu that ticked itself on the click (Linux's) shows the truth.
            // On macOS, never re-attach NSMenu here: pop_star_menu already detached it, and attaching it again makes
            // left-clicking the star open the right-click menu instead of the panel (R10-1). macOS rebuilds on right-click.
            #[cfg(not(target_os = "macos"))]
            refresh_star_menu(app);
            if result.is_err() {
                let app = app.clone();
                let how = if cfg!(target_os = "macos") {
                    "请将 Starmeter 拖到「应用程序」后重试，或检查系统设置中的登录项。"
                } else if cfg!(windows) {
                    "请在「设置 › 应用 › 启动」里查看 Starmeter 是否被关闭，再重试。"
                } else {
                    "请确认 ~/.config/autostart 文件夹可以写入，再重试。"
                };
                tauri::async_runtime::spawn(async move {
                    host::alert(&app, "操作未完成", how).await;
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

/// The star menu's words, in the panel's language (language.rs). English takes the panel's own words where it has
/// them (en.ts: 主页模块 Home modules, 自动检测 Auto-detect, 更新记录 Changelog).
struct StarLabels {
    open: String,
    modules: String,
    detect: String,
    login: String,
    notices: String,
    about: String,
    changelog: String,
    repository: String,
    quit: String,
}

fn star_labels(language: language::Language, version: &str, notices_answered: bool) -> StarLabels {
    let pick = |chinese: &str, english: &str| match language {
        language::Language::Simplified => chinese.to_string(),
        language::Language::Traditional => starmeter_core::hant::convert(chinese),
        language::Language::English => english.to_string(),
    };
    StarLabels {
        open: pick("打开面板", "Open panel"),
        modules: pick("主页模块…", "Home modules…"),
        detect: pick("自动检测…", "Auto-detect…"),
        login: pick("登录时启动", "Start at login"),
        notices: if notices_answered {
            pick("通知设置…", "Notification settings…")
        } else {
            pick("开启系统通知…", "Turn on notifications…")
        },
        about: pick("关于 Starmeter", "About Starmeter"),
        changelog: pick(&format!("版本 {version} · 更新记录"), &format!("Version {version} · Changelog")),
        repository: pick("作者 timecatbit · GitHub 仓库", "By timecatbit · GitHub repository"),
        quit: pick("退出 Starmeter", "Quit Starmeter"),
    }
}

/// The star's right-click menu: 主页模块…, 自动检测…, the login item and notifications, 关于 Starmeter, 退出, in the
/// panel's language. On Linux it starts with 打开面板: the tray there (AppIndicator) hands no click to the app, and any
/// click opens this menu, so the panel had no way in.
fn star_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let words = star_labels(language::current(), starmeter_core::VERSION, host::notifications_answered());
    let login = CheckMenuItem::with_id(
        app,
        "login",
        words.login,
        true,
        app.autolaunch().is_enabled().unwrap_or(false),
        None::<&str>,
    )?;
    let notices = MenuItem::with_id(app, "notices", words.notices, true, None::<&str>)?;
    let about = Submenu::with_items(
        app,
        words.about,
        true,
        &[
            &MenuItem::with_id(app, "changelog", words.changelog, true, None::<&str>)?,
            &MenuItem::with_id(app, "repository", words.repository, true, None::<&str>)?,
        ],
    )?;
    let open = MenuItem::with_id(app, "open", words.open, true, None::<&str>)?;
    let modules = MenuItem::with_id(app, "modules", words.modules, true, None::<&str>)?;
    let detect = MenuItem::with_id(app, "detect", words.detect, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", words.quit, true, None::<&str>)?;
    let (first, second, third) =
        (PredefinedMenuItem::separator(app)?, PredefinedMenuItem::separator(app)?, PredefinedMenuItem::separator(app)?);
    let mut items: Vec<&dyn tauri::menu::IsMenuItem<tauri::Wry>> = Vec::new();
    if cfg!(target_os = "linux") {
        items.extend([&open as &dyn tauri::menu::IsMenuItem<tauri::Wry>, &first]);
    }
    items.extend([
        &modules as &dyn tauri::menu::IsMenuItem<tauri::Wry>,
        &detect,
        &second,
        &login,
        &notices,
        &about,
        &third,
        &quit,
    ]);
    Menu::with_items(app, &items)
}

/// The star's menu built afresh (the login item's tick, the notifications item, the language); false without a star
/// (SwiftBar's panel) or a menu.
fn refresh_star_menu(app: &AppHandle) -> bool {
    let (Some(tray), Ok(menu)) = (app.tray_by_id("star"), star_menu(app)) else { return false };
    tray.set_menu(Some(menu)).is_ok()
}

fn build_star(app: &AppHandle) -> tauri::Result<()> {
    let star = TrayIconBuilder::with_id("star").icon(tauri::include_image!("icons/tray.png")).icon_as_template(true);
    // A tooltip only where the figure goes in it (Windows, Linux): on a Mac it adds a tracking area to the menu bar
    // button, and AppKit then sets the cursor again at every pointer move over the star, which with a custom pointer
    // (Accessibility) is drawn anew with a blur on the main thread.
    let star = if cfg!(target_os = "macos") { star } else { star.tooltip("Starmeter") };
    star.menu(&star_menu(app)?)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| handle_menu(app, event.id().as_ref()))
        .on_tray_icon_event(|tray, event| {
            let app = tray.app_handle();
            match event {
                TrayIconEvent::Click {
                    button: MouseButton::Left, button_state: MouseButtonState::Up, rect, ..
                } => {
                    panel::toggle(app, star_anchor(app, Some(rect)), false);
                    // The menu brought up to date while it is not on screen (the login item's tick, the notifications
                    // item), for the next right-click; not as it opens (below). Skipped on macOS (R10-1).
                    #[cfg(not(target_os = "macos"))]
                    refresh_star_menu(app);
                }
                // The right-click menu replaces the panel; the menu's own items then decide. It is not built afresh
                // here: on Windows the tray shows its menu itself as the button comes up, and this runs while it shows
                // (the click comes through the app's queue, which the menu's own loop runs): a new menu put in then
                // dropped the one on screen, and it closed at once (用户：windows用户说，右键菜单点不开). It is built
                // afresh when what it says changes: the language, the login item clicked, a left click. (On a Mac the
                // star's clicks are its own, and the menu is built and then shown: take_over_star.)
                TrayIconEvent::Click { button: MouseButton::Right, button_state: MouseButtonState::Down, .. } => {
                    panel::close(app, false);
                }
                _ => {}
            }
        })
        .build(app)?;
    #[cfg(target_os = "macos")]
    take_over_star(app);
    panel::update_star(app);
    Ok(())
}

/// On macOS the star's button reports its own clicks: a status item with a menu attached opens it on any click, so
/// the tray library's left-click handling never sees them on newer systems.
#[cfg(target_os = "macos")]
fn take_over_star(app: &AppHandle) {
    let Some(tray) = app.tray_by_id("star") else { return };
    let item = tray
        .with_inner_tray_icon(|inner| inner.ns_status_item().map(|item| objc2::rc::Retained::as_ptr(&item) as usize));
    let Ok(Some(item)) = item else { return };
    let app = app.clone();
    macos::take_over_star(item, move |secondary| {
        if secondary {
            // The right-click menu replaces the panel; its own items then decide.
            panel::close(&app, false);
            if refresh_star_menu(&app) {
                macos::pop_star_menu();
            }
        } else {
            panel::toggle(&app, star_anchor(&app, None), false);
        }
    });
}

/// The homepage or the module list a launch asks for: the plugin's panel-open.json in SwiftBar's panel (its
/// right-click 主页模块… writes it before opening the app), else the launch's own --modules.
fn requested_modules(app: &AppHandle, args: &[String]) -> Option<bool> {
    if let data::Data::SwiftBar(files) = &*app.state::<data::Data>()
        && let Some(modules) = files.requested_modules()
    {
        return Some(modules);
    }
    args.iter().any(|a| a == "--modules").then_some(true)
}

/// The global shortcut: the panel opens or closes as a click on the star does; SwiftBar's star is out of this app's
/// reach, so its panel opens at the top right.
fn shortcut_pressed(app: &AppHandle) {
    let swiftbar = app.state::<panel::Panel>().mode == Mode::SwiftBar;
    #[cfg(target_os = "macos")]
    let anchor = if swiftbar { macos::top_right() } else { star_anchor(app, None) };
    #[cfg(not(target_os = "macos"))]
    let anchor = if swiftbar { pointer_anchor(app) } else { star_anchor(app, None) };
    panel::toggle(app, anchor, false);
}

/// A second launch (SwiftBar clicked again, or the app opened twice): show or hide the panel it asks for. Open on the
/// same page, it closes; asked for the other page, it switches.
fn relaunched(app: &AppHandle, args: Vec<String>) {
    let swiftbar = app.state::<panel::Panel>().mode == Mode::SwiftBar;
    let anchor = if swiftbar { pointer_anchor(app) } else { star_anchor(app, None) };
    match requested_modules(app, &args) {
        Some(modules) => panel::toggle(app, anchor, modules),
        None if panel::visible(app) => panel::close(app, true),
        None => panel::open(app, anchor, false),
    }
}

/// Where the data comes from: the fixtures when asked (STARMETER_FIXTURE), the plugin's files in SwiftBar's panel,
/// else the core in this process.
fn data_source(app: &AppHandle, args: &[String], mode: Mode) -> data::Data {
    if std::env::var("STARMETER_FIXTURE").is_ok_and(|name| !name.is_empty()) {
        return data::Data::Fixtures(std::sync::Mutex::new(fixtures::Fixtures::load()));
    }
    // SwiftBar's panel never runs the engine itself (it would meet macOS's Documents prompts under this app's name);
    // opened without the plugin's arguments it simply has nothing to show.
    if mode == Mode::SwiftBar {
        let files = std::sync::Arc::new(swiftbar::Files::from_args(args).unwrap_or_else(swiftbar::Files::empty));
        files.watch(app.clone());
        return data::Data::SwiftBar(files);
    }
    let engine = engine::Engine::new(app);
    engine.prepare();
    engine::start_clock(engine.clone());
    data::Data::Engine(engine)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let context = tauri::generate_context!();
    let launch = request(&args, &context.config().identifier);
    let args = std::sync::Arc::new(args);
    let mode = launch.mode;
    #[allow(unused_mut)]
    let mut autostart = tauri_plugin_autostart::Builder::new();
    #[cfg(target_os = "macos")]
    {
        autostart = autostart.macos_launcher(tauri_plugin_autostart::MacosLauncher::LaunchAgent);
    }
    if cfg!(windows) {
        // The login item keeps 5.x's name and argument, so an upgrade finds and replaces it.
        autostart = autostart.app_name("TokenMonitor").args(["--background"]);
    }
    #[cfg(windows)]
    win::claim_identity();
    let mut builder = tauri::Builder::default()
        // The plugin tells from a worker thread on macOS: the panel opens and closes on the main thread, where AppKit
        // and the app it hands the focus back to (kept per thread) are.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            let handle = app.clone();
            let _ = app.run_on_main_thread(move || relaunched(&handle, args));
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(shortcut::plugin(shortcut_pressed));
    if mode == Mode::Standalone {
        builder = builder.plugin(autostart.build());
    }
    #[cfg(not(target_os = "macos"))]
    {
        builder = builder.plugin(tauri_plugin_clipboard_manager::init());
    }
    let app = builder
        .manage(panel::Panel::new(mode))
        .invoke_handler(tauri::generate_handler![
            data::snapshot,
            data::dashboard,
            data::action,
            panel::shell_info,
            panel::close_panel,
            panel::resize_panel,
            panel::set_pinned,
            panel::drag_panel,
            panel::size_panel,
            panel::set_min_height,
            shortcut::set_shortcut,
            guide::guide_shown,
            guide::mark_guide_shown,
            language::set_language,
            style::set_style,
            phone::live_now,
            phone::recent_readings,
            phone::phone_start,
            phone::phone_status,
            phone::phone_stop,
            phone::phone_domain,
            phone::phone_domain_start,
            phone::phone_vpn_start,
            phone::phone_vpn_stop,
            phone::phone_domain_stop,
            phone::phone_stop_near,
            phone::phone_new_link,
            phone::phone_domain_check,
            phone::phone_public,
            phone::phone_private,
            tunnel::tunnel_ready,
            panel::panel_shown,
            panel::panel_menu,
            panel::panel_typing,
            panel::hold_open,
            panel::set_look,
            panel::system_symbol,
            host::open_link,
            host::copy_picture,
            host::share_picture,
            host::save_picture,
            ask::answer_question,
        ])
        .setup(move |app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            let handle = app.handle().clone();
            app.manage(data_source(&handle, &args, mode));
            app.manage(phone::Phone::default());
            // The last minutes of live readings, from the start: the panel's 实时 and its pinned line, and a phone's page,
            // open with them.
            phone::keep_recording(handle.clone());
            // 在手机上看 as the user left it: on again if it was on (an update too), off otherwise. Off the main thread:
            // 自有域名's held port, still taken by something else at login, is asked again for a moment before giving up.
            {
                let handle = handle.clone();
                std::thread::spawn(move || phone::resume(&handle));
            }
            // A check of the phone view without the panel: STARMETER_PHONE starts it at launch and prints where.
            // STARMETER_PHONE=public opens the public link too, as 任何网络 does after the user's yes.
            if let Some(which) = std::env::var_os("STARMETER_PHONE") {
                match phone::start_local(handle.clone()) {
                    Ok(view) => eprintln!("phone: {}", serde_json::to_string(&view).unwrap_or_default()),
                    Err(error) => eprintln!("phone: {error}"),
                }
                if which == "public" {
                    let public = handle.clone();
                    tauri::async_runtime::spawn(async move {
                        match phone::phone_public(public).await {
                            Ok(view) => eprintln!("phone public: {}", serde_json::to_string(&view).unwrap_or_default()),
                            Err(error) => eprintln!("phone public: {error}"),
                        }
                    });
                }
            }
            if mode == Mode::Standalone {
                build_star(&handle)?;
                host::ask_for_notifications(&handle);
            }
            let window = app.get_webview_window(PANEL).expect("the panel window is configured");
            #[cfg(target_os = "macos")]
            {
                macos::configure_panel(&window);
                macos::take_first_clicks(&window);
                let hover = handle.clone();
                macos::track_hover(&window, move |inside| panel::hovered(&hover, inside));
            }
            let _ = window;
            if opens_at_launch(&launch, cfg!(target_os = "linux"), guide::guide_shown) {
                let anchor = if mode == Mode::SwiftBar { pointer_anchor(&handle) } else { star_anchor(&handle, None) };
                let modules = requested_modules(&handle, &args).unwrap_or(launch.modules);
                panel::open(&handle, anchor, modules);
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
                WindowEvent::Resized(_) => panel::resized(window.app_handle()),
                WindowEvent::CloseRequested { api, .. } => {
                    api.prevent_close();
                    panel::close(window.app_handle(), true);
                }
                _ => {}
            }
        })
        .build(context)
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
        } else if let RunEvent::Exit = event {
            // Quitting stops the phone view, and cloudflared with it.
            phone::phone_quit(app);
        }
        let _ = app;
    });
}

#[cfg(test)]
mod tests {
    use super::{Mode, opens_at_launch, request, star_labels};

    #[test]
    fn on_linux_the_first_launch_opens_the_panel_and_later_ones_stay_in_the_tray() {
        let plain = request(&["Starmeter".to_string()], "com.timecatbit.starmeter");
        assert!(opens_at_launch(&plain, true, || false), "Linux, the tour not shown yet");
        assert!(!opens_at_launch(&plain, true, || true), "Linux, the tour shown");
        assert!(!opens_at_launch(&plain, false, || false), "macOS and Windows: the star is there to click");
        let swiftbar = request(&["Starmeter".to_string(), "--swiftbar-panel".to_string()], "com.timecatbit.starmeter");
        assert!(swiftbar.mode == Mode::SwiftBar);
        assert!(opens_at_launch(&swiftbar, false, || true), "SwiftBar's click always opens it");
        assert!(opens_at_launch(&swiftbar, true, || panic!("not asked: SwiftBar's panel opens anyway")));
    }

    #[test]
    fn the_star_menu_speaks_the_panels_language() {
        use super::language::Language;
        let all = |language, answered| {
            let words = star_labels(language, "6.0.8", answered);
            [
                words.open,
                words.modules,
                words.detect,
                words.login,
                words.notices,
                words.about,
                words.changelog,
                words.repository,
                words.quit,
            ]
        };
        assert_eq!(
            all(Language::Simplified, true),
            [
                "打开面板",
                "主页模块…",
                "自动检测…",
                "登录时启动",
                "通知设置…",
                "关于 Starmeter",
                "版本 6.0.8 · 更新记录",
                "作者 timecatbit · GitHub 仓库",
                "退出 Starmeter",
            ]
        );
        assert_eq!(
            all(Language::Traditional, true),
            [
                "打開面板",
                "主頁模塊…",
                "自動檢測…",
                "登錄時啟動",
                "通知設置…",
                "關於 Starmeter",
                "版本 6.0.8 · 更新記錄",
                "作者 timecatbit · GitHub 倉庫",
                "退出 Starmeter",
            ]
        );
        assert_eq!(
            all(Language::English, true),
            [
                "Open panel",
                "Home modules…",
                "Auto-detect…",
                "Start at login",
                "Notification settings…",
                "About Starmeter",
                "Version 6.0.8 · Changelog",
                "By timecatbit · GitHub repository",
                "Quit Starmeter",
            ]
        );
        assert_eq!(star_labels(Language::Simplified, "6.0.8", false).notices, "开启系统通知…");
        assert_eq!(star_labels(Language::Traditional, "6.0.8", false).notices, "開啟系統通知…");
        assert_eq!(star_labels(Language::English, "6.0.8", false).notices, "Turn on notifications…");
        // English shows no Chinese at all.
        let chinese = |text: &str| {
            text.chars().any(|c| ('\u{3000}'..='\u{9fff}').contains(&c) || ('\u{ff00}'..='\u{ffef}').contains(&c))
        };
        for answered in [false, true] {
            assert!(
                all(Language::English, answered).iter().all(|text| !chinese(text)),
                "{:?}",
                all(Language::English, answered)
            );
        }
    }
}
