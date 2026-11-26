//! 在手机上看: a small read-only web server the app runs while the user asks for it, so a phone can open the homepage
//! by scanning a QR code. It serves the panel's own page and, behind a key, the homepage's figures; it never takes an
//! action, and it leaves out the settings and every row's insides (folders, key files, connection addresses).
//!
//! Two ways in, each turned on and off on its own: 同一个网络 (this computer's network address, and its Tailscale one)
//! until its 停止, and the public link (Cloudflare's quick tunnel, see `tunnel`) for an hour at most. Each has a key of
//! its own; the server runs while either is on. A key travels in the address's fragment (#k=…), which a browser never
//! sends; the page sends it back in a header, so it stays out of logs.

use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{IpAddr, TcpListener, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tauri::{AppHandle, Manager};

use crate::data::Data;

/// How long the public link stays open: it reaches the computer from anywhere. On the same network the phone view stays
/// until it is stopped, and is on again when the app starts again (Kept).
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

/// The server runs while either way in is on, and the two come and go on their own (用户：同一网络和任何网络……互不影响):
/// 同一个网络 (this computer's network address and its Tailscale one) with its key, and the public link with a key of its
/// own, so stopping one leaves the other's code opening as it did.
struct Running {
    port: u16,
    keys: Arc<Mutex<Keys>>,
    stop: Arc<AtomicBool>,
    public: Option<Public>,
    /// The public link's round: closed (its 停止, or the server stopping), a link still being made is closed as it
    /// comes instead of shown.
    public_round: Arc<AtomicBool>,
    /// Asks for the public link on their way: the server stays for them with 同一个网络 off.
    opening: usize,
}

/// The keys the server takes. Only the public link's comes from this computer itself (cloudflared hands its requests on
/// from 127.0.0.1); a phone on the network (or the tailnet) needs 同一个网络's, and is turned away while it is off.
#[derive(Default)]
struct Keys {
    local: Option<String>,
    public: Option<String>,
}

struct Public {
    tunnel: crate::tunnel::Tunnel,
    /// When its hour is up.
    until: Instant,
    key: String,
}

/// What the panel shows: the addresses to scan, and how long the public link has left.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PhoneView {
    /// Whether 同一个网络 is on: its addresses below, when this computer has them.
    local_open: bool,
    /// http://192.168.1.5:51234/#k=… on this network; None without one, or with 同一个网络 off.
    local: Option<String>,
    /// http://100.x.y.z:51234/#k=… through Tailscale, when this computer is on a tailnet: a phone on it opens the page
    /// from anywhere, nothing going through Cloudflare.
    tailnet: Option<String>,
    /// https://….trycloudflare.com/#k=… once the public link is up.
    public: Option<String>,
    /// The public link's time left; 0 without one.
    seconds_left: u64,
    /// Whether the public link was seen to open from outside before it was shown (PHONE-11); false only when the wait
    /// for it ran out, so the panel says the phone may have to wait a moment.
    public_checked: bool,
}

/// A public link still to be used: its cloudflared running and its hour not up. Past the hour it is closed within a few
/// seconds (serve looks every five); until then it isn't offered, with no time left on it.
fn usable(public: &Public) -> bool {
    public.tunnel.alive() && Instant::now() < public.until
}

