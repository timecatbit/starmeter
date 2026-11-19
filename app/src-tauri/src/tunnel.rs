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
use std::sync::mpsc;
use std::time::Duration;

use starmeter_core::http::{self, Request};
use tauri::{AppHandle, Manager};

const RELEASES: &str = "https://api.github.com/repos/cloudflare/cloudflared/releases/latest";

pub struct Tunnel {
    pub url: String,
    child: Option<Child>,
}

impl Tunnel {
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
    let (found, heard) = mpsc::channel();
    // Read to the end, so cloudflared never blocks on a full pipe; the first address is the one.
    std::thread::spawn(move || {
        let mut sent = false;
        for line in BufReader::new(output).lines().map_while(Result::ok) {
            if !sent && let Some(url) = address(&line) {
                sent = found.send(url).is_ok();
            }
        }
    });
    match heard.recv_timeout(Duration::from_secs(45)) {
        Ok(url) => Ok(Tunnel { url, child: Some(child) }),
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            Err("Cloudflare 这次没有给出链接，稍后再试".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::address;

    #[test]
    fn the_address_cloudflared_prints() {
        let line =
            "2026-11-19T14:00:00Z INF |  https://quiet-river-1234.trycloudflare.com                                  |";
        assert_eq!(address(line).as_deref(), Some("https://quiet-river-1234.trycloudflare.com"));
        assert_eq!(address("INF Requesting new quick Tunnel on trycloudflare.com..."), None);
        assert_eq!(address("see https://developers.cloudflare.com/x"), None);
    }
}
