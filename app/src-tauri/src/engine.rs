//! The standalone app's data: starmeter-core in this process. The app is the core's desktop host: it answers the
//! core's questions with its own dialogs, takes over its refreshes (one background queue), saves the range on screen
//! and the connection settings, and tells it whether the panel is open so closed panels refresh less often.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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
/// When a phone last read the homepage (在手机上看), in seconds: for a minute after, the core keeps the open panel's
/// pace, so a monitor's status or an account's figure is current on the phone too.
pub static PHONE_READ: AtomicU64 = AtomicU64::new(0);

fn watched() -> bool {
    PANEL_OPEN.load(Ordering::SeqCst) || (now() as u64).saturating_sub(PHONE_READ.load(Ordering::SeqCst)) < 60
}

/// Work waiting for the background, merged while it waits: one range read, the key checks asked for, one refresh.
/// Range reads and key checks are local and quick; a refresh may wait on the network, so it has its own thread and a
/// range switch never queues behind it.
#[derive(Default)]
struct Jobs {
    range: bool,
    /// The range menu is open: its ranges read ahead (prefetch_ranges), after any range picked.
    prefetch: bool,
    tests: Vec<(String, String)>,
    refresh: bool,
    /// Back from sleep: before the refresh, what failed meanwhile is read again now (see retry_after_wake).
    after_wake: bool,
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
/// also looks up to three folders above, where the repository's own are. Failing that, the program's own folder: the
/// build's folder is not written into the program, which is installed elsewhere.
fn resources(app: &AppHandle) -> String {
    app.path()
        .resource_dir()
        .ok()
        .or_else(|| std::env::current_exe().ok()?.parent().map(Path::to_path_buf))
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default()
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
        // The price tables where this runtime's environment says they are (the bundle's lib/), and the copies the daily
        // update leaves. The runtime the engine started with guessed them from the program's own folder (on a .deb,
        // /usr/bin/lib, where there are none), and the folders, set only once, kept that guess: every model unpriced.
        starmeter_core::pricing::set_folders(vec![
            starmeter_core::runtime::lib_directory(&runtime.plugin).into(),
            format!("{}/pricing", runtime.state_dir).into(),
        ]);
        runtime.host =
            Arc::new(AppHost { app: self.app.clone(), state: runtime.state_dir.clone(), engine: Arc::downgrade(self) });
        runtime.host_cadence = Some(Box::new(|| (watched(), false)));
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
        self.dashboard_grouped(range, None)
    }

    /// The dashboard for a range with the cards grouped as asked (a phone's own choice), the computer's grouping when
    /// none is.
    pub fn dashboard_grouped(&self, range: &str, group: Option<&str>) -> Value {
        let runtime = self.runtime();
        let loaded = runtime.range_with_devices(range);
        let _turn = self.rendering.lock().expect("the render lock");
        let value = runtime.render(|| runtime.range_dashboard(range, group, loaded, now()));
        to_value(Json::Object(value))
    }

