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
pub const AUTHOR: &str = "timecatbit";

struct Failed(String);

/// Hosts that serve github.com through themselves (`{mirror}https://github.com/…`), for when GitHub can't be reached
/// (from mainland China, often): the release check and the download go through them in turn. Checked 2026-11-23: the
/// first two give the latest release's redirect and its asset list, all three serve the files.
pub const MIRRORS: [&str; 3] = ["https://ghfast.top/", "https://ghproxy.net/", "https://gh-proxy.com/"];

/// The mirrors to try: the user's own first (STARMETER_GITHUB_MIRRORS in config.local.sh, comma separated, https
/// only), each marked trusted alone; then the built-in ones, trusted only when two agree (the checksums come through
/// them too, when GitHub can't be reached at all).
pub fn mirror_list(own: Option<&str>) -> Vec<(String, bool)> {
    let mut list: Vec<(String, bool)> = Vec::new();
    for mirror in own.unwrap_or("").split(',').map(str::trim).filter(|mirror| mirror.starts_with("https://")) {
        let mirror = if mirror.ends_with('/') { mirror.to_string() } else { format!("{mirror}/") };
        if !list.iter().any(|(known, _)| *known == mirror) {
            list.push((mirror, true));
        }
    }
    for mirror in MIRRORS {
        if !list.iter().any(|(known, _)| known == mirror) {
            list.push((mirror.to_string(), false));
        }
    }
    list
}

/// A page as fetched: its status, where it redirects, its body.
pub(crate) type Fetched = (u16, Option<String>, String);

/// Fetches one source: the file, or with `probe` an empty body once a byte of it came; Err(None) when it couldn't be
/// reached, Err(Some(reason)) when it answered with something wrong.
pub(crate) type Getter<'a> = dyn Fn(&str, bool) -> Result<Vec<u8>, Option<String>> + 'a;

/// The asset list a release page loads: each file with the SHA-256 GitHub computed, as the API gives it.
fn listed_assets(listing: &str, version: &str) -> Vec<Json> {
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
    assets
}

/// The version a latest-release answer names: where it redirects (with a mirror's own prefix in front or not), or the
/// first release tag on the page it gives.
fn named_version(answer: &Fetched) -> Option<String> {
    let escaped = regex::escape(REPOSITORY);
    if let Some(found) =
        answer.1.as_deref().and_then(|to| search(&format!(r"{escaped}/releases/tag/v(\d+\.\d+\.\d+)$"), to))
    {
        return Some(found[1].clone());
    }
    let path = reqwest::Url::parse(REPOSITORY).map(|url| url.path().to_string()).unwrap_or_default();
    search(&format!(r"{}/releases/tag/v(\d+\.\d+\.\d+)", regex::escape(&path)), &answer.2).map(|found| found[1].clone())
}

/// The latest release read through mirrors, when GitHub can't be reached: its version from the latest page's redirect,
/// its files and their SHA-256 from the release's asset list. One of the user's own mirrors is taken at its word; the
/// built-in ones only when two give the same version and files with the same checksums, so one mirror alone can't
/// hand over a file of its own making with a checksum to match. A version no newer than this one needs no agreeing, but
/// the other mirrors are still asked: one whose cache is behind would otherwise hide a release the rest already have.
pub(crate) fn mirrored_release(
    mirrors: &[(String, bool)],
    current: &str,
    fetch: &dyn Fn(&str) -> Option<Fetched>,
) -> Option<Object> {
    let current = version_tuple(Some(&Json::from(current)))?;
    let mut seen: Vec<(String, Json, String)> = Vec::new();
    let mut no_newer: Option<Object> = None;
    for (mirror, own) in mirrors {
        let Some(version) = fetch(&format!("{mirror}{REPOSITORY}/releases/latest")).as_ref().and_then(named_version)
        else {
            continue;
        };
        let page = format!("{REPOSITORY}/releases/tag/v{version}");
        if version_tuple(Some(&Json::from(version.as_str())))? <= current {
            no_newer.get_or_insert(object! { "latest" => version, "page" => page, "assets" => Json::Array(vec![]) });
            continue;
        }
        let Some((200, _, listing)) = fetch(&format!("{mirror}{REPOSITORY}/releases/expanded_assets/v{version}"))
        else {
            continue;
        };
        let assets = Json::Array(listed_assets(&listing, &version));
        if assets.as_array().is_none_or(Vec::is_empty) {
            continue;
        }
        let agreed = *own || seen.iter().any(|(other, files, _)| *other == version && *files == assets);
        if agreed {
            return Some(object! { "latest" => version, "page" => page, "assets" => assets, "via" => mirror.as_str() });
        }
        seen.push((version, assets, mirror.clone()));
    }
    no_newer
}

