//! 在手机上看: a small read-only web server the app runs while the user asks for it, so a phone on the same network
//! can open the homepage by scanning a QR code. It serves the panel's own page and, behind a one-time key, the
//! homepage's figures; it never takes an action, and it leaves out the settings and every row's insides (folders, key
//! files, connection addresses). It stops when the user stops it, or an hour after it started.
//!
//! The key travels in the address's fragment (#k=…), which a browser never sends; the page sends it back in a header,
//! so it stays out of logs. The optional public link (Cloudflare's quick tunnel, see `tunnel`) reaches this same
//! server from outside and needs the same key.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{IpAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
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
/// At most this many connections answered at once (a phone loads the page's few files side by side); one more is
/// closed straight away, so a burst, from a scan of the public link say, can't use up the threads (PHONE-7).
const CONNECTIONS: usize = 24;
/// A connection's whole request read within this; a client trickling a byte now and then is let go.
const REQUEST_TIME: Duration = Duration::from_secs(15);

/// A request read with one deadline for all of it: each read waits only for what is left of it, so a client trickling
/// a byte now and then (each byte used to start the socket's ten seconds again) is let go at the deadline.
struct Deadline {
    stream: TcpStream,
    until: Instant,
}

impl Read for Deadline {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let left = self.until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(std::io::ErrorKind::TimedOut.into());
        }
        self.stream.set_read_timeout(Some(left.min(Duration::from_secs(10))))?;
        self.stream.read(buf)
    }
}

/// The connections being answered: a slot taken for each, given back when its answer ends, however it ends.
struct Slots(Arc<AtomicUsize>);

struct Slot(Arc<AtomicUsize>);

