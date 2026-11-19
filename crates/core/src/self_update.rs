//! The plugin's own updates from its GitHub releases, notices of new app versions, and what the background worker
//! does once its reads are in.

use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use regex::Regex;

use crate::background::external_ttl;
use crate::http::{self, Request};
use crate::json::{Json, Object};
use crate::more_rows::version_tuple;
use crate::runtime::{self, Loaded, Runtime, VERSION, now};
use crate::{accounts, files, object, platform, py, shell};

pub const REPOSITORY: &str = "https://github.com/timecatbit/starmeter";
const RELEASES_API: &str = "https://api.github.com/repos/timecatbit/starmeter/releases/latest";
pub const AUTHOR: &str = "minicoke";

struct Failed(String);

fn user_agent() -> Vec<(&'static str, String)> {
    vec![("User-Agent", format!("Starmeter/{VERSION}"))]
}

/// A body up to `limit` bytes from a 2xx answer; Err(None) when the request itself failed.
fn download(url: &str, limit: usize) -> Result<Vec<u8>, Option<Failed>> {
    let response =
        http::send(url, Request { headers: user_agent(), timeout: 30.0, limit: limit + 1, ..Request::default() })
            .map_err(|_| None)?;
    if !(200..300).contains(&response.status) {
        return Err(None);
    }
    if response.body.len() > limit {
        return Err(Some(Failed("安装包大小异常".into())));
    }
    Ok(response.body)
}

fn search(pattern: &str, text: &str) -> Option<Vec<String>> {
    let expression = Regex::new(pattern).ok()?;
    let found = expression.captures(text)?;
    Some((0..found.len()).map(|index| found.get(index).map_or(String::new(), |m| m.as_str().to_string())).collect())
}

/// The program, arguments and environment a run of the installer gets, with its output after at most ten minutes.
fn run_installer(arguments: &[String]) -> Result<(i32, Vec<u8>, Vec<u8>), Failed> {
    let (program, rest) = arguments.split_first().ok_or_else(|| Failed("安装失败：OSError".into()))?;
    let mut child = Command::new(program)
        .args(rest)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| Failed("安装失败：OSError".into()))?;
    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    let out = std::thread::spawn(move || {
        let mut data = Vec::new();
        if let Some(stream) = stdout.as_mut() {
            let _ = stream.read_to_end(&mut data);
        }
        data
    });
    let err = std::thread::spawn(move || {
        let mut data = Vec::new();
        if let Some(stream) = stderr.as_mut() {
            let _ = stream.read_to_end(&mut data);
        }
        data
    });
    let deadline = Instant::now() + Duration::from_secs(600);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(200)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Failed("安装失败：TimeoutExpired".into()));
            }
        }
    };
    let code = status.code().unwrap_or(-1);
    Ok((code, out.join().unwrap_or_default(), err.join().unwrap_or_default()))
}

impl Runtime {
    fn plugin_dir(&self) -> String {
        let path = runtime::absolute(&self.plugin);
        Path::new(&path).parent().map(|parent| parent.to_string_lossy().into_owned()).unwrap_or_default()
    }

    /// A release found through the public pages when the anonymous API is rate-limited. The asset list the release
    /// page loads names each file with the SHA-256 GitHub computed, as the API would.
    fn self_update_web(&self) -> Option<Object> {
        let answer = http::send(
            &format!("{REPOSITORY}/releases/latest"),
            Request { headers: user_agent(), timeout: 8.0, follow_redirects: false, ..Request::default() },
        )
        .ok()?;
        let page = if (300..400).contains(&answer.status) {
            answer.header("Location").unwrap_or("").to_string()
        } else {
            format!("{REPOSITORY}/releases/latest")
        };
        let escaped = regex::escape(REPOSITORY);
        let found = search(&format!(r"^{escaped}/releases/tag/v(\d+\.\d+\.\d+)$"), &page)?;
        let version = found[1].clone();
        let latest = version_tuple(Some(&Json::from(version.as_str())))?;
        let current = version_tuple(Some(&Json::from(VERSION)))?;
        if current >= latest {
            return Some(object! { "latest" => version, "page" => page, "assets" => Json::Array(vec![]) });
        }
        let listing = download(&format!("{REPOSITORY}/releases/expanded_assets/v{version}"), 1024 * 1024).ok()?;
        let listing = String::from_utf8_lossy(&listing).into_owned();
        let path = reqwest::Url::parse(REPOSITORY).map(|url| url.path().to_string()).unwrap_or_default();
        let folder = format!("{path}/releases/download/v{version}/");
        let mut assets: Vec<Json> = Vec::new();
        for row in listing.split("<li").skip(1) {
            let (Some(link), Some(digest)) = (
                search(&format!("href=\"{}([\\w.+-]+)\"", regex::escape(&folder)), row),
                search(r"sha256:([0-9a-f]{64})\b", row),
            ) else {
                continue;
            };
            assets.push(
                object! {
                    "name" => link[1].as_str(), "url" => format!("{REPOSITORY}/releases/download/v{version}/{}", link[1]),
                    "digest" => format!("sha256:{}", digest[1]),
                }
                .into(),
            );
        }
        let asset = self.update_asset_name(&version);
        if !assets.iter().any(|entry| entry.get("name") == Some(&Json::from(asset.as_str()))) {
            return None;
        }
        Some(object! { "latest" => version, "page" => page, "assets" => Json::Array(assets) })
    }

