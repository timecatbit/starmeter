//! The system around the panel: confirmation and save dialogs, links, the share picture on the clipboard or in the
//! share menu, and notifications. macOS goes through AppKit (macos.rs); Windows and Linux through Tauri's plugins.

use std::path::PathBuf;

use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;
#[cfg(not(target_os = "macos"))]
use tauri_plugin_dialog::{MessageDialogButtons, MessageDialogKind, MessageDialogResult};

#[cfg(target_os = "macos")]
use crate::macos;
use crate::panel;

/// A question whose default answer is no: the panel's 自动检测, removing a Key, deleting the archive.
pub async fn confirm(app: &AppHandle, title: &str, message: &str, proceed: &str) -> bool {
    let (app, title, message, proceed) = (app.clone(), title.to_owned(), message.to_owned(), proceed.to_owned());
    tauri::async_runtime::spawn_blocking(move || {
        #[cfg(target_os = "macos")]
        {
            macos::on_main(&app, move || macos::confirm(&title, &message, &proceed)).unwrap_or(false)
        }
        #[cfg(not(target_os = "macos"))]
        {
            let answer = app
                .dialog()
                .message(message)
                .title(title)
                .kind(MessageDialogKind::Warning)
                .buttons(MessageDialogButtons::OkCancelCustom(proceed.clone(), "取消".into()))
                .blocking_show_with_result();
            matches!(answer, MessageDialogResult::Ok)
                || matches!(answer, MessageDialogResult::Custom(ref label) if *label == proceed)
        }
    })
    .await
    .unwrap_or(false)
}

pub async fn alert(app: &AppHandle, title: &str, message: &str) {
    let (app, title, message) = (app.clone(), title.to_owned(), message.to_owned());
    let _ = tauri::async_runtime::spawn_blocking(move || {
        #[cfg(target_os = "macos")]
        macos::on_main(&app, move || macos::alert(&title, &message));
        #[cfg(not(target_os = "macos"))]
        app.dialog().message(message).title(title).blocking_show();
    })
    .await;
}

/// Where an export or a picture goes: a file name picked in the system's save dialog, on the desktop by default.
pub async fn save_file(app: &AppHandle, name: &str, extension: &str) -> Option<PathBuf> {
    let (app, name, extension) = (app.clone(), name.to_owned(), extension.to_owned());
    tauri::async_runtime::spawn_blocking(move || {
        #[cfg(target_os = "macos")]
        macos::activate();
        let mut dialog =
            app.dialog().file().set_file_name(&name).add_filter(extension.to_uppercase(), &[extension.as_str()]);
        if let Ok(desktop) = app.path().desktop_dir() {
            dialog = dialog.set_directory(desktop);
        }
        dialog.blocking_save_file().and_then(|path| path.into_path().ok())
    })
    .await
    .ok()
    .flatten()
}

/// Opens a row's link: a web page comes to the front, an installed app opens to update itself, a swiftbar:// command
/// runs without taking the focus from the app the user was in.
#[tauri::command]
pub fn open_link(app: AppHandle, href: String) {
    let quiet = href.starts_with("swiftbar:");
    if !(href.starts_with("http:") || href.starts_with("https:") || href.starts_with("file:") || quiet) {
        return;
    }
    panel::close(&app, quiet);
    #[cfg(target_os = "macos")]
    {
        let _ = app.run_on_main_thread(move || {
            macos::open_url(&href, !quiet);
        });
    }
    #[cfg(not(target_os = "macos"))]
    {
        use tauri_plugin_opener::OpenerExt;
        let _ = app.opener().open_url(href, None::<&str>);
    }
}

fn decode(text: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let (mut buffer, mut bits) = (0u32, 0u32);
    for c in text.bytes() {
        let value = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => continue,
        } as u32;
        buffer = buffer << 6 | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
        }
    }
    out
}

/// The share picture on the clipboard: PNG and TIFF on macOS, the image itself elsewhere; never a file path.
#[tauri::command]
pub fn copy_picture(app: AppHandle, png: String) -> bool {
    let bytes = decode(&png);
    #[cfg(target_os = "macos")]
    {
        macos::on_main(&app, move || macos::copy_png(&bytes)).unwrap_or(false)
    }
    #[cfg(not(target_os = "macos"))]
    {
        use tauri_plugin_clipboard_manager::ClipboardExt;
        tauri::image::Image::from_bytes(&bytes).is_ok_and(|image| app.clipboard().write_image(&image).is_ok())
    }
}

/// 分享…: the system share menu at the share button (macOS), or 保存图片… elsewhere.
#[tauri::command]
pub async fn share_picture(app: AppHandle, png: String, name: String, x: f64, y: f64, width: f64, height: f64) {
    let bytes = decode(&png);
    #[cfg(target_os = "macos")]
    {
        let path = std::env::temp_dir().join(name.replace('/', "-"));
        if std::fs::write(&path, &bytes).is_err() {
            return;
        }
        // The share sheet draws in another process; its clicks must not close the panel.
        panel::hold_for_share(&app);
        let _ = (x, y, width, height);
        let window = app.get_webview_window(panel::PANEL);
        let _ = app.run_on_main_thread(move || {
            if let Some(window) = window {
                macos::share_file(&window, &path, (x, y, width, height));
            }
        });
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (x, y, width, height);
        if let Some(path) = save_file(&app, &name, "png").await {
            let _ = std::fs::write(path, bytes);
        }
    }
}

// MARK: Notifications

/// Standalone, the app asks once at its first launch; the answer stays with the system.
pub fn ask_for_notifications(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    {
        let _ = app;
        std::thread::spawn(|| {
            if macos::notification_status() == Some(0) {
                macos::request_notifications();
            }
        });
    }
    #[cfg(not(target_os = "macos"))]
    {
        use tauri_plugin_notification::{NotificationExt, PermissionState};
        if !matches!(app.notification().permission_state(), Ok(PermissionState::Granted)) {
            let _ = app.notification().request_permission();
        }
    }
}

/// 开启系统通知… before the user answered, 通知设置… after.
pub fn notifications_answered() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos::notification_status().is_some_and(|status| status != 0)
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

/// Not asked yet: ask. Answered: the system shows no second request, so open this app's notification settings.
pub fn notification_settings(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    {
        let identifier = app.config().identifier.clone();
        std::thread::spawn(move || match macos::notification_status() {
            Some(0) => macos::request_notifications(),
            _ => {
                macos::open_url(
                    &format!("x-apple.systempreferences:com.apple.preference.notifications?id={identifier}"),
                    true,
                );
            }
        });
    }
    #[cfg(not(target_os = "macos"))]
    {
        use tauri_plugin_opener::OpenerExt;
        let _ = app.opener().open_url("ms-settings:notifications", None::<&str>);
    }
}

/// A notice from the engine (a usage, balance or reset alert): title Starmeter, a subtitle saying which kind.
#[allow(dead_code)]
pub fn notify(app: &AppHandle, subtitle: &str, body: &str) {
    #[cfg(target_os = "macos")]
    {
        let _ = app;
        macos::notify(subtitle, body);
    }
    #[cfg(not(target_os = "macos"))]
    {
        use tauri_plugin_notification::NotificationExt;
        let _ = app.notification().builder().title("Starmeter").body(format!("{subtitle} · {body}")).show();
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn decodes_base64() {
        assert_eq!(super::decode("TWFu"), b"Man");
        assert_eq!(super::decode("TWE="), b"Ma");
    }
}