impl Slots {
    fn take(&self) -> Option<Slot> {
        let mut taken = self.0.load(Ordering::SeqCst);
        while taken < CONNECTIONS {
            match self.0.compare_exchange_weak(taken, taken + 1, Ordering::SeqCst, Ordering::SeqCst) {
                Ok(_) => return Some(Slot(self.0.clone())),
                Err(now) => taken = now,
            }
        }
        None
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

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

/// A public link still to be used: its cloudflared running and its hour not up. Past the hour it is closed within a few
/// seconds (serve looks every five); until then it isn't offered, with no time left on it.
fn usable((tunnel, until): &(crate::tunnel::Tunnel, Instant)) -> bool {
    tunnel.alive() && Instant::now() < *until
}

fn view(running: &Running) -> PhoneView {
    let fragment = format!("#k={}", running.key);
    PhoneView {
        local: lan_address().map(|ip| format!("http://{ip}:{}/{fragment}", running.port)),
        public: running
            .public
            .as_ref()
            .filter(|public| usable(public))
            .map(|(tunnel, _)| format!("{}/{fragment}", tunnel.url)),
        seconds_left: running
            .public
            .as_ref()
            .filter(|public| usable(public))
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
    if running.public.as_ref().is_some_and(|public| !usable(public))
        && let Some((mut over, _)) = running.public.take()
    {
        over.stop();
    }
}

/// One public link opened at a time: an ask while another is on its way waits for it and takes its link, so two quick
/// clicks start one cloudflared, and a first download isn't written twice at once (TUNNEL-1).
static OPENING: Mutex<()> = Mutex::new(());

/// Opens the public link (Cloudflare's quick tunnel) to the server already running, and says where it is. The link
/// belongs to the round it was asked in: stopped and started again meanwhile, it is closed, not handed to the new one.
#[tauri::command]
pub async fn phone_public(app: AppHandle) -> Result<PhoneView, String> {
    let (port, round) = {
        let state = app.state::<Phone>();
        let running = state.running.lock().expect("the phone lock");
        let Some(current) = running.as_ref().filter(|current| !current.stop.load(Ordering::SeqCst)) else {
            return Err("先打开手机查看".into());
        };
        if current.public.as_ref().is_some_and(usable) {
            return Ok(view(current));
        }
        (current.port, current.stop.clone())
    };
    let (handle, asked) = (app.clone(), round.clone());
    let opened = tauri::async_runtime::spawn_blocking(move || -> Result<Option<crate::tunnel::Tunnel>, String> {
        let _turn = OPENING.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        // The link an earlier ask of this round opened while this one waited serves it too.
        // Stopped while it waited its turn: nothing to open a link for.
        if asked.load(Ordering::SeqCst) {
            return Ok(None);
        }
        let state = handle.state::<Phone>();
        let served =
            state.running.lock().expect("the phone lock").as_ref().is_some_and(|current| {
                Arc::ptr_eq(&current.stop, &asked) && current.public.as_ref().is_some_and(usable)
            });
        if served {
            return Ok(None);
        }
        crate::tunnel::open(&handle, port).map(Some)
    })
    .await
    .map_err(|error| error.to_string())??;
    let state = app.state::<Phone>();
    let mut running = state.running.lock().expect("the phone lock");
    let Some(current) =
        running.as_mut().filter(|current| Arc::ptr_eq(&current.stop, &round) && !round.load(Ordering::SeqCst))
    else {
        if let Some(mut tunnel) = opened {
            tunnel.stop();
        }
        return Err("手机查看已经关闭".into());
    };
    if let Some(tunnel) = opened
        && let Some((mut old, _)) = current.public.replace((tunnel, Instant::now() + PUBLIC_LIFETIME))
    {
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
    let slots = Slots(Arc::new(AtomicUsize::new(0)));
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
            // Past the limit, or with no thread to be had, the connection is closed and the server goes on.
            Ok((stream, _)) => {
                let Some(slot) = slots.take() else { continue };
                let (app, key) = (app.clone(), key.clone());
                let _ = std::thread::Builder::new().name("starmeter-phone".into()).spawn(move || {
                    let _slot = slot;
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
    let started = Instant::now();
    let mut reader =
        BufReader::new(Deadline { stream: stream.try_clone()?, until: started + REQUEST_TIME }.take(16 * 1024));
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or("/"));
    let mut given = String::new();
    loop {
        if started.elapsed() > REQUEST_TIME {
            return Err(std::io::ErrorKind::TimedOut.into());
        }
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
            "live" => live(app, query.split('&').any(|pair| pair == "history=1")),
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
        // The page itself carries the computer's look as it is now, for its first frame (first-frame.js): iOS Safari
        // takes its bars' colours from that frame and keeps them.
        Some(asset) if file == "index.html" => {
            let page = with_look(&String::from_utf8_lossy(&asset.bytes), &computer_look(&app.state::<Data>()));
            respond(&mut stream, 200, &asset.mime_type, page.as_bytes(), false)
        }
        Some(asset) => respond(&mut stream, 200, &asset.mime_type, &asset.bytes, false),
        None => respond(&mut stream, 404, "text/plain; charset=utf-8", b"not here", false),
    }
}

/// The computer's theme (system, light, dark) and look (标准, 手绘) as the phone's page should first draw them.
fn computer_look(data: &Data) -> Value {
    let theme = match data {
        Data::Engine(engine) => Value::from(
            engine.runtime().settings().get("theme").and_then(starmeter_core::json::Json::as_str).unwrap_or("system"),
        ),
        Data::SwiftBar(files) => files.peek().get("theme").cloned().unwrap_or(Value::Null),
        Data::Fixtures(fixtures) => {
            fixtures.lock().expect("the fixtures").snapshot().get("theme").cloned().unwrap_or(Value::Null)
        }
    };
    let theme =
        theme.as_str().filter(|theme| ["system", "light", "dark"].contains(theme)).unwrap_or("system").to_string();
    json!({"theme": theme, "style": crate::style::current()})
}

/// The page with the computer's look written in before its first script runs.
fn with_look(page: &str, look: &Value) -> String {
    let script = format!("<script>window.STARMETER_LOOK={look};</script>\n    ");
    match page.find("<script src=\"/first-frame.js\"") {
        Some(at) => format!("{}{script}{}", &page[..at], &page[at..]),
        None => page.to_string(),
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
    // The computer's look (标准 or 手绘): the phone follows it, as it follows the theme and the background.
    snapshot["style"] = json!(crate::style::current());
    if let Some(entries) = snapshot.get_mut("entries").and_then(Value::as_array_mut) {
        for entry in entries {
            // 会话 keeps its sessions for the phone's 会话 tab, and the mark the panel knows the row by; every other row
            // only itself.
            let mark = json!(["refresh-source", "sessions"]);
            if entry.get("onOpenAction") == Some(&mark) {
                phone_sessions(entry);
                entry["onOpenAction"] = mark;
            } else {
                if let Some(object) = entry.as_object_mut() {
                    object.insert("children".into(), json!([]));
                }
                strip(entry);
            }
        }
    }
    snapshot
}

/// The live readings of the last few minutes, today's figures by tool in short (what a live line counts), so a phone's
/// page opened or reloaded while the work goes on, the panel's 实时 opened, or the panel pinned, has its last minutes at
/// once: not a line starting from nothing, a speed of 0 while the sessions run, an empty 动态. Kept here for every one of
/// them alike, whatever the figures come from (the app's own logs, SwiftBar's plugin, the fixtures): from each live
/// answer, and from the app's start on its own (`keep_recording`), phone view on or not. A phone asks for them with
/// ?history=1, the panel with live_now's `history` or recent_readings. With 会话 off the homepage they carry none of it.
static RECENT: Mutex<VecDeque<Value>> = Mutex::new(VecDeque::new());
/// How long the readings are kept (a page's line spans five minutes, its speed the minute before), and how far apart:
/// every couple of seconds while someone looks (the panel showing, the phone view on), every ten otherwise (the figures
/// the reading takes change only as the app refreshes them, about once a minute while nobody looks), well inside the
/// 15 seconds a page takes for a gap.
const RECENT_FOR: f64 = 400.0;
const RECENT_EVERY: f64 = 2.0;
const RECENT_IDLE: f64 = 10.0;

/// The last minutes recorded from the app's start: a reading every couple of seconds while the panel shows or the
/// phone view is on, every ten otherwise. Cheap (STAGE-14): today's figures from the app's cache only, rendered only
/// when that cache was written again (otherwise the last reading again, at this moment), never a read of the logs, never
/// waiting on a render under way; whether the panel shows from its flag, not asked of the main thread.
pub fn keep_recording(app: AppHandle) {
    let _ = std::thread::Builder::new().name("starmeter-live-readings".into()).spawn(move || {
        let mut seen = 0.0;
        loop {
            sample_cached(&app.state::<Data>(), &mut seen);
            let watched = crate::engine::PANEL_OPEN.load(Ordering::SeqCst)
                || app
                    .state::<Phone>()
                    .running
                    .lock()
                    .is_ok_and(|running| running.as_ref().is_some_and(|r| !r.stop.load(Ordering::SeqCst)));
            std::thread::sleep(Duration::from_secs_f64(if watched { RECENT_EVERY } else { RECENT_IDLE }));
        }
    });
}

/// A reading for the last minutes from what the source has cached: the app's today when its cache was written again
/// (else the last reading's figures again, so the readings run on without a gap), SwiftBar's snapshot looked at, the
/// fixtures. Nothing when the app is busy rendering: the next turn takes it.
fn sample_cached(data: &Data, seen: &mut f64) {
    let Data::Engine(engine) = data else { return sample(data) };
    let Some(mut sessions) = engine.sessions_if_free() else { return };
    if sessions.is_object() {
        phone_sessions(&mut sessions);
    }
    let sessions = sessions.is_object().then_some(&sessions);
    let mut recent = RECENT.lock().expect("the live readings");
    let dashboard = match engine.today_cached(seen) {
        Some(dashboard) => dashboard,
        None => match recent.back() {
            Some(last) => json!({"date": last["date"], "rangeKey": last["rangeKey"], "providers": last["providers"]}),
            None => return,
        },
    };
    keep_recent(&mut recent, starmeter_core::runtime::now(), &dashboard, sessions);
}

/// A reading kept: when, the day and range, and each tool's figures the line counts (Summary's share, what a gateway
/// alone logged).
fn keep_recent(recent: &mut VecDeque<Value>, moment: f64, dashboard: &Value, sessions: Option<&Value>) {
    let Some(providers) = dashboard.get("providers").and_then(Value::as_array) else { return };
    if recent.back().and_then(|last| last["at"].as_f64()).is_some_and(|at| moment - at < RECENT_EVERY && moment >= at) {
        return;
    }
    let short: Vec<Value> = providers
        .iter()
        .map(|p| {
            let mut kept = serde_json::Map::new();
            for name in ["id", "available", "summaryIncluded", "tokens", "output", "cost"] {
                if let Some(value) = p.get(name) {
                    kept.insert(name.into(), value.clone());
                }
            }
            if let Some(counted) = p.get("counted").filter(|counted| counted.is_object()) {
                let part: serde_json::Map<String, Value> = ["tokens", "output", "cost"]
                    .iter()
                    .filter_map(|name| counted.get(*name).map(|value| (name.to_string(), value.clone())))
                    .collect();
                kept.insert("counted".into(), Value::Object(part));
            }
            Value::Object(kept)
        })
        .collect();
    // Readings too old to draw, and any ahead of this moment (the clock set back), let go wherever they are; those
    // kept carry 会话 as it stood when each was read. 会话 off the homepage now: none of it kept at all (STAGE-1).
    let sessions_on = sessions.is_some();
    let mut stood: Option<Value> = None;
    let mut kept_stood: Option<Value> = None;
    let kept: VecDeque<Value> = recent
        .drain(..)
        .filter_map(|mut reading| {
            if let Some(sessions) = reading.get("sessions") {
                stood = Some(sessions.clone());
            }
            let at = reading["at"].as_f64()?;
            if moment - at > RECENT_FOR || at > moment {
                return None;
            }
            if let Some(object) = reading.as_object_mut() {
                object.remove("sessions");
                if sessions_on
                    && stood != kept_stood
                    && let Some(sessions) = &stood
                {
                    object.insert("sessions".into(), sessions.clone());
                }
            }
            if sessions_on {
                kept_stood.clone_from(&stood);
            }
            Some(reading)
        })
        .collect();
    *recent = kept;
    let mut reading = serde_json::json!({
        "at": moment, "date": dashboard["date"], "rangeKey": dashboard["rangeKey"], "providers": short,
    });
    // 会话 in short when it changed since the last reading kept: what a page opened late replays its 动态 from (a tool
    // called, a turn begun or done).
    if let Some(sessions) = sessions.map(short_sessions)
        && kept_stood.as_ref() != Some(&sessions)
    {
        reading["sessions"] = sessions;
    }
    recent.push_back(reading);
}

/// 会话 as the readings keep it: each session's title, key, light and state, and its latest turn's line (tokens, cost,
/// requests and the tools called), the rest of the row left out.
fn short_sessions(entry: &Value) -> Value {
    let rows: Vec<Value> = entry
        .get("children")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|row| row.get("statusLight").is_some())
        .map(|row| {
            let turn = row
                .get("children")
                .and_then(Value::as_array)
                .and_then(|turns| turns.iter().find(|turn| turn.get("detail").is_some()))
                .map(|turn| json!({"label": turn["label"], "detail": turn["detail"]}));
            json!({"label": row["label"], "key": row["key"], "statusLight": row["statusLight"], "detail": row["detail"],
                   "children": turn.into_iter().collect::<Vec<_>>()})
        })
        .collect();
    Value::Array(rows)
}

/// 网页版's live view, asked every couple of seconds while it is on screen: today's figures by tool, 会话 (its
/// sessions only, as the snapshot sends them) and when each session's turn began. The app reads today's logs afresh
/// for it (Engine::live, at most every 1.5 s); SwiftBar's panel has what the plugin last wrote, for the range the
/// computer shows (the phone says which, and compares figures of one range only), the fixtures creep up.
fn live(app: &AppHandle, history: bool) -> Value {
    live_from(&app.state::<Data>(), history)
}

/// Today's figures and 会话 as the source has them now, read cheaply (no logs read again, SwiftBar's snapshot only
/// looked at, not taken as seen): what `sample` keeps.
fn today_now(data: &Data) -> Option<(Value, Value)> {
    let (dashboard, sessions) = match data {
        Data::Engine(engine) => (engine.dashboard_grouped("today", Some("tool")), engine.sessions_now()),
        Data::SwiftBar(files) => {
            let snapshot = files.peek();
            (snapshot.get("dashboard").cloned().unwrap_or(Value::Null), sessions_of(&snapshot))
        }
        Data::Fixtures(fixtures) => {
            let fixtures = fixtures.lock().expect("the fixtures");
            (fixtures.dashboard("today"), sessions_of(&fixtures.snapshot()))
        }
    };
    dashboard.is_object().then_some((dashboard, sessions))
}

/// A reading kept for the last minutes, the server on and no page asking.
fn sample(data: &Data) {
    if let Some((dashboard, mut sessions)) = today_now(data) {
        if sessions.is_object() {
            phone_sessions(&mut sessions);
        }
        let sessions = sessions.is_object().then_some(&sessions);
        keep_recent(
            &mut RECENT.lock().expect("the live readings"),
            starmeter_core::runtime::now(),
            &dashboard,
            sessions,
        );
    }
}

fn live_from(data: &Data, history: bool) -> Value {
    let mut answer = match data {
        Data::Engine(engine) => engine.live(),
        Data::SwiftBar(files) => {
            let snapshot = files.snapshot();
            json!({
                "at": starmeter_core::runtime::now(),
                "dashboard": snapshot.get("dashboard").cloned().unwrap_or(Value::Null),
                "sessions": sessions_of(&snapshot),
                "turns": {},
            })
        }
        Data::Fixtures(fixtures) => {
            let mut fixtures = fixtures.lock().expect("the fixtures");
            let _ = fixtures.act(&["refresh-live".to_string()]);
            let snapshot = fixtures.snapshot();
            json!({
                "at": starmeter_core::runtime::now(),
                "dashboard": fixtures.dashboard("today"),
                "sessions": sessions_of(&snapshot),
                "turns": {},
            })
        }
    };
    if let Some(entry) = answer.get_mut("sessions").filter(|entry| entry.is_object()) {
        phone_sessions(entry);
    }
    // The last minutes for a page just opened, from whatever source; then this reading kept with them.
    let sessions_on = answer.get("sessions").is_some_and(Value::is_object);
    let mut recent = RECENT.lock().expect("the live readings");
    if history {
        answer["history"] = history_of(&recent, sessions_on);
    }
    if let (Some(at), Some(dashboard)) = (answer["at"].as_f64(), answer.get("dashboard")) {
        keep_recent(&mut recent, at, dashboard, answer.get("sessions").filter(|sessions| sessions.is_object()));
    }
    answer
}

/// The last minutes as a page is given them: with 会话 off the homepage, none of its sessions (STAGE-1).
fn history_of(recent: &VecDeque<Value>, sessions_on: bool) -> Value {
    Value::Array(
        recent
            .iter()
            .cloned()
            .map(|mut reading| {
                if !sessions_on && let Some(object) = reading.as_object_mut() {
                    object.remove("sessions");
                }
                reading
            })
            .collect(),
    )
}

/// The computer's own panel: the live answer a phone's 网页版 gets (today by tool, 会话's sessions, each turn's start),
/// for 实时's 正在运行 and 动态, with the last minutes when it opens (`history`). Off the main thread: the app reads
/// today's logs for it.
#[tauri::command]
pub async fn live_now(app: AppHandle, history: Option<bool>) -> Value {
    tauri::async_runtime::spawn_blocking(move || live(&app, history.unwrap_or(false))).await.unwrap_or(Value::Null)
}

/// The last minutes only, at once (nothing read again, off the main thread): the panel pinned draws its live line from
/// them before its first tick. Whether 会话 is on asked of the preferences alone (STAGE-14: asking it of today's
/// figures could read every log on the main thread).
#[tauri::command]
pub async fn recent_readings(app: AppHandle) -> Value {
    tauri::async_runtime::spawn_blocking(move || {
        let data = app.state::<Data>();
        let sessions_on = match &*data {
            Data::Engine(engine) => engine.sessions_on(),
            Data::SwiftBar(files) => sessions_of(&files.peek()).is_object(),
            Data::Fixtures(fixtures) => sessions_of(&fixtures.lock().expect("the fixtures").snapshot()).is_object(),
        };
        let recent = RECENT.lock().expect("the live readings");
        json!({"at": starmeter_core::runtime::now(), "history": history_of(&recent, sessions_on)})
    })
    .await
    .unwrap_or(Value::Null)
}

/// 会话's row in a snapshot, if it is on the homepage.
fn sessions_of(snapshot: &Value) -> Value {
    let mark = json!(["refresh-source", "sessions"]);
    snapshot
        .get("entries")
        .and_then(Value::as_array)
        .and_then(|entries| entries.iter().find(|entry| entry.get("onOpenAction") == Some(&mark)))
        .cloned()
        .unwrap_or(Value::Null)
}

/// 会话 as a phone has it: its sessions (titles, models, tokens: no paths), never its switch (这一轮完成时提醒), and
/// nothing to do.
fn phone_sessions(entry: &mut Value) {
    if let Some(children) = entry.get_mut("children").and_then(Value::as_array_mut) {
        children.retain(|child| child.get("action").is_none());
        while children.last().is_some_and(|child| child.get("separator").is_some()) {
            children.pop();
        }
    }
    strip(entry);
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
    use super::{AtomicUsize, CONNECTIONS, Deadline, Ordering, Slot, Slots, same};
    use std::io::{BufRead, BufReader};
    use std::net::{TcpListener, TcpStream};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    /// A client sending a byte every 100 ms, never ending its line: the read stops at the deadline, not at each
    /// byte's own timeout.
    #[test]
    fn a_trickling_client_is_let_go_at_the_deadline() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let client = std::thread::spawn(move || {
            let mut stream = TcpStream::connect(address).unwrap();
            for _ in 0..30 {
                if std::io::Write::write_all(&mut stream, b"G").is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        });
        let (server, _) = listener.accept().unwrap();
        let started = Instant::now();
        let mut reader = BufReader::new(Deadline { stream: server, until: started + Duration::from_millis(500) });
        let mut line = String::new();
        let read = reader.read_line(&mut line);
        assert!(read.is_err(), "{read:?} {line:?}");
        assert!(started.elapsed() < Duration::from_millis(1500), "{:?}", started.elapsed());
        drop(reader);
        client.join().unwrap();
    }

    #[test]
    fn connections_past_the_limit_wait_for_a_slot_given_back_however_an_answer_ends() {
        // PHONE-7: a connection each, at most CONNECTIONS at once; a slot comes back when its answer ends, a panic too.
        let slots = Slots(Arc::new(AtomicUsize::new(0)));
        let mut taken: Vec<Slot> = (0..CONNECTIONS).map(|_| slots.take().expect("a slot")).collect();
        assert!(slots.take().is_none());
        taken.pop();
        let again = slots.take().expect("the slot given back");
        drop(again);
        let slot = slots.take().expect("a slot");
        let _ = std::thread::spawn(move || {
            let _slot = slot;
            panic!("an answer gone wrong");
        })
        .join();
        assert_eq!(slots.0.load(Ordering::SeqCst), CONNECTIONS - 1);
    }

    #[test]
    fn a_phone_has_the_sessions_without_the_switch_or_anything_to_do() {
        let mut entry = serde_json::json!({
            "label": "会话", "onOpenAction": ["refresh-source", "sessions"],
            "children": [
                {"label": "重做设置页", "key": "s1", "statusLight": "new", "href": "x",
                 "children": [{"label": "15:36 · 2.41M Token · $0.92", "detail": "Bash ×4", "action": ["open"]}]},
                {"label": "", "separator": true},
                {"label": "这一轮完成时提醒", "checked": true, "action": ["set-session-alerts", "false"]},
            ],
        });
        super::phone_sessions(&mut entry);
        assert_eq!(entry["children"].as_array().unwrap().len(), 1);
        assert_eq!(entry["children"][0]["key"], "s1");
        assert!(entry["children"][0].get("href").is_none());
        assert!(entry["children"][0]["children"][0].get("action").is_none());
        assert!(entry.get("onOpenAction").is_none());
    }

    /// The readings a page opened late is given: two seconds apart at most, a few minutes back, each tool's figures the
    /// line counts and nothing else.
    #[test]
    fn the_live_readings_kept_are_the_last_minutes_in_short() {
        let dashboard = |tokens: f64| {
            serde_json::json!({"date": "2026-11-25", "rangeKey": "today", "chartUnit": "每小时", "providers": [
                {"id": "claude", "available": true, "tokens": tokens, "output": tokens / 100.0, "cost": 1.5, "history": [1, 2, 3],
                 "counted": {"tokens": tokens, "output": 4.0, "cost": 1.0, "history": [1]}, "name": "Claude"},
            ]})
        };
        let mut recent = std::collections::VecDeque::new();
        super::keep_recent(&mut recent, 1000.0, &dashboard(100.0), None);
        super::keep_recent(&mut recent, 1001.0, &dashboard(150.0), None);
        super::keep_recent(&mut recent, 1002.5, &dashboard(200.0), None);
        assert_eq!(recent.len(), 2, "one a second is too often");
        let first = &recent[0];
        assert_eq!(first["date"], "2026-11-25");
        assert_eq!(first["providers"][0]["tokens"], 100.0);
        assert_eq!(first["providers"][0]["counted"], serde_json::json!({"tokens": 100.0, "output": 4.0, "cost": 1.0}));
        assert!(first["providers"][0].get("history").is_none() && first["providers"][0].get("name").is_none());
        super::keep_recent(&mut recent, 1000.0 + super::RECENT_FOR + 2.0, &dashboard(300.0), None);
        assert_eq!(
            recent.iter().map(|r| r["at"].as_f64().unwrap()).collect::<Vec<_>>(),
            vec![1002.5, 1000.0 + super::RECENT_FOR + 2.0]
        );
        // The clock set back: what is now ahead of it goes.
        super::keep_recent(&mut recent, 900.0, &dashboard(300.0), None);
        assert_eq!(recent.len(), 1);
    }

    /// 会话 off the homepage: the readings kept let go of every session they held, and the last minutes a page is
    /// given carry none (STAGE-1: what was off went out over the phone link and into 动态 still).
    #[test]
    fn with_sessions_off_the_readings_carry_none_of_them() {
        let dashboard = serde_json::json!({"date": "2026-11-25", "rangeKey": "today",
            "providers": [{"id": "claude", "available": true, "tokens": 100.0, "output": 1.0, "cost": 1.0}]});
        let sessions = serde_json::json!({"children": [{"label": "重做设置页的分段控件", "key": "session-1", "statusLight": "new", "detail": "运行中"}]});
        let mut recent = std::collections::VecDeque::new();
        super::keep_recent(&mut recent, 1000.0, &dashboard, Some(&sessions));
        super::keep_recent(&mut recent, 1003.0, &dashboard, Some(&sessions));
        assert!(recent.iter().any(|reading| reading.get("sessions").is_some()));
        // Still on: given as kept.
        assert!(super::history_of(&recent, true).to_string().contains("重做设置页的分段控件"));
        // Off: not given, then not kept.
        assert!(!super::history_of(&recent, false).to_string().contains("重做设置页的分段控件"));
        super::keep_recent(&mut recent, 1006.0, &dashboard, None);
        assert_eq!(recent.len(), 3);
        assert!(recent.iter().all(|reading| reading.get("sessions").is_none()), "{recent:?}");
        // On again: kept from then on.
        super::keep_recent(&mut recent, 1009.0, &dashboard, Some(&sessions));
        assert_eq!(recent.iter().filter(|reading| reading.get("sessions").is_some()).count(), 1);
    }

    /// SwiftBar's figures (the plugin's snapshot) have their last minutes too: kept while the server is on, given
    /// with ?history=1 as the app's own are, not only on the standalone app.
    #[test]
    fn swiftbars_figures_have_their_last_minutes_too() {
        let folder =
            std::env::temp_dir().join(format!("starmeter-phone-recent-{}", starmeter_core::runtime::random_hex(6)));
        std::fs::create_dir_all(&folder).unwrap();
        let view = folder.join("panel-view.json");
        let write = |tokens: f64| {
            let snapshot = serde_json::json!({"schema": 1, "entries": [], "dashboard": {"date": "2026-11-25", "rangeKey": "today",
                "providers": [{"id": "claude", "available": true, "tokens": tokens, "output": tokens / 100.0, "cost": 1.0}]}});
            std::fs::write(&view, snapshot.to_string()).unwrap();
        };
        let args = ["--snapshot".to_string(), view.to_string_lossy().into_owned()];
        let data = crate::data::Data::SwiftBar(Arc::new(crate::swiftbar::Files::from_args(&args).unwrap()));
        super::RECENT.lock().unwrap().clear();
        write(100.0);
        super::sample(&data);
        let answer = super::live_from(&data, true);
        let history = answer["history"].as_array().expect("the last minutes");
        assert_eq!(history.len(), 1);
        assert_eq!(history[0]["providers"][0]["tokens"], 100.0);
        assert!(super::live_from(&data, false).get("history").is_none());
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// 会话 goes with the readings in short, when it changed: a session's title, light and its latest turn's line (what
    /// a page opened late replays its 动态 from); the first reading left after the old ones go carries it as it stood.
    #[test]
    fn the_sessions_go_with_the_readings_when_they_change() {
        let dashboard = serde_json::json!({"date": "2026-11-25", "rangeKey": "today", "providers": [{"id": "claude", "available": true, "tokens": 1.0}]});
        let sessions = |tools: &str| {
            serde_json::json!({"label": "会话", "children": [
                {"label": "重做设置页", "key": "s1", "statusLight": "new", "detail": "运行中 · Opus 5.5", "value": "$0.92", "image": "x",
                 "children": [{"label": "15:36 · 2.41M Token · $0.92", "detail": tools}, {"label": "15:20 · 1K Token · $0.01", "detail": "1 次请求"}]},
                {"label": "", "separator": true},
            ]})
        };
        let mut recent = std::collections::VecDeque::new();
        super::keep_recent(&mut recent, 1000.0, &dashboard, Some(&sessions("9 次请求 · Edit ×6")));
        super::keep_recent(&mut recent, 1003.0, &dashboard, Some(&sessions("9 次请求 · Edit ×6")));
        super::keep_recent(&mut recent, 1006.0, &dashboard, Some(&sessions("10 次请求 · Edit ×6 · Bash ×1")));
        let kept: Vec<bool> = recent.iter().map(|reading| reading.get("sessions").is_some()).collect();
        assert_eq!(kept, vec![true, false, true]);
        assert_eq!(
            recent[0]["sessions"],
            serde_json::json!([{"label": "重做设置页", "key": "s1", "statusLight": "new",
            "detail": "运行中 · Opus 5.5", "children": [{"label": "15:36 · 2.41M Token · $0.92", "detail": "9 次请求 · Edit ×6"}]}])
        );
        // The first two go: the one left first carries 会话 as it stood then.
        super::keep_recent(
            &mut recent,
            1003.5 + super::RECENT_FOR,
            &dashboard,
            Some(&sessions("10 次请求 · Edit ×6 · Bash ×1")),
        );
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0]["sessions"][0]["children"][0]["detail"], "10 次请求 · Edit ×6 · Bash ×1");
        assert!(recent[1].get("sessions").is_none());
    }

    /// The clock set back to between readings kept: every reading ahead of it goes, not only from the front (STAGE-10).
    #[test]
    fn a_clock_set_back_lets_go_of_every_reading_ahead_of_it() {
        let dashboard = serde_json::json!({"date": "2026-11-25", "rangeKey": "today", "providers": [{"id": "claude", "tokens": 1.0}]});
        let sessions =
            serde_json::json!({"children": [{"label": "s", "key": "s1", "statusLight": "new", "children": []}]});
        let mut recent = std::collections::VecDeque::new();
        super::keep_recent(&mut recent, 800.0, &dashboard, None);
        super::keep_recent(&mut recent, 1000.0, &dashboard, Some(&sessions));
        super::keep_recent(&mut recent, 1002.0, &dashboard, Some(&sessions));
        super::keep_recent(&mut recent, 900.0, &dashboard, Some(&sessions));
        let at: Vec<f64> = recent.iter().map(|reading| reading["at"].as_f64().unwrap()).collect();
        assert_eq!(at, vec![800.0, 900.0]);
        // 会话 as it stands at 900, though the reading that first had it is gone.
        assert!(recent[1].get("sessions").is_some());
    }

    /// The page carries the computer's look just before first-frame.js reads it.
    #[test]
    fn the_page_carries_the_computers_look_for_its_first_frame() {
        let page = "<head>\n    <title>Starmeter</title>\n    <script src=\"/first-frame.js\"></script>\n  </head>";
        let shown = super::with_look(page, &serde_json::json!({"theme": "dark", "style": "standard"}));
        let look = shown.find("window.STARMETER_LOOK={\"style\":\"standard\",\"theme\":\"dark\"};").expect("the look");
        assert!(look < shown.find("first-frame.js").unwrap());
        // A page without the script is left as it is.
        assert_eq!(super::with_look("<html></html>", &serde_json::json!({})), "<html></html>");
    }

    #[test]
    fn the_key_in_full_and_nothing_less() {
        assert!(same("ab12", "ab12"));
        assert!(!same("ab1", "ab12"));
        assert!(!same("", "ab12"));
        assert!(!same("ab13", "ab12"));
    }
}