    /// The 会话 row with its figures and note in `range`, for a phone that chose another range than this computer.
    pub fn sessions_entry(&self, range: &str) -> Value {
        let runtime = self.runtime();
        let _turn = self.rendering.lock().expect("the render lock");
        let entry = runtime.render(|| {
            let data = runtime.cached_data();
            let lines = runtime.sessions_lines_in(&data, &runtime.currency_unit(&data), range);
            starmeter_core::panel_snapshot::panel_entries(&lines).into_iter().next().unwrap_or(Json::Null)
        });
        to_value(entry)
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
            "prefetch-ranges" => self.queue(|jobs| jobs.prefetch = true),
            "refresh-live" => {}
            _ => self.queue(|jobs| jobs.refresh = true),
        }
        Ok(preferences)
    }

    /// The periodic read: what is due, at most four sources at a time; the core keeps its own locks.
    pub fn refresh(&self) {
        self.queue(|jobs| jobs.refresh = true);
    }

    /// The refresh after a sleep, once the network is back.
    pub fn refresh_after_wake(&self) {
        self.queue(|jobs| {
            jobs.after_wake = true;
            jobs.refresh = true;
        });
    }

    fn queue(&self, change: impl FnOnce(&mut Jobs)) {
        change(&mut self.jobs.lock().expect("the job lock"));
        self.wake.notify_all();
    }

    /// One lane of the background: the refresh (slow) or the range reads and key checks (quick).
    fn work(self: Arc<Self>, slow: bool) {
        loop {
            let (range, tests, refresh, after_wake, prefetch) = {
                let mut jobs = self.jobs.lock().expect("the job lock");
                while if slow { !jobs.refresh } else { !jobs.range && jobs.tests.is_empty() && !jobs.prefetch } {
                    jobs = self.wake.wait(jobs).expect("the job lock");
                }
                if slow {
                    jobs.refresh = false;
                    (false, Vec::new(), true, std::mem::take(&mut jobs.after_wake), false)
                } else {
                    (
                        std::mem::take(&mut jobs.range),
                        std::mem::take(&mut jobs.tests),
                        false,
                        false,
                        std::mem::take(&mut jobs.prefetch),
                    )
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
            if prefetch {
                // A range picked meanwhile is read first: the reading ahead stops for it.
                runtime.prefetch_ranges(&|| self.jobs.lock().map(|jobs| jobs.range).unwrap_or(false));
            }
            if after_wake {
                runtime.retry_after_wake();
            }
            if refresh {
                runtime.background_refresh(true);
                crate::data::changed(&self.app);
            }
        }
    }
}

/// Actions whose receipt carries the display preferences (5.x's RECEIPT_ACTIONS).
const PREFERENCE_ACTIONS: [&str; 9] = [
    "set-module",
    "set-appearance",
    "set-theme",
    "reset-modules",
    "set-collapse",
    "set-expanded",
    "detect-modules",
    "move-module",
    "reset-module-order",
];

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
        // AppKit's own work on the main thread: this runs on a worker.
        #[cfg(target_os = "macos")]
        macos::on_main(&self.app, macos::activate);
        crate::panel::step_aside(&self.app, true);
        let dialog = self.app.dialog().file().set_title(message);
        let picked = if folder { dialog.blocking_pick_folder() } else { dialog.blocking_pick_file() };
        crate::panel::step_aside(&self.app, false);
        picked.and_then(|path| path.into_path().ok()).map(|path| path.to_string_lossy().into_owned())
    }

    fn save_path(&self, message: &str, name: &str) -> Option<String> {
        // AppKit's own work on the main thread: this runs on a worker.
        #[cfg(target_os = "macos")]
        macos::on_main(&self.app, macos::activate);
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

    /// The Claude app where there is one (macOS, Windows), else Claude on the web, where the same account shows its
    /// usage; said when neither opens.
    fn open_claude(&self) {
        #[cfg(target_os = "macos")]
        if std::process::Command::new("/usr/bin/open")
            .args(["-a", "Claude"])
            .status()
            .is_ok_and(|status| status.success())
        {
            return;
        }
        #[cfg(windows)]
        if let Some(claude) = std::env::var_os("LOCALAPPDATA")
            .map(|folder| Path::new(&folder).join("AnthropicClaude").join("claude.exe"))
            .filter(|path| path.is_file())
            && std::process::Command::new(claude).spawn().is_ok()
        {
            return;
        }
        use tauri_plugin_opener::OpenerExt;
        if self.app.opener().open_url("https://claude.ai/", None::<&str>).is_err() {
            self.error("无法打开 Claude", "请在浏览器里打开 https://claude.ai/");
        }
    }

    /// The CLI's own update in a terminal, with its log, as 5.x ran it: a Terminal tab on macOS, a terminal window on
    /// Linux, a console window on Windows (the updater's own output there, without the log).
    fn update_cli(&self, binary: &str, key: &str, run: &[String]) {
        // Windows' console shows the updater's own output, without the log that notes the versions.
        #[cfg(windows)]
        let _ = binary;
        #[cfg(windows)]
        if let Err(failure) = terminal_jobs::launch_windows(run, key, &self.state) {
            self.error("无法打开命令提示符", &failure.0);
        }
        #[cfg(not(windows))]
        {
            let zone = jiff::tz::TimeZone::system();
            match terminal_jobs::update_command(binary, key, run, &self.state, None, &zone) {
                Ok(command) => {
                    #[cfg(target_os = "macos")]
                    let launched = terminal_jobs::launch(&command, "", &self.state);
                    #[cfg(not(target_os = "macos"))]
                    let launched = terminal_jobs::launch_linux(&command, &self.state);
                    if let Err(failure) = launched {
                        self.error("无法打开终端", &failure.0);
                    }
                }
                Err(failure) => self.error("无法开始更新", &failure.0),
            }
        }
    }

    /// 应用内升级: downloaded and checked in the background (the row says 正在更新 meanwhile), then the helper takes
    /// over and the app quits, to be opened again new; a failure stays on the row, with a notice, and the app goes on.
    fn update_app(&self, version: &str) -> bool {
        let Some(engine) = self.engine.upgrade() else { return false };
        let (app, version) = (self.app.clone(), version.to_string());
        std::thread::Builder::new()
            .name("starmeter-app-update".into())
            .spawn(move || {
                crate::data::changed(&app);
                let runtime = engine.runtime();
                match runtime.update_app_in_place(&version, std::process::id()) {
                    Ok(true) => app.exit(0),
                    // Another update already under way says how it goes itself.
                    Ok(false) => crate::data::changed(&app),
                    Err(reason) => {
                        runtime.host.notify(&format!("更新失败：{reason}"), "Starmeter 更新");
                        crate::data::changed(&app);
                    }
                }
            })
            .is_ok()
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
                Some("--prefetch-ranges") => engine.queue(|jobs| jobs.prefetch = true),
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

/// The background clock: every minute what is due is read, each source by its own lifetime. Pinned as well: the live
/// tick re-reads the local logs every second or two, but the monitors, accounts and API balances still come from here
/// (a monitor's status counts as current for a minute and a half). The range on screen, just read by the live tick,
/// is not due, and a range read gives way while the live tick holds the lock.
pub fn start_clock(engine: Arc<Engine>) {
    std::thread::Builder::new()
        .name("starmeter-clock".into())
        .spawn(move || {
            let mut clock = Clock::new(now());
            loop {
                std::thread::sleep(Duration::from_secs_f64(TICK));
                match clock.tick(now(), online) {
                    Tick::Refresh => engine.refresh(),
                    Tick::AfterWake => engine.refresh_after_wake(),
                    Tick::Wait => {}
                }
            }
        })
        .expect("the clock thread");
}

/// The clock looks at the time this often (a look costs nothing; a refresh comes once a minute).
const TICK: f64 = 2.0;
/// The wall clock moving this much further than a tick: the machine slept. The clock's own sleep doesn't count the
/// time asleep, the wall clock does.
const SLEPT: f64 = 30.0;
/// Back from sleep, the longest wait for the network before reading anyway.
const NETWORK_WAIT: f64 = 30.0;
/// After the read on waking, a second one this much later for anything the first found still joining the network.
const FOLLOW_UP: f64 = 20.0;

#[derive(Debug, PartialEq)]
enum Tick {
    Wait,
    Refresh,
    AfterWake,
}

/// When the clock reads: every minute; and on waking from sleep (the lid opened, or a maintenance wake), as soon as
/// the network is back, so the panel shows current figures, not the failures of the minute the Wi-Fi was away.
struct Clock {
    last: f64,
    next: f64,
    woke: Option<f64>,
    follow_up: Option<f64>,
}

impl Clock {
    fn new(moment: f64) -> Self {
        Clock { last: moment, next: moment + 60.0, woke: None, follow_up: None }
    }

    fn tick(&mut self, moment: f64, online: impl FnOnce() -> bool) -> Tick {
        if moment - self.last > TICK + SLEPT {
            self.woke = Some(moment);
            self.follow_up = None;
        } else if moment < self.last {
            // The clock was set back: everything waited for moves back with it and keeps what was left of its wait
            // (WAKE-1: the wait for the network after waking kept the old time, and an hour back meant an hour's wait).
            let back = self.last - moment;
            self.next -= back;
            self.woke = self.woke.map(|at| at - back);
            self.follow_up = self.follow_up.map(|at| at - back);
        }
        self.last = moment;
        if let Some(woke) = self.woke {
            if moment - woke < NETWORK_WAIT && !online() {
                return Tick::Wait;
            }
            self.woke = None;
            self.follow_up = Some(moment + FOLLOW_UP);
            self.next = moment + 60.0;
            return Tick::AfterWake;
        }
        if self.follow_up.is_some_and(|at| moment >= at) {
            self.follow_up = None;
            self.next = moment + 60.0;
            return Tick::AfterWake;
        }
        if moment >= self.next {
            self.next = moment + 60.0;
            return Tick::Refresh;
        }
        Tick::Wait
    }
}

fn online() -> bool {
    #[cfg(target_os = "macos")]
    return macos::online();
    #[cfg(not(target_os = "macos"))]
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two-second looks from `from` to `to`, each answered by `online`; the moments the clock read, and how.
    fn run(clock: &mut Clock, from: f64, to: f64, online: impl Fn(f64) -> bool) -> Vec<(f64, Tick)> {
        let mut out = Vec::new();
        let mut moment = from;
        while moment <= to {
            let tick = clock.tick(moment, || online(moment));
            if tick != Tick::Wait {
                out.push((moment, tick));
            }
            moment += TICK;
        }
        out
    }

    #[test]
    fn awake_it_refreshes_every_minute() {
        let mut clock = Clock::new(0.0);
        let ticks = run(&mut clock, 2.0, 180.0, |_| true);
        assert_eq!(ticks, vec![(60.0, Tick::Refresh), (120.0, Tick::Refresh), (180.0, Tick::Refresh)]);
    }

    #[test]
    fn waking_it_reads_once_the_network_is_back_and_again_twenty_seconds_later() {
        let mut clock = Clock::new(0.0);
        assert_eq!(run(&mut clock, 2.0, 50.0, |_| true), vec![]);
        // Asleep for an hour; the Wi-Fi joins six seconds after the lid opens.
        let woke = 3650.0;
        let ticks = run(&mut clock, woke, woke + 90.0, |moment| moment >= woke + 6.0);
        assert_eq!(
            ticks,
            vec![(woke + 6.0, Tick::AfterWake), (woke + 26.0, Tick::AfterWake), (woke + 86.0, Tick::Refresh)]
        );
    }

    #[test]
    fn with_no_network_it_reads_anyway_after_half_a_minute() {
        let mut clock = Clock::new(0.0);
        let woke = 7200.0;
        let ticks = run(&mut clock, woke, woke + 52.0, |_| false);
        assert_eq!(ticks, vec![(woke + 30.0, Tick::AfterWake), (woke + 50.0, Tick::AfterWake)]);
    }

    #[test]
    fn the_network_question_links_and_answers() {
        // On a machine with a network this says so; the answer is printed for a look by hand.
        println!("online: {}", online());
    }

    #[test]
    fn a_clock_set_back_while_waiting_for_the_network_still_waits_half_a_minute_at_most() {
        for back in [180.0, 600.0, 3600.0] {
            let mut clock = Clock::new(0.0);
            let woke = 7200.0;
            assert_eq!(clock.tick(woke, || false), Tick::Wait);
            assert_eq!(clock.tick(woke + 10.0, || false), Tick::Wait);
            // Ten seconds into the wait, the clock goes back; offline, the read comes about twenty seconds later all
            // the same (the look that saw the jump counts for nothing: one look, two seconds, at most).
            let now = woke + 12.0 - back;
            let ticks = run(&mut clock, now, now + 42.0, |_| false);
            assert_eq!(ticks, vec![(now + 20.0, Tick::AfterWake), (now + 40.0, Tick::AfterWake)], "{back}s back");
        }
    }

    #[test]
    fn a_clock_set_back_keeps_the_minute() {
        let mut clock = Clock::new(1000.0);
        assert_eq!(clock.tick(500.0, || true), Tick::Wait);
        assert_eq!(run(&mut clock, 502.0, 562.0, |_| true), vec![(560.0, Tick::Refresh)]);
    }
}
