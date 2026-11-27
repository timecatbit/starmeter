//! 在手机上看: a small read-only web server the app runs while the user asks for it, so a phone can open the homepage
//! by scanning a QR code. It serves the panel's own page and, behind a key, the homepage's figures; it never takes an
//! action, and it leaves out the settings and every row's insides (folders, key files, connection addresses).
//!
//! Two ways in, each turned on and off on its own: 同一个网络 (this computer's network address, and its Tailscale and
//! WireGuard ones) until its 停止, and the public link (Cloudflare's quick tunnel, see `tunnel`) for an hour at most. Each has a key of
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
    /// Asks belonging to the current (un-cancelled) round: shown in PhoneView so 停止 clears the spinner immediately.
    opening_active: usize,
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
    /// http://10.x.y.z:51234/#k=… through a WireGuard tunnel this computer has up, the same way.
    wireguard: Option<String>,
    /// https://….trycloudflare.com/#k=… once the public link is up.
    public: Option<String>,
    /// The public link's time left; 0 without one.
    seconds_left: u64,
    /// Whether the public link was seen to open from outside before it was shown (PHONE-11); false only when the wait
    /// for it ran out, so the panel says the phone may have to wait a moment.
    public_checked: bool,
    /// Whether a public link is currently being opened (startup resume or opened before leaving PhonePage, R10-8).
    public_opening: bool,
}

/// A public link still to be used: its cloudflared running and its hour not up. Past the hour it is closed within a few
/// seconds (serve looks every five); until then it isn't offered, with no time left on it.
fn usable(public: &Public) -> bool {
    public.tunnel.alive() && Instant::now() < public.until
}

fn view(running: &Running) -> PhoneView {
    let local = running.keys.lock().expect("the keys").local.clone();
    let found = local.as_ref().map(|_| addresses()).unwrap_or_default();
    let address = |ip: IpAddr| format!("http://{ip}:{}/#k={}", running.port, local.as_deref().unwrap_or_default());
    let public = running.public.as_ref().filter(|public| usable(public));
    PhoneView {
        local_open: local.is_some(),
        local: found.lan.map(address),
        tailnet: found.tailnet.map(address),
        wireguard: found.wireguard.map(address),
        public: public.map(|public| format!("{}/#k={}", public.tunnel.url, public.key)),
        seconds_left: public.map_or(0, |public| public.until.saturating_duration_since(Instant::now()).as_secs()),
        public_checked: running.public.as_ref().is_none_or(|public| public.tunnel.checked),
        public_opening: running.opening_active > 0,
    }
}

/// The view shown to the panel, or None when neither way is on and no active-round public link is opening.
fn visible_view(running: &Running) -> Option<PhoneView> {
    let v = view(running);
    (v.local_open || v.public.is_some() || v.public_opening).then_some(v)
}

/// This computer's addresses for 同一个网络's codes: on its network, on a tailnet, on a WireGuard network.
#[derive(Default, Debug, PartialEq)]
struct Addresses {
    lan: Option<IpAddr>,
    tailnet: Option<IpAddr>,
    wireguard: Option<IpAddr>,
}

/// The network's address is usually the one this computer's traffic leaves from (a UDP socket "connected" to an outside
/// address picks it without sending anything). A proxy's TUN mode (Clash, Surge: a point-to-point utun with an address
/// in 198.18.0.0/15) or a VPN can hold that route, and a phone on the Wi-Fi can't reach the tunnel's address; then it is
/// the address of the network the computer is on (用户：网卡选择了198的网段……这个是clash的tunnel，应该选192网段).
/// Tailscale's is the one its traffic to Tailscale's own service address (100.100.100.100) leaves from; WireGuard's is
/// its tunnel's (用户：如果有wireguard也和tailscale一样处理哈).
fn addresses() -> Addresses {
    let routed = leaves_from("192.0.2.1:9");
    let tailscale = leaves_from("100.100.100.100:53");
    let wireguard = wireguard_interfaces();
    let adapters = adapter_descriptions();
    let nics: Vec<Nic> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter(|interface| interface.is_oper_up())
        .map(|interface| {
            let described = adapters
                .iter()
                .find(|(friendly, _, _)| *friendly == interface.name)
                .map(|(_, description, kind)| (description.clone(), *kind));
            nic(interface.name.clone(), interface.is_p2p(), interface.ip(), described, &wireguard)
        })
        .collect();
    networks(routed, tailscale, &nics)
}

fn leaves_from(target: &str) -> Option<IpAddr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect(target).ok()?;
    Some(socket.local_addr().ok()?.ip())
}

/// One of the computer's addresses: its interface's name, whether that interface is a tunnel (point-to-point, or a
/// VPN's or a proxy's adapter known by its name or its description), and whether it is a WireGuard tunnel.
struct Nic {
    name: String,
    tunnel: bool,
    wireguard: bool,
    ip: IpAddr,
}

