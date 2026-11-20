//! The public link for 在手机上看: Cloudflare's quick tunnel (`cloudflared tunnel --url …`), a random
//! https://….trycloudflare.com address that reaches the phone server from anywhere, with no account. What passes
//! through it goes by way of Cloudflare, so the panel asks first, and the server still wants its key.
//!
//! cloudflared is the user's own when they have it (Homebrew's, or on the PATH); otherwise, with the user's yes, the
//! release for this computer is downloaded once from Cloudflare's GitHub releases, checked against the SHA-256 GitHub
//! lists for it, and kept in the app's data folder.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use starmeter_core::http::{self, Request};
use tauri::{AppHandle, Manager};

const RELEASES: &str = "https://api.github.com/repos/cloudflare/cloudflared/releases/latest";

pub struct Tunnel {
    pub url: String,
    child: Option<Child>,
    /// Cleared when cloudflared ends on its own (its connection to Cloudflare lost for good).
    alive: Arc<AtomicBool>,
}

impl Tunnel {
    /// Whether cloudflared still runs: a link that has gone is not shown as one to scan.
    pub fn alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    pub fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        self.stop();
    }
}

fn program_name() -> &'static str {
    if cfg!(windows) { "cloudflared.exe" } else { "cloudflared" }
}

/// The one downloaded before, in the app's data folder.
fn kept(app: &AppHandle) -> Option<PathBuf> {
    Some(app.path().app_data_dir().ok()?.join("bin").join(program_name()))
}

/// The user's own cloudflared, or the one kept from before.
fn existing(app: &AppHandle) -> Option<PathBuf> {
    let mut places: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).map(|folder| folder.join(program_name())).collect())
        .unwrap_or_default();
    places.extend(["/opt/homebrew/bin/cloudflared", "/usr/local/bin/cloudflared"].map(PathBuf::from));
    places.extend(kept(app));
    places.into_iter().find(|place| place.is_file())
}

/// Whether the public link can start without a download first.
#[tauri::command]
pub fn tunnel_ready(app: AppHandle) -> bool {
    existing(&app).is_some()
}

/// This computer's release among Cloudflare's files.
fn asset_name() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Some("cloudflared-darwin-arm64.tgz"),
        ("macos", "x86_64") => Some("cloudflared-darwin-amd64.tgz"),
        ("windows", "x86_64") => Some("cloudflared-windows-amd64.exe"),
        ("windows", "aarch64") => Some("cloudflared-windows-arm64.exe"),
        _ => None,
    }
}

fn agent() -> Vec<(&'static str, String)> {
    vec![
        ("User-Agent", format!("Starmeter/{}", starmeter_core::VERSION)),
        ("Accept", "application/vnd.github+json".into()),
    ]
}

/// Downloads cloudflared once: the latest release's file for this computer, its SHA-256 checked against GitHub's.
fn download(app: &AppHandle) -> Result<PathBuf, String> {
    let name = asset_name().ok_or("这台电脑的系统没有对应的 cloudflared")?;
    let target = kept(app).ok_or("找不到 App 的数据文件夹")?;
    let release =
        http::send(RELEASES, Request { headers: agent(), timeout: 20.0, limit: 2 << 20, ..Request::default() })
            .map_err(|_| "连不上 GitHub，没能下载 cloudflared")?;
    let release: serde_json::Value = serde_json::from_slice(&release.body).map_err(|_| "GitHub 的发布信息读不懂")?;
    let asset = release
        .get("assets")
        .and_then(|assets| assets.as_array())
        .and_then(|assets| assets.iter().find(|asset| asset.get("name").and_then(|n| n.as_str()) == Some(name)))
        .ok_or("最新发布里没有这台电脑用的 cloudflared")?;
    let url = asset.get("browser_download_url").and_then(|u| u.as_str()).unwrap_or("");
    let digest = asset.get("digest").and_then(|d| d.as_str()).and_then(|d| d.strip_prefix("sha256:")).unwrap_or("");
    if !url.starts_with("https://github.com/cloudflare/cloudflared/releases/download/") || digest.len() != 64 {
        return Err("cloudflared 的下载信息不完整，没有下载".into());
    }
    let file = http::send(url, Request { headers: agent(), timeout: 180.0, limit: 120 << 20, ..Request::default() })
        .map_err(|_| "下载 cloudflared 失败")?;
    if !(200..300).contains(&file.status) || starmeter_core::py::sha256_hex(&file.body) != digest.to_ascii_lowercase() {
        return Err("下载的 cloudflared 校验不通过，已丢弃".into());
    }
    let folder = target.parent().ok_or("找不到 App 的数据文件夹")?;
    std::fs::create_dir_all(folder).map_err(|error| error.to_string())?;
    if name.ends_with(".tgz") {
        let archive = folder.join(".cloudflared.tgz");
        std::fs::write(&archive, &file.body).map_err(|error| error.to_string())?;
        let unpacked = Command::new("/usr/bin/tar")
            .args(["-xzf"])
            .arg(&archive)
            .arg("-C")
            .arg(folder)
            .arg("cloudflared")
            .status()
            .is_ok_and(|status| status.success());
        let _ = std::fs::remove_file(&archive);
        if !unpacked {
            return Err("cloudflared 解压失败".into());
        }
    } else {
        std::fs::write(&target, &file.body).map_err(|error| error.to_string())?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755)).map_err(|error| error.to_string())?;
    }
    Ok(target)
}

