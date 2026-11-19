//! The panel's data: the one boundary between the web UI and the engine.
//!
//! The UI (app/ui/src/lib/backend.ts) calls three commands and listens for one event:
//!
//! - `snapshot() -> PanelSnapshot`: everything the panel shows, in the 5.x engine's JSON (PanelSnapshot.swift):
//!   `entries` (the homepage rows), `moduleOptions` (the module grid and 外观与设置), `rangeOptions`, `dashboard` (the
//!   Summary, chart, heatmap and cards of the selected range, DashboardPayload.swift), `preferences` (the display
//!   preferences with their revision), `appearance`, `theme`, `pinInterval`, `menuBarText`, `updateCount`, `project`.
//!   The types are written out in app/ui/src/lib/types.ts.
//! - `dashboard(range) -> Dashboard`: the dashboard of one range, without changing the selected one.
//! - `action(action: [String]) -> Receipt`: one of the panel's actions, named as the 5.x engine names them (set-range,
//!   set-module, refresh-live, test-api-key, export-usage, …). The receipt says done, failed (with a message) or
//!   cancelled (a dialog was dismissed), and carries the saved display preferences when it changed them, so an older
//!   snapshot never undoes a newer choice. The UI shows a choice at once and reloads the snapshot after the receipt.
//! - event `snapshot-changed`: the data changed without the panel asking (a live tick, a file the engine watches);
//!   the UI asks for a fresh snapshot.
//!
//! Actions that ask the user something (自动检测, 导出用量, removing a Key) show their dialog here, with the panel
//! already out of the way; the UI hides it first for every action that doesn't keep the panel open.
//!
//! Today the data comes from the synthetic fixtures (fixtures.rs). Wiring in starmeter-core means replacing
//! `Fixtures` with the core's snapshot and action handler; the UI and this contract stay as they are.

use std::sync::Mutex;

use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::fixtures::Fixtures;
use crate::{host, panel};

pub struct Data(pub Mutex<Fixtures>);

impl Data {
    pub fn new() -> Self {
        Data(Mutex::new(Fixtures::load()))
    }
}

#[derive(Serialize)]
pub struct Receipt {
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    preferences: Option<Value>,
}

impl Receipt {
    fn done(preferences: Option<Value>) -> Self {
        Receipt { status: "done", message: None, preferences }
    }
    fn cancelled() -> Self {
        Receipt { status: "cancelled", message: None, preferences: None }
    }
    fn failed(message: impl Into<String>) -> Self {
        Receipt { status: "failed", message: Some(message.into()), preferences: None }
    }
}

#[tauri::command]
pub fn snapshot(data: State<'_, Data>) -> Value {
    data.0.lock().expect("the data lock").snapshot()
}

#[tauri::command]
pub fn dashboard(data: State<'_, Data>, range: String) -> Value {
    data.0.lock().expect("the data lock").dashboard(&range)
}

#[tauri::command]
pub async fn action(app: AppHandle, action: Vec<String>) -> Receipt {
    run(&app, action).await
}

/// The data changed: the panel reloads, and the star's figure follows.
pub fn changed(app: &AppHandle) {
    let _ = app.emit("snapshot-changed", ());
    panel::update_star(app);
}

pub fn menu_bar_text(app: &AppHandle) -> String {
    app.state::<Data>().0.lock().expect("the data lock").menu_bar_text()
}

fn apply(app: &AppHandle, action: &[String]) -> Receipt {
    let outcome = app.state::<Data>().0.lock().expect("the data lock").act(action);
    if outcome.changed {
        changed(app);
    }
    Receipt::done(outcome.preferences)
}

/// Rows that will ask for text (a Key, a price, an address) once the engine is wired in.
const PROMPTS: [&str; 9] = [
    "add-api-key",
    "rename-api-key",
    "set-subscription-price",
    "set-cny-rate",
    "set-balance-alert",
    "configure-connection",
    "set-proxy-address",
    "choose-source",
    "choose-client-home",
];

pub async fn run(app: &AppHandle, action: Vec<String>) -> Receipt {
    let name = action.first().cloned().unwrap_or_default();
    let receipt = match name.as_str() {
        "detect-modules" => {
            let message = "将启用：Summary、用量热力图、Claude、ChatGPT、Magpie、Cursor 等检测到的模块。\n将取消：没有检测到的模块。\n\n确认后覆盖当前的模块选择；检测不读取对话正文，不显示 Key。";
            if host::confirm(app, "自动检测主页模块", message, "检测并替换").await {
                apply(app, &action)
            } else {
                Receipt::cancelled()
            }
        }
        "export-usage" => {
            let format = action.get(1).map(String::as_str).unwrap_or("csv").to_owned();
            let label = app.state::<Data>().0.lock().expect("the data lock").range_label();
            match host::save_file(app, &format!("Starmeter 用量 {label}.{format}"), &format).await {
                Some(path) => {
                    let text = app.state::<Data>().0.lock().expect("the data lock").export(&format);
                    match std::fs::write(&path, text) {
                        Ok(()) => Receipt::done(None),
                        Err(error) => Receipt::failed(format!("无法写入文件：{error}")),
                    }
                }
                None => Receipt::cancelled(),
            }
        }
        "remove-api-key" | "clear-connection" => {
            let what = if name == "remove-api-key" { "删除这把 Key？" } else { "清除连接设置？" };
            if host::confirm(app, what, "只删除 Starmeter 自己保存的副本，不改动其他文件。", "删除").await
            {
                apply(app, &action)
            } else {
                Receipt::cancelled()
            }
        }
        "set-keep-usage" if action.last().map(String::as_str) == Some("false") => {
            if host::confirm(
                app,
                "不再保留已清理记录的用量？",
                "存档会被删除，工具清理掉的旧记录不再计入“全部”等统计。",
                "删除存档",
            )
            .await
            {
                apply(app, &action)
            } else {
                Receipt::cancelled()
            }
        }
        "test-api-key" => {
            tokio_sleep(1200).await;
            Receipt::done(None)
        }
        _ if PROMPTS.contains(&name.as_str()) => {
            host::alert(app, "这是样例数据", "这一步会询问 Key、金额或地址；连接 Starmeter 核心后可用。").await;
            Receipt::cancelled()
        }
        _ => apply(app, &action),
    };
    eprintln!("[fixtures] action {action:?} → {}", receipt.status);
    // A dialog took the focus while the panel was out of the way: the app used before gets it back.
    if !panel::visible(app) {
        panel::return_focus(app);
    }
    receipt
}

async fn tokio_sleep(ms: u64) {
    let _ =
        tauri::async_runtime::spawn_blocking(move || std::thread::sleep(std::time::Duration::from_millis(ms))).await;
}