/// An interface's address as the system lists it: point-to-point or not, on Windows with its adapter's description and
/// type, and the names of the WireGuard tunnels up (macOS, Linux).
fn nic(name: String, p2p: bool, ip: IpAddr, described: Option<(String, u32)>, wireguard: &[String]) -> Nic {
    let wireguard = wireguard.contains(&name)
        || described.as_ref().is_some_and(|(description, _)| description.to_lowercase().contains("wireguard"));
    Nic { tunnel: p2p || wireguard || tunnel_named(&name, described.as_ref()), wireguard, ip, name }
}

/// The three addresses from the routes' and the interfaces': Tailscale's when its route isn't the default one (with an
/// exit node both are its, and its interface is a tunnel), never the network's; WireGuard's tunnel's, beside them.
fn networks(routed: Option<IpAddr>, tailscale: Option<IpAddr>, nics: &[Nic]) -> Addresses {
    let tailnet_route = tailscale.filter(|ip| Some(*ip) != routed && in_tailscales_range(*ip));
    let lan = lan_choice(routed, nics, tailnet_route);
    let tailnet = tailscale.and_then(|ip| tailnet_ip(ip, lan));
    let wireguard = nics
        .iter()
        .find(|nic| nic.wireguard && sendable(nic.ip) && Some(nic.ip) != lan && Some(nic.ip) != tailnet)
        .map(|nic| nic.ip);
    Addresses { lan, tailnet, wireguard }
}

/// In the range proxies take for their TUN (198.18.0.0/15, which Clash's TUN also has on Windows, where it isn't
/// point-to-point).
fn proxied(ip: IpAddr) -> bool {
    matches!(ip, IpAddr::V4(v4) if v4.octets()[0] == 198 && v4.octets()[1] & 0xFE == 18)
}

/// An address a phone could be sent to: IPv4, not the loopback, a link-local one or a proxy's.
fn sendable(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => !v4.is_loopback() && !v4.is_unspecified() && !v4.is_link_local() && !proxied(ip),
        IpAddr::V6(_) => false,
    }
}

fn in_tailscales_range(ip: IpAddr) -> bool {
    matches!(ip, IpAddr::V4(v4) if v4.octets()[0] == 100 && v4.octets()[1] & 0xC0 == 64)
}

/// The route's own address when a phone on the network can reach it, as it always was; otherwise the computer's
/// address on a network: not a tunnel's (nor Tailscale's, known from its route), not a proxy's, a real network card
/// before a virtual machine's or a container's bridge (whatever its address: a campus's Wi-Fi can hand out 100.64/10,
/// and a bridge has 192.168…, R11-1), and a private address (192.168…, 10…, 172.16–31…) before another. None when the
/// interfaces can't be listed and the route's address is a proxy's.
fn lan_choice(routed: Option<IpAddr>, nics: &[Nic], tailnet: Option<IpAddr>) -> Option<IpAddr> {
    let reachable = |nic: &&Nic| !nic.tunnel && sendable(nic.ip) && Some(nic.ip) != tailnet;
    if let Some(ip) = routed {
        match nics.iter().find(|nic| nic.ip == ip) {
            Some(nic) if reachable(&nic) => return Some(ip),
            None if nics.is_empty() && sendable(ip) && Some(ip) != tailnet => return Some(ip),
            _ => {}
        }
    }
    let bridge = |nic: &Nic| named(&nic.name, &BRIDGES);
    let private = |nic: &Nic| matches!(nic.ip, IpAddr::V4(v4) if v4.is_private());
    nics.iter().filter(reachable).min_by_key(|nic| (bridge(nic), !private(nic))).map(|nic| nic.ip)
}

/// A virtual machine's or a container's network (Parallels' vnic, macOS's feth among them), behind a real card.
const BRIDGES: [&str; 16] = [
    "bridge",
    "vmnet",
    "vboxnet",
    "docker",
    "br-",
    "veth",
    "virbr",
    "vethernet",
    "lxc",
    "lxdbr",
    "podman",
    "cni",
    "vmware",
    "virtualbox",
    "vnic",
    "feth",
];

/// A VPN's or a proxy's adapter, by its name: Windows calls Wintun's and TAP's neither point-to-point nor a tunnel
/// (if-addrs knows only PPP's and the system's own tunnels), and Linux's are named so too (R11-2).
const TUNNELS: [&str; 26] = [
    "utun",
    "tun",
    "tap",
    "wg",
    "ppp",
    "ipsec",
    "gpd",
    "cscotun",
    "tailscale",
    "wireguard",
    "zerotier",
    "zt",
    "wintun",
    "openvpn",
    "nordlynx",
    "mullvad",
    "proton",
    "sing-box",
    "singbox",
    "mihomo",
    "clash",
    "meta",
    "v2ray",
    "xray",
    "nekoray",
    "cloudflarewarp",
];