/// What cloudflared said on its way up: the address it was given, that Cloudflare took the connection, or that it
/// ended (and why, as the user reads it).
enum Heard {
    Address(String),
    Connected,
    Ended(String),
}

/// Why cloudflared ended before the link worked, from what it printed last.
fn reason(said: &[String]) -> String {
    let all = said.join("\n");
    if all.contains("origintunneld") || all.contains("edge discovery") {
        "这个网络的 DNS 查不到 Cloudflare 隧道的地址（常见于代理或 VPN），换个网络或关掉代理再试".into()
    } else if all.contains("429") || all.contains("Too Many Requests") {
        "Cloudflare 暂时不给新的临时链接，稍后再试".into()
    } else {
        "Cloudflare 这次没有连上，稍后再试".into()
    }
}

/// The address cloudflared prints once the tunnel is up.
fn address(line: &str) -> Option<String> {
    let start = line.find("https://")?;
    let rest = &line[start..];
    let end = rest.find(|c: char| c.is_whitespace() || c == '|').unwrap_or(rest.len());
    let url = &rest[..end];
    (url.ends_with(".trycloudflare.com") && url.len() > "https://.trycloudflare.com".len()).then(|| url.to_string())
}

/// Starts the tunnel to the phone server and waits for its address.
pub fn open(app: &AppHandle, port: u16) -> Result<Tunnel, String> {
    let program = match existing(app) {
        Some(program) => program,
        None => download(app)?,
    };
    start(&program, port)
}

fn start(program: &Path, port: u16) -> Result<Tunnel, String> {
    let mut command = Command::new(program);
    command
        .args(["tunnel", "--no-autoupdate", "--url", &format!("http://127.0.0.1:{port}")])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut child = command.spawn().map_err(|error| format!("cloudflared 没能启动：{error}"))?;
    let Some(output) = child.stderr.take() else {
        let _ = child.kill();
        return Err("cloudflared 没能启动".into());
    };
    let (tell, heard) = mpsc::channel();
    let alive = Arc::new(AtomicBool::new(true));
    let running = alive.clone();
    // Read to the end, so cloudflared never blocks on a full pipe: the first address is the one, and the link works
    // once Cloudflare has registered a connection. The address comes first, before cloudflared has reached Cloudflare
    // at all: on a network whose DNS can't find Cloudflare's servers it ends right after.
    std::thread::spawn(move || {
        let (mut addressed, mut connected) = (false, false);
        let mut said: Vec<String> = Vec::new();
        for line in BufReader::new(output).lines().map_while(Result::ok) {
            if !addressed && let Some(url) = address(&line) {
                addressed = true;
                let _ = tell.send(Heard::Address(url));
            }
            if !connected && line.contains("Registered tunnel connection") {
                connected = true;
                let _ = tell.send(Heard::Connected);
            }
            if line.contains(" ERR ") {
                said.push(line);
                if said.len() > 20 {
                    said.remove(0);
                }
            }
        }
        running.store(false, Ordering::SeqCst);
        let _ = tell.send(Heard::Ended(reason(&said)));
    });
    let deadline = Instant::now() + Duration::from_secs(45);
    let mut url: Option<String> = None;
    let failed = loop {
        match heard.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(Heard::Address(given)) => url = Some(given),
            Ok(Heard::Connected) if url.is_some() => {
                return Ok(Tunnel { url: url.unwrap_or_default(), child: Some(child), alive });
            }
            Ok(Heard::Connected) => {}
            Ok(Heard::Ended(why)) => break why,
            Err(_) => break "Cloudflare 这次没有给出链接，稍后再试".into(),
        }
    };
    let _ = child.kill();
    let _ = child.wait();
    Err(failed)
}

#[cfg(test)]
mod tests {
    use super::{address, reason};

    #[test]
    fn why_cloudflared_ended() {
        let dns = "2026-11-20T01:58:31Z ERR edge discovery: error looking up Cloudflare edge IPs: the DNS query failed \
                   error=\"lookup _v2-origintunneld._tcp.argotunnel.com on 198.19.0.3:53: no such host\"";
        assert!(reason(&[dns.into()]).contains("DNS"));
        assert!(reason(&["ERR failed to request quick Tunnel: 429 Too Many Requests".into()]).contains("暂时不给"));
        assert!(reason(&[]).contains("没有连上"));
    }

    #[test]
    fn the_address_cloudflared_prints() {
        let line =
            "2026-11-19T14:00:00Z INF |  https://quiet-river-1234.trycloudflare.com                                  |";
        assert_eq!(address(line).as_deref(), Some("https://quiet-river-1234.trycloudflare.com"));
        assert_eq!(address("INF Requesting new quick Tunnel on trycloudflare.com..."), None);
        assert_eq!(address("see https://developers.cloudflare.com/x"), None);
    }
}
