//! The public link for 在手机上看: Cloudflare's quick tunnel (`cloudflared tunnel --url …`), a random
//! https://….trycloudflare.com address that reaches the phone server from anywhere, with no account. What passes
//! through it goes by way of Cloudflare, so the panel asks first, and the server still wants its key.
//!
//! cloudflared is the user's own when they have it (Homebrew's, or on the PATH); otherwise, with the user's yes, the
//! release for this computer is downloaded once from Cloudflare's GitHub releases, checked against the SHA-256 GitHub
//! lists for it, and kept in the app's data folder.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
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
    /// The pipe the watcher waits on (macOS, Linux): closed, it ends cloudflared.
    guard: Option<ChildStdin>,
    /// Cleared when cloudflared ends on its own (its connection to Cloudflare lost for good).
    alive: Arc<AtomicBool>,
    /// Seen to open from outside before it was shown (PHONE-11).
    pub checked: bool,
}

impl Tunnel {
    /// Whether cloudflared still runs: a link that has gone is not shown as one to scan.
    pub fn alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    pub fn stop(&mut self) {
        if let Some(child) = self.child.take() {
            end(child, self.guard.take());
        }
    }
}

/// Ends cloudflared: the watcher's pipe closed first, so it ends cloudflared itself; the watcher killed only if it
/// doesn't go within a moment.
fn end(mut child: Child, guard: Option<ChildStdin>) {
    drop(guard);
    let until = Instant::now() + Duration::from_secs(3);
    while Instant::now() < until {
        if child.try_wait().ok().flatten().is_some() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    // The watcher leads a process group of its own (command): killing the group takes cloudflared with it, where
    // killing the watcher alone would leave cloudflared running with no one to end it.
    #[cfg(unix)]
    let _ = Command::new("/bin/kill")
        .args(["-KILL", "--", &format!("-{}", child.id())])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let _ = child.kill();
    let _ = child.wait();
}

/// How cloudflared is run. On macOS and Linux under a small shell that waits on a pipe from this app: however the app
/// ends (quit, a crash, a force quit), the pipe closes and the shell ends cloudflared, so no tunnel outlives the app;
/// it had kept running when the app was killed, quietly enough over http2 never to notice. The shell lets go of the
/// error output it passed on, so cloudflared ending on its own still ends the stream read here.
#[cfg(unix)]
fn command(program: &Path) -> Command {
    use std::os::unix::process::CommandExt;
    let mut command = Command::new("/bin/sh");
    command
        .arg("-c")
        .arg(r#""$0" "$@" </dev/null & child=$!; exec 2>&-; read -r _; kill "$child" 2>/dev/null; wait "$child""#)
        .arg(program)
        .stdin(Stdio::piped())
        // Its own process group, which end can kill whole if the watcher doesn't go.
        .process_group(0);
    command
}

#[cfg(not(unix))]
fn command(program: &Path) -> Command {
    let mut command = Command::new(program);
    command.stdin(Stdio::null());
    command
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
        // Linux: the program itself, made executable once checked.
        ("linux", "x86_64") => Some("cloudflared-linux-amd64"),
        ("linux", "aarch64") => Some("cloudflared-linux-arm64"),
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
    // Written and unpacked under names of its own, then put in place in one step: the program is never seen half
    // written (TUNNEL-1).
    let work = folder.join(format!(".cloudflared-{}", starmeter_core::runtime::random_hex(8)));
    std::fs::create_dir_all(&work).map_err(|error| error.to_string())?;
    let placed = (|| -> Result<(), String> {
        let program = work.join(target.file_name().ok_or("找不到 App 的数据文件夹")?);
        if name.ends_with(".tgz") {
            let archive = work.join("cloudflared.tgz");
            std::fs::write(&archive, &file.body).map_err(|error| error.to_string())?;
            let unpacked = Command::new("/usr/bin/tar")
                .args(["-xzf"])
                .arg(&archive)
                .arg("-C")
                .arg(&work)
                .arg("cloudflared")
                .status()
                .is_ok_and(|status| status.success());
            if !unpacked || !program.is_file() {
                return Err("cloudflared 解压失败".into());
            }
        } else {
            std::fs::write(&program, &file.body).map_err(|error| error.to_string())?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755))
                .map_err(|error| error.to_string())?;
        }
        std::fs::rename(&program, &target).map_err(|error| error.to_string())
    })();
    let _ = std::fs::remove_dir_all(&work);
    placed.map(|()| target)
}