/// The same by a Windows adapter's description (WireGuard names its adapter after the tunnel, "WireGuard Tunnel" its
/// description; a TAP adapter keeps "Local Area Connection 2"), or its type: Wintun's is a virtual one (53).
const TUNNEL_DESCRIPTIONS: [&str; 11] = [
    "wintun",
    "wireguard",
    "tap-",
    "tap adapter",
    "tailscale",
    "zerotier",
    "openvpn",
    "vpn",
    "anyconnect",
    "pangp",
    "fortinet",
];

fn named(name: &str, prefixes: &[&str]) -> bool {
    let name = name.to_lowercase();
    prefixes.iter().any(|prefix| name.starts_with(prefix))
}

fn tunnel_named(name: &str, described: Option<&(String, u32)>) -> bool {
    named(name, &TUNNELS)
        || described.is_some_and(|(description, kind)| {
            let description = description.to_lowercase();
            *kind == 53 || TUNNEL_DESCRIPTIONS.iter().any(|word| description.contains(word))
        })
}

/// Every Windows adapter's friendly name (if-addrs's name there), description and interface type; none elsewhere.
#[cfg(not(windows))]
fn adapter_descriptions() -> Vec<(String, String, u32)> {
    Vec::new()
}
#[cfg(windows)]
fn adapter_descriptions() -> Vec<(String, String, u32)> {
    use windows::Win32::NetworkManagement::IpHelper::{
        GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER, GAA_FLAG_SKIP_MULTICAST, GetAdaptersAddresses,
        IP_ADAPTER_ADDRESSES_LH,
    };
    use windows::Win32::Networking::WinSock::AF_UNSPEC;
    let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;
    let mut size: u32 = 16 * 1024;
    for _ in 0..3 {
        // u64s, so the list lies on the alignment its structures need.
        let mut buffer = vec![0u64; (size as usize).div_ceil(8)];
        let first = buffer.as_mut_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>();
        let result = unsafe { GetAdaptersAddresses(u32::from(AF_UNSPEC.0), flags, None, Some(first), &mut size) };
        if result == 111 {
            continue; // ERROR_BUFFER_OVERFLOW: size now says how much it needs.
        }
        if result != 0 {
            return Vec::new();
        }
        let mut adapters = Vec::new();
        let mut next = first;
        while !next.is_null() {
            let adapter = unsafe { &*next };
            let text = |text: windows::core::PWSTR| {
                if text.is_null() { String::new() } else { unsafe { text.to_string() }.unwrap_or_default() }
            };
            adapters.push((text(adapter.FriendlyName), text(adapter.Description), adapter.IfType));
            next = adapter.Next;
        }
        return adapters;
    }
    Vec::new()
}

/// The interfaces of the WireGuard tunnels up. macOS: a VPN service of WireGuard's app (its SubType says so) with an
/// address, from the system's configuration store. Linux: a device of WireGuard's type (or named wg…, as wireguard-go's
/// are). Windows knows it from the adapter's description instead.
#[cfg(target_os = "macos")]
fn wireguard_interfaces() -> Vec<String> {
    vpn_interfaces("wireguard")
}

/// The interfaces of the VPN services up whose SubType (their app's identifier) has the word in it.
#[cfg(target_os = "macos")]
fn vpn_interfaces(word: &str) -> Vec<String> {
    use system_configuration::core_foundation::base::{CFType, TCFType};
    use system_configuration::core_foundation::dictionary::CFDictionary;
    use system_configuration::core_foundation::string::CFString;
    use system_configuration::dynamic_store::SCDynamicStoreBuilder;
    let Some(store) = SCDynamicStoreBuilder::new("starmeter-phone").build() else { return Vec::new() };
    let text = |dictionary: &CFDictionary<CFString, CFType>, key: &'static str| {
        dictionary
            .find(CFString::from_static_string(key))
            .and_then(|value| value.downcast::<CFString>())
            .map(|value| value.to_string())
    };
    let dictionary = |key: String| {
        store
            .get(CFString::new(&key))
            .and_then(|value| value.downcast_into::<CFDictionary>())
            .map(|value| unsafe { CFDictionary::<CFString, CFType>::wrap_under_get_rule(value.as_concrete_TypeRef()) })
    };
    let Some(keys) = store.get_keys("Setup:/Network/Service/[^/]+/Interface") else { return Vec::new() };
    keys.iter()
        .filter_map(|key| {
            let key = key.to_string();
            let interface = dictionary(key.clone())?;
            if !text(&interface, "SubType")?.to_lowercase().contains(word) {
                return None;
            }
            let service = key.strip_prefix("Setup:")?.strip_suffix("/Interface")?;
            text(&dictionary(format!("State:{service}/IPv4"))?, "InterfaceName")
        })
        .collect()
}
#[cfg(target_os = "linux")]
fn wireguard_interfaces() -> Vec<String> {
    let Ok(entries) = std::fs::read_dir("/sys/class/net") else { return Vec::new() };
    entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let uevent = std::fs::read_to_string(entry.path().join("uevent")).unwrap_or_default();
            (uevent.lines().any(|line| line.trim() == "DEVTYPE=wireguard") || name.starts_with("wg")).then_some(name)
        })
        .collect()
}
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn wireguard_interfaces() -> Vec<String> {
    Vec::new()
}