/// The first of `sources` (one URL each) that gives a file with the SHA-256 `digest`: each but the last is tried with a
/// small request first, so one that can't be reached is passed over in seconds, not waited on. A file that doesn't
/// match is a bad copy, and the next source is tried.
pub(crate) fn checked_from(sources: &[String], digest: &str, get: &Getter) -> Result<Vec<u8>, String> {
    let mut failure: Option<String> = None;
    for (index, source) in sources.iter().enumerate() {
        if index + 1 < sources.len() && get(source, true).is_err() {
            continue;
        }
        match get(source, false) {
            Ok(data) if py::sha256_hex(&data) == digest => return Ok(data),
            Ok(_) => failure = Some("安装包校验失败".into()),
            Err(Some(reason)) => failure = Some(reason),
            Err(None) => {}
        }
    }
    Err(failure.unwrap_or_else(|| "下载失败：GitHub 和镜像都连不上".into()))
}

fn user_agent() -> Vec<(&'static str, String)> {
    vec![("User-Agent", format!("Starmeter/{VERSION}"))]
}

/// A body up to `limit` bytes from a 2xx answer within `timeout` seconds; Err(None) when the request itself failed.
fn download(url: &str, limit: usize, timeout: f64) -> Result<Vec<u8>, Option<Failed>> {
    let response = http::send(url, Request { headers: user_agent(), timeout, limit: limit + 1, ..Request::default() })
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
        let listing = download(&format!("{REPOSITORY}/releases/expanded_assets/v{version}"), 1024 * 1024, 30.0).ok()?;
        let listing = String::from_utf8_lossy(&listing).into_owned();
        let assets = listed_assets(&listing, &version);
        let asset = self.update_asset_name(&version);
        if !assets.iter().any(|entry| entry.get("name") == Some(&Json::from(asset.as_str()))) {
            return None;
        }
        Some(object! { "latest" => version, "page" => page, "assets" => Json::Array(assets) })
    }

    /// The mirrors this computer tries when GitHub can't be reached.
    pub fn github_mirrors(&self) -> Vec<(String, bool)> {
        mirror_list(self.environment.get("STARMETER_GITHUB_MIRRORS").map(String::as_str))
    }

    /// The latest release through the mirrors, for when GitHub doesn't answer.
    fn self_update_mirrored(&self) -> Loaded {
        let fetch = |url: &str| -> Option<Fetched> {
            let answer = http::send(
                url,
                Request {
                    headers: user_agent(),
                    timeout: 10.0,
                    follow_redirects: false,
                    limit: 2 * 1024 * 1024,
                    ..Request::default()
                },
            )
            .ok()?;
            let location = answer.header("Location").map(String::from);
            Some((answer.status, location, String::from_utf8_lossy(&answer.body).into_owned()))
        };
        match mirrored_release(&self.github_mirrors(), VERSION, &fetch) {
            Some(release) => (Some(release), None),
            None => (None, Some("检查失败".into())),
        }
    }

    /// The latest GitHub release of this plugin: a plain GET with no identifiers. A draft or pre-release offers
    /// nothing. GitHub out of reach (or failing), the mirrors.
    pub fn self_update_load(&self) -> Loaded {
        let mut headers = user_agent();
        headers.push(("Accept", "application/vnd.github+json".into()));
        let Ok(response) =
            http::send(RELEASES_API, Request { headers, timeout: 8.0, limit: 1024 * 1024 + 1, ..Request::default() })
        else {
            return self.self_update_mirrored();
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
                self.self_update_mirrored()
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
    /// A file of the release `version`, as GitHub serves it, checked against the SHA-256 GitHub computed for it (releases
    /// publish no checksum files of their own): the plugin's zip, or the app's own package for an update in place.
    pub(crate) fn release_asset(
        &self,
        version: &str,
        name: &str,
        limit: usize,
        timeout: f64,
    ) -> Result<Vec<u8>, String> {
        let cached =
            self.read_cache("self_update").get("payload").and_then(Json::as_object).cloned().unwrap_or_default();
        let mut release = self.plugin_update(&object! { "self_update" => cached });
        if release.as_ref().and_then(|release| release.get("latest")) != Some(&Json::from(version)) {
            let fresh = self.self_update_load().0.unwrap_or_default();
            release = self.plugin_update(&object! { "self_update" => fresh });
        }
        let release = release
            .filter(|release| release.get("latest") == Some(&Json::from(version)))
            .ok_or_else(|| format!("找不到版本 {version} 的发布"))?;
        checked_asset(&release, version, name, limit, timeout, &self.github_mirrors())
    }
}

/// The file `name` of the release `version` described by `release` ({"assets": [{"name", "url", "digest"}]}, as
/// self_update_load keeps it), downloaded from where GitHub serves it and checked against GitHub's SHA-256.
pub(crate) fn checked_asset(
    release: &Object,
    version: &str,
    name: &str,
    limit: usize,
    timeout: f64,
    mirrors: &[(String, bool)],
) -> Result<Vec<u8>, String> {
    let expected = format!("{REPOSITORY}/releases/download/v{version}/{name}");
    let asset = release
        .get("assets")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
        .filter_map(Json::as_object)
        .rfind(|asset| asset.get("name") == Some(&Json::from(name)))
        .filter(|asset| asset.get("url") == Some(&Json::from(expected.as_str())))
        .ok_or_else(|| "发布中没有安装包".to_string())?;
    let published = py::str(Some(&py::or(asset.get("digest"), Json::from(""))));
    let digest = published.strip_prefix("sha256:").ok_or_else(|| "发布中没有校验值".to_string())?;
    if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)) {
        return Err("校验值格式错误".into());
    }
    // GitHub first, then the mirrors; the mirror the release was read through first when GitHub couldn't be reached.
    let through = |mirror: &str| format!("{mirror}{expected}");
    let via = release.get("via").and_then(Json::as_str);
    let mut sources: Vec<String> = Vec::new();
    if let Some(via) = via {
        sources.push(through(via));
        sources.extend(mirrors.iter().filter(|(mirror, _)| mirror != via).map(|(mirror, _)| through(mirror)));
        sources.push(expected.clone());
    } else {
        sources.push(expected.clone());
        sources.extend(mirrors.iter().map(|(mirror, _)| through(mirror)));
    }
    let get = |url: &str, probe: bool| -> Result<Vec<u8>, Option<String>> {
        if probe {
            let mut headers = user_agent();
            headers.push(("Range", "bytes=0-0".into()));
            return http::send(url, Request { headers, timeout: 10.0, limit: 64 * 1024, ..Request::default() })
                .ok()
                .filter(|answer| (200..300).contains(&answer.status))
                .map(|_| Vec::new())
                .ok_or(None);
        }
        download(url, limit, timeout).map_err(|failed| failed.map(|Failed(reason)| reason))
    };
    checked_from(&sources, digest, &get)
}