/// What cloudflared said on its way up: the address it was given, that Cloudflare took the connection, or that it
/// ended (and why, as the user reads it).
enum Heard {
    Address(String),
    Connected,
    Ended(String, bool),
}

/// Why cloudflared ended before the link worked, from what it printed last; and whether it was the edge's addresses it
/// couldn't look up (a proxy's or VPN's DNS that doesn't answer SRV queries), which naming the edge gets past.
fn reason(said: &[String]) -> (String, bool) {
    let all = said.join("\n");
    let unfound = all.contains("origintunneld") || all.contains("edge discovery");
    (message(&all, unfound), unfound)
}

fn message(all: &str, unfound: bool) -> String {
    if unfound {
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

/// Cloudflare's tunnel servers by name, for when they can't be looked up the usual way; over TCP (http2), which a proxy
/// carries where QUIC may not get through.
const NAMED_EDGE: [&str; 6] =
    ["--edge", "region1.v2.argotunnel.com:7844", "--edge", "region2.v2.argotunnel.com:7844", "--protocol", "http2"];

/// Starts the tunnel to the phone server and waits for its address. On a network whose DNS doesn't answer the SRV
/// query cloudflared finds Cloudflare's servers with (a proxy's or VPN's, though the network itself works), once more
/// with the servers named.
pub fn open(app: &AppHandle, port: u16) -> Result<Tunnel, String> {
    let program = match existing(app) {
        Some(program) => program,
        None => download(app)?,
    };
    match start(&program, port, &[]) {
        Err((_, true)) => start(&program, port, &NAMED_EDGE).map_err(|(why, _)| why),
        other => other.map_err(|(why, _)| why),
    }
}

fn start(program: &Path, port: u16, extra: &[&str]) -> Result<Tunnel, (String, bool)> {
    let mut command = command(program);
    command
        .args(["tunnel", "--no-autoupdate"])
        .args(extra)
        .args(["--url", &format!("http://127.0.0.1:{port}")])
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut child = command.spawn().map_err(|error| (format!("cloudflared 没能启动：{error}"), false))?;
    let guard = child.stdin.take();
    let Some(output) = child.stderr.take() else {
        end(child, guard);
        return Err(("cloudflared 没能启动".into(), false));
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
        let (why, unfound) = reason(&said);
        let _ = tell.send(Heard::Ended(why, unfound));
    });
    // Ready once both are heard, in whichever order they come (TUNNEL-2: a connection reported before the address
    // was dropped, and the link given up as not made after 45 seconds).
    let deadline = Instant::now() + Duration::from_secs(45);
    let (mut url, mut connected): (Option<String>, bool) = (None, false);
    let failed = loop {
        match heard.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(Heard::Address(given)) => url = Some(given),
            Ok(Heard::Connected) => connected = true,
            Ok(Heard::Ended(why, unfound)) => break (why, unfound),
            Err(_) => break ("Cloudflare 这次没有给出链接，稍后再试".into(), false),
        }
        if connected && let Some(url) = url.take() {
            return Ok(Tunnel { url, child: Some(child), guard, alive, checked: false });
        }
    };
    end(child, guard);
    Err(failed)
}

/// How the wait for the link to open ended.
#[derive(Debug, PartialEq, Eq)]
pub enum Reach {
    Opens,
    /// Stopped meanwhile, or cloudflared ended.
    Gone,
    /// Not seen to open in time: shown all the same, the panel saying a phone may have to wait a moment.
    Unconfirmed,
}

/// What Cloudflare's own resolver says of the link's name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Named {
    Yes,
    NotYet,
    /// Its resolver out of reach from this computer (blocked on its network).
    Unknown,
}

/// Measured on this network (2026-11-25): the name was in Cloudflare's resolver about 9 s after cloudflared had
/// connected, and the page opened through the link about 14 s after; the wait gives up well beyond.
const OPEN_LIMIT: Duration = Duration::from_secs(45);
/// With Cloudflare's resolver out of reach, the page is first fetched this long after the connection, when the name
/// has most likely been added (an earlier look-up by this computer's resolver could leave it "no such name").
const UNNAMED_GRACE: Duration = Duration::from_secs(15);