fn view(running: &Running) -> PhoneView {
    let local = running.keys.lock().expect("the keys").local.clone();
    let lan = local.as_ref().and_then(|_| lan_address());
    let address = |ip: IpAddr| format!("http://{ip}:{}/#k={}", running.port, local.as_deref().unwrap_or_default());
    let public = running.public.as_ref().filter(|public| usable(public));
    PhoneView {
        local_open: local.is_some(),
        local: lan.map(address),
        tailnet: local.as_ref().and_then(|_| tailnet_address(lan)).map(address),
        public: public.map(|public| format!("{}/#k={}", public.tunnel.url, public.key)),
        seconds_left: public.map_or(0, |public| public.until.saturating_duration_since(Instant::now()).as_secs()),
        public_checked: running.public.as_ref().is_none_or(|public| public.tunnel.checked),
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

/// This computer's Tailscale address: the one its traffic to Tailscale's own service address (100.100.100.100, on the
/// tailnet's route) would leave from, when that is a Tailscale address (100.64.0.0/10) and not the network's own.
fn tailnet_address(lan: Option<IpAddr>) -> Option<IpAddr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("100.100.100.100:53").ok()?;
    tailnet_ip(socket.local_addr().ok()?.ip(), lan)
}

/// A Wi-Fi of its own can hand out 100.64.0.0/10 too (a carrier's or a campus's): without Tailscale the route to
/// 100.100.100.100 is the default one, from the network's own address, which is no tailnet's.
fn tailnet_ip(ip: IpAddr, lan: Option<IpAddr>) -> Option<IpAddr> {
    match ip {
        IpAddr::V4(v4) if Some(ip) != lan && v4.octets()[0] == 100 && v4.octets()[1] & 0xC0 == 64 => Some(ip),
        _ => None,
    }
}

fn new_key() -> String {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("the system's random numbers");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// What the user left on, kept in the settings folder (outside the app, so an update keeps it too), and on again when
/// the app starts (用户：默认是关的，但是你要记住用户的状态。就算重新安装新版本，也要记住之前用户开没开): 同一个网络 with
/// the server's port and its key, so a phone's link from before keeps opening; the public link until its hour is up
/// (its address is Cloudflare's, new each time, so a phone scans its new code).
#[derive(Serialize, Deserialize, Default, Clone, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
struct Kept {
    #[serde(default)]
    port: Option<u16>,
    #[serde(default)]
    local: Option<String>,
    /// When the public link's hour is up, in seconds since 1970.
    #[serde(default)]
    public_until: Option<f64>,
}

fn kept_file() -> PathBuf {
    let environment: HashMap<String, String> = std::env::vars().collect();
    let shell = starmeter_core::desktop::Shell::current();
    PathBuf::from(starmeter_core::desktop::state_dir(shell, &environment)).join("phone.json")
}

/// The kept state; a key that isn't one of ours (32 hex digits) is left out.
fn read_kept(path: &Path) -> Kept {
    let mut kept: Kept =
        std::fs::read(path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
    kept.local = kept.local.filter(|key| key.len() == 32 && key.bytes().all(|b| b.is_ascii_hexdigit()));
    kept
}

/// Written whole (a file beside it, then put in its place), readable by this user alone: it holds the key; nothing
/// kept, no file.
fn write_kept(path: &Path, kept: &Kept) {
    if *kept == Kept::default() {
        let _ = std::fs::remove_file(path);
        return;
    }
    let Ok(bytes) = serde_json::to_vec(kept) else { return };
    if let Some(folder) = path.parent() {
        let _ = std::fs::create_dir_all(folder);
    }
    let fresh = path.with_extension("json.new");
    let _ = std::fs::remove_file(&fresh);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    if let Ok(mut file) = options.open(&fresh)
        && file.write_all(&bytes).is_ok()
    {
        let _ = std::fs::rename(&fresh, path);
    }
}

fn keep(change: impl FnOnce(&mut Kept)) {
    let path = kept_file();
    let mut kept = read_kept(&path);
    change(&mut kept);
    write_kept(&path, &kept);
}

/// The server's socket, on the port it had before when it is free (a phone's link names it), else on a new one.
fn bind(port: Option<u16>) -> std::io::Result<TcpListener> {
    port.and_then(|port| TcpListener::bind(("0.0.0.0", port)).ok()).map_or_else(|| TcpListener::bind("0.0.0.0:0"), Ok)
}

/// The app started (an update too): what the user left on, on again, before anyone opens the panel.
pub fn resume(app: &AppHandle) {
    let kept = read_kept(&kept_file());
    if kept.local.is_some() {
        let _ = phone_start(app.clone());
    }
    if kept.public_until.is_some_and(|until| until > starmeter_core::runtime::now()) {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let _ = phone_public(app).await;
        });
    }
}

/// The server running (started if it isn't, with neither way in on yet).
fn serving<'a>(app: &AppHandle, running: &'a mut Option<Running>) -> Result<&'a mut Running, String> {
    if running.as_ref().is_some_and(|current| current.stop.load(Ordering::SeqCst))
        && let Some(mut over) = running.take()
    {
        stop_running(&mut over);
    }
    if running.is_none() {
        let listener = bind(read_kept(&kept_file()).port).map_err(|error| format!("没能开始：{error}"))?;
        let port = listener.local_addr().map_err(|error| error.to_string())?.port();
        listener.set_nonblocking(true).map_err(|error| error.to_string())?;
        keep(|kept| kept.port = Some(port));
        let keys = Arc::new(Mutex::new(Keys::default()));
        let stop = Arc::new(AtomicBool::new(false));
        {
            let (app, keys, stop) = (app.clone(), keys.clone(), stop.clone());
            std::thread::spawn(move || serve(app, listener, keys, stop));
        }
        *running = Some(Running {
            port,
            keys,
            stop,
            public: None,
            public_round: Arc::new(AtomicBool::new(false)),
            opening: 0,
        });
    }
    Ok(running.as_mut().expect("the server just started"))
}

