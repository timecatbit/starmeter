//! The panel's data: the one boundary between the web UI and the engine.
//!
//! The UI (app/ui/src/lib/backend.ts) calls three commands and listens for one event:
//!
//! - `snapshot() -> PanelSnapshot`: everything the panel shows, in the engine's JSON (5.x's PanelSnapshot, now
//!   starmeter-core's `panel_snapshot`): `entries` (the homepage rows), `moduleOptions` (the module grid and 外观与设置),
//!   `rangeOptions`, `dashboard` (Summary, chart, heatmap and cards of the selected range), `preferences` (the display
//!   preferences with their revision), `appearance`, `theme`, `pinInterval`, `menuBarText`, `updateCount`, `project`.
//!   The types are written out in app/ui/src/lib/types.ts.
//! - `dashboard(range) -> Dashboard`: the dashboard of one range, without changing the selected one.
//! - `action(action: [String]) -> Receipt`: one of the panel's actions, named as the engine names them (set-range,
//!   set-module, refresh-live, test-api-key, export-usage, …). The receipt says done, failed (with a message) or
//!   cancelled, and carries the saved display preferences when it changed them, so an older snapshot never undoes a
//!   newer choice. The UI shows a choice at once and reloads the snapshot after the receipt.
//! - event `snapshot-changed`: the data changed without the panel asking (a background read finished, the plugin
//!   wrote a new snapshot, a live tick); the UI asks for a fresh snapshot.
//!
//! Where the data comes from:
//!
//! - standalone (the app's own star): starmeter-core in this process, with the app as its desktop host (engine.rs);
//! - SwiftBar's panel (`--plugin … --snapshot …`): the plugin's snapshot file and its action/receipt files
//!   (swiftbar.rs), so the engine keeps running under the plugin's permissions;
//! - `STARMETER_FIXTURE=default|failed|pending`: the synthetic fixtures (fixtures.rs), for looking at the panel
//!   without real usage.
//!
//! Actions that ask the user something run on a worker thread with the panel already out of the way: the UI hides
//! it first for every action that doesn't keep it open.

use std::sync::{Arc, Mutex};

use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager};

use crate::engine::Engine;
use crate::fixtures::Fixtures;
use crate::swiftbar::Files;
use crate::{host, panel};

pub enum Data {
    Engine(Arc<Engine>),
    SwiftBar(Arc<Files>),
    Fixtures(Mutex<Fixtures>),
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
pub async fn snapshot(app: AppHandle) -> Value {
    tauri::async_runtime::spawn_blocking(move || match &*app.state::<Data>() {
        Data::Engine(engine) => engine.snapshot(),
        Data::SwiftBar(files) => files.snapshot(),
        Data::Fixtures(fixtures) => fixtures.lock().expect("the fixtures").snapshot(),
    })
    .await
    .unwrap_or(Value::Null)
}

#[tauri::command]
pub async fn dashboard(app: AppHandle, range: String) -> Value {
    tauri::async_runtime::spawn_blocking(move || match &*app.state::<Data>() {
        Data::Engine(engine) => engine.dashboard(&range),
        Data::SwiftBar(files) => files.snapshot().get("dashboard").cloned().unwrap_or(Value::Null),
        Data::Fixtures(fixtures) => fixtures.lock().expect("the fixtures").dashboard(&range),
    })
    .await
    .unwrap_or(Value::Null)
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
    match &*app.state::<Data>() {
        Data::Engine(engine) => engine.menu_bar_text(),
        Data::SwiftBar(_) => String::new(),
        Data::Fixtures(fixtures) => fixtures.lock().expect("the fixtures").menu_bar_text(),
    }
}

pub async fn run(app: &AppHandle, action: Vec<String>) -> Receipt {
    let state = app.state::<Data>();
    let receipt = match &*state {
        Data::Engine(engine) => {
            let engine = engine.clone();
            let task = action.clone();
            match tauri::async_runtime::spawn_blocking(move || engine.act(&task)).await {
                Ok(Ok(preferences)) => Receipt::done(preferences),
                Ok(Err(message)) => Receipt::failed(message),
                Err(_) => Receipt::failed("action failed"),
            }
        }
        Data::SwiftBar(files) => {
            let files = files.clone();
            let task = action.clone();
            match tauri::async_runtime::spawn_blocking(move || files.act(&task)).await {
                Ok((status, preferences)) if status == "done" => Receipt::done(preferences),
                _ => Receipt::failed("SwiftBar 未完成这次操作，请重试。"),
            }
        }
        Data::Fixtures(_) => fixture_action(app, &action).await,
    };
    // A dialog took the focus while the panel was out of the way: the app used before gets it back.
    if !panel::visible(app) {
        panel::return_focus(app);
    }
    receipt
}

// MARK: The fixtures' stand-in actions

fn apply(app: &AppHandle, action: &[String]) -> Receipt {
    let Data::Fixtures(fixtures) = &*app.state::<Data>() else { return Receipt::cancelled() };
    let outcome = fixtures.lock().expect("the fixtures").act(action);
    if outcome.changed {
        changed(app);
    }
    Receipt::done(outcome.preferences)
}

/// Rows that ask for text (a Key, a price, an address) in the engine.
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

async fn fixture_action(app: &AppHandle, action: &[String]) -> Receipt {
    let name = action.first().cloned().unwrap_or_default();
    match name.as_str() {
        "detect-modules" => {
            let message = "将启用：Summary、用量热力图、Claude、ChatGPT、Magpie、Cursor 等检测到的模块。\n将取消：没有检测到的模块。\n\n确认后覆盖当前的模块选择；检测不读取对话正文，不显示 Key。";
            if host::confirm(app, "自动检测主页模块", message, "检测并替换").await {
                apply(app, action)
            } else {
                Receipt::cancelled()
            }
        }
        "export-usage" => {
            let format = action.get(1).map(String::as_str).unwrap_or("csv").to_owned();
            let (label, text) = match &*app.state::<Data>() {
                Data::Fixtures(fixtures) => {
                    let fixtures = fixtures.lock().expect("the fixtures");
                    (fixtures.range_label(), fixtures.export(&format))
                }
                _ => return Receipt::cancelled(),
            };
            match host::save_file(app, &format!("Starmeter 用量 {label}.{format}"), &format).await {
                Some(path) => match std::fs::write(&path, text) {
                    Ok(()) => Receipt::done(None),
                    Err(error) => Receipt::failed(format!("无法写入文件：{error}")),
                },
                None => Receipt::cancelled(),
            }
        }
        "remove-api-key" | "clear-connection" => {
            let what = if name == "remove-api-key" { "删除这把 Key？" } else { "清除连接设置？" };
            if host::confirm(app, what, "只删除 Starmeter 自己保存的副本，不改动其他文件。", "删除").await
            {
                apply(app, action)
            } else {
                Receipt::cancelled()
            }
        }
        "test-api-key" => {
            let _ = tauri::async_runtime::spawn_blocking(|| std::thread::sleep(std::time::Duration::from_millis(1200)))
                .await;
            Receipt::done(None)
        }
        _ if PROMPTS.contains(&name.as_str()) => {
            host::alert(app, "这是样例数据", "这一步会询问 Key、金额或地址；用真实数据运行时可用。").await;
            Receipt::cancelled()
        }
        _ => apply(app, action),
    }
}
