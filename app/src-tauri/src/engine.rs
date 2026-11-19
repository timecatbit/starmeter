//! The standalone app's data: starmeter-core in this process. The app is the core's desktop host: it answers the
//! core's questions with its own dialogs, takes over its refreshes (one background queue), saves the range on screen
//! and the connection settings, and tells it whether the panel is open so closed panels refresh less often.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::time::Duration;

use serde_json::Value;
use starmeter_core::desktop::{self, Shell};
use starmeter_core::host::{self, DesktopHost, Host, HostFailure, connections, terminal_jobs};
use starmeter_core::json::{Json, Object};
use starmeter_core::runtime::{Runtime, now};
use starmeter_core::{discovery, files};
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;

use crate::host as shell;
#[cfg(target_os = "macos")]
use crate::macos;

/// Whether the panel is on screen, for the core's refresh cadence.
pub static PANEL_OPEN: AtomicBool = AtomicBool::new(false);

/// Work waiting for the background, merged while it waits: one range read, the key checks asked for, one refresh.
/// Range reads and key checks are local and quick; a refresh may wait on the network, so it has its own thread and a
/// range switch never queues behind it.
#[derive(Default)]
struct Jobs {
    range: bool,
    tests: Vec<(String, String)>,
    refresh: bool,
}

pub struct Engine {
    app: AppHandle,
    resources: String,
    runtime: RwLock<Arc<Runtime>>,
    /// One render at a time: a render reads the settings once and keeps them for its length.
    rendering: Mutex<()>,
    jobs: Mutex<Jobs>,
    wake: Condvar,
}

fn to_value(json: Json) -> Value {
    serde_json::from_str(&json.serialized()).unwrap_or(Value::Null)
}

/// Where the app's lib/ (price tables) and assets/ (logos) are: the bundle's resources; in a source checkout the core
/// also looks up to three folders above, where the repository's own are.
fn resources(app: &AppHandle) -> String {
    app.path()
        .resource_dir()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|_| env!("CARGO_MANIFEST_DIR").to_owned())
}

impl Engine {
    pub fn new(app: &AppHandle) -> Arc<Self> {
        let resources = resources(app);
        let engine = Arc::new(Engine {
            app: app.clone(),
            resources: resources.clone(),
            runtime: RwLock::new(Arc::new(Runtime::from_process())),
            rendering: Mutex::new(()),
            jobs: Mutex::new(Jobs::default()),
            wake: Condvar::new(),
        });
        engine.rebuild();
        let quick = engine.clone();
        std::thread::Builder::new()
            .name("starmeter-range".into())
            .spawn(move || quick.work(false))
            .expect("the range thread");
        let slow = engine.clone();
        std::thread::Builder::new()
            .name("starmeter-refresh".into())
            .spawn(move || slow.work(true))
            .expect("the refresh thread");
        engine
    }

    /// A fresh runtime: after connection settings changed, the environment (config.local.sh, config.local.json) is read again.
    pub fn rebuild(self: &Arc<Self>) {
        let environment: HashMap<String, String> =
            desktop::environment(Shell::current(), std::env::vars().collect(), &self.resources);
        let argv0 = std::env::args().next().unwrap_or_else(|| "starmeter".into());
        let mut runtime = Runtime::new(environment, &argv0);
        runtime.host =
            Arc::new(AppHost { app: self.app.clone(), state: runtime.state_dir.clone(), engine: Arc::downgrade(self) });
        runtime.host_cadence = Some(Box::new(|| (PANEL_OPEN.load(Ordering::SeqCst), false)));
        *self.runtime.write().expect("the runtime lock") = Arc::new(runtime);
    }

    pub fn runtime(&self) -> Arc<Runtime> {
        self.runtime.read().expect("the runtime lock").clone()
    }

    /// First launch: choose the modules found on this machine, as 5.x's first run did.
    pub fn prepare(&self) {
        let runtime = self.runtime();
        if !Path::new(&format!("{}/preferences.json", runtime.state_dir)).exists() {
            discovery::initialize(&runtime.state_dir, &runtime.detected_modules());
        }
        self.queue(|jobs| jobs.refresh = true);
    }