/// 同一个网络 on (or kept on) and where it is: its key as kept when it was on as the app quit (a phone's link keeps
/// opening), else a new one, so a code from before its 停止 stays shut. The public link, if up, is left as it is.
#[tauri::command]
pub fn phone_start(app: AppHandle) -> Result<PhoneView, String> {
    let state = app.state::<Phone>();
    let mut running = state.running.lock().expect("the phone lock");
    let current = serving(&app, &mut running)?;
    let key = current
        .keys
        .lock()
        .expect("the keys")
        .local
        .get_or_insert_with(|| read_kept(&kept_file()).local.unwrap_or_else(new_key))
        .clone();
    keep(|kept| kept.local = Some(key));
    Ok(view(current))
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
    settle(&mut running);
    running.as_ref().map(view)
}

/// 同一个网络's 停止: its addresses (the network's and Tailscale's) no longer open, its key gone with them; the public
/// link, if up, stays.
#[tauri::command]
pub fn phone_stop(app: AppHandle) -> Option<PhoneView> {
    let state = app.state::<Phone>();
    let mut running = state.running.lock().expect("the phone lock");
    if let Some(current) = running.as_mut() {
        current.keys.lock().expect("the keys").local = None;
    }
    keep(|kept| kept.local = None);
    settle(&mut running);
    running.as_ref().map(view)
}

/// The app quitting: the server and the public link with it, what was on kept for the next start.
pub fn phone_quit(app: &AppHandle) {
    let state = app.state::<Phone>();
    if let Some(mut over) = state.running.lock().expect("the phone lock").take() {
        stop_running(&mut over);
    }
}

fn stop_running(running: &mut Running) {
    running.stop.store(true, Ordering::SeqCst);
    running.public_round.store(true, Ordering::SeqCst);
    *running.keys.lock().expect("the keys") = Keys::default();
    if let Some(mut public) = running.public.take() {
        public.tunnel.stop();
    }
}

/// The server stays while something is on: 同一个网络, the public link, or an ask for it on its way.
fn settle(running: &mut Option<Running>) {
    let idle = running.as_ref().is_some_and(|current| {
        current.keys.lock().expect("the keys").local.is_none() && current.public.is_none() && current.opening == 0
    });
    if idle && let Some(mut over) = running.take() {
        stop_running(&mut over);
    }
}

/// A public link closed: cloudflared ended, and its key no longer taken.
fn close_public(keys: &Mutex<Keys>, mut public: Public) {
    public.tunnel.stop();
    forget_public_key(keys, &public.key);
}

fn forget_public_key(keys: &Mutex<Keys>, key: &str) {
    let mut keys = keys.lock().expect("the keys");
    if keys.public.as_deref() == Some(key) {
        keys.public = None;
    }
}