/// A Wi-Fi of its own can hand out 100.64.0.0/10 too (a carrier's or a campus's): without Tailscale the route to
/// 100.100.100.100 is the default one, from the network's own address, which is no tailnet's.
fn tailnet_ip(ip: IpAddr, lan: Option<IpAddr>) -> Option<IpAddr> {
    (Some(ip) != lan && in_tailscales_range(ip)).then_some(ip)
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

fn kept_file(app: &AppHandle) -> PathBuf {
    let environment: HashMap<String, String> = std::env::vars().collect();
    let shell = starmeter_core::desktop::Shell::current();
    // Separate files for Standalone vs SwiftBar so running both never overwrites the other's port or tunnel (R10-8).
    let name = if matches!(&*app.state::<Data>(), Data::SwiftBar(_)) { "phone-swiftbar.json" } else { "phone.json" };
    PathBuf::from(starmeter_core::desktop::state_dir(shell, &environment)).join(name)
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

fn keep(app: &AppHandle, change: impl FnOnce(&mut Kept)) {
    let path = kept_file(app);
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
    let kept = read_kept(&kept_file(app));
    if kept.local.is_some() {
        let _ = start_local(app.clone());
    }
    if kept.public_until.is_some_and(|until| until > starmeter_core::runtime::now()) {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let _ = open_public(app, true).await;
        });
    } else if kept.public_until.is_some() {
        keep(app, |kept| kept.public_until = None);
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
        let listener = bind(read_kept(&kept_file(app)).port).map_err(|error| format!("没能开始：{error}"))?;
        let port = listener.local_addr().map_err(|error| error.to_string())?.port();
        listener.set_nonblocking(true).map_err(|error| error.to_string())?;
        keep(app, |kept| kept.port = Some(port));
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
            opening_active: 0,
        });
    }
    Ok(running.as_mut().expect("the server just started"))
}

/// Synchronous core for starting 同一个网络 (used by startup resume and async Tauri command).
pub fn start_local(app: AppHandle) -> Result<PhoneView, String> {
    let state = app.state::<Phone>();
    let mut running = state.running.lock().expect("the phone lock");
    let current = serving(&app, &mut running)?;
    let key = current
        .keys
        .lock()
        .expect("the keys")
        .local
        .get_or_insert_with(|| read_kept(&kept_file(&app)).local.unwrap_or_else(new_key))
        .clone();
    keep(&app, |kept| kept.local = Some(key));
    Ok(view(current))
}

/// 同一个网络 on (or kept on) and where it is: its key as kept when it was on as the app quit (a phone's link keeps
/// opening), else a new one, so a code from before its 停止 stays shut. The public link, if up, is left as it is.
/// Runs off the main thread so network interface enumeration and socket bind never stall AppKit (PERF-7).
#[tauri::command]
pub async fn phone_start(app: AppHandle) -> Result<PhoneView, String> {
    tauri::async_runtime::spawn_blocking(move || start_local(app)).await.map_err(|error| error.to_string())?
}

/// Where the phone view stands, if it is on. Runs off the main thread (PERF-7).
#[tauri::command]
pub async fn phone_status(app: AppHandle) -> Option<PhoneView> {
    tauri::async_runtime::spawn_blocking(move || {
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
            close_public_if_over(&app, current);
        }
        settle(&mut running);
        running.as_ref().and_then(visible_view)
    })
    .await
    .ok()
    .flatten()
}

/// 同一个网络's 停止: its addresses (the network's and Tailscale's) no longer open, its key gone with them; the public
/// link, if up, stays. Runs off the main thread so tunnel teardown never blocks AppKit (PERF-7).
#[tauri::command]
pub async fn phone_stop(app: AppHandle) -> Option<PhoneView> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<Phone>();
        let mut running = state.running.lock().expect("the phone lock");
        if let Some(current) = running.as_mut() {
            current.keys.lock().expect("the keys").local = None;
        }
        keep(&app, |kept| kept.local = None);
        settle(&mut running);
        running.as_ref().and_then(visible_view)
    })
    .await
    .ok()
    .flatten()
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
fn close_public_if_over(app: &AppHandle, running: &mut Running) {
    if running.public.as_ref().is_some_and(|public| !usable(public))
        && let Some(over) = running.public.take()
    {
        close_public(&running.keys, over);
        keep(app, |kept| kept.public_until = None);
    }
}

/// One public link opened at a time: an ask while another is on its way waits for it and takes its link, so two quick
/// clicks start one cloudflared, and a first download isn't written twice at once (TUNNEL-1).
static OPENING: Mutex<()> = Mutex::new(());