    pub fn snapshot(&self) -> Value {
        let runtime = self.runtime();
        let _turn = self.rendering.lock().expect("the render lock");
        let value = runtime.render(|| runtime.panel_snapshot(&runtime.cached_data(), None, None, now()));
        // Each snapshot notes its range, so while a new range is read the last one stays on screen.
        let range =
            value.get("dashboard").and_then(|dashboard| dashboard.get("rangeKey")).cloned().unwrap_or(Json::Null);
        let mut note = Object::new();
        note.insert("schema", 1);
        let mut dashboard = Object::new();
        dashboard.insert("rangeKey", range);
        note.insert("dashboard", Json::Object(dashboard));
        runtime.write_json_cache(&runtime.cache_path("panel-view"), &Json::Object(note));
        to_value(Json::Object(value))
    }

    /// One range's dashboard without changing the one on screen; reads it if the cache has none.
    pub fn dashboard(&self, range: &str) -> Value {
        let runtime = self.runtime();
        let (bundle, error) = runtime.range_usage(Some(range), None);
        let _turn = self.rendering.lock().expect("the render lock");
        let value = runtime.render(|| {
            let mut data = runtime.cached_data();
            data.insert("range_usage", bundle.map(Json::Object).unwrap_or(Json::Null));
            data.insert("range_usage_error", error.clone().map(Json::from).unwrap_or(Json::Null));
            data.insert("displayed_range", range);
            runtime.dashboard_payload(&data, error.as_deref(), now())
        });
        to_value(Json::Object(value))
    }

    pub fn menu_bar_text(&self) -> String {
        let runtime = self.runtime();
        let _turn = self.rendering.lock().expect("the render lock");
        runtime.render(|| runtime.menu_bar_value(Some(&runtime.cached_data())))
    }

    /// Runs an action as the core names it. Blocking (it may ask the user), so it runs off the main thread.
    pub fn act(self: &Arc<Self>, action: &[String]) -> Result<Option<Value>, String> {
        let Some((name, rest)) = action.split_first() else { return Err("invalid action".into()) };
        let runtime = self.runtime();
        runtime.run_action(name, rest).map_err(|error| error.0)?;
        let preferences =
            PREFERENCE_ACTIONS.contains(&name.as_str()).then(|| to_value(runtime.presentation_settings()));
        // Connection settings live in the environment the runtime was made with.
        if CONNECTION_ACTIONS.contains(&name.as_str()) {
            self.rebuild();
        }
        // The macOS app queues its own reads after an action (the core leaves them to it); other shells are asked by
        // the core and the queue merges a repeat.
        match name.as_str() {
            "set-range" => self.queue(|jobs| jobs.range = true),
            "refresh-live" => {}
            _ => self.queue(|jobs| jobs.refresh = true),
        }
        Ok(preferences)
    }

    /// The periodic read: what is due, at most four sources at a time; the core keeps its own locks.
    pub fn refresh(&self) {
        self.queue(|jobs| jobs.refresh = true);
    }

    fn queue(&self, change: impl FnOnce(&mut Jobs)) {
        change(&mut self.jobs.lock().expect("the job lock"));
        self.wake.notify_all();
    }

    /// One lane of the background: the refresh (slow) or the range reads and key checks (quick).
    fn work(self: Arc<Self>, slow: bool) {
        loop {
            let (range, tests, refresh) = {
                let mut jobs = self.jobs.lock().expect("the job lock");
                while if slow { !jobs.refresh } else { !jobs.range && jobs.tests.is_empty() } {
                    jobs = self.wake.wait(jobs).expect("the job lock");
                }
                if slow {
                    jobs.refresh = false;
                    (false, Vec::new(), true)
                } else {
                    (std::mem::take(&mut jobs.range), std::mem::take(&mut jobs.tests), false)
                }
            };
            let runtime = self.runtime();
            for (provider, id) in tests {
                let _ = runtime.test_api_key(&provider, &id);
                crate::data::changed(&self.app);
            }
            if range {
                runtime.refresh_usage_range(true);
                crate::data::changed(&self.app);
            }
            if refresh {
                runtime.background_refresh(true);
                crate::data::changed(&self.app);
            }
        }
    }
}

/// Actions whose receipt carries the display preferences (5.x's RECEIPT_ACTIONS).
const PREFERENCE_ACTIONS: [&str; 7] =
    ["set-module", "set-appearance", "set-theme", "reset-modules", "set-collapse", "set-expanded", "detect-modules"];

/// Actions that change the connection settings the runtime read at its start.
const CONNECTION_ACTIONS: [&str; 12] = [
    "configure-connection",
    "clear-connection",
    "choose-source",
    "auto-source",
    "choose-client-home",
    "reset-client-connection",
    "set-proxy-address",
    "choose-admin-token",
    "add-api-key",
    "remove-api-key",
    "rename-api-key",
    "set-claude-local",
];

