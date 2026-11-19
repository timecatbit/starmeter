//! SwiftBar's panel: the plugin's engine renders the snapshot and runs the actions with the plugin's own permissions;
//! the panel only reads and asks, through files, as 5.x's panel did (running the engine here would meet macOS's
//! Documents/TCC prompts).
//!
//! - The snapshot is the file passed as `--snapshot` (the plugin's cache/panel-view.json); its folder is watched.
//! - An action is written beside it as panel-action.json `{"id", "action", "createdAt"}`, then SwiftBar is asked to
//!   run the plugin (swiftbar://refreshplugin?name=<plugin>) without coming forward. The engine runs it only if the
//!   snapshot offered it, and leaves panel-action-receipt.json `{"id", "status", "preferences", …}`.
//! - The panel says whether it is open in panel-state.json, so the engine relaxes its pace while it is closed; the
//!   plugin's panel-open.json says whether a click asked for the homepage or the module list.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use serde_json::{Value, json};
use tauri::AppHandle;

pub struct Files {
    snapshot: PathBuf,
    folder: PathBuf,
    /// SwiftBar's name for the plugin: its file name up to the first dot (2.1.x matches nothing else).
    name: String,
    seen: Mutex<Option<SystemTime>>,
    /// One action at a time, as the engine claims one request file.
    turn: tokio_mutex::Mutex,
}

/// A tiny async mutex (the app has no tokio dependency of its own): actions queue in order.
mod tokio_mutex {
    use std::sync::{Arc, Condvar, Mutex as StdMutex};

    #[derive(Default, Clone)]
    pub struct Mutex(Arc<(StdMutex<bool>, Condvar)>);

    pub struct Guard(Arc<(StdMutex<bool>, Condvar)>);

    impl Mutex {
        /// Blocking; called from a blocking task.
        pub fn lock(&self) -> Guard {
            let (busy, wake) = &*self.0;
            let mut taken = busy.lock().expect("the action turn");
            while *taken {
                taken = wake.wait(taken).expect("the action turn");
            }
            *taken = true;
            Guard(self.0.clone())
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            let (busy, wake) = &*self.0;
            *busy.lock().expect("the action turn") = false;
            wake.notify_one();
        }
    }
}

fn argument(args: &[String], flag: &str) -> Option<String> {
    args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned()
}

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|meta| meta.modified()).ok()
}

/// Written whole, private to the user (the folder is the plugin's private cache).
fn write_private(path: &Path, value: &Value) -> bool {
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    if std::fs::write(&temporary, value.to_string()).is_err() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(0o600));
    }
    std::fs::rename(&temporary, path).is_ok()
}

fn read(path: &Path) -> Option<Value> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