/// Opens the public link (Cloudflare's quick tunnel), the server started for it if 同一个网络 is off, and says where it
/// is. The link belongs to the round it was asked in: its 停止 meanwhile (or the app quitting), it is closed, not shown.
#[tauri::command]
pub async fn phone_public(app: AppHandle) -> Result<PhoneView, String> {
    open_public(app, false).await
}

async fn open_public(app: AppHandle, resuming: bool) -> Result<PhoneView, String> {
    let (port, server, round, keys) = {
        let state = app.state::<Phone>();
        let mut running = state.running.lock().expect("the phone lock");
        let current = serving(&app, &mut running)?;
        close_public_if_over(&app, current);
        if current.public.as_ref().is_some_and(usable) {
            return Ok(view(current));
        }
        current.opening += 1;
        current.opening_active += 1;
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
    if Arc::ptr_eq(&current.public_round, &round) {
        current.opening_active = current.opening_active.saturating_sub(1);
    }
    let answer = match opened {
        Err(error) => {
            // Startup resume that failed clears its kept expiry so a later manual 开始 gets a full hour (R10-8).
            if resuming {
                keep(&app, |kept| kept.public_until = None);
            }
            Err(error)
        }
        Ok(opened) if round.load(Ordering::SeqCst) => {
            if let Some((mut tunnel, key)) = opened {
                tunnel.stop();
                forget_public_key(&current.keys, &key);
            }
            Err("外网链接已经关闭".into())
        }
        Ok(Some((tunnel, key))) => {
            // Startup resume keeps what is left of the previous hour; a manual 开始 always starts a fresh hour (R10-8).
            let now = starmeter_core::runtime::now();
            let until = if resuming {
                read_kept(&kept_file(&app))
                    .public_until
                    .filter(|until| *until > now && *until <= now + PUBLIC_LIFETIME.as_secs_f64())
                    .unwrap_or(now + PUBLIC_LIFETIME.as_secs_f64())
            } else {
                now + PUBLIC_LIFETIME.as_secs_f64()
            };
            keep(&app, |kept| kept.public_until = Some(until));
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
/// Runs off the main thread so cloudflared process stop (up to 3s) never stalls AppKit (PERF-7).
#[tauri::command]
pub async fn phone_private(app: AppHandle) -> Option<PhoneView> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<Phone>();
        let mut running = state.running.lock().expect("the phone lock");
        if let Some(current) = running.as_mut() {
            current.public_round.store(true, Ordering::SeqCst);
            current.public_round = Arc::new(AtomicBool::new(false));
            current.opening_active = 0;
            if let Some(public) = current.public.take() {
                close_public(&current.keys, public);
            }
        }
        keep(&app, |kept| kept.public_until = None);
        settle(&mut running);
        running.as_ref().and_then(visible_view)
    })
    .await
    .ok()
    .flatten()
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
                    close_public_if_over(&app, current);
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
        // takes its bars' colours from that frame and keeps them. No live AI activity or session counts go into this
        // unauthenticated HTML (R10-2): reloads read their own last state from localStorage, fresh scans show 正在读取….
        Some(asset) if file == "index.html" => {
            let page = with_look(&String::from_utf8_lossy(&asset.bytes), &computer_look(&app.state::<Data>()));
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

/// The page with the computer's look written in before its first script runs (no live activity or session counts in
/// unauthenticated HTML, R10-2).
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

#[derive(Default)]
struct SampleCache {
    seen_today: f64,
    seen_sessions: f64,
    sessions_was_on: Option<bool>,
    last_sessions: Option<Value>,
}

/// The last minutes recorded from the app's start: a reading every couple of seconds while the panel shows or the
/// phone view is on, every ten otherwise. Cheap (STAGE-14, R10-9): today's figures and 会话 from their caches only,
/// rendered only when their caches changed, never holding RECENT.lock() across disk I/O or waiting on a render under
/// way; whether the panel shows from its flag, not asked of the main thread.
pub fn keep_recording(app: AppHandle) {
    let _ = std::thread::Builder::new().name("starmeter-live-readings".into()).spawn(move || {
        let mut cache = SampleCache::default();
        loop {
            sample_cached(&app.state::<Data>(), &mut cache);
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
/// fixtures. Nothing when the app is busy rendering or within RECENT_EVERY of the last reading: the next turn takes it.
fn sample_cached(data: &Data, cache: &mut SampleCache) {
    let Data::Engine(engine) = data else { return sample(data) };
    let moment = starmeter_core::runtime::now();
    // Check throttle and grab any fallback dashboard before releasing RECENT so disk I/O never blocks requests (R10-9).
    let fallback = {
        let recent = RECENT.lock().expect("the live readings");
        if recent
            .back()
            .and_then(|last| last["at"].as_f64())
            .is_some_and(|at| moment - at < RECENT_EVERY && moment >= at)
        {
            return;
        }
        recent
            .back()
            .map(|last| json!({"date": last["date"], "rangeKey": last["rangeKey"], "providers": last["providers"]}))
    };
    let Some(updated_sessions) = engine.sessions_cached_if_free(&mut cache.seen_sessions, &mut cache.sessions_was_on)
    else {
        return;
    };
    if let Some(mut fresh) = updated_sessions {
        if fresh.is_object() {
            phone_sessions(&mut fresh);
            cache.last_sessions = Some(fresh);
        } else {
            cache.last_sessions = None;
        }
    }
    let fresh_today = engine.today_cached(cache.seen_today);
    let (dashboard, next_today) = match fresh_today {
        Some((dashboard, stamp)) => (dashboard, Some(stamp)),
        None => match fallback {
            Some(last) => (last, None),
            None => return,
        },
    };
    let sessions = cache.last_sessions.as_ref();
    let stored = {
        let mut recent = RECENT.lock().expect("the live readings");
        keep_recent(&mut recent, starmeter_core::runtime::now(), &dashboard, sessions)
    };
    // Advance seen_today only once the reading actually landed in RECENT (R10-9: never drop an update to the throttle).
    if stored && let Some(stamp) = next_today {
        cache.seen_today = stamp;
    }
}

/// A reading kept: when, the day and range, and each tool's figures the line counts (Summary's share, what a gateway
/// alone logged). Returns true if stored, false if skipped/throttled.
fn keep_recent(recent: &mut VecDeque<Value>, moment: f64, dashboard: &Value, sessions: Option<&Value>) -> bool {
    let Some(providers) = dashboard.get("providers").and_then(Value::as_array) else { return false };
    if recent.back().and_then(|last| last["at"].as_f64()).is_some_and(|at| moment - at < RECENT_EVERY && moment >= at) {
        return false;
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
    true
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
            let sessions = sessions_of(&snapshot);
            let turns = if sessions.is_object() { files.turns() } else { serde_json::Map::new() };
            json!({
                "at": starmeter_core::runtime::now(),
                "dashboard": snapshot.get("dashboard").cloned().unwrap_or(Value::Null),
                "sessions": sessions,
                "turns": turns,
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
        Addresses, AtomicBool, AtomicUsize, CONNECTIONS, Deadline, Kept, Keys, Mutex, Nic, Ordering, Running, Slot,
        Slots, admitted, bind, lan_choice, networks, new_key, nic, read_kept, same, settle, tailnet_ip, write_kept,
    };

    fn ip(text: &str) -> std::net::IpAddr {
        text.parse().unwrap()
    }
    /// An interface as macOS or Linux list it: its name, point-to-point or not, its address.
    fn unix(name: &str, p2p: bool, address: &str) -> Nic {
        nic(name.into(), p2p, ip(address), None, &[])
    }
    /// One as Windows lists it, with its adapter's description and type (6 an Ethernet card's, 71 Wi-Fi's, 53 a virtual
    /// one's such as Wintun's).
    fn windows(name: &str, description: &str, kind: u32, address: &str) -> Nic {
        nic(name.into(), false, ip(address), Some((description.into(), kind)), &[])
    }

    /// The route's own address as before; a proxy's TUN or a VPN holding the route gives way to the Wi-Fi's.
    #[test]
    fn the_networks_address_is_the_wifis_not_a_proxys_tunnel() {
        // The report: Clash's TUN on macOS (utun1500, point-to-point, 198.18.0.0) holds the route; en0 is the Wi-Fi.
        let clash = [
            unix("lo0", false, "127.0.0.1"),
            unix("en0", false, "192.168.31.234"),
            unix("utun1500", true, "198.18.0.0"),
        ];
        assert_eq!(lan_choice(Some(ip("198.18.0.0")), &clash, None), Some(ip("192.168.31.234")));
        // Clash's TUN on Windows isn't point-to-point; its range gives it away.
        let meta = [
            windows("Meta", "Meta Tunnel", 53, "198.18.0.1"),
            windows("vEthernet (WSL)", "Hyper-V Virtual Ethernet Adapter", 6, "172.24.80.1"),
            windows("Wi-Fi", "Intel(R) Wi-Fi 6E AX211 160MHz", 71, "192.168.1.8"),
        ];
        assert_eq!(lan_choice(Some(ip("198.18.0.1")), &meta, None), Some(ip("192.168.1.8")));
        // A VPN's tunnel holds the route: the Wi-Fi before a virtual machine's bridge.
        let vpn = [
            unix("bridge100", false, "192.168.64.1"),
            unix("utun4", true, "10.8.0.2"),
            unix("en0", false, "192.168.1.5"),
        ];
        assert_eq!(lan_choice(Some(ip("10.8.0.2")), &vpn, None), Some(ip("192.168.1.5")));
        // Without a proxy, the route's address, even one outside the private ranges (a campus's 100.64/10 Wi-Fi).
        let plain = [unix("en0", false, "100.69.38.146"), unix("utun3", true, "100.76.55.73")];
        assert_eq!(lan_choice(Some(ip("100.69.38.146")), &plain, None), Some(ip("100.69.38.146")));
        assert_eq!(lan_choice(Some(ip("192.168.1.5")), &vpn, None), Some(ip("192.168.1.5")));
        // The interfaces couldn't be listed: the route's address, unless it is a proxy's.
        assert_eq!(lan_choice(Some(ip("192.168.1.5")), &[], None), Some(ip("192.168.1.5")));
        assert_eq!(lan_choice(Some(ip("198.18.0.0")), &[], None), None);
        // Only tunnels and the loopback: nothing a phone could reach.
        assert_eq!(
            lan_choice(
                Some(ip("198.18.0.0")),
                &[unix("lo0", false, "127.0.0.1"), unix("utun1500", true, "198.18.0.0")],
                None
            ),
            None
        );
    }

    /// R11-1: a campus's Wi-Fi hands out 100.64/10, outside the private ranges, and a proxy's TUN holds the route; a
    /// virtual machine's or a container's bridge (192.168.64.1, WSL's 172.24…) is private, but a phone can't reach it.
    #[test]
    fn a_real_card_comes_before_a_bridge_whatever_their_addresses() {
        let mac = [
            unix("bridge100", false, "192.168.64.1"),
            unix("en0", false, "100.69.38.146"),
            unix("utun5", true, "198.18.0.1"),
            unix("vnic0", false, "10.211.55.2"),
            unix("feth2", false, "10.10.0.1"),
        ];
        assert_eq!(lan_choice(Some(ip("198.18.0.1")), &mac, None), Some(ip("100.69.38.146")));
        let windows_campus = [
            windows("vEthernet (WSL)", "Hyper-V Virtual Ethernet Adapter", 6, "172.24.80.1"),
            windows("Meta", "Meta Tunnel", 53, "198.18.0.1"),
            windows("WLAN", "Realtek 8852BE Wireless LAN WiFi 6 PCI-E NIC", 71, "100.69.38.146"),
        ];
        assert_eq!(lan_choice(Some(ip("198.18.0.1")), &windows_campus, None), Some(ip("100.69.38.146")));
        // Only bridges: one still, a private one first (Hyper-V's external switch holds the card's own address).
        let bridges = [unix("bridge100", false, "100.70.0.1"), unix("vEthernet (External)", false, "192.168.1.20")];
        assert_eq!(lan_choice(Some(ip("198.18.0.1")), &bridges, None), Some(ip("192.168.1.20")));
    }

    /// R11-2: Windows lists Wintun's adapters (WireGuard, Tailscale, sing-box, Clash's) as virtual ones and TAP's as
    /// Ethernet cards, neither point-to-point: their names, descriptions or type say they are tunnels, whatever their
    /// addresses; the route through one gives way to the Wi-Fi's.
    #[test]
    fn windows_tunnels_are_known_by_their_adapters() {
        let wifi = || windows("Wi-Fi", "Intel(R) Wi-Fi 6E AX211 160MHz", 71, "192.168.1.8");
        let cases = [
            (windows("sing-box", "sing-box Tunnel", 53, "172.19.0.1"), "172.19.0.1"),
            (windows("office", "WireGuard Tunnel", 53, "10.8.0.2"), "10.8.0.2"),
            (windows("Local Area Connection 2", "TAP-Windows Adapter V9", 6, "10.8.0.6"), "10.8.0.6"),
            (windows("ZeroTier One [8056c2e21c000001]", "ZeroTier Virtual Port", 6, "10.147.17.5"), "10.147.17.5"),
            (windows("Tailscale", "Tailscale Tunnel", 53, "100.76.55.73"), "100.76.55.73"),
            (windows("Ethernet 3", "Some VPN Client Virtual Adapter", 6, "10.20.0.3"), "10.20.0.3"),
        ];
        for (tunnel, routed) in cases {
            let name = tunnel.name.clone();
            assert!(tunnel.tunnel, "{name}");
            assert_eq!(lan_choice(Some(ip(routed)), &[tunnel, wifi()], None), Some(ip("192.168.1.8")), "{name}");
        }
        // Real cards are none: an Ethernet card, a Wi-Fi card, Hyper-V's switch (a bridge, not a tunnel).
        for card in [
            windows("Ethernet", "Intel(R) Ethernet Connection (17) I219-LM", 6, "192.168.1.9"),
            windows("以太网", "Realtek PCIe GbE Family Controller", 6, "192.168.1.9"),
            wifi(),
            windows("vEthernet (Default Switch)", "Hyper-V Virtual Ethernet Adapter", 6, "172.17.0.1"),
        ] {
            assert!(!card.tunnel, "{}", card.name);
        }
    }

    /// Tailscale's and WireGuard's addresses go on codes of their own, never on the network's: Tailscale's from its route
    /// (an interface not known as its, an exit node holding the default route), WireGuard's from its tunnel (on macOS a
    /// VPN service of its app, on Linux a device of its type, on Windows its adapter's description).
    #[test]
    fn tailscale_and_wireguard_have_codes_of_their_own() {
        let (lan, tailnet, wireguard) = (Some(ip("192.168.1.5")), Some(ip("100.76.55.73")), Some(ip("10.0.0.2")));
        // macOS: Tailscale and WireGuard both up beside the Wi-Fi.
        let mac = [
            unix("en0", false, "192.168.1.5"),
            unix("utun4", true, "100.76.55.73"),
            nic("utun6".into(), true, ip("10.0.0.2"), None, &["utun6".into()]),
        ];
        let both = Addresses { lan, tailnet, wireguard };
        assert_eq!(networks(Some(ip("192.168.1.5")), Some(ip("100.76.55.73")), &mac), both);
        // WireGuard holding every route (AllowedIPs 0.0.0.0/0), Tailscale's exit node too: the same three.
        assert_eq!(networks(Some(ip("10.0.0.2")), Some(ip("100.76.55.73")), &mac), both);
        assert_eq!(networks(Some(ip("100.76.55.73")), Some(ip("100.76.55.73")), &mac), both);
        // Another VPN's utun is no WireGuard's.
        let other = [unix("en0", false, "192.168.1.5"), unix("utun6", true, "10.0.0.2")];
        assert_eq!(
            networks(Some(ip("10.0.0.2")), Some(ip("192.168.1.5")), &other),
            Addresses { lan, ..Addresses::default() }
        );
        // Linux's wg0 and tailscale0; Windows's tunnel named after its user's choice, described as WireGuard's.
        let linux = [
            unix("wlp2s0", false, "192.168.1.5"),
            unix("tailscale0", true, "100.76.55.73"),
            nic("wg0".into(), true, ip("10.0.0.2"), None, &["wg0".into()]),
        ];
        assert_eq!(networks(Some(ip("192.168.1.5")), Some(ip("100.76.55.73")), &linux), both);
        let pc = [
            windows("Wi-Fi", "Intel(R) Wi-Fi 6E AX211 160MHz", 71, "192.168.1.5"),
            windows("Tailscale", "Tailscale Tunnel", 53, "100.76.55.73"),
            windows("home", "WireGuard Tunnel", 53, "10.0.0.2"),
        ];
        assert_eq!(networks(Some(ip("100.76.55.73")), Some(ip("100.76.55.73")), &pc), both);
        // A campus's 100.64/10 Wi-Fi with Tailscale on an interface not known as its: Tailscale's route tells them apart.
        let campus = [unix("Ethernet 3", false, "100.76.55.73"), unix("Wi-Fi", false, "100.69.38.146")];
        assert_eq!(
            networks(Some(ip("198.18.0.1")), Some(ip("100.76.55.73")), &campus),
            Addresses { lan: Some(ip("100.69.38.146")), tailnet, wireguard: None }
        );
        // No Tailscale on that campus: its route is the default one, the Wi-Fi's.
        let alone = [unix("en0", false, "100.69.38.146")];
        assert_eq!(
            networks(Some(ip("100.69.38.146")), Some(ip("100.69.38.146")), &alone),
            Addresses { lan: Some(ip("100.69.38.146")), tailnet: None, wireguard: None }
        );
    }

    #[test]
    fn a_tailnet_address_is_tailscales_range_and_not_the_networks_own() {
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
        // With sessions.json beside panel-view.json: turns stays empty when 会话 is off the homepage, and carries exact
        // per-second turn start timestamps once 会话 is on the homepage (matching standalone's m:ss clock).
        std::fs::write(
            folder.join("sessions.json"),
            serde_json::json!({"schema": 1, "payload": {"sessions": [{"id": "s1", "turns": [{"at": 1700000123.5}]}]}})
                .to_string(),
        )
        .unwrap();
        assert!(super::live_from(&data, false)["turns"].as_object().unwrap().is_empty());
        let with_sessions = serde_json::json!({"schema": 1, "entries": [{"label": "会话", "onOpenAction": ["refresh-source", "sessions"], "children": []}],
            "dashboard": {"date": "2026-11-25", "rangeKey": "today", "providers": [{"id": "claude", "available": true, "tokens": 100.0, "output": 1.0, "cost": 1.0}]}});
        std::fs::write(&view, with_sessions.to_string()).unwrap();
        assert_eq!(super::live_from(&data, false)["turns"]["s1"].as_f64(), Some(1700000123.5));
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
        let shown = super::with_look(page, &serde_json::json!({"theme": "dark", "style": "standard"}));
        let look = shown.find("window.STARMETER_LOOK={\"style\":\"standard\",\"theme\":\"dark\"};").expect("the look");
        assert!(look < shown.find("first-frame.js").unwrap());
        // Unauthenticated HTML never carries live AI activity or session counts (R10-2).
        assert!(!shown.contains("STARMETER_LIVE"));
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
            opening_active: opening,
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
