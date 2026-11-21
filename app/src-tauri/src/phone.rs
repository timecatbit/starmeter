//! 在手机上看: a small read-only web server the app runs while the user asks for it, so a phone on the same network
//! can open the homepage by scanning a QR code. It serves the panel's own page and, behind a one-time key, the
//! homepage's figures; it never takes an action, and it leaves out the settings and every row's insides (folders, key
//! files, connection addresses). It stops when the user stops it, or an hour after it started.
//!
//! The key travels in the address's fragment (#k=…), which a browser never sends; the page sends it back in a header,
//! so it stays out of logs. The optional public link (Cloudflare's quick tunnel, see `tunnel`) reaches this same
//! server from outside and needs the same key.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{IpAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{Value, json};
use tauri::{AppHandle, Manager};

use crate::data::Data;

/// How long the public link stays open: it reaches the computer from anywhere. On the same network the phone view stays
/// until it is stopped or the app quits.
const PUBLIC_LIFETIME: Duration = Duration::from_secs(3600);
const RANGES: [&str; 8] = ["today", "yesterday", "24h", "7d", "30d", "week", "month", "all"];
const GROUPS: [&str; 3] = ["tool", "model", "project"];

#[derive(Default)]
pub struct Phone {
    running: Mutex<Option<Running>>,
}

struct Running {
    port: u16,
    key: String,
    stop: Arc<AtomicBool>,
    /// The public link, and when its hour is up.
    public: Option<(crate::tunnel::Tunnel, Instant)>,
}

/// What the panel shows: the addresses to scan, and how long the public link has left.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PhoneView {
    /// http://192.168.1.5:51234/#k=… on this network; None without one.
    local: Option<String>,
    /// https://….trycloudflare.com/#k=… once the public link is up.
    public: Option<String>,
    /// The public link's time left; 0 without one.
    seconds_left: u64,
}

fn view(running: &Running) -> PhoneView {
    let fragment = format!("#k={}", running.key);
    PhoneView {
        local: lan_address().map(|ip| format!("http://{ip}:{}/{fragment}", running.port)),
        public: running
            .public
            .as_ref()
            .filter(|(tunnel, _)| tunnel.alive())
            .map(|(tunnel, _)| format!("{}/{fragment}", tunnel.url)),
        seconds_left: running
            .public
            .as_ref()
            .filter(|(tunnel, _)| tunnel.alive())
            .map_or(0, |(_, until)| until.saturating_duration_since(Instant::now()).as_secs()),
    }
}

/// This computer's address on its network: the one its traffic would leave from. A UDP socket "connected" to an
/// outside address picks it without sending anything.
fn lan_address() -> Option<IpAddr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("192.0.2.1:9").ok()?;
    let ip = socket.local_addr().ok()?.ip();
    (!ip.is_loopback() && !ip.is_unspecified()).then_some(ip)
}