fn now() -> f64 {
    SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

impl Files {
    /// From the plugin's launch arguments: `--plugin <entry> --snapshot <cache>/panel-view.json`.
    pub fn from_args(args: &[String]) -> Option<Self> {
        let snapshot = PathBuf::from(argument(args, "--snapshot")?);
        let plugin = argument(args, "--plugin").unwrap_or_default();
        let name = Path::new(&plugin)
            .file_name()
            .map(|file| file.to_string_lossy().split('.').next().unwrap_or("starmeter").to_owned())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "starmeter".into());
        let folder = snapshot.parent().map(Path::to_path_buf).unwrap_or_default();
        Some(Files { snapshot, folder, name, seen: Mutex::new(None), turn: tokio_mutex::Mutex::default() })
    }

    /// No plugin arguments: an empty panel, nothing read or sent.
    pub fn empty() -> Self {
        Files {
            snapshot: PathBuf::new(),
            folder: PathBuf::new(),
            name: "starmeter".into(),
            seen: Mutex::new(None),
            turn: tokio_mutex::Mutex::default(),
        }
    }

    pub fn snapshot(&self) -> Value {
        *self.seen.lock().expect("the seen lock") = modified(&self.snapshot);
        read(&self.snapshot).unwrap_or_else(|| json!({"schema": 1, "entries": []}))
    }

    /// The homepage or the module list, when the plugin's click asked within the last 30 seconds.
    pub fn requested_modules(&self) -> Option<bool> {
        let path = self.folder.join("panel-open.json");
        let record = read(&path)?;
        let created = record.get("createdAt").and_then(Value::as_f64)?;
        let _ = std::fs::remove_file(&path);
        (now() - created < 30.0).then(|| record.get("mode").and_then(Value::as_str) == Some("modules"))
    }

    pub fn publish_visibility(&self, visible: bool) {
        if self.folder.as_os_str().is_empty() {
            return;
        }
        write_private(
            &self.folder.join("panel-state.json"),
            &json!({"schema": 1, "visible": visible, "pid": std::process::id()}),
        );
    }

    /// Watches the snapshot (written whole by the engine, so its folder's files are replaced): a new one reaches the
    /// panel at once while it is open, within a few seconds otherwise.
    pub fn watch(self: &std::sync::Arc<Self>, app: AppHandle) {
        let files = self.clone();
        std::thread::Builder::new()
            .name("starmeter-snapshot".into())
            .spawn(move || {
                loop {
                    let open = crate::engine::PANEL_OPEN.load(std::sync::atomic::Ordering::SeqCst);
                    std::thread::sleep(Duration::from_millis(if open { 150 } else { 2000 }));
                    let current = modified(&files.snapshot);
                    let changed = {
                        let mut seen = files.seen.lock().expect("the seen lock");
                        let changed = current.is_some() && current != *seen;
                        *seen = current;
                        changed
                    };
                    if changed {
                        crate::data::changed(&app);
                    }
                }
            })
            .expect("the snapshot watcher");
    }

    fn wake(&self) {
        let url = format!("swiftbar://refreshplugin?name={}", self.name);
        #[cfg(all(target_os = "macos", not(test)))]
        crate::macos::open_url(&url, false);
        #[cfg(any(not(target_os = "macos"), test))]
        let _ = url;
    }

    /// Queues an action for the plugin and waits for its receipt: automatic refreshes give up after 3 seconds, display
    /// preferences after 20, others after 8 unless the engine has claimed them (a dialog may be waiting on the user).
    pub fn act(&self, action: &[String]) -> (String, Option<Value>) {
        if self.folder.as_os_str().is_empty() {
            return ("failed".into(), None);
        }
        let _turn = self.turn.lock();
        let id = format!(
            "{:x}{:x}",
            std::process::id(),
            SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
        );
        let request = self.folder.join("panel-action.json");
        let receipt = self.folder.join("panel-action-receipt.json");
        if !write_private(&request, &json!({"id": id, "action": action, "createdAt": now()})) {
            return ("failed".into(), None);
        }
        self.wake();
        let automatic =
            matches!(action.first().map(String::as_str), Some("refresh-view" | "refresh-source" | "refresh-live"));
        let preference = matches!(
            action.first().map(String::as_str),
            Some("set-module" | "set-appearance" | "set-theme" | "reset-modules" | "set-collapse" | "set-expanded")
        );
        let limit = Duration::from_secs(if automatic {
            3
        } else if preference {
            20
        } else {
            8
        });
        let started = Instant::now();
        let mut woken_again = false;
        loop {
            std::thread::sleep(Duration::from_millis(60));
            if let Some(answer) =
                read(&receipt).filter(|answer| answer.get("id").and_then(Value::as_str) == Some(id.as_str()))
            {
                let status = answer.get("status").and_then(Value::as_str).unwrap_or("failed");
                let preferences = answer.get("preferences").filter(|p| p.is_object()).cloned();
                return (if status == "done" { "done" } else { "failed" }.into(), preferences);
            }
            let claimed = !request.exists();
            // SwiftBar can miss a wake-up; ask once more (it claims the same file at most once).
            if !woken_again && !claimed && started.elapsed() > Duration::from_millis(800) {
                woken_again = true;
                self.wake();
            }
            if started.elapsed() > limit && (automatic || preference || !claimed) {
                if !claimed {
                    let _ = std::fs::remove_file(&request);
                }
                return ("failed".into(), None);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder() -> PathBuf {
        let folder = std::env::temp_dir().join(format!(
            "starmeter-swiftbar-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&folder).expect("a test folder");
        folder
    }

    #[test]
    fn reads_the_arguments_and_names_the_plugin() {
        let args: Vec<String> = ["app", "--plugin", "/x/starmeter.30s.sh", "--snapshot", "/c/panel-view.json"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let files = Files::from_args(&args).expect("panel mode");
        assert_eq!(files.name, "starmeter");
        assert_eq!(files.folder, PathBuf::from("/c"));
    }

    #[test]
    fn an_action_goes_out_as_a_file_and_its_receipt_comes_back() {
        let folder = folder();
        let snapshot = folder.join("panel-view.json");
        std::fs::write(&snapshot, b"{}").expect("a snapshot");
        let args = vec![
            "app".to_string(),
            "--plugin".into(),
            "/x/starmeter.30s.sh".into(),
            "--snapshot".into(),
            snapshot.to_string_lossy().into_owned(),
        ];
        let files = Files::from_args(&args).expect("panel mode");
        // The plugin's engine: claims the request and answers it.
        let engine = folder.clone();
        let worker = std::thread::spawn(move || {
            let request = engine.join("panel-action.json");
            for _ in 0..200 {
                if let Some(record) = read(&request) {
                    let _ = std::fs::remove_file(&request);
                    assert_eq!(record["action"], json!(["set-theme", "dark"]));
                    let receipt = json!({"schema": 1, "id": record["id"], "status": "done", "preferences": {"revision": 7, "theme": "dark"}});
                    std::fs::write(engine.join("panel-action-receipt.json"), receipt.to_string()).expect("a receipt");
                    return;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            panic!("no request");
        });
        let (status, preferences) = files.act(&["set-theme".into(), "dark".into()]);
        worker.join().expect("the engine thread");
        assert_eq!(status, "done");
        assert_eq!(preferences.and_then(|p| p.get("revision").cloned()), Some(json!(7)));
        let _ = std::fs::remove_dir_all(folder);
    }
}