impl Tunnel {
    /// Waits until the link opens from outside, before it is shown (PHONE-11). A quick tunnel's name is added to
    /// trycloudflare.com's DNS some seconds after cloudflared prints it (the zone has no wildcard), and a phone that
    /// looks the name up first is told it doesn't exist, which its resolver keeps for up to a minute (the zone's
    /// negative TTL is 60 s; measured: AliDNS, asked once early, answered "no such name" for 70 s, and Google's flapped
    /// as long). So until the name is known only Cloudflare's own resolver is asked, never the ones phones use nor this
    /// computer's (its network's may be the phone's); Cloudflare's knew the name within seconds and didn't keep a "no".
    /// Then the server's check is fetched through the link with the key, until this computer's server answers it.
    pub fn confirm(&self, key: &str, stopped: &AtomicBool) -> Reach {
        let host = self.url.trim_start_matches("https://").to_string();
        wait_reachable(
            || cloudflare_knows(&host),
            || opens(&self.url, key),
            || stopped.load(Ordering::SeqCst) || !self.alive(),
            (OPEN_LIMIT, UNNAMED_GRACE, Duration::from_secs(1)),
        )
    }
}

/// The wait itself, its look-ups given: (limit, grace without Cloudflare's resolver, pause between tries).
fn wait_reachable(
    mut named: impl FnMut() -> Named,
    mut opens: impl FnMut() -> bool,
    gone: impl Fn() -> bool,
    (limit, grace, pause): (Duration, Duration, Duration),
) -> Reach {
    let started = Instant::now();
    let mut known = false;
    loop {
        if gone() {
            return Reach::Gone;
        }
        if started.elapsed() >= limit {
            return Reach::Unconfirmed;
        }
        if !known {
            known = match named() {
                Named::Yes => true,
                Named::NotYet => false,
                Named::Unknown => started.elapsed() >= grace,
            };
        }
        if known && opens() {
            return Reach::Opens;
        }
        std::thread::sleep(pause);
    }
}

/// Asks Cloudflare's resolver (DNS over HTTPS, by way of the system's proxy like every request) for the name.
fn cloudflare_knows(host: &str) -> Named {
    let request = Request {
        headers: vec![("Accept", "application/dns-json".into())],
        timeout: 3.0,
        follow_redirects: false,
        limit: 64 << 10,
        ..Request::default()
    };
    match http::send(&format!("https://cloudflare-dns.com/dns-query?name={host}&type=A"), request) {
        Ok(response) if response.status == 200 => named(&response.body),
        _ => Named::Unknown,
    }
}

/// A resolver's JSON answer: an address (or an alias) for the name, or a "no such name" or "nothing yet".
fn named(body: &[u8]) -> Named {
    let Ok(answer) = serde_json::from_slice::<serde_json::Value>(body) else { return Named::Unknown };
    let Some(status) = answer["Status"].as_u64() else { return Named::Unknown };
    let records = answer["Answer"].as_array().map_or(&[][..], Vec::as_slice);
    if status == 0 && records.iter().any(|record| matches!(record["type"].as_u64(), Some(1 | 5 | 28))) {
        Named::Yes
    } else {
        Named::NotYet
    }
}

/// Whether the server's check comes back through the link: this computer's own server answering, not one of
/// Cloudflare's pages (530 while the link isn't routed yet).
fn opens(url: &str, key: &str) -> bool {
    let request = Request {
        headers: vec![("X-Starmeter-Key", key.to_string())],
        timeout: 5.0,
        follow_redirects: false,
        limit: 1024,
        ..Request::default()
    };
    http::send(&format!("{url}/api/ping"), request)
        .is_ok_and(|response| response.status == 200 && response.body == crate::phone::PING)
}

#[cfg(test)]
mod tests {
    use super::{Named, Reach, address, named, reason, wait_reachable};
    use std::cell::{Cell, RefCell};
    use std::time::Duration;

    const QUICK: (Duration, Duration, Duration) = (Duration::from_secs(5), Duration::ZERO, Duration::from_millis(1));