fn new_key() -> String {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("the system's random numbers");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Starts the server (or keeps the one running) and says where it is.
#[tauri::command]
pub fn phone_start(app: AppHandle) -> Result<PhoneView, String> {
    let state = app.state::<Phone>();
    let mut running = state.running.lock().expect("the phone lock");
    if let Some(current) = running.as_mut()
        && !current.stop.load(Ordering::SeqCst)
    {
        return Ok(view(current));
    }
    let listener = TcpListener::bind("0.0.0.0:0").map_err(|error| format!("没能开始：{error}"))?;
    let port = listener.local_addr().map_err(|error| error.to_string())?.port();
    listener.set_nonblocking(true).map_err(|error| error.to_string())?;
    let key = new_key();
    let stop = Arc::new(AtomicBool::new(false));
    {
        let (app, key, stop) = (app.clone(), key.clone(), stop.clone());
        std::thread::spawn(move || serve(app, listener, key, stop));
    }
    let started = Running { port, key, stop, public: None };
    let shown = view(&started);
    *running = Some(started);
    Ok(shown)
}

/// Where the phone view stands, if it is on.
#[tauri::command]
pub fn phone_status(app: AppHandle) -> Option<PhoneView> {
    let state = app.state::<Phone>();
    let mut running = state.running.lock().expect("the phone lock");
    if running.as_ref().is_some_and(|current| current.stop.load(Ordering::SeqCst)) {
        if let Some(mut over) = running.take() {
            stop_running(&mut over);
        }
        return None;
    }
    // A public link whose hour is up, or whose cloudflared has ended, goes; the panel says so.
    if let Some(current) = running.as_mut() {
        close_public_if_over(current);
    }
    running.as_ref().map(view)
}

/// Stops the server and the public link with it.
#[tauri::command]
pub fn phone_stop(app: AppHandle) {
    let state = app.state::<Phone>();
    if let Some(mut over) = state.running.lock().expect("the phone lock").take() {
        stop_running(&mut over);
    }
}

fn stop_running(running: &mut Running) {
    running.stop.store(true, Ordering::SeqCst);
    if let Some((mut tunnel, _)) = running.public.take() {
        tunnel.stop();
    }
}

/// Closes the public link once its hour is up or its cloudflared has ended; the server stays.
fn close_public_if_over(running: &mut Running) {
    if running.public.as_ref().is_some_and(|(tunnel, until)| !tunnel.alive() || Instant::now() >= *until)
        && let Some((mut over, _)) = running.public.take()
    {
        over.stop();
    }
}

/// Opens the public link (Cloudflare's quick tunnel) to the server already running, and says where it is.
#[tauri::command]
pub async fn phone_public(app: AppHandle) -> Result<PhoneView, String> {
    let port = {
        let state = app.state::<Phone>();
        let running = state.running.lock().expect("the phone lock");
        let Some(current) = running.as_ref().filter(|current| !current.stop.load(Ordering::SeqCst)) else {
            return Err("先打开手机查看".into());
        };
        if current.public.as_ref().is_some_and(|(tunnel, _)| tunnel.alive()) {
            return Ok(view(current));
        }
        current.port
    };
    let handle = app.clone();
    let tunnel = tauri::async_runtime::spawn_blocking(move || crate::tunnel::open(&handle, port))
        .await
        .map_err(|error| error.to_string())??;
    let state = app.state::<Phone>();
    let mut running = state.running.lock().expect("the phone lock");
    let Some(current) = running.as_mut().filter(|current| current.port == port) else {
        let mut tunnel = tunnel;
        tunnel.stop();
        return Err("手机查看已经关闭".into());
    };
    if let Some((mut old, _)) = current.public.replace((tunnel, Instant::now() + PUBLIC_LIFETIME)) {
        old.stop();
    }
    Ok(view(current))
}

/// Closes the public link and keeps the network one.
#[tauri::command]
pub fn phone_private(app: AppHandle) -> Option<PhoneView> {
    let state = app.state::<Phone>();
    let mut running = state.running.lock().expect("the phone lock");
    let current = running.as_mut()?;
    if let Some((mut tunnel, _)) = current.public.take() {
        tunnel.stop();
    }
    Some(view(current))
}

fn serve(app: AppHandle, listener: TcpListener, key: String, stop: Arc<AtomicBool>) {
    let mut checked = Instant::now();
    while !stop.load(Ordering::SeqCst) {
        // The public link's hour, looked at every few seconds whether or not the panel asks.
        if checked.elapsed() >= Duration::from_secs(5) {
            checked = Instant::now();
            if let Ok(mut running) = app.state::<Phone>().running.lock()
                && let Some(current) = running.as_mut().filter(|current| Arc::ptr_eq(&current.stop, &stop))
            {
                close_public_if_over(current);
            }
        }
        match listener.accept() {
            Ok((stream, _)) => {
                let (app, key) = (app.clone(), key.clone());
                std::thread::spawn(move || {
                    let _ = answer(&app, stream, &key);
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(80))
            }
            Err(_) => std::thread::sleep(Duration::from_millis(200)),
        }
    }
    // 停止 (or the app quitting): the public link goes with the server.
    let state = app.state::<Phone>();
    let mut running = state.running.lock().expect("the phone lock");
    if running.as_ref().is_some_and(|current| Arc::ptr_eq(&current.stop, &stop))
        && let Some(mut over) = running.take()
    {
        stop_running(&mut over);
    }
}

fn answer(app: &AppHandle, mut stream: TcpStream, key: &str) -> std::io::Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.set_write_timeout(Some(Duration::from_secs(20)))?;
    let mut reader = BufReader::new(stream.try_clone()?.take(16 * 1024));
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or("/"));
    let mut given = String::new();
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header)? == 0 || header.trim().is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':')
            && name.trim().eq_ignore_ascii_case("x-starmeter-key")
        {
            given = value.trim().to_string();
        }
    }
    if method != "GET" {
        return respond(&mut stream, 405, "text/plain; charset=utf-8", b"GET only", false);
    }
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    if let Some(call) = path.strip_prefix("/api/") {
        if !same(&given, key) {
            return respond(&mut stream, 403, "text/plain; charset=utf-8", b"key needed", true);
        }
        let range =
            query.split('&').find_map(|pair| pair.strip_prefix("range=")).filter(|range| RANGES.contains(range));
        // The phone's own grouping of the cards, as its range is its own: the computer's choices stay as they are.
        let group =
            query.split('&').find_map(|pair| pair.strip_prefix("group=")).filter(|group| GROUPS.contains(group));
        looking(app);
        let body = match call {
            "snapshot" => phone_snapshot(app, range, group),
            "dashboard" => dashboard(app, range.unwrap_or("today"), group),
            _ => return respond(&mut stream, 404, "text/plain; charset=utf-8", b"not here", true),
        };
        return respond(&mut stream, 200, "application/json; charset=utf-8", body.to_string().as_bytes(), true);
    }
    // The page itself: the panel's own files, the same for every phone, no figures in them.
    let file = path.trim_start_matches('/');
    let file = if file.is_empty() { "index.html" } else { file };
    if file.contains("..") {
        return respond(&mut stream, 404, "text/plain; charset=utf-8", b"not here", false);
    }
    match app.asset_resolver().get(file.to_string()) {
        Some(asset) => respond(&mut stream, 200, &asset.mime_type, &asset.bytes, false),
        None => respond(&mut stream, 404, "text/plain; charset=utf-8", b"not here", false),
    }
}