/// The core's questions, notices and files, answered by the app's own dialogs; refreshes go to the engine's queue.
struct AppHost {
    app: AppHandle,
    state: String,
    engine: std::sync::Weak<Engine>,
}

impl Host for AppHost {
    // Questions go to the panel first, answered under the row that asked; a system dialog only when it is closed.
    fn confirm(&self, message: &str, detail: &str, accept: &str) -> bool {
        let question = serde_json::json!({"kind": "confirm", "title": message, "detail": detail, "accept": accept});
        match crate::ask::ask(&self.app, question) {
            Some(answer) => answer.as_bool().unwrap_or(false),
            None => shell::confirm_now(&self.app, message, detail, accept),
        }
    }

    fn text_dialog(&self, message: &str, value: &str, hidden: bool, title: &str) -> Option<String> {
        let question =
            serde_json::json!({"kind": "text", "title": title, "detail": message, "value": value, "hidden": hidden});
        if let Some(answer) = crate::ask::ask(&self.app, question) {
            return answer.as_str().map(String::from);
        }
        #[cfg(target_os = "macos")]
        {
            let (message, value, title) = (message.to_owned(), value.to_owned(), title.to_owned());
            macos::on_main(&self.app, move || macos::prompt(&title, &message, &value, hidden)).flatten()
        }
        #[cfg(not(target_os = "macos"))]
        {
            // Tauri has no text field dialog yet on Windows and Linux; say so instead of failing silently.
            let _ = (value, hidden);
            self.error(
                title,
                &format!("{message}\n\n这一步需要输入文字，Windows 和 Linux 版还不支持，请编辑高级配置文件。"),
            );
            None
        }
    }

    fn choose(&self, title: &str, prompt: &str, first: (&str, &str), second: (&str, &str)) -> Option<String> {
        let question = serde_json::json!({"kind": "choice", "title": title, "detail": prompt,
            "options": [[first.0, first.1], [second.0, second.1]]});
        if let Some(answer) = crate::ask::ask(&self.app, question) {
            return answer.as_str().filter(|value| [first.1, second.1].contains(value)).map(String::from);
        }
        #[cfg(target_os = "macos")]
        {
            let (title, prompt, a, b) = (title.to_owned(), prompt.to_owned(), first.0.to_owned(), second.0.to_owned());
            let picked = macos::on_main(&self.app, move || macos::choose(&title, &prompt, &a, &b)).flatten()?;
            Some(if picked == 0 { first.1 } else { second.1 }.to_owned())
        }
        #[cfg(not(target_os = "macos"))]
        {
            use tauri_plugin_dialog::{MessageDialogButtons, MessageDialogResult};
            let answer = self
                .app
                .dialog()
                .message(prompt)
                .title(title)
                .buttons(MessageDialogButtons::YesNoCancelCustom(first.0.into(), second.0.into(), "取消".into()))
                .blocking_show_with_result();
            match answer {
                MessageDialogResult::Custom(label) if label == first.0 => Some(first.1.to_owned()),
                MessageDialogResult::Custom(label) if label == second.0 => Some(second.1.to_owned()),
                MessageDialogResult::Yes => Some(first.1.to_owned()),
                MessageDialogResult::No => Some(second.1.to_owned()),
                _ => None,
            }
        }
    }

    // The system's own pickers: the panel stays open, below them, and comes back after.
    fn choose_path(&self, message: &str, folder: bool) -> Option<String> {
        #[cfg(target_os = "macos")]
        macos::activate();
        crate::panel::step_aside(&self.app, true);
        let dialog = self.app.dialog().file().set_title(message);
        let picked = if folder { dialog.blocking_pick_folder() } else { dialog.blocking_pick_file() };
        crate::panel::step_aside(&self.app, false);
        picked.and_then(|path| path.into_path().ok()).map(|path| path.to_string_lossy().into_owned())
    }

    fn save_path(&self, message: &str, name: &str) -> Option<String> {
        #[cfg(target_os = "macos")]
        macos::activate();
        crate::panel::step_aside(&self.app, true);
        let mut dialog = self.app.dialog().file().set_title(message).set_file_name(name);
        if let Ok(desktop) = self.app.path().desktop_dir() {
            dialog = dialog.set_directory(desktop);
        }
        let picked = dialog.blocking_save_file();
        crate::panel::step_aside(&self.app, false);
        picked.and_then(|path| path.into_path().ok()).map(|path| path.to_string_lossy().into_owned())
    }

    fn error(&self, title: &str, detail: &str) {
        if !crate::ask::tell(&self.app, title, detail) {
            shell::alert_now(&self.app, title, detail);
        }
    }