    #[test]
    fn the_link_is_shown_once_it_opens_and_never_fetched_before_its_name_is_known() {
        // Cloudflare's resolver doesn't know the name twice, then does; the page answers on the second fetch.
        let said = RefCell::new(vec![Named::NotYet, Named::NotYet, Named::Yes]);
        let (asked, fetched) = (Cell::new(0), Cell::new(0));
        let reach = wait_reachable(
            || {
                asked.set(asked.get() + 1);
                let mut said = said.borrow_mut();
                if said.len() > 1 { said.remove(0) } else { said[0] }
            },
            || {
                // The page is fetched (this computer's resolver asked) only once the name is in DNS.
                assert!(asked.get() >= 3, "fetched before the name was known");
                fetched.set(fetched.get() + 1);
                fetched.get() == 2
            },
            || false,
            (Duration::from_secs(5), Duration::from_secs(5), Duration::from_millis(1)),
        );
        assert_eq!(reach, Reach::Opens);
        // Known once, the name isn't asked again.
        assert_eq!((asked.get(), fetched.get()), (3, 2));
    }

    #[test]
    fn without_cloudflares_resolver_the_page_is_fetched_after_the_grace() {
        let fetched = Cell::new(false);
        let reach = wait_reachable(
            || Named::Unknown,
            || {
                fetched.set(true);
                true
            },
            || false,
            QUICK,
        );
        assert_eq!(reach, Reach::Opens);
        assert!(fetched.get());
        // Before the grace, it isn't.
        let started = std::time::Instant::now();
        let reach = wait_reachable(
            || Named::Unknown,
            || started.elapsed() >= Duration::from_millis(30),
            || false,
            (Duration::from_secs(5), Duration::from_millis(30), Duration::from_millis(1)),
        );
        assert_eq!(reach, Reach::Opens);
        assert!(started.elapsed() >= Duration::from_millis(30));
    }

    #[test]
    fn a_link_stopped_or_ended_while_waiting_is_gone_and_one_never_opening_is_unconfirmed() {
        let stopped = Cell::new(0);
        let gone = || {
            stopped.set(stopped.get() + 1);
            stopped.get() > 3
        };
        assert_eq!(wait_reachable(|| Named::Yes, || false, gone, QUICK), Reach::Gone);
        let limit = (Duration::from_millis(20), Duration::ZERO, Duration::from_millis(1));
        assert_eq!(wait_reachable(|| Named::Yes, || false, || false, limit), Reach::Unconfirmed);
    }