/// A phone is reading: for the next minute the figures keep the open panel's pace (a monitor's status is current
/// only for a minute and a half), and what is due is read now, at most every 30 seconds, so the phone's next look
/// finds it fresh.
fn looking(app: &AppHandle) {
    static ASKED: AtomicU64 = AtomicU64::new(0);
    let now = starmeter_core::runtime::now() as u64;
    crate::engine::PHONE_READ.store(now, Ordering::SeqCst);
    if now.saturating_sub(ASKED.load(Ordering::SeqCst)) < 30 {
        return;
    }
    ASKED.store(now, Ordering::SeqCst);
    match &*app.state::<Data>() {
        Data::Engine(engine) => engine.refresh(),
        // The plugin reads its pace from panel-state.json at its run each minute.
        Data::SwiftBar(files) => files.phone_looking(now as f64 + 60.0),
        Data::Fixtures(_) => {}
    }
}

/// The key compared in full whatever it holds.
fn same(given: &str, key: &str) -> bool {
    given.len() == key.len() && given.bytes().zip(key.bytes()).fold(0u8, |diff, (a, b)| diff | (a ^ b)) == 0
}

fn respond(stream: &mut TcpStream, status: u16, kind: &str, body: &[u8], fresh: bool) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        403 => "Forbidden",
        404 => "Not Found",
        _ => "Method Not Allowed",
    };
    let cache = if fresh { "no-store" } else { "max-age=300" };
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: {cache}\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()
}

fn dashboard(app: &AppHandle, range: &str, group: Option<&str>) -> Value {
    match &*app.state::<Data>() {
        Data::Engine(engine) => engine.dashboard_grouped(range, group),
        Data::SwiftBar(files) => files.snapshot().get("dashboard").cloned().unwrap_or(Value::Null),
        Data::Fixtures(fixtures) => fixtures.lock().expect("the fixtures").dashboard(range),
    }
}