/// Closes the public link once its hour is up or its cloudflared has ended; 同一个网络 stays.
fn close_public_if_over(running: &mut Running) {
    if running.public.as_ref().is_some_and(|public| !usable(public))
        && let Some(over) = running.public.take()
    {
        close_public(&running.keys, over);
        keep(|kept| kept.public_until = None);
    }
}

/// One public link opened at a time: an ask while another is on its way waits for it and takes its link, so two quick
/// clicks start one cloudflared, and a first download isn't written twice at once (TUNNEL-1).
static OPENING: Mutex<()> = Mutex::new(());

/// Opens the public link (Cloudflare's quick tunnel), the server started for it if 同一个网络 is off, and says where it
/// is. The link belongs to the round it was asked in: its 停止 meanwhile (or the app quitting), it is closed, not shown.
#[tauri::command]
pub async fn phone_public(app: AppHandle) -> Result<PhoneView, String> {
    let (port, server, round, keys) = {
        let state = app.state::<Phone>();
        let mut running = state.running.lock().expect("the phone lock");
        let current = serving(&app, &mut running)?;
        close_public_if_over(current);
        if current.public.as_ref().is_some_and(usable) {
            return Ok(view(current));
        }
        current.opening += 1;
        (current.port, current.stop.clone(), current.public_round.clone(), current.keys.clone())
    };
    let (handle, asked) = (app.clone(), round.clone());
    let joined =
        tauri::async_runtime::spawn_blocking(move || -> Result<Option<(crate::tunnel::Tunnel, String)>, String> {
            let _turn = OPENING.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            // Stopped while it waited its turn: nothing to open a link for.
            if asked.load(Ordering::SeqCst) {
                return Ok(None);
            }
            // The link an earlier ask of this round opened while this one waited serves it too.
            let state = handle.state::<Phone>();
            let served = state.running.lock().expect("the phone lock").as_ref().is_some_and(|current| {
                Arc::ptr_eq(&current.public_round, &asked) && current.public.as_ref().is_some_and(usable)
            });
            if served {
                return Ok(None);
            }
            // Its own key, taken from cloudflared only, before the link is checked through it.
            let key = new_key();
            keys.lock().expect("the keys").public = Some(key.clone());
            let mut tunnel = crate::tunnel::open(&handle, port).inspect_err(|_| forget_public_key(&keys, &key))?;
            // Shown once it opens from outside (PHONE-11): a phone that looks the name up before Cloudflare has added it
            // is told it doesn't exist, and keeps being told so for up to a minute.
            match tunnel.confirm(&key, &asked) {
                crate::tunnel::Reach::Opens => tunnel.checked = true,
                crate::tunnel::Reach::Unconfirmed => {}
                // Stopped meanwhile: the round's over, and the link is closed below with it.
                crate::tunnel::Reach::Gone if asked.load(Ordering::SeqCst) => {}
                // cloudflared ended before the link opened: the panel asks once more on its own.
                crate::tunnel::Reach::Gone => {
                    tunnel.stop();
                    forget_public_key(&keys, &key);
                    return Err("Cloudflare 这次没有连上，稍后再试".into());
                }
            }
            Ok(Some((tunnel, key)))
        })
        .await;
    let opened = joined.unwrap_or_else(|error| Err(error.to_string()));
    let state = app.state::<Phone>();
    let mut running = state.running.lock().expect("the phone lock");
    let Some(current) = running.as_mut().filter(|current| Arc::ptr_eq(&current.stop, &server)) else {
        // The app quitting meanwhile: the server, and every key, already gone.
        if let Ok(Some((mut tunnel, _))) = opened {
            tunnel.stop();
        }
        return Err("手机查看已经关闭".into());
    };
    current.opening -= 1;
    let answer = match opened {
        Err(error) => Err(error),
        Ok(opened) if round.load(Ordering::SeqCst) => {
            if let Some((mut tunnel, key)) = opened {
                tunnel.stop();
                forget_public_key(&current.keys, &key);
            }
            Err("外网链接已经关闭".into())
        }
        Ok(Some((tunnel, key))) => {
            // Its hour: what is left of the one kept (the app started again within it), else a new one, kept.
            let now = starmeter_core::runtime::now();
            let until = read_kept(&kept_file())
                .public_until
                .filter(|until| *until > now && *until <= now + PUBLIC_LIFETIME.as_secs_f64())
                .unwrap_or(now + PUBLIC_LIFETIME.as_secs_f64());
            keep(|kept| kept.public_until = Some(until));
            let until = Instant::now() + Duration::from_secs_f64(until - now);
            if let Some(old) = current.public.replace(Public { tunnel, until, key }) {
                close_public(&current.keys, old);
            }
            Ok(view(current))
        }
        Ok(None) => Ok(view(current)),
    };
    settle(&mut running);
    answer
}