    /// The latest GitHub release of this plugin: a plain GET with no identifiers. A draft or pre-release offers
    /// nothing.
    pub fn self_update_load(&self) -> Loaded {
        let mut headers = user_agent();
        headers.push(("Accept", "application/vnd.github+json".into()));
        let Ok(response) =
            http::send(RELEASES_API, Request { headers, timeout: 8.0, limit: 1024 * 1024 + 1, ..Request::default() })
        else {
            return (None, Some("检查失败".into()));
        };
        if !(200..300).contains(&response.status) {
            if [403, 429].contains(&response.status)
                && let Some(release) = self.self_update_web()
            {
                return (Some(release), None);
            }
            return if response.status == 404 {
                (Some(object! { "latest" => Json::Null }), None)
            } else {
                (None, Some("检查失败".into()))
            };
        }
        let release = (response.body.len() <= 1024 * 1024)
            .then(|| Json::parse(&response.body).ok().and_then(|value| value.as_object().cloned()))
            .flatten();
        let Some(release) = release else { return (None, Some("发布信息无法读取".into())) };
        let latest = version_tuple(release.get("tag_name"));
        let (Some(latest), false, false) = (
            latest,
            release.get("draft").is_some_and(Json::is_truthy),
            release.get("prerelease").is_some_and(Json::is_truthy),
        ) else {
            return (Some(object! { "latest" => Json::Null }), None);
        };
        let mut page = py::str(Some(&py::or(release.get("html_url"), Json::from(""))));
        if !page.starts_with(&format!("{REPOSITORY}/releases/")) {
            page = format!("{REPOSITORY}/releases");
        }
        let text = |value: Option<&Json>| py::str(Some(&py::or(value, Json::from(""))));
        let assets: Vec<Json> = release
            .get("assets")
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
            .filter_map(Json::as_object)
            .map(|asset| {
                object! {
                    "name" => text(asset.get("name")), "url" => text(asset.get("browser_download_url")),
                    "digest" => text(asset.get("digest")), "size" => asset.get("size").cloned().unwrap_or(Json::Null),
                }
                .into()
            })
            .collect();
        let version: Vec<String> = latest.iter().map(i64::to_string).collect();
        (Some(object! { "latest" => version.join("."), "page" => page, "assets" => Json::Array(assets) }), None)
    }

    pub fn self_update_cached(&self) {
        self.cached_fetch("self_update", external_ttl("self_update").unwrap_or(21600.0), 60.0, || {
            Ok(self.self_update_load())
        });
    }