/// What a phone sees: the homepage for the range it asked for, and nothing else. The settings name folders and key
/// files, and the rows' insides hold addresses and paths: both stay on the computer.
fn phone_snapshot(app: &AppHandle, range: Option<&str>, group: Option<&str>) -> Value {
    let mut snapshot = match &*app.state::<Data>() {
        Data::Engine(engine) => engine.snapshot(),
        Data::SwiftBar(files) => files.snapshot(),
        Data::Fixtures(fixtures) => fixtures.lock().expect("the fixtures").snapshot(),
    };
    if range.is_some() || group.is_some() {
        let current = snapshot.pointer("/dashboard/rangeKey").and_then(Value::as_str).unwrap_or("today").to_string();
        let wanted = range.unwrap_or(current.as_str());
        let asked = dashboard(app, wanted, group);
        if asked.get("rangeKey").and_then(Value::as_str) == Some(wanted) {
            snapshot["dashboard"] = asked;
            // 会话's figures and its note count a range too: the phone's, not the one the computer shows.
            if let Data::Engine(engine) = &*app.state::<Data>() {
                let mark = json!(["refresh-source", "sessions"]);
                let fresh = engine.sessions_entry(wanted);
                let entries = snapshot.get_mut("entries").and_then(Value::as_array_mut);
                if let (Some(children), Some(entry)) = (
                    fresh.get("children").cloned(),
                    entries
                        .and_then(|entries| entries.iter_mut().find(|entry| entry.get("onOpenAction") == Some(&mark))),
                ) {
                    entry["children"] = children;
                }
            }
            if let Some(options) = snapshot.get_mut("rangeOptions").and_then(Value::as_array_mut) {
                for option in options {
                    let mine =
                        option.get("action").and_then(|action| action.get(1)).and_then(Value::as_str) == Some(wanted);
                    option["checked"] = json!(mine);
                }
            }
        }
    }
    // The plugin's panel has the computer's range and grouping only: no choices on the phone that would do nothing
    // (a range it cannot read would leave the phone waiting, and its share page with nothing to save).
    if matches!(&*app.state::<Data>(), Data::SwiftBar(_)) {
        // The grouping the computer shows (its cards are grouped that way), not always 工具.
        let shown = snapshot.pointer("/dashboard/grouping").and_then(Value::as_str).unwrap_or("tool").to_string();
        if let Some(groupings) = snapshot.pointer_mut("/dashboard/groupings").and_then(Value::as_array_mut) {
            groupings.retain(|grouping| grouping.get("key").and_then(Value::as_str) == Some(shown.as_str()));
        }
        if let Some(options) = snapshot.get_mut("rangeOptions").and_then(Value::as_array_mut) {
            options.retain(|option| option.get("checked") == Some(&json!(true)));
        }
    }
    snapshot["moduleOptions"] = json!([]);
    if let Some(entries) = snapshot.get_mut("entries").and_then(Value::as_array_mut) {
        for entry in entries {
            // 会话 keeps its sessions (titles, models, tokens: no paths) for the phone's 会话 tab, and the mark the panel
            // knows the row by; its switch (这一轮完成时提醒) is the computer's.
            let mark = json!(["refresh-source", "sessions"]);
            let sessions = entry.get("onOpenAction") == Some(&mark);
            if let Some(object) = entry.as_object_mut() {
                let children = object.entry("children").or_insert_with(|| json!([]));
                match children.as_array_mut() {
                    Some(children) if sessions => {
                        children.retain(|child| child.get("action").is_none());
                        while children.last().is_some_and(|child| child.get("separator").is_some()) {
                            children.pop();
                        }
                    }
                    _ => *children = json!([]),
                }
            }
            strip(entry);
            if sessions {
                entry["onOpenAction"] = mark;
            }
        }
    }
    snapshot
}

/// Nothing to do on a phone: actions and links go, all the way down. The values' shorter forms stay, for a narrow
/// screen.
fn strip(entry: &mut Value) {
    if let Some(object) = entry.as_object_mut() {
        for name in ["action", "onOpenAction", "href"] {
            object.remove(name);
        }
        if let Some(children) = object.get_mut("children").and_then(Value::as_array_mut) {
            children.iter_mut().for_each(strip);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::same;

    #[test]
    fn the_key_in_full_and_nothing_less() {
        assert!(same("ab12", "ab12"));
        assert!(!same("ab1", "ab12"));
        assert!(!same("", "ab12"));
        assert!(!same("ab13", "ab12"));
    }
}