/// 任何网络's 停止: the public link closes (one still being made is closed as it comes); 同一个网络 stays as it is.
#[tauri::command]
pub fn phone_private(app: AppHandle) -> Option<PhoneView> {
    let state = app.state::<Phone>();
    let mut running = state.running.lock().expect("the phone lock");
    if let Some(current) = running.as_mut() {
        current.public_round.store(true, Ordering::SeqCst);
        current.public_round = Arc::new(AtomicBool::new(false));
        if let Some(public) = current.public.take() {
            close_public(&current.keys, public);
        }
    }
    keep(|kept| kept.public_until = None);
    settle(&mut running);
    running.as_ref().map(view)
}

fn serve(app: AppHandle, listener: TcpListener, keys: Arc<Mutex<Keys>>, stop: Arc<AtomicBool>) {
    let mut checked = Instant::now();
    let slots = Slots(Arc::new(AtomicUsize::new(0)));
    while !stop.load(Ordering::SeqCst) {
        // The public link's hour, looked at every few seconds whether or not the panel asks.
        if checked.elapsed() >= Duration::from_secs(5) {
            checked = Instant::now();
            if let Ok(mut running) = app.state::<Phone>().running.lock()
                && running.as_ref().is_some_and(|current| Arc::ptr_eq(&current.stop, &stop))
            {
                if let Some(current) = running.as_mut() {
                    close_public_if_over(current);
                }
                settle(&mut running);
            }
        }
        match listener.accept() {
            Ok((stream, peer)) => {
                // From the network (or the tailnet) with 同一个网络 off: turned away at once, as if nothing listened.
                let nearby = !peer.ip().is_loopback();
                if nearby && keys.lock().expect("the keys").local.is_none() {
                    continue;
                }
                // Past the limit, or with no thread to be had, the connection is closed and the server goes on.
                let Some(slot) = slots.take() else { continue };
                let (app, keys) = (app.clone(), keys.clone());
                let _ = std::thread::Builder::new().name("starmeter-phone".into()).spawn(move || {
                    let _slot = slot;
                    let _ = answer(&app, stream, &keys, nearby);
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(80))
            }
            Err(_) => std::thread::sleep(Duration::from_millis(200)),
        }
    }
    // Both ways in off (or the app quitting): the public link goes with the server.
    let state = app.state::<Phone>();
    let mut running = state.running.lock().expect("the phone lock");
    if running.as_ref().is_some_and(|current| Arc::ptr_eq(&current.stop, &stop))
        && let Some(mut over) = running.take()
    {
        stop_running(&mut over);
    }
}

/// Whether a request's key opens the page's figures: 同一个网络's from anywhere while it is on; the public link's only
/// as cloudflared hands it on, from this computer itself.
fn admitted(keys: &Mutex<Keys>, given: &str, nearby: bool) -> bool {
    let keys = keys.lock().expect("the keys");
    keys.local.as_deref().is_some_and(|key| same(given, key))
        || (!nearby && keys.public.as_deref().is_some_and(|key| same(given, key)))
}

fn answer(app: &AppHandle, mut stream: TcpStream, keys: &Mutex<Keys>, nearby: bool) -> std::io::Result<()> {
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
        if !admitted(keys, &given, nearby) {
            return respond(&mut stream, 403, "text/plain; charset=utf-8", b"key needed", true);
        }
        let range =
            query.split('&').find_map(|pair| pair.strip_prefix("range=")).filter(|range| RANGES.contains(range));
        // The phone's own grouping of the cards, as its range is its own: the computer's choices stay as they are.
        let group =
            query.split('&').find_map(|pair| pair.strip_prefix("group=")).filter(|group| GROUPS.contains(group));
        // The panel's check that the public link reaches this server (PHONE-11): nothing read, nothing refreshed.
        if call == "ping" {
            return respond(&mut stream, 200, "text/plain; charset=utf-8", PING, true);
        }
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
            let live = live_state(&RECENT.lock().expect("the live readings"), starmeter_core::runtime::now());
            let page = with_look(&String::from_utf8_lossy(&asset.bytes), &computer_look(&app.state::<Data>()), &live);
            respond(&mut stream, 200, &asset.mime_type, page.as_bytes(), fresh_asset(file))
        }
        Some(asset) => respond(&mut stream, 200, &asset.mime_type, &asset.bytes, false),
        None => respond(&mut stream, 404, "text/plain; charset=utf-8", b"not here", false),
    }
}