    /// One release from this repository only: its published SHA-256 checked, unpacked privately, and the installer
    /// it ships run. Backups and the user's settings are the installer's job.
    fn install_release(&self, version: &str, log: &str) -> Result<(), Failed> {
        let cached =
            self.read_cache("self_update").get("payload").and_then(Json::as_object).cloned().unwrap_or_default();
        let mut release = self.plugin_update(&object! { "self_update" => cached });
        if release.as_ref().and_then(|release| release.get("latest")) != Some(&Json::from(version)) {
            let fresh = self.self_update_load().0.unwrap_or_default();
            release = self.plugin_update(&object! { "self_update" => fresh });
        }
        let release = release
            .filter(|release| release.get("latest") == Some(&Json::from(version)))
            .ok_or_else(|| Failed(format!("找不到版本 {version} 的发布")))?;
        let name = format!("token-monitor-{version}.zip");
        let expected = format!("{REPOSITORY}/releases/download/v{version}/{name}");
        let asset = release
            .get("assets")
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
            .filter_map(Json::as_object)
            .rfind(|asset| asset.get("name") == Some(&Json::from(name.as_str())))
            .filter(|asset| asset.get("url") == Some(&Json::from(expected.as_str())))
            .ok_or_else(|| Failed("发布中没有安装包".into()))?;
        // GitHub computes each asset's SHA-256 itself; releases publish no separate checksum files.
        let published = py::str(Some(&py::or(asset.get("digest"), Json::from(""))));
        let digest = published.strip_prefix("sha256:").ok_or_else(|| Failed("发布中没有校验值".into()))?;
        if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)) {
            return Err(Failed("校验值格式错误".into()));
        }
        let archive = match download(&expected, 50 * 1024 * 1024) {
            Ok(archive) => archive,
            Err(Some(failed)) => return Err(failed),
            Err(None) => return Err(Failed("安装失败：下载失败".into())),
        };
        if py::sha256_hex(&archive) != digest {
            return Err(Failed("安装包校验失败".into()));
        }
        let folder = std::env::temp_dir().join(format!("starmeter-update-{}", runtime::random_hex(12)));
        let result = (|| {
            let os_error = |_| Failed("安装失败：OSError".into());
            files::make_directory(&folder, 0o700).map_err(|_| Failed("安装失败：OSError".into()))?;
            let path = folder.join(&name);
            std::fs::write(&path, &archive).map_err(os_error)?;
            let path_text = path.to_string_lossy().into_owned();
            let root = format!("token-monitor-{version}/");
            let listing = shell::run(&["/usr/bin/zipinfo", "-1", &path_text], 60.0, None, None)
                .ok_or_else(|| Failed("安装失败：BadZipFile".into()))?;
            if listing
                .split('\n')
                .filter(|line| !line.is_empty())
                .any(|line| !line.starts_with(&root) || line.split('/').any(|part| part == ".."))
            {
                return Err(Failed("安装包结构异常".into()));
            }
            // ditto keeps the panel's executable bits.
            let folder_text = folder.to_string_lossy().into_owned();
            shell::run(&["/usr/bin/ditto", "-x", "-k", &path_text, &folder_text], 120.0, None, None)
                .ok_or_else(|| Failed("安装失败：CalledProcessError".into()))?;
            let unpacked = format!("{folder_text}/{root}");
            let own = format!("{unpacked}starmeter-install");
            let mut installer = if shell::is_executable(Path::new(&own)) {
                vec![own]
            } else {
                vec![
                    shell::which("python3", &self.environment).unwrap_or_else(|| "/usr/bin/python3".into()),
                    format!("{unpacked}install.py"),
                ]
            };
            installer.extend(["--plugin-dir".to_string(), self.plugin_dir(), "--no-refresh".to_string()]);
            let (code, out, err) = run_installer(&installer)?;
            if let Some(parent) = Path::new(log).parent() {
                files::make_directory(parent, 0o700).map_err(|_| Failed("安装失败：OSError".into()))?;
            }
            let text = format!(
                "Starmeter {VERSION} → {version}\nexit {code}\n\n{}\n{}",
                String::from_utf8_lossy(&out),
                String::from_utf8_lossy(&err)
            );
            platform::remove(Path::new(log));
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
            let mut file = options.open(log).map_err(os_error)?;
            file.write_all(text.as_bytes()).map_err(os_error)?;
            if code != 0 {
                return Err(Failed(format!("安装程序退出码 {code}")));
            }
            Ok(())
        })();
        let _ = std::fs::remove_dir_all(&folder);
        result
    }

    fn set_self_update_state(&self, state: Object) {
        let mut state = state;
        state.insert("at", now());
        let path = format!("{}/self-update.json", self.state_dir);
        let _ = files::make_directory(Path::new(&self.state_dir), 0o700);
        let _ = files::replace_atomically(
            Path::new(&path),
            Json::Object(state).serialized().as_bytes(),
            ".self-update-",
            || Ok(()),
        );
    }

    /// Detached from the menu run: one update at a time, then the panel and SwiftBar reload.
    pub fn self_update_worker(&self, version: &str) -> i32 {
        let _ = files::make_directory(Path::new(&self.state_dir), 0o700);
        let Some(_lock) = platform::lock(&Path::new(&self.state_dir).join("self-update.lock"), false) else { return 0 };
        self.set_self_update_state(object! { "status" => "running", "version" => version });
        match self.install_release(version, &format!("{}/logs/self-update.log", self.state_dir)) {
            Ok(()) => {
                self.set_self_update_state(object! { "status" => "done", "version" => version });
                self.invalidate_caches(&["self_update"]);
                // The running panel is the old build; it reopens from the new one on the next click.
                let quit = "if application id \"cc.opencodex.swiftbar.panel\" is running then tell application id \"cc.opencodex.swiftbar.panel\" to quit";
                let _ = shell::run(&["/usr/bin/osascript", "-e", quit], 8.0, None, None);
                self.notify(&format!("插件已更新到 {version}"), "Starmeter 插件更新");
            }
            Err(Failed(reason)) => {
                let reason = if reason.is_empty() { "更新失败".to_string() } else { reason };
                self.set_self_update_state(
                    object! { "status" => "failed", "version" => version, "reason" => reason.as_str() },
                );
                self.notify(&format!("插件更新失败：{reason}"), "详情见 SwiftBarToken/logs/self-update.log");
            }
        }
        self.refresh_swiftbar();
        0
    }

    // New app versions

    fn send_update_notification(&self, rows: &[Object]) -> bool {
        let body: Vec<String> =
            rows.iter().map(|row| format!("{} {}", py::str(row.get("name")), py::str(row.get("latest")))).collect();
        let body = body.join("；");
        if self.mac_app {
            return self.post_notice("应用有新版本", &body);
        }
        if let Some(notifier) = &self.notifier {
            notifier("应用有新版本", &body);
            return true;
        }
        let name = self.swiftbar_name();
        let refresh = format!("swiftbar://refreshplugin?name={}", accounts::quote(&name));
        let query: Vec<String> = [
            ("plugin", name.as_str()),
            ("title", "应用有新版本"),
            ("body", body.as_str()),
            ("subtitle", "打开星星 → Harness 更新查看"),
            ("silent", "true"),
            ("href", refresh.as_str()),
        ]
        .iter()
        .map(|(key, value)| format!("{key}={}", accounts::form_quote(value)))
        .collect();
        shell::run(&["/usr/bin/open", "-g", &format!("swiftbar://notify?{}", query.join("&"))], 5.0, None, None)
            .is_some()
    }

    /// Each new version is told once; what was told is kept in updates-notified.json.
    pub fn notify_app_updates(&self, rows: &[Object]) {
        if !self.module_enabled("updates") {
            return;
        }
        let selected = self.selected_update_ids();
        let rows: Vec<&Object> =
            rows.iter().filter(|row| selected.iter().any(|id| *id == py::str(row.get("id")))).collect();
        if rows.is_empty() {
            return;
        }
        let path = format!("{}/updates-notified.json", self.state_dir);
        let _ = files::make_directory(Path::new(&self.state_dir), 0o700);
        let Some(_lock) = platform::lock(&Path::new(&self.state_dir).join("updates.lock"), true) else { return };
        let mut seen = Object::new();
        if let Some(value) =
            std::fs::read(&path).ok().and_then(|data| Json::parse(&data).ok()).and_then(|v| v.as_object().cloned())
        {
            for (key, entry) in value.iter() {
                match entry {
                    Json::String(text) => seen.insert(key.clone(), Json::Array(vec![Json::from(text.as_str())])),
                    Json::Array(_) => seen.insert(key.clone(), entry.clone()),
                    _ => {}
                }
            }
        }
        let moment = now();
        let ttl = external_ttl("app_updates").unwrap_or(21600.0);
        let fresh: Vec<Object> = rows
            .into_iter()
            .filter(|row| {
                let id = py::str(row.get("id"));
                let Some(latest) = row.get("latest").filter(|latest| latest.is_truthy()) else { return false };
                if id == "opencodex" || !row.get("available").is_some_and(Json::is_truthy) {
                    return false;
                }
                if moment - py::or(row.get("checkedAt"), Json::Int(0)).number().unwrap_or(0.0) >= ttl {
                    return false;
                }
                let version = py::or(row.get("build"), latest.clone());
                !seen.get(&id).and_then(Json::as_array).is_some_and(|told| told.contains(&version))
            })
            .cloned()
            .collect();
        if !fresh.is_empty() && self.send_update_notification(&fresh) {
            for row in &fresh {
                let id = py::str(row.get("id"));
                let mut told = seen.get(&id).and_then(Json::as_array).cloned().unwrap_or_default();
                told.push(py::or(row.get("build"), row.get("latest").cloned().unwrap_or(Json::Null)));
                seen.insert(id, Json::Array(told));
            }
            self.write_json_cache(&path, &Json::Object(seen));
        }
    }

    /// What the background worker does once its reads are in: OpenCodex's own update, new-version notices, alerts.
    pub fn after_reads(&self) {
        if self.automatic_opencodex()
            && self.opencodex_configured()
            && (self.module_enabled("opencodex")
                || self.auto_update_state().get("status") == Some(&Json::from("running")))
        {
            self.auto_update_opencodex();
        }
        if self.module_enabled("updates") {
            self.notify_app_updates(&self.update_rows(&Object::new()));
        }
        self.check_usage_alerts();
    }
}
