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

/// 网页版's live view, asked every couple of seconds while it is on screen: today's figures by tool, 会话 (its
/// sessions only, as the snapshot sends them) and when each session's turn began. The app reads today's logs afresh
/// for it (Engine::live, at most every 1.5 s); SwiftBar's panel has what the plugin last wrote, for the range the
/// computer shows (the phone says which, and compares figures of one range only), the fixtures creep up.
fn live(app: &AppHandle, history: bool) -> Value {
    let mut answer = match &*app.state::<Data>() {
        Data::Engine(engine) => engine.live(history),
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
    answer
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

    #[test]
    fn the_key_in_full_and_nothing_less() {
        assert!(same("ab12", "ab12"));
        assert!(!same("ab1", "ab12"));
        assert!(!same("", "ab12"));
        assert!(!same("ab13", "ab12"));
    }
}