    fn notify(&self, message: &str, subtitle: &str) {
        shell::notify(&self.app, subtitle, message);
    }

    fn open_local(&self, path: &str, text: bool) -> Result<(), HostFailure> {
        #[cfg(target_os = "macos")]
        {
            let mut command = std::process::Command::new("/usr/bin/open");
            if text {
                command.arg("-t");
            }
            command
                .arg(path)
                .status()
                .ok()
                .filter(|status| status.success())
                .map(|_| ())
                .ok_or_else(|| HostFailure("open failed".into()))
        }
        #[cfg(not(target_os = "macos"))]
        {
            use tauri_plugin_opener::OpenerExt;
            let _ = text;
            self.app.opener().open_path(path, None::<&str>).map_err(|error| HostFailure(error.to_string()))
        }
    }

    fn copy(&self, text: &str) {
        #[cfg(target_os = "macos")]
        {
            use std::io::Write;
            if let Ok(mut child) =
                std::process::Command::new("/usr/bin/pbcopy").stdin(std::process::Stdio::piped()).spawn()
            {
                if let Some(mut input) = child.stdin.take() {
                    let _ = input.write_all(text.as_bytes());
                }
                let _ = child.wait();
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            use tauri_plugin_clipboard_manager::ClipboardExt;
            let _ = self.app.clipboard().write_text(text);
        }
    }

    fn desktop(&self) -> Option<&dyn DesktopHost> {
        Some(self)
    }
}

impl DesktopHost for AppHost {
    fn save_overrides(&self, values: &[(String, String)]) -> Result<(), HostFailure> {
        if cfg!(target_os = "macos") {
            connections::write_overrides(&self.state, values).map_err(|error| {
                HostFailure(match error {
                    starmeter_core::preferences::Error::Invalid(reason) => reason.to_owned(),
                    starmeter_core::preferences::Error::Write(failure) => failure.0,
                })
            })
        } else {
            desktop::save_json_overrides(&self.state, values)
        }
    }

    fn config_file(&self) -> String {
        if cfg!(target_os = "macos") {
            connections::ensure_file(&self.state).unwrap_or_else(|_| format!("{}/config.local.sh", self.state))
        } else {
            desktop::config_file(&self.state)
        }
    }

    fn open_claude(&self) {
        #[cfg(target_os = "macos")]
        let _ = std::process::Command::new("/usr/bin/open").args(["-a", "Claude"]).status();
    }

    /// The CLI's own update in a terminal tab, with its log, as 5.x ran it.
    fn update_cli(&self, binary: &str, key: &str) {
        let zone = jiff::tz::TimeZone::system();
        match terminal_jobs::update_command(binary, key, &self.state, None, &zone) {
            Ok(command) => {
                if let Err(failure) = terminal_jobs::launch(&command, "", &self.state) {
                    self.error("无法打开终端", &failure.0);
                }
            }
            Err(failure) => self.error("无法开始更新", &failure.0),
        }
    }

    fn submit_test(&self, provider: &str, id: &str) {
        if let Some(engine) = self.engine.upgrade() {
            engine.queue(|jobs| jobs.tests.push((provider.to_owned(), id.to_owned())));
        }
    }

    fn request_refresh(&self, mode: Option<&str>) {
        if let Some(engine) = self.engine.upgrade() {
            match mode {
                Some("--refresh-range") => engine.queue(|jobs| jobs.range = true),
                _ => engine.queue(|jobs| jobs.refresh = true),
            }
        }
    }

    /// The range on screen, which the core's selected_range() reads back for a desktop host.
    fn save_range(&self, key: &str) {
        let _ = files::make_directory(Path::new(&self.state), 0o700);
        let mut record = Object::new();
        record.insert("range", key);
        let _ = files::replace_atomically(
            Path::new(&format!("{}/view-range.json", self.state)),
            Json::Object(record).serialized().as_bytes(),
            ".range-",
            || Ok(()),
        );
    }

    fn local_proxy(&self, base: &str) -> bool {
        host::local_address(base)
    }
}

/// The background clock: every minute what is due is read, except while pinned (the live tick reads then).
pub fn start_clock(engine: Arc<Engine>, pinned: impl Fn() -> bool + Send + 'static) {
    std::thread::Builder::new()
        .name("starmeter-clock".into())
        .spawn(move || {
            loop {
                std::thread::sleep(Duration::from_secs(60));
                if !pinned() {
                    engine.refresh();
                }
            }
        })
        .expect("the clock thread");
}