impl Runtime {
    fn install_release(&self, version: &str, log: &str) -> Result<(), Failed> {
        let name = format!("starmeter-{version}.zip");
        // Ten minutes: through a slow mirror the 30 MB zip can take a few.
        let archive = self.release_asset(version, &name, 50 * 1024 * 1024, 600.0).map_err(Failed)?;
        let folder = std::env::temp_dir().join(format!("starmeter-update-{}", runtime::random_hex(12)));
        let result = (|| {
            let os_error = |_| Failed("安装失败：OSError".into());
            files::make_directory(&folder, 0o700).map_err(|_| Failed("安装失败：OSError".into()))?;
            let path = folder.join(&name);
            std::fs::write(&path, &archive).map_err(os_error)?;
            let path_text = path.to_string_lossy().into_owned();
            let root = format!("starmeter-{version}/");
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
        if let Some(desktop) = self.desktop() {
            desktop.notify(&body, "应用有新版本");
            return true;
        }
        let name = self.swiftbar_name();
        let refresh = format!("swiftbar://refreshplugin?name={}", accounts::quote(&name));
        let query: Vec<String> = [
            ("plugin", name.as_str()),
            ("title", "应用有新版本"),
            ("body", body.as_str()),
            ("subtitle", "打开星星 → 工具更新查看"),
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
        // 多台电脑合并: this computer's figures, at most every ten minutes.
        self.publish_device(false);
    }
}

#[cfg(test)]
mod mirror_tests {
    use super::*;

    fn listing(version: &str, digest: &str) -> String {
        format!(
            "<ul><li><a href=\"/timecatbit/starmeter/releases/download/v{version}/starmeter-{version}.zip\">zip</a> sha256:{digest}</li></ul>"
        )
    }

    /// A fake set of mirrors: each answers the latest page (by redirect or page) and the asset list as given.
    fn fetcher(answers: Vec<(&'static str, Option<Fetched>, Option<String>)>) -> impl Fn(&str) -> Option<Fetched> {
        move |url: &str| {
            for (mirror, latest, assets) in &answers {
                if url.starts_with(mirror) {
                    return if url.ends_with("/releases/latest") {
                        latest.clone()
                    } else {
                        assets.clone().map(|body| (200, None, body))
                    };
                }
            }
            None
        }
    }

    fn redirect(mirror: &str, version: &str) -> Option<Fetched> {
        Some((302, Some(format!("{mirror}{REPOSITORY}/releases/tag/v{version}")), String::new()))
    }

    #[test]
    fn two_mirrors_agreeing_give_the_release() {
        let good = "a".repeat(64);
        let mirrors = mirror_list(None);
        let fetch = fetcher(vec![
            ("https://ghfast.top/", redirect("https://ghfast.top/", "9.0.0"), Some(listing("9.0.0", &good))),
            ("https://ghproxy.net/", redirect("https://ghproxy.net/", "9.0.0"), Some(listing("9.0.0", &good))),
        ]);
        let release = mirrored_release(&mirrors, "6.0.14", &fetch).expect("agreed");
        assert_eq!(release.get("latest"), Some(&Json::from("9.0.0")));
        assert_eq!(release.get("via"), Some(&Json::from("https://ghproxy.net/")));
        let asset = release.get("assets").and_then(Json::as_array).and_then(|assets| assets.first()).cloned().unwrap();
        assert_eq!(asset.get("digest"), Some(&Json::from(format!("sha256:{good}").as_str())));
        assert_eq!(
            asset.get("url"),
            Some(&Json::from(format!("{REPOSITORY}/releases/download/v9.0.0/starmeter-9.0.0.zip").as_str()))
        );
    }

    #[test]
    fn one_mirror_alone_or_two_that_differ_give_nothing_newer() {
        let (good, forged) = ("a".repeat(64), "b".repeat(64));
        let mirrors = mirror_list(None);
        // One answers; the others don't.
        let fetch = fetcher(vec![(
            "https://ghfast.top/",
            redirect("https://ghfast.top/", "9.0.0"),
            Some(listing("9.0.0", &good)),
        )]);
        assert!(mirrored_release(&mirrors, "6.0.14", &fetch).is_none());
        // Two answer with different checksums: neither is taken.
        let fetch = fetcher(vec![
            ("https://ghfast.top/", redirect("https://ghfast.top/", "9.0.0"), Some(listing("9.0.0", &forged))),
            ("https://ghproxy.net/", redirect("https://ghproxy.net/", "9.0.0"), Some(listing("9.0.0", &good))),
        ]);
        assert!(mirrored_release(&mirrors, "6.0.14", &fetch).is_none());
        // A third agreeing with one of them settles it.
        let fetch = fetcher(vec![
            ("https://ghfast.top/", redirect("https://ghfast.top/", "9.0.0"), Some(listing("9.0.0", &forged))),
            ("https://ghproxy.net/", redirect("https://ghproxy.net/", "9.0.0"), Some(listing("9.0.0", &good))),
            (
                "https://gh-proxy.com/",
                Some((200, None, "<a href=\"/timecatbit/starmeter/releases/tag/v9.0.0\">".to_string())),
                Some(listing("9.0.0", &good)),
            ),
        ]);
        let release = mirrored_release(&mirrors, "6.0.14", &fetch).expect("two agree");
        assert!(py::str(release.get("assets")).contains(&good));
    }

    #[test]
    fn the_users_own_mirror_is_taken_alone_and_no_newer_version_needs_no_second() {
        let good = "a".repeat(64);
        let mirrors = mirror_list(Some("https://mirror.example/ , http://plain.example/"));
        assert_eq!(mirrors[0], ("https://mirror.example/".to_string(), true));
        assert!(mirrors.iter().all(|(mirror, _)| mirror.starts_with("https://")), "{mirrors:?}");
        let fetch = fetcher(vec![(
            "https://mirror.example/",
            redirect("https://mirror.example/", "9.0.0"),
            Some(listing("9.0.0", &good)),
        )]);
        assert_eq!(
            mirrored_release(&mirrors, "6.0.14", &fetch).and_then(|r| r.get("latest").cloned()),
            Some(Json::from("9.0.0"))
        );
        let fetch = fetcher(vec![("https://ghfast.top/", redirect("https://ghfast.top/", "6.0.14"), None)]);
        assert_eq!(
            mirrored_release(&mirror_list(None), "6.0.14", &fetch).and_then(|r| r.get("latest").cloned()),
            Some(Json::from("6.0.14"))
        );
    }

    #[test]
    fn a_mirror_whose_cache_is_behind_does_not_hide_a_release_the_others_have() {
        let good = "a".repeat(64);
        let fetch = fetcher(vec![
            ("https://ghfast.top/", redirect("https://ghfast.top/", "6.0.14"), None),
            ("https://ghproxy.net/", redirect("https://ghproxy.net/", "9.0.0"), Some(listing("9.0.0", &good))),
            ("https://gh-proxy.com/", redirect("https://gh-proxy.com/", "9.0.0"), Some(listing("9.0.0", &good))),
        ]);
        let release = mirrored_release(&mirror_list(None), "6.0.14", &fetch).expect("two agree");
        assert_eq!(release.get("latest"), Some(&Json::from("9.0.0")));
        // One mirror alone with something newer, beside one behind: nothing newer is taken.
        let fetch = fetcher(vec![
            ("https://ghfast.top/", redirect("https://ghfast.top/", "6.0.14"), None),
            ("https://ghproxy.net/", redirect("https://ghproxy.net/", "9.0.0"), Some(listing("9.0.0", &good))),
        ]);
        assert_eq!(
            mirrored_release(&mirror_list(None), "6.0.14", &fetch).and_then(|r| r.get("latest").cloned()),
            Some(Json::from("6.0.14"))
        );
    }

    #[test]
    fn the_file_comes_from_the_first_source_that_has_it_right() {
        let data = b"the package".to_vec();
        let digest = py::sha256_hex(&data);
        let sources: Vec<String> =
            ["https://github.com/x", "https://bad.example/x", "https://good.example/x"].map(String::from).to_vec();
        // GitHub out of reach, a mirror with a bad copy, then one with the right file.
        let get = |url: &str, _probe: bool| -> Result<Vec<u8>, Option<String>> {
            match url {
                "https://github.com/x" => Err(None),
                "https://bad.example/x" => Ok(b"something else".to_vec()),
                _ => Ok(data.clone()),
            }
        };
        assert_eq!(checked_from(&sources, &digest, &get), Ok(data.clone()));
        let none = |_: &str, _: bool| -> Result<Vec<u8>, Option<String>> { Err(None) };
        assert_eq!(checked_from(&sources, &digest, &none), Err("下载失败：GitHub 和镜像都连不上".into()));
        let bad = |_: &str, _: bool| -> Result<Vec<u8>, Option<String>> { Ok(b"forged".to_vec()) };
        assert_eq!(checked_from(&sources, &digest, &bad), Err("安装包校验失败".into()));
    }

    /// Over the network (run with --ignored): v6.0.14's zip through the built-in mirrors, checked against the SHA-256
    /// GitHub's API gives, and the latest release read through them as when GitHub can't be reached.
    #[test]
    #[ignore]
    fn the_mirrors_serve_a_release_and_its_files() {
        let version = "6.0.14";
        let name = format!("starmeter-{version}.zip");
        let answer = http::send(
            &format!("https://api.github.com/repos/timecatbit/starmeter/releases/tags/v{version}"),
            Request {
                headers: vec![("User-Agent", "Starmeter-test".into())],
                timeout: 30.0,
                limit: 4 << 20,
                ..Request::default()
            },
        )
        .expect("GitHub answers");
        let release = Json::parse(&answer.body).expect("a release");
        let digest = release
            .get("assets")
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
            .find(|asset| asset.get("name") == Some(&Json::from(name.as_str())))
            .and_then(|asset| asset.get("digest"))
            .and_then(Json::as_str)
            .and_then(|digest| digest.strip_prefix("sha256:"))
            .expect("a digest")
            .to_string();
        let url = format!("{REPOSITORY}/releases/download/v{version}/{name}");
        for mirror in MIRRORS {
            let data = download(&format!("{mirror}{url}"), 50 << 20, 120.0).ok().expect(mirror);
            assert_eq!(py::sha256_hex(&data), digest, "{mirror}");
        }
        let fetch = |url: &str| -> Option<Fetched> {
            let answer = http::send(
                url,
                Request {
                    headers: user_agent(),
                    timeout: 15.0,
                    follow_redirects: false,
                    limit: 2 << 20,
                    ..Request::default()
                },
            )
            .ok()?;
            Some((
                answer.status,
                answer.header("Location").map(String::from),
                String::from_utf8_lossy(&answer.body).into_owned(),
            ))
        };
        let read = mirrored_release(&mirror_list(None), "6.0.13", &fetch).expect("two mirrors agree");
        assert!(version_tuple(read.get("latest")) >= version_tuple(Some(&Json::from(version))));
        assert!(read.get("via").is_some());
    }
}