    /// The whole wait against Cloudflare (PHONE-11): `STARMETER_CLOUDFLARED=<path> cargo test -p starmeter-app
    /// a_real_quick_tunnel -- --ignored --nocapture`. A stand-in server answers the check; once the wait says the link
    /// opens, AliDNS (which phones in mainland China often use, and which no one has asked yet) knows the name at once.
    #[test]
    #[ignore = "opens a real quick tunnel through Cloudflare"]
    fn a_real_quick_tunnel_is_shown_once_it_opens() {
        use std::io::{Read, Write};
        let program = std::path::PathBuf::from(std::env::var("STARMETER_CLOUDFLARED").expect("STARMETER_CLOUDFLARED"));
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for mut stream in listener.incoming().map_while(Result::ok) {
                let mut asked = [0u8; 2048];
                let _ = stream.read(&mut asked);
                let keyed = String::from_utf8_lossy(&asked).to_ascii_lowercase().contains("x-starmeter-key: k");
                let body: &[u8] = if keyed { crate::phone::PING } else { b"no" };
                let _ =
                    write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                let _ = stream.write_all(body);
            }
        });
        let started = std::time::Instant::now();
        let mut tunnel = super::start(&program, port, &super::NAMED_EDGE).unwrap_or_else(|(why, _)| panic!("{why}"));
        let up = started.elapsed();
        let reach = tunnel.confirm("k", &std::sync::atomic::AtomicBool::new(false));
        let opened = started.elapsed();
        let host = tunnel.url.trim_start_matches("https://").to_string();
        let ali = starmeter_core::http::send(
            &format!("https://dns.alidns.com/resolve?name={host}&type=A"),
            starmeter_core::http::Request { timeout: 5.0, ..Default::default() },
        )
        .map(|response| named(&response.body));
        tunnel.stop();
        eprintln!("{host}: up {up:?}, opens {opened:?}, AliDNS {ali:?}");
        assert_eq!(reach, Reach::Opens);
        assert_eq!(ali, Ok(Named::Yes));
    }

    #[test]
    fn a_resolvers_answer_says_whether_the_name_is_there() {
        let answer =
            br#"{"Status":0,"Answer":[{"name":"a.trycloudflare.com","type":1,"TTL":300,"data":"104.16.230.132"}]}"#;
        assert_eq!(named(answer), Named::Yes);
        let none = br#"{"Status":3,"Authority":[{"name":"trycloudflare.com","type":6,"TTL":60}]}"#;
        assert_eq!(named(none), Named::NotYet);
        assert_eq!(named(br#"{"Status":0}"#), Named::NotYet);
        assert_eq!(named(b"<html>blocked</html>"), Named::Unknown);
    }

    /// A stand-in cloudflared that says its two lines in the order given, then stays up.
    #[cfg(unix)]
    fn stand_in(lines: &[&str]) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path =
            std::env::temp_dir().join(format!("starmeter-cloudflared-{}", starmeter_core::runtime::random_hex(8)));
        let said: String = lines.iter().map(|line| format!("echo '{line}' >&2\n")).collect();
        std::fs::write(&path, format!("#!/bin/sh\n{said}sleep 30\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[cfg(unix)]
    #[test]
    fn the_link_is_ready_once_both_lines_are_heard_in_either_order() {
        // TUNNEL-2: the connection reported before the address used to be dropped, and the link waited 45 seconds.
        let address_line = "2026-11-23T01:00:00Z INF |  https://quiet-river-1234.trycloudflare.com  |";
        let connected = "2026-11-23T01:00:01Z INF Registered tunnel connection connIndex=0";
        for order in [[address_line, connected], [connected, address_line]] {
            let program = stand_in(&order);
            let started = std::time::Instant::now();
            let mut tunnel = super::start(&program, 9, &[]).unwrap_or_else(|(why, _)| panic!("{why}"));
            assert!(started.elapsed() < std::time::Duration::from_secs(10), "{:?}", started.elapsed());
            assert_eq!(tunnel.url, "https://quiet-river-1234.trycloudflare.com");
            tunnel.stop();
            let _ = std::fs::remove_file(&program);
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_cloudflared_that_wont_end_goes_with_its_watcher() {
        // One that ignores the watcher's TERM: after the wait its whole process group is killed, not the watcher alone.
        use std::os::unix::fs::PermissionsExt;
        let tag = starmeter_core::runtime::random_hex(8);
        let (program, noted) = (
            std::env::temp_dir().join(format!("starmeter-stubborn-{tag}")),
            std::env::temp_dir().join(format!("starmeter-stubborn-{tag}.pid")),
        );
        std::fs::write(
            &program,
            format!("#!/bin/sh\ntrap '' TERM\necho $$ > '{}'\nwhile :; do sleep 1; done\n", noted.display()),
        )
        .unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut child = super::command(&program).spawn().unwrap();
        let guard = child.stdin.take();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !noted.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let pid = std::fs::read_to_string(&noted).unwrap().trim().to_string();
        let alive = |pid: &str| std::process::Command::new("/bin/kill").args(["-0", pid]).status().unwrap().success();
        assert!(alive(&pid));
        super::end(child, guard);
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(!alive(&pid), "the stand-in outlived its watcher");
        let _ = std::fs::remove_file(&program);
        let _ = std::fs::remove_file(&noted);
    }

    #[test]
    fn why_cloudflared_ended() {
        let dns = "2026-11-20T01:58:31Z ERR edge discovery: error looking up Cloudflare edge IPs: the DNS query failed \
                   error=\"lookup _v2-origintunneld._tcp.argotunnel.com on 198.19.0.3:53: no such host\"";
        let (why, unfound) = reason(&[dns.into()]);
        assert!(why.contains("DNS") && unfound);
        let (why, unfound) = reason(&["ERR failed to request quick Tunnel: 429 Too Many Requests".into()]);
        assert!(why.contains("暂时不给") && !unfound);
        assert!(reason(&[]).0.contains("没有连上"));
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