/// Whether a file of the page must never be kept by the browser: the page itself carries the computer's look as it is
/// now (STAGE-15: kept five minutes, a reload drew the look from before); the scripts, styles and pictures, named by
/// their content, may be.
fn fresh_asset(file: &str) -> bool {
    file == "index.html"
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

/// Whether the AI is at work now, as 网页版 judges it (phoneLive.svelte.ts: a session running, or Summary's tokens grown
/// within the last 40 seconds, readings either side of a gap not counting), from the last minutes kept, and how many
/// sessions run: written into the page, so a page reloaded while the work goes on is at work from its first frame (it
/// was resting until its first answer came, a flash of 休息中).
fn live_state(recent: &VecDeque<Value>, now: f64) -> Value {
    let counted = |reading: &Value| -> f64 {
        reading["providers"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|p| p["available"] != Value::Bool(false) && p["summaryIncluded"] != Value::Bool(false))
            .map(|p| p["counted"]["tokens"].as_f64().or_else(|| p["tokens"].as_f64()).unwrap_or(0.0))
            .sum()
    };
    let grew = recent
        .iter()
        .zip(recent.iter().skip(1))
        .filter(|(before, after)| {
            before["date"] == after["date"]
                && before["rangeKey"] == after["rangeKey"]
                && after["at"].as_f64().zip(before["at"].as_f64()).is_some_and(|(a, b)| a - b <= 15.0)
                && counted(after) > counted(before) + 0.5
        })
        .filter_map(|(_, after)| after["at"].as_f64())
        .next_back();
    let running = recent
        .iter()
        .rev()
        .find_map(|reading| reading.get("sessions"))
        .and_then(Value::as_array)
        .map_or(0, |rows| rows.iter().filter(|row| row["statusLight"] == "new").count());
    json!({"working": running > 0 || grew.is_some_and(|at| now - at < 40.0), "running": running})
}

/// The page with the computer's look, and whether the AI is at work, written in before its first script runs.
fn with_look(page: &str, look: &Value, live: &Value) -> String {
    let script = format!("<script>window.STARMETER_LOOK={look};window.STARMETER_LIVE={live};</script>\n    ");
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

/// What the server answers the panel's check with, telling it from Cloudflare's own pages.
pub const PING: &[u8] = b"starmeter";

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
    use super::{
        AtomicBool, AtomicUsize, CONNECTIONS, Deadline, Kept, Keys, Mutex, Ordering, Running, Slot, Slots, admitted,
        bind, new_key, read_kept, same, settle, tailnet_ip, write_kept,
    };

    #[test]
    fn a_tailnet_address_is_tailscales_range_and_not_the_networks_own() {
        let ip = |text: &str| text.parse::<std::net::IpAddr>().unwrap();
        // Tailscale on: its own address, beside the Wi-Fi's.
        assert_eq!(tailnet_ip(ip("100.76.55.73"), Some(ip("192.168.1.5"))), Some(ip("100.76.55.73")));
        // A Wi-Fi that hands out 100.64.0.0/10 itself, and no Tailscale: the route is the default one, no tailnet.
        assert_eq!(tailnet_ip(ip("100.69.38.146"), Some(ip("100.69.38.146"))), None);
        // Both in the range and apart: Tailscale's beside a carrier's network.
        assert_eq!(tailnet_ip(ip("100.76.55.73"), Some(ip("100.69.38.146"))), Some(ip("100.76.55.73")));
        // Outside the range (another VPN's), or the range's edges.
        for other in ["192.168.1.5", "10.0.0.2", "100.63.255.255", "100.128.0.1"] {
            assert_eq!(tailnet_ip(ip(other), Some(ip("192.168.1.5"))), None, "{other}");
        }
    }
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
    fn the_page_with_the_computers_look_is_never_kept_by_the_browser() {
        assert!(super::fresh_asset("index.html"));
        assert!(!super::fresh_asset("assets/index-3f9a.js"));
    }

    #[test]
    fn the_page_carries_the_computers_look_for_its_first_frame() {
        let page = "<head>\n    <title>Starmeter</title>\n    <script src=\"/first-frame.js\"></script>\n  </head>";
        let live = serde_json::json!({"working": true, "running": 2});
        let shown = super::with_look(page, &serde_json::json!({"theme": "dark", "style": "standard"}), &live);
        let look = shown.find("window.STARMETER_LOOK={\"style\":\"standard\",\"theme\":\"dark\"};").expect("the look");
        assert!(look < shown.find("first-frame.js").unwrap());
        assert!(shown.contains("window.STARMETER_LIVE={\"running\":2,\"working\":true};"));
        // A page without the script is left as it is.
        assert_eq!(super::with_look("<html></html>", &serde_json::json!({}), &live), "<html></html>");
    }

    /// At work or resting as 网页版 would judge it, for the page's first frame: a session running; Summary's tokens grown
    /// in the last 40 seconds; not tokens grown across a gap, nor long ago, nor by a tool left out of Summary.
    #[test]
    fn the_page_is_told_whether_the_ai_is_at_work() {
        let reading = |at: f64, claude: f64, other: f64| {
            serde_json::json!({"at": at, "date": "2026-11-25", "rangeKey": "today", "providers": [
                {"id": "claude", "available": true, "summaryIncluded": true, "tokens": claude},
                {"id": "left-out", "available": true, "summaryIncluded": false, "tokens": other}]})
        };
        let state = |readings: Vec<serde_json::Value>, now: f64| {
            let recent: std::collections::VecDeque<_> = readings.into_iter().collect();
            super::live_state(&recent, now)
        };
        let working = |value: serde_json::Value| value["working"].as_bool().unwrap();
        assert!(!working(state(vec![], 1000.0)));
        assert!(working(state(vec![reading(990.0, 100.0, 0.0), reading(992.0, 300.0, 0.0)], 1000.0)));
        assert!(!working(state(
            vec![reading(900.0, 100.0, 0.0), reading(902.0, 300.0, 0.0), reading(998.0, 300.0, 0.0)],
            1000.0
        )));
        assert!(!working(state(vec![reading(960.0, 100.0, 0.0), reading(990.0, 300.0, 0.0)], 1000.0)), "across a gap");
        assert!(
            !working(state(vec![reading(990.0, 100.0, 0.0), reading(992.0, 100.0, 500.0)], 1000.0)),
            "left out of Summary"
        );
        let mut quiet = reading(998.0, 100.0, 0.0);
        quiet["sessions"] =
            serde_json::json!([{"label": "a", "statusLight": "new"}, {"label": "b", "statusLight": "idle"}]);
        let running = state(vec![reading(996.0, 100.0, 0.0), quiet], 1000.0);
        assert!(working(running.clone()));
        assert_eq!(running["running"], 1);
    }

    #[test]
    fn the_key_in_full_and_nothing_less() {
        assert!(same("ab12", "ab12"));
        assert!(!same("ab1", "ab12"));
        assert!(!same("", "ab12"));
        assert!(!same("ab13", "ab12"));
    }

    #[test]
    fn each_way_in_takes_its_own_key_and_stops_on_its_own() {
        // 用户：同一网络和任何网络……互不影响.
        let (local, public) = (new_key(), new_key());
        let keys = Mutex::new(Keys { local: Some(local.clone()), public: Some(public.clone()) });
        // 同一个网络's key from the network (or the tailnet) and from this computer.
        assert!(admitted(&keys, &local, true));
        assert!(admitted(&keys, &local, false));
        // The public link's only as cloudflared hands it on, from this computer itself.
        assert!(admitted(&keys, &public, false));
        assert!(!admitted(&keys, &public, true));
        // 同一个网络 stopped: its code no longer opens; the public link's still does.
        keys.lock().unwrap().local = None;
        assert!(!admitted(&keys, &local, true));
        assert!(!admitted(&keys, &local, false));
        assert!(admitted(&keys, &public, false));
        // The public link stopped and 同一个网络 on again, with a new key: only that one opens.
        let again = new_key();
        *keys.lock().unwrap() = Keys { local: Some(again.clone()), public: None };
        assert!(admitted(&keys, &again, true));
        assert!(!admitted(&keys, &local, true));
        assert!(!admitted(&keys, &public, false));
        assert!(!admitted(&keys, "", false));
    }

    #[test]
    fn the_server_stays_while_either_way_in_is_on() {
        let running = |local: bool, opening: usize| Running {
            port: 1,
            keys: Arc::new(Mutex::new(Keys { local: local.then(new_key), public: None })),
            stop: Arc::new(AtomicBool::new(false)),
            public: None,
            public_round: Arc::new(AtomicBool::new(false)),
            opening,
        };
        let mut on = Some(running(true, 0));
        settle(&mut on);
        assert!(on.is_some());
        // 同一个网络 off while the public link is still being made: kept for it.
        let mut making = Some(running(false, 1));
        settle(&mut making);
        assert!(making.is_some());
        // Neither on: the server stops, and a link still on its way with it.
        let mut off = Some(running(false, 0));
        let (stop, round) = off.as_ref().map(|r| (r.stop.clone(), r.public_round.clone())).unwrap();
        settle(&mut off);
        assert!(off.is_none());
        assert!(stop.load(Ordering::SeqCst));
        assert!(round.load(Ordering::SeqCst));
    }

    #[test]
    fn what_was_on_is_kept_for_the_next_start_and_a_stop_leaves_nothing() {
        // 用户：默认是关的，但是你要记住用户的状态。就算重新安装新版本，也要记住之前用户开没开。
        let folder = std::env::temp_dir().join(format!("starmeter-phone-kept-{}", new_key()));
        let path = folder.join("phone.json");
        assert_eq!(read_kept(&path), Kept::default());
        let on = Kept { port: Some(51234), local: Some(new_key()), public_until: Some(1_800_000_000.5) };
        write_kept(&path, &on);
        assert_eq!(read_kept(&path), on);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
        // A key not one of ours is left out; the rest stands.
        std::fs::write(&path, r#"{"port":51234,"local":"not a key","publicUntil":null}"#).unwrap();
        assert_eq!(read_kept(&path), Kept { port: Some(51234), local: None, public_until: None });
        // Both ways stopped, the port forgotten too: no file left.
        write_kept(&path, &Kept::default());
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn the_server_takes_its_port_from_before_when_it_is_free() {
        let held = std::net::TcpListener::bind("0.0.0.0:0").unwrap();
        let port = held.local_addr().unwrap().port();
        // Taken by something else: another port, the server still starts.
        let other = bind(Some(port)).unwrap();
        assert_ne!(other.local_addr().unwrap().port(), port);
        drop(held);
        drop(other);
        // Free again: the same port, so a phone's link from before opens.
        let again = bind(Some(port)).unwrap();
        assert_eq!(again.local_addr().unwrap().port(), port);
        assert!(bind(None).unwrap().local_addr().unwrap().port() > 0);
    }
}
