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
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

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

/// The server runs while a way in is on, and they come and go on their own (用户：同一网络和任何网络……互不影响):
/// 同一个网络 (this computer's network address and its Tailscale one) with its key, the public link with a key of its
/// own, and 自有域名 (the user's own domain, through their own reverse proxy or tunnel) with a third, so stopping one
/// leaves the others' codes opening as they did.
struct Running {
    port: u16,
    keys: Arc<Mutex<Keys>>,
    stop: Arc<AtomicBool>,
    /// 自有域名's address (https://meter.example.com) while it is on.
    origin: Option<String>,
    public: Option<Public>,
    /// The public link's round: closed (its 停止, or the server stopping), a link still being made is closed as it
    /// comes instead of shown.
    public_round: Arc<AtomicBool>,
    /// Asks for the public link on their way: the server stays for them with 同一个网络 off.
    opening: usize,
    /// Asks belonging to the current (un-cancelled) round: shown in PhoneView so 停止 clears the spinner immediately.
    opening_active: usize,
}

/// The keys the server takes, each way's own. The network gets in through 同一个网络 only (用户：这样同一网络那里就直接
/// 打开了吧，逻辑上很不对), a VPN (Tailscale, WireGuard) through 任何网络's VPN page only (用户：同一网络|反代，任何网络，
/// 就这两个分类。tailscale,等vpn, cloudflare放到任意网络里): with its way off, a connection to that address is turned
/// away at once. The public link's key and 自有域名's come through
/// a proxy or tunnel on this computer (cloudflared; Caddy, Nginx, a tunnel's client), which hands requests on from
/// 127.0.0.1; 自有域名's from the network too while 同一个网络 is on, for a proxy on a NAS or the router forwarding to
/// this computer's network address.
#[derive(Default)]
struct Keys {
    local: Option<String>,
    vpn: Option<String>,
    public: Option<String>,
    /// 自有域名's host (meter.example.com) while one is set: a request naming it came through the user's proxy, and
    /// opens with 同一个网络's key while that is on (the domain is a row of its page, no switch or key of its own:
    /// 用户：去掉开关，改成域名一行).
    domain_host: Option<String>,
    /// 自有域名's port (443 for https, 80 for http, or an explicit port): a direct VPN/LAN connection to the server's
    /// own port on the same IP does not match `Via::Domain`.
    domain_port: Option<u16>,
}

/// Where a connection comes from: this computer itself, a phone on a VPN (to this computer's Tailscale or WireGuard
/// address), or the network (its address on the Wi-Fi).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Origin {
    Here,
    Vpn,
    Network,
}

/// Which name a request came by (its Host): Cloudflare's quick tunnel (cloudflared hands requests on with the name the
/// phone asked for, a trycloudflare.com one), 自有域名's, or neither (an address: the network's, a VPN's, 127.0.0.1).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Via {
    Direct,
    Tunnel,
    Domain,
}

/// The host a Host header names, lower case, without its port.
fn host_name(header: &str) -> String {
    let header = header.trim().to_ascii_lowercase();
    let name = match header.strip_prefix('[') {
        Some(rest) => rest.split(']').next().unwrap_or("").to_string(),
        None => header.split(':').next().unwrap_or("").to_string(),
    };
    name.trim_end_matches('.').to_string()
}

/// The explicit port in a Host header, if any.
fn host_port(header: &str) -> Option<u16> {
    let header = header.trim();
    let port_str = match header.strip_prefix('[') {
        Some(rest) => rest.split_once(']')?.1.strip_prefix(':')?,
        None => header.rsplit_once(':')?.1,
    };
    port_str.parse::<u16>().ok()
}

/// 自有域名's host, for Keys, from its origin (https://meter.example.com).
fn domain_host(origin: Option<&str>) -> Option<String> {
    origin.and_then(|origin| tauri::Url::parse(origin).ok()).and_then(|url| url.host_str().map(host_name))
}

/// 自有域名's effective port (explicit or scheme default 80/443), for port-aware `Via::Domain` matching.
fn domain_port(origin: Option<&str>) -> Option<u16> {
    origin.and_then(|origin| tauri::Url::parse(origin).ok()).and_then(|url| url.port_or_known_default())
}

fn via_with_port(header: &str, domain: Option<&str>, port: Option<u16>) -> Via {
    let host = host_name(header);
    // Check Cloudflare's quick tunnel first so setting a *.trycloudflare.com domain can never hijack Via::Tunnel.
    if host.ends_with(".trycloudflare.com") {
        Via::Tunnel
    } else if !host.is_empty()
        && domain == Some(host.as_str())
        && match (port, host_port(header)) {
            (Some(expected), Some(actual)) => actual == expected,
            _ => true,
        }
    {
        Via::Domain
    } else {
        Via::Direct
    }
}

#[cfg(test)]
fn via(host: &str, domain: Option<&str>) -> Via {
    via_with_port(host, domain, None)
}

impl Keys {
    /// Whether a connection from there may ask anything: this computer always; a VPN with its page on (or when a
    /// reverse proxy reaches this computer over VPN for 自有域名); the network with 同一个网络 on.
    fn welcome(&self, from: Origin) -> bool {
        match from {
            Origin::Here => true,
            Origin::Vpn => self.vpn.is_some() || (self.local.is_some() && self.domain_host.is_some()),
            Origin::Network => self.local.is_some(),
        }
    }

    /// For connections arriving on a VPN interface when the VPN page itself is off, only allow requests naming
    /// 自有域名 (from a reverse proxy over Tailscale/WireGuard), rejecting direct VPN probes before serving HTML.
    fn vpn_allows_host(&self, host: &str) -> bool {
        self.vpn.is_some()
            || (self.local.is_some()
                && via_with_port(host, self.domain_host.as_deref(), self.domain_port) == Via::Domain)
    }
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
    /// Whether the VPN page is on (任何网络 › Tailscale 等 VPN): its addresses below, on the VPNs this computer is on.
    vpn_open: bool,
    /// http://100.x.y.z:51234/#k=… through Tailscale, with the VPN key: a phone on the tailnet opens the page from
    /// anywhere, nothing going through Cloudflare.
    tailnet: Option<String>,
    /// http://10.x.y.z:51234/#k=… through a WireGuard tunnel this computer has up, the same way.
    wireguard: Option<String>,
    /// https://meter.example.com/#k=… while 同一个网络 is on and a domain is set: the user's own proxy hands it on to
    /// this server; 同一个网络's key.
    domain: Option<String>,
    /// The server's port (自有域名's proxy forwards to 127.0.0.1 on it).
    port: u16,
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
    let (local, vpn) = {
        let keys = running.keys.lock().expect("the keys");
        (keys.local.clone(), keys.vpn.clone())
    };
    let found = (local.is_some() || vpn.is_some()).then(addresses).unwrap_or_default();
    let address = |ip: IpAddr, key: &str| format!("http://{ip}:{}/#k={key}", running.port);
    let public = running.public.as_ref().filter(|public| usable(public));
    PhoneView {
        local_open: local.is_some(),
        local: local.as_deref().and_then(|key| found.lan.map(|ip| address(ip, key))),
        vpn_open: vpn.is_some(),
        tailnet: vpn.as_deref().and_then(|key| found.tailnet.map(|ip| address(ip, key))),
        wireguard: vpn.as_deref().and_then(|key| found.wireguard.map(|ip| address(ip, key))),
        domain: running.origin.as_ref().zip(local.as_ref()).map(|(origin, key)| format!("{origin}/#k={key}")),
        port: running.port,
        public: public.map(|public| format!("{}/#k={}", public.tunnel.url, public.key)),
        seconds_left: public.map_or(0, |public| public.until.saturating_duration_since(Instant::now()).as_secs()),
        public_checked: running.public.as_ref().is_none_or(|public| public.tunnel.checked),
        public_opening: running.opening_active > 0,
    }
}

/// The view shown to the panel, or None when no way is on and no active-round public link is opening.
fn visible_view(running: &Running) -> Option<PhoneView> {
    let v = view(running);
    (v.local_open || v.vpn_open || v.public.is_some() || v.public_opening).then_some(v)
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

/// The three addresses from the routes' and the interfaces': Tailscale's when its route leaves through a tunnel in
/// 100.64.0.0/10 (with an exit node both routes are its), never the network's: a campus's 100.64/10 Wi-Fi card, the
/// way to 100.100.100.100 with Tailscale off and a proxy's TUN leaving 100.64/10 out, stays the network's; WireGuard's
/// tunnel's, beside them.
fn networks(routed: Option<IpAddr>, tailscale: Option<IpAddr>, nics: &[Nic]) -> Addresses {
    let tailnets =
        |ip: IpAddr| in_tailscales_range(ip) && (nics.is_empty() || nics.iter().any(|nic| nic.ip == ip && nic.tunnel));
    let tailnet_route = tailscale.filter(|ip| Some(*ip) != routed && tailnets(*ip));
    let lan = lan_choice(routed, nics, tailnet_route);
    let tailnet = tailscale.filter(|ip| tailnets(*ip)).and_then(|ip| tailnet_ip(ip, lan));
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

/// The key each way's link opens with, on or off: the same from one 开始 to the next and through the app's restarts
/// and updates, so a phone's link (a code scanned once, a bookmark) keeps opening whenever its way is on, until the
/// user asks for a new one with 换链接 (用户：每次的内网链接都不同？那个key可不可以固定啊？用户可以手动刷新，刷新之前就
/// 不要改变). Before, a way's 停止 let go of its key and the next 开始 made another.
#[derive(Serialize, Deserialize, Default, Clone, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
struct Fixed {
    #[serde(default)]
    local: Option<String>,
    #[serde(default)]
    vpn: Option<String>,
}

/// What the user left on, kept in the settings folder (outside the app, so an update keeps it too), and on again when
/// the app starts (用户：默认是关的，但是你要记住用户的状态。就算重新安装新版本，也要记住之前用户开没开): 同一个网络 with
/// the server's port and its key, so a phone's link from before keeps opening; the public link until its hour is up
/// (its address is Cloudflare's, new each time, so a phone scans its new code). `local`, `vpn` and `domain_key` say a
/// way is on (None: off); `fixed` has each way's key whether it is on or not.
#[derive(Serialize, Deserialize, Default, Clone, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
struct Kept {
    #[serde(default)]
    port: Option<u16>,
    /// The port is one the user gave (同一网络's or 反代's 换端口): held from then on, never moved from for a free one,
    /// until they leave its field empty again (用户：同一网络也支持指定端口号吧).
    #[serde(default)]
    port_chosen: bool,
    #[serde(default)]
    local: Option<String>,
    /// The VPN page's key (Tailscale, WireGuard) while it is on.
    #[serde(default)]
    vpn: Option<String>,
    /// When the public link's hour is up, in seconds since 1970.
    #[serde(default)]
    public_until: Option<f64>,
    /// 自有域名's address as the user gave it (https://meter.example.com). Kept while set, on or off: the server's port
    /// is then held to `port` (the user's proxy forwards there) instead of moving to a free one when it is taken.
    #[serde(default)]
    domain: Option<String>,
    #[serde(default)]
    fixed: Fixed,
}

/// A way's key as it starts: the one it is on with (the server running), else the one kept from when it was left on
/// (the app starting again), else its link's own from before its 停止, else a new one (its first 开始).
fn starting_key(on: &mut Option<String>, kept_on: Option<String>, fixed: Option<String>) -> String {
    on.get_or_insert_with(|| kept_on.or(fixed).unwrap_or_else(new_key)).clone()
}

/// A page of 在手机上看 whose links 换链接 changes: 同一网络 / 反代 (the network's code and 自有域名's, one key), or the
/// VPN page (Tailscale's and WireGuard's, one key).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum LinkPage {
    Near,
    Vpn,
}

/// 换链接: a new key for each of a page's ways, the one before opening nothing from then on. A way that is on goes on
/// with the new key at once (its code changes on the page); one that is off starts with it next time.
fn renew(keys: &mut Keys, kept: &mut Kept, page: LinkPage) {
    fn fresh(on: &mut Option<String>, kept_on: &mut Option<String>, fixed: &mut Option<String>) {
        let key = new_key();
        if on.is_some() {
            *on = Some(key.clone());
        }
        if kept_on.is_some() {
            *kept_on = Some(key.clone());
        }
        *fixed = Some(key);
    }
    match page {
        LinkPage::Near => {
            fresh(&mut keys.local, &mut kept.local, &mut kept.fixed.local);
        }
        LinkPage::Vpn => fresh(&mut keys.vpn, &mut kept.vpn, &mut kept.fixed.vpn),
    }
}

fn kept_file(app: &AppHandle) -> PathBuf {
    let environment: HashMap<String, String> = std::env::vars().collect();
    let shell = starmeter_core::desktop::Shell::current();
    // Separate files for Standalone vs SwiftBar so running both never overwrites the other's port or tunnel (R10-8).
    let name = if matches!(&*app.state::<Data>(), Data::SwiftBar(_)) { "phone-swiftbar.json" } else { "phone.json" };
    PathBuf::from(starmeter_core::desktop::state_dir(shell, &environment)).join(name)
}

/// A domain set: the port is held, the user's proxy forwarding there.
fn proxy_set_up(kept: &Kept) -> bool {
    kept.domain.is_some()
}

/// The kept state; a key that isn't one of ours (32 hex digits), or an address that isn't one, is left out.
fn read_kept(path: &Path) -> Kept {
    let mut kept: Kept =
        std::fs::read(path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
    let ours = |key: &String| key.len() == 32 && key.bytes().all(|b| b.is_ascii_hexdigit());
    kept.port = kept.port.filter(|port| valid_port(*port).is_ok());
    kept.port_chosen = kept.port_chosen && kept.port.is_some();
    kept.local = kept.local.filter(ours);
    kept.vpn = kept.vpn.filter(ours);
    kept.domain = kept.domain.and_then(|domain| origin_of(&domain).ok());
    kept.fixed.local = kept.fixed.local.filter(ours);
    kept.fixed.vpn = kept.fixed.vpn.filter(ours);
    kept
}

/// 自有域名's address as typed (meter.example.com, https://meter.example.com/, http://home.lan:8080) as an origin:
/// https unless it says http, no path, query, # or user (the page's files and its /api/ are at the root, and its key
/// travels after the #).
fn origin_of(typed: &str) -> Result<String, String> {
    let typed = typed.trim();
    if typed.is_empty() {
        return Err("填写你的域名或 IP，比如 starmeter.example.com".into());
    }
    let wrong = || "格式不对：填域名或 IP，比如 starmeter.example.com".to_string();
    // Reject internal whitespace (Url::parse silently strips \t, \r, \n) and malformed hosts.
    if typed.chars().any(char::is_whitespace) || !host_as_typed(typed) {
        return Err(wrong());
    }
    let given = typed.contains("://");
    let mut url =
        tauri::Url::parse(&if given { typed.to_string() } else { format!("https://{typed}") }).map_err(|_| wrong())?;
    // An IP with no scheme typed is http (a proxy at an address seldom has a certificate for it; 用户：域名那里是域名/IP),
    // read again so a port given stays as given.
    let host = url.host_str().unwrap_or_default().trim_start_matches('[').trim_end_matches(']');
    if host.parse::<std::net::Ipv4Addr>().is_ok() {
        // Reject hex/octal/fullwidth IPv4 shorthands (e.g. 1.0x1, １.２) that Url::parse normalizes into IPv4.
        let rest = typed.split_once("://").map_or(typed, |(_, rest)| rest);
        let raw_host = rest.split(['/', '?', '#']).next().unwrap_or_default().split(':').next().unwrap_or_default();
        if !raw_host.bytes().all(|b| b.is_ascii_digit() || b == b'.') || raw_host.parse::<std::net::Ipv4Addr>().is_err()
        {
            return Err(wrong());
        }
    }
    if !given && host.parse::<IpAddr>().is_ok() {
        url = tauri::Url::parse(&format!("http://{typed}")).map_err(|_| wrong())?;
    }
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none_or(str::is_empty)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path(), "" | "/")
    {
        return Err(wrong());
    }
    Ok(url.origin().ascii_serialization())
}

/// Whether what was typed names a host as one is written: an IPv4 address in its four parts, an IPv6 one in brackets,
/// or a domain of two labels or more (letters, digits, - inside a label, the last not all digits; any language's
/// letters), with a port of 1 to 65535 if one is given. The scheme and a / after the host are left to the parser.
fn host_as_typed(typed: &str) -> bool {
    let rest = typed.split_once("://").map_or(typed, |(_, rest)| rest);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let (host, port) = if let Some(inside) = authority.strip_prefix('[') {
        let Some((address, after)) = inside.split_once(']') else { return false };
        if address.parse::<std::net::Ipv6Addr>().is_err() {
            return false;
        }
        (None, if after.is_empty() { None } else { Some(after.strip_prefix(':').unwrap_or("x")) })
    } else {
        match authority.split_once(':') {
            Some((host, port)) => (Some(host), Some(port)),
            None => (Some(authority), None),
        }
    };
    if let Some(port) = port
        && !(port.bytes().all(|b| b.is_ascii_digit()) && port.parse::<u16>().is_ok_and(|port| port > 0))
    {
        return false;
    }
    let Some(host) = host else { return true };
    if host.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
        return host.parse::<std::net::Ipv4Addr>().is_ok();
    }
    let labels: Vec<&str> = host.split('.').collect();
    let label = |label: &&str| {
        !label.is_empty()
            && label.chars().count() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label.chars().all(|c| c.is_alphanumeric() || c == '-')
    };
    host.chars().count() <= 253
        && labels.len() >= 2
        && labels.iter().all(label)
        && !labels.last().is_some_and(|last| last.chars().all(|c| c.is_ascii_digit()))
}

/// A port the user can give: one an app may take without the system's say (1024 and above).
fn valid_port(port: u16) -> Result<u16, String> {
    if port >= 1024 { Ok(port) } else { Err("端口填 1024 到 65535 之间的数".into()) }
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
    if let Ok(mut file) = options.open(&fresh) {
        let wrote = file.write_all(&bytes).is_ok();
        drop(file);
        if !wrote || std::fs::rename(&fresh, path).is_err() {
            let _ = std::fs::remove_file(&fresh);
        }
    }
}

fn keep(app: &AppHandle, change: impl FnOnce(&mut Kept)) {
    let path = kept_file(app);
    let mut kept = read_kept(&path);
    change(&mut kept);
    write_kept(&path, &kept);
}

/// Whether something answers on this computer's own address at the port. On macOS (and Windows) a program listening
/// on 127.0.0.1 alone doesn't stop the server binding all addresses at the same port, and then takes every connection
/// from this computer: cloudflared's, the user's proxy's. Such a port counts as taken.
fn loopback_taken(port: u16) -> bool {
    TcpStream::connect_timeout(&std::net::SocketAddr::from(([127, 0, 0, 1], port)), Duration::from_millis(200)).is_ok()
}

/// The server's socket, on the port it had before when it is free (a phone's link names it), else on a new one. Held
/// (自有域名 set: the user's proxy forwards to that port), only that port: taken, a reason saying so. A server just
/// stopped lets go of it within a beat of its loop, so a taken port is asked again for a moment before giving up.
fn bind(port: Option<u16>, held: bool) -> Result<TcpListener, String> {
    match port.filter(|_| held) {
        Some(port) => {
            let given_up = Instant::now() + Duration::from_millis(800);
            loop {
                if loopback_taken(port) {
                    if Instant::now() < given_up {
                        std::thread::sleep(Duration::from_millis(50));
                        continue;
                    }
                    return Err(format!("端口 {port} 被别的程序占用"));
                }
                match TcpListener::bind(("0.0.0.0", port)) {
                    Ok(listener) => return Ok(listener),
                    Err(error) if error.kind() == std::io::ErrorKind::AddrInUse && Instant::now() < given_up => {
                        std::thread::sleep(Duration::from_millis(50))
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
                        return Err(format!("端口 {port} 被别的程序占用"));
                    }
                    Err(error) => return Err(format!("没能开始：{error}")),
                }
            }
        }
        None => port
            .and_then(|port| {
                // A server stopped a moment ago lets go of the port within a beat of its loop (R12-9): asked again
                // for a moment, so a quick 停止 then 开始 keeps the address a phone's link names.
                let given_up = Instant::now() + Duration::from_millis(200);
                loop {
                    if !loopback_taken(port)
                        && let Ok(listener) = TcpListener::bind(("0.0.0.0", port))
                    {
                        return Some(listener);
                    }
                    if Instant::now() >= given_up {
                        return None;
                    }
                    std::thread::sleep(Duration::from_millis(40));
                }
            })
            .map_or_else(|| TcpListener::bind("0.0.0.0:0"), Ok)
            .map_err(|error| format!("没能开始：{error}")),
    }
}

/// A server on `listener`, its ways in all off.
fn spawn(app: &AppHandle, listener: TcpListener) -> Result<Running, String> {
    let port = listener.local_addr().map_err(|error| error.to_string())?.port();
    listener.set_nonblocking(true).map_err(|error| error.to_string())?;
    let saved = read_kept(&kept_file(app)).domain;
    let keys = Arc::new(Mutex::new(Keys {
        domain_host: domain_host(saved.as_deref()),
        domain_port: domain_port(saved.as_deref()),
        ..Keys::default()
    }));
    // 自有域名's address, the same network's code at it shown beside the network's.
    let stop = Arc::new(AtomicBool::new(false));
    {
        let (app, keys, stop) = (app.clone(), keys.clone(), stop.clone());
        std::thread::spawn(move || serve(app, listener, keys, stop));
    }
    Ok(Running {
        port,
        keys,
        stop,
        origin: saved,
        public: None,
        public_round: Arc::new(AtomicBool::new(false)),
        opening: 0,
        opening_active: 0,
    })
}

/// The app started (an update too): what the user left on, on again, before anyone opens the panel. Each step looks at
/// what is kept as it starts, under the lock (R12-2): a way the user stopped meanwhile stays stopped, and a link the
/// user opened meanwhile isn't closed.
pub fn resume(app: &AppHandle) {
    let _ = resume_local(app.clone());
    let _ = resume_vpn(app.clone());
    {
        let state = app.state::<Phone>();
        let _running = state.running.lock().expect("the phone lock");
        let now = starmeter_core::runtime::now();
        match read_kept(&kept_file(app)).public_until {
            Some(until) if until > now => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = open_public(app, true).await;
                });
            }
            // Its hour over while the app was off.
            Some(_) => keep(app, |kept| kept.public_until = kept.public_until.filter(|until| *until > now)),
            None => {}
        }
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
        // A port the user gave is held: taken, a reason saying so, and its field to change it. One the app picked: that
        // one when it is free, another when it isn't, so a port taken never keeps a way from starting (R12-6); with a
        // domain set it stays kept all the same (its proxy forwards there), the server on another for now and 反代's
        // 开始 moving it back.
        let kept = read_kept(&kept_file(app));
        let fresh = spawn(app, bind(kept.port, kept.port_chosen)?)?;
        keep(app, |kept| {
            if kept.port.is_none() || !(kept.port_chosen || proxy_set_up(kept)) {
                kept.port = Some(fresh.port);
            }
        });
        *running = Some(fresh);
    }
    Ok(running.as_mut().expect("the server just started"))
}

/// Synchronous core for starting 同一个网络 (used by startup resume and async Tauri command).
pub fn start_local(app: AppHandle) -> Result<PhoneView, String> {
    start_local_if(app, None, false)
}

/// 同一个网络 on again as the app starts, if it is still kept on.
fn resume_local(app: AppHandle) -> Result<PhoneView, String> {
    start_local_if(app, None, true)
}

/// `port`: as its page's 换端口 left it. A number: the server on that port (moved there when it runs on another, as
/// 反代's start moves it), and the port held from then on. 0: the field left empty, the app picks again. None: as it
/// is.
fn start_local_if(app: AppHandle, port: Option<u16>, kept_on: bool) -> Result<PhoneView, String> {
    let state = app.state::<Phone>();
    let mut running = state.running.lock().expect("the phone lock");
    if kept_on && read_kept(&kept_file(&app)).local.is_none() {
        return Err("已停止".into());
    }
    match port {
        Some(0) => keep(&app, |kept| kept.port_chosen = false),
        Some(port) => {
            on_port(&app, &mut running, valid_port(port)?)?;
            keep(&app, |kept| {
                kept.port = Some(port);
                kept.port_chosen = true;
            });
        }
        // A domain set: on the port its proxy forwards to, said when taken (as a port the user gave is).
        None => {
            let kept = read_kept(&kept_file(&app));
            if let (Some(port), true) = (kept.port, proxy_set_up(&kept)) {
                on_port(&app, &mut running, port)?;
            }
        }
    }
    let current = serving(&app, &mut running)?;
    let kept = read_kept(&kept_file(&app));
    let key = starting_key(&mut current.keys.lock().expect("the keys").local, kept.local, kept.fixed.local);
    let actual_port = current.port;
    keep(&app, |kept| {
        kept.local = Some(key.clone());
        kept.fixed.local = Some(key);
        // When the user cleared the port field (Some(0)), sync kept.port to the actual bound port even if a domain is set.
        if port == Some(0) {
            kept.port = Some(actual_port);
        }
    });
    Ok(view(current))
}

/// The VPN page on (任何网络 › Tailscale 等 VPN), with its own key: the one its links had before (a phone's link keeps
/// opening, through its 停止 and the app's restarts), a new one the first time. The other ways as they are.
pub fn start_vpn(app: AppHandle) -> Result<PhoneView, String> {
    start_vpn_if(app, false)
}

/// The VPN page on again as the app starts, if it is still kept on.
fn resume_vpn(app: AppHandle) -> Result<PhoneView, String> {
    start_vpn_if(app, true)
}

fn start_vpn_if(app: AppHandle, kept_on: bool) -> Result<PhoneView, String> {
    let state = app.state::<Phone>();
    let mut running = state.running.lock().expect("the phone lock");
    if kept_on && read_kept(&kept_file(&app)).vpn.is_none() {
        return Err("已停止".into());
    }
    let current = serving(&app, &mut running)?;
    let kept = read_kept(&kept_file(&app));
    let key = starting_key(&mut current.keys.lock().expect("the keys").vpn, kept.vpn, kept.fixed.vpn);
    keep(&app, |kept| {
        kept.vpn = Some(key.clone());
        kept.fixed.vpn = Some(key);
    });
    Ok(view(current))
}

#[tauri::command]
pub async fn phone_vpn_start(app: AppHandle) -> Result<PhoneView, String> {
    tauri::async_runtime::spawn_blocking(move || start_vpn(app)).await.map_err(|error| error.to_string())?
}

/// The VPN page's 停止: its addresses no longer open (its key stays for its next 开始: the same links); the other ways
/// as they are.
#[tauri::command]
pub async fn phone_vpn_stop(app: AppHandle) -> Option<PhoneView> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<Phone>();
        let mut running = state.running.lock().expect("the phone lock");
        if let Some(current) = running.as_mut() {
            current.keys.lock().expect("the keys").vpn = None;
        }
        keep(&app, |kept| kept.vpn = None);
        settle(&mut running);
        running.as_ref().and_then(visible_view)
    })
    .await
    .ok()
    .flatten()
}

/// 同一个网络 on (or kept on) and where it is: its key the one its link had before (a phone's link keeps opening,
/// through its 停止 and the app's restarts, until 换链接), a new one the first time. The public link, if up, is left as
/// it is.
/// Runs off the main thread so network interface enumeration and socket bind never stall AppKit (PERF-7).
#[tauri::command]
pub async fn phone_start(app: AppHandle, port: Option<u16>) -> Result<PhoneView, String> {
    tauri::async_runtime::spawn_blocking(move || start_local_if(app, port, false))
        .await
        .map_err(|error| error.to_string())?
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

/// 同一个网络's 停止: its address no longer opens (connections to it are let go of as they come; its key stays for its
/// next 开始: the same link); the public link, if up, stays. Runs off the main thread so tunnel teardown never blocks
/// AppKit (PERF-7).
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

/// 换链接 on a page of 在手机上看 ("near": 同一网络 / 反代; "vpn": the VPN page): new keys for its links, at once. The
/// codes and links from before open nothing from then on (a phone showing the page says its link is no longer good),
/// and the page's codes are the new ones; the other page and the public link as they are.
#[tauri::command]
pub async fn phone_new_link(app: AppHandle, page: String) -> Option<PhoneView> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<Phone>();
        let mut running = state.running.lock().expect("the phone lock");
        let page = match page.as_str() {
            "near" => Some(LinkPage::Near),
            "vpn" => Some(LinkPage::Vpn),
            _ => None,
        };
        if let Some(page) = page {
            keep(&app, |kept| match running.as_mut() {
                Some(current) => renew(&mut current.keys.lock().expect("the keys"), kept, page),
                None => renew(&mut Keys::default(), kept, page),
            });
        }
        running.as_ref().and_then(visible_view)
    })
    .await
    .ok()
    .flatten()
}

/// 自有域名 as set, for its page: the address, the port the user's proxy forwards to, and this computer's network
/// address (for a proxy on another device).
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DomainSetup {
    /// https://meter.example.com; None until one is set.
    domain: Option<String>,
    /// The server's port: the one it is on, else the one kept, else a free one now, kept so it is the same next time.
    port: u16,
    /// The port is one the user gave (held), not one the app picked.
    chosen: bool,
    /// 192.168.1.5, when this computer is on a network.
    lan: Option<String>,
    /// 100.76.55.73, when this computer is on a tailnet; 10.0.0.2 when it has a WireGuard tunnel up (the VPN page says
    /// which before its 开始).
    tailnet: Option<String>,
    wireguard: Option<String>,
}

#[tauri::command]
pub async fn phone_domain(app: AppHandle) -> Option<DomainSetup> {
    tauri::async_runtime::spawn_blocking(move || {
        // Under the lock: a free port kept here isn't written over a server just started on another.
        let state = app.state::<Phone>();
        let running = state.running.lock().expect("the phone lock");
        let on = running.as_ref().map(|current| current.port);
        let kept = read_kept(&kept_file(&app));
        // With a domain set, the port its proxy forwards to (the server may be on another for now, 反代 off).
        let port =
            kept.port.filter(|_| proxy_set_up(&kept) || kept.port_chosen).or(on).or(kept.port).unwrap_or_else(|| {
                let free =
                    TcpListener::bind("0.0.0.0:0").and_then(|socket| socket.local_addr()).map_or(51234, |a| a.port());
                keep(&app, |kept| kept.port = Some(free));
                free
            });
        drop(running);
        let found = addresses();
        DomainSetup {
            domain: kept.domain,
            port,
            chosen: kept.port_chosen,
            lan: found.lan.map(|ip| ip.to_string()),
            tailnet: found.tailnet.map(|ip| ip.to_string()),
            wireguard: found.wireguard.map(|ip| ip.to_string()),
        }
    })
    .await
    .ok()
}

/// 自有域名 set (the domain's row, 应用), or cleared (its field emptied): kept, and while the server runs, its code
/// beside the network's at once, the server's port kept as the one its proxy forwards to. Nothing starts or stops.
#[tauri::command]
pub async fn phone_domain_set(app: AppHandle, domain: String) -> Result<Option<PhoneView>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let origin = if domain.trim().is_empty() { None } else { Some(origin_of(&domain)?) };
        let state = app.state::<Phone>();
        let mut running = state.running.lock().expect("the phone lock");
        let on = running.as_ref().map(|current| current.port);
        if let Some(current) = running.as_mut() {
            let mut keys = current.keys.lock().expect("the keys");
            keys.domain_host = domain_host(origin.as_deref());
            keys.domain_port = domain_port(origin.as_deref());
            drop(keys);
            current.origin.clone_from(&origin);
        }
        keep(&app, |kept| {
            let had_domain = kept.domain.is_some();
            kept.domain = origin;
            // Only lock kept.port to the running server's port when 同一个网络 is active or no port was held yet
            // (avoid clobbering a held port with a temporary VPN/Cloudflare fallback port while 同一个网络 is off).
            if kept.domain.is_some()
                && on.is_some()
                && (kept.local.is_some() || kept.port.is_none() || !(kept.port_chosen || had_domain))
            {
                kept.port = on;
            }
        });
        Ok(running.as_ref().and_then(visible_view))
    })
    .await
    .map_err(|error| error.to_string())?
}

/// The server on the port asked for, and no other. A server whose ways all went off a moment ago goes first; then it is
/// moved there when it runs on another (同一个网络's codes go with it, Cloudflare's link closes), started there when it
/// isn't running. Taken, a reason saying so and nothing changed.
fn on_port(app: &AppHandle, running: &mut Option<Running>, port: u16) -> Result<(), String> {
    if running.as_ref().is_some_and(|current| current.stop.load(Ordering::SeqCst))
        && let Some(mut over) = running.take()
    {
        stop_running(&mut over);
    }
    match running.as_ref().map(|current| current.port) {
        Some(on) if on != port => {
            let listener = bind(Some(port), true)?;
            moved(app, running, listener)?;
        }
        Some(_) => {}
        None => *running = Some(spawn(app, bind(Some(port), true)?)?),
    }
    Ok(())
}

/// The server on another port (自有域名's, changed): 同一个网络 and 自有域名 go on, their keys with them; the public
/// link, Cloudflare's to the port before, is closed.
fn moved(app: &AppHandle, running: &mut Option<Running>, listener: TcpListener) -> Result<(), String> {
    let Some(mut over) = running.take() else { return Ok(()) };
    let kept_keys = std::mem::take(&mut *over.keys.lock().expect("the keys"));
    let origin = over.origin.take();
    stop_running(&mut over);
    keep(app, |kept| kept.public_until = None);
    let fresh = spawn(app, listener)?;
    *fresh.keys.lock().expect("the keys") = Keys { public: None, ..kept_keys };
    *running = Some(Running { origin, ..fresh });
    Ok(())
}

/// How 自有域名's address answered this computer: through the user's proxy to this server, or why not.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DomainCheck {
    /// This server answered through it.
    opens: bool,
    /// Something else answered, with this status (a proxy's 502, a redirect).
    status: Option<u16>,
    /// Nothing answered: why (a name not found, a time out, a certificate).
    reason: Option<String>,
}

/// Whether 自有域名's address opens this server from here, asked once it is on (and on 再检查). This computer getting
/// through is a good sign, not a promise for a phone outside (and a router may not hand its own network's requests
/// back in, so a miss here is said as a caution).
#[tauri::command]
pub async fn phone_domain_check(app: AppHandle) -> Option<DomainCheck> {
    tauri::async_runtime::spawn_blocking(move || {
        let (origin, key) = {
            let state = app.state::<Phone>();
            let running = state.running.lock().expect("the phone lock");
            let current = running.as_ref()?;
            (current.origin.clone()?, current.keys.lock().expect("the keys").local.clone()?)
        };
        let request = starmeter_core::http::Request {
            headers: vec![("X-Starmeter-Key", key)],
            timeout: 8.0,
            follow_redirects: false,
            limit: 1024,
            ..Default::default()
        };
        Some(checked(starmeter_core::http::send_explained(&format!("{origin}/api/ping"), request)))
    })
    .await
    .ok()
    .flatten()
}

fn checked(answer: Result<starmeter_core::http::Response, starmeter_core::http::Unreachable>) -> DomainCheck {
    match answer {
        Ok(answer) if answer.status == 200 && answer.body == PING => {
            DomainCheck { opens: true, status: None, reason: None }
        }
        Ok(answer) => DomainCheck { opens: false, status: Some(answer.status), reason: None },
        Err(why) => DomainCheck { opens: false, status: None, reason: Some(why.0) },
    }
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

/// The server stays while something is on: 同一个网络, 自有域名, the public link, or an ask for it on its way.
fn settle(running: &mut Option<Running>) {
    let idle = running.as_ref().is_some_and(|current| {
        let keys = current.keys.lock().expect("the keys");
        keys.local.is_none() && keys.vpn.is_none() && current.public.is_none() && current.opening == 0
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
        // The app starting: only if its hour is still kept (its 停止 meanwhile keeps it closed, R12-2).
        if resuming
            && !read_kept(&kept_file(&app)).public_until.is_some_and(|until| until > starmeter_core::runtime::now())
        {
            return Err("外网链接已经关闭".into());
        }
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
            // Startup resume that failed clears its kept expiry so a later manual 开始 gets a full hour (R10-8); a link
            // open meanwhile keeps its own.
            if resuming && current.public.is_none() {
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
        Ok(Some((mut tunnel, key))) => 'opened: {
            // Startup resume keeps what is left of the previous hour, and opens nothing once it is gone (R12-2); a
            // manual 开始 always starts a fresh hour (R10-8).
            let now = starmeter_core::runtime::now();
            let until = if resuming {
                let left = read_kept(&kept_file(&app))
                    .public_until
                    .filter(|until| *until > now && *until <= now + PUBLIC_LIFETIME.as_secs_f64());
                let Some(left) = left else {
                    tunnel.stop();
                    forget_public_key(&current.keys, &key);
                    break 'opened Err("外网链接已经关闭".into());
                };
                left
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
            // Not waited for: whoever holds it may be binding this very port again, and this loop holds the port.
            if let Ok(mut running) = app.state::<Phone>().running.try_lock()
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
                // From the network with 同一个网络 off, or to a VPN's address with the VPN page off: turned away at once,
                // as if nothing listened.
                let from = if peer.ip().is_loopback() {
                    Origin::Here
                } else if stream.local_addr().ok().is_some_and(|to| vpn_addresses().contains(&Some(to.ip()))) {
                    Origin::Vpn
                } else {
                    Origin::Network
                };
                if !keys.lock().map(|keys| keys.welcome(from)).unwrap_or(false) {
                    continue;
                }
                // Past the limit, or with no thread to be had, the connection is closed and the server goes on.
                let Some(slot) = slots.take() else { continue };
                let (app, keys) = (app.clone(), keys.clone());
                let _ = std::thread::Builder::new().name("starmeter-phone".into()).spawn(move || {
                    let _slot = slot;
                    let _ = answer(&app, stream, &keys, from);
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(80))
            }
            Err(_) => std::thread::sleep(Duration::from_millis(200)),
        }
    }
    // The port let go before the lock is waited for: whoever holds it may be binding this very port again (自有域名's
    // held port, asked again for a moment while a server just stopped lets go of it).
    drop(listener);
    // Both ways in off (or the app quitting): the public link goes with the server.
    let state = app.state::<Phone>();
    let mut running = state.running.lock().expect("the phone lock");
    if running.as_ref().is_some_and(|current| Arc::ptr_eq(&current.stop, &stop))
        && let Some(mut over) = running.take()
    {
        stop_running(&mut over);
    }
}

/// Whether a request's key opens the page's figures. From this computer, any way's (a proxy or tunnel here hands them
/// all on). From a VPN, the VPN page's, and 自有域名's while it is on (a proxy elsewhere reaching this computer over the
/// tailnet). From the network, 同一个网络's, and 自有域名's while 同一个网络 is on (a proxy on a NAS or the router).
/// Each door opens with its own way only; the public link's key only ever from this computer, as cloudflared hands it on.
/// A request that names Cloudflare's link takes its key alone, and one that names 自有域名 takes 自有域名's alone, so a
/// key for the network or a VPN doesn't open the page through either (R12-1): with 反代 off, its proxy still forwarding
/// to this computer gets nothing. A proxy that sends its own address as the name (127.0.0.1:port, the network address)
/// is taken as that address.
fn admitted(keys: &Mutex<Keys>, given: &str, from: Origin, host: &str) -> bool {
    let keys = keys.lock().expect("the keys");
    let opens = |key: &Option<String>| key.as_deref().is_some_and(|key| same(given, key));
    match via_with_port(host, keys.domain_host.as_deref(), keys.domain_port) {
        Via::Tunnel => from == Origin::Here && opens(&keys.public),
        // The user's proxy, wherever it is (here, a NAS on the network, elsewhere over a VPN): 同一个网络's key.
        Via::Domain => opens(&keys.local),
        Via::Direct => match from {
            Origin::Here => opens(&keys.local) || opens(&keys.vpn) || opens(&keys.public),
            Origin::Vpn => opens(&keys.vpn),
            Origin::Network => opens(&keys.local),
        },
    }
}

/// This computer's VPN addresses (Tailscale's, WireGuard's), looked at again every few seconds: a connection to one is
/// a VPN's.
fn vpn_addresses() -> [Option<IpAddr>; 2] {
    static SEEN: Mutex<Option<(Instant, [Option<IpAddr>; 2])>> = Mutex::new(None);
    let mut seen = SEEN.lock().expect("the VPN addresses");
    match *seen {
        Some((at, found)) if at.elapsed() < Duration::from_secs(5) => found,
        _ => {
            let found = addresses();
            let found = [found.tailnet, found.wireguard];
            *seen = Some((Instant::now(), found));
            found
        }
    }
}

fn answer(app: &AppHandle, mut stream: TcpStream, keys: &Mutex<Keys>, from: Origin) -> std::io::Result<()> {
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
    let (mut given, mut host) = (String::new(), String::new());
    loop {
        if started.elapsed() > REQUEST_TIME {
            return Err(std::io::ErrorKind::TimedOut.into());
        }
        let mut header = String::new();
        if reader.read_line(&mut header)? == 0 || header.trim().is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            if name.trim().eq_ignore_ascii_case("x-starmeter-key") {
                given = value.trim().to_string();
            } else if name.trim().eq_ignore_ascii_case("host") {
                host = value.trim().to_string();
            }
        }
    }
    if from == Origin::Vpn && !keys.lock().expect("the keys").vpn_allows_host(&host) {
        return Ok(());
    }
    if method != "GET" {
        return respond(&mut stream, 405, "text/plain; charset=utf-8", b"GET only", false);
    }
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    if let Some(call) = path.strip_prefix("/api/") {
        if !admitted(keys, &given, from, &host) {
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
    let was_active = crate::engine::phone_active();
    crate::engine::PHONE_READ.store(now, Ordering::SeqCst);
    if !was_active {
        crate::engine::wake_clock();
        wake_recorder();
    }
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
        Data::SwiftBar(files) => files.snapshot_readonly().get("dashboard").cloned().unwrap_or(Value::Null),
        Data::Fixtures(fixtures) => fixtures.lock().expect("the fixtures").dashboard(range),
    }
}

/// What a phone sees: the homepage for the range it asked for, and nothing else. The settings name folders and key
/// files, and the rows' insides hold addresses and paths: both stay on the computer.
fn phone_snapshot(app: &AppHandle, range: Option<&str>, group: Option<&str>) -> Value {
    let mut snapshot = match &*app.state::<Data>() {
        Data::Engine(engine) => engine.snapshot(),
        Data::SwiftBar(files) => files.snapshot_readonly(),
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
const RECENT_IDLE: f64 = 30.0;

fn recorder_wake() -> &'static (Mutex<()>, Condvar) {
    static WAKE: OnceLock<(Mutex<()>, Condvar)> = OnceLock::new();
    WAKE.get_or_init(|| (Mutex::new(()), Condvar::new()))
}

/// Wakes the live-history recorder immediately when the panel opens/pins or a web viewer connects.
pub fn wake_recorder() {
    let (_, wake) = recorder_wake();
    wake.notify_all();
}

#[derive(Default)]
struct SampleCache {
    seen_today: f64,
    seen_sessions: f64,
    sessions_was_on: Option<bool>,
    last_sessions: Option<Value>,
    seen_snapshot: Option<SystemTime>,
    last_snapshot_dashboard: Option<Value>,
    last_snapshot_sessions: Option<Value>,
}

/// The last minutes recorded from the app's start: a reading every couple of seconds while the panel is open/pinned or
/// a web viewer is actively reading (within 60s), and relaxed to 30s with zero redundant disk/JSON reads when idle.
pub fn keep_recording(app: AppHandle) {
    let _ = std::thread::Builder::new().name("starmeter-live-readings".into()).spawn(move || {
        let mut cache = SampleCache::default();
        let (lock, wake) = recorder_wake();
        loop {
            let watched = crate::engine::watched();
            sample_cached(&app.state::<Data>(), &mut cache, watched);
            let wait = Duration::from_secs_f64(if watched { RECENT_EVERY } else { RECENT_IDLE });
            let guard = lock.lock().expect("the recorder lock");
            let _ = wake.wait_timeout(guard, wait);
        }
    });
}

/// A reading for the last minutes from what the source has cached: the app's today when its cache was written again
/// (else the last reading's figures again while watched so the curve runs without a gap), SwiftBar's snapshot only
/// re-read when its file mtime actually changed (zero disk read / JSON parse when idle and unchanged).
fn sample_cached(data: &Data, cache: &mut SampleCache, watched: bool) {
    match data {
        Data::SwiftBar(files) => {
            let mtime = files.snapshot_modified();
            let changed = mtime.is_some() && mtime != cache.seen_snapshot;
            if !changed {
                if !watched {
                    return;
                }
                if let Some(dashboard) = cache.last_snapshot_dashboard.as_ref() {
                    let mut recent = RECENT.lock().expect("the live readings");
                    keep_recent(
                        &mut recent,
                        starmeter_core::runtime::now(),
                        dashboard,
                        cache.last_snapshot_sessions.as_ref(),
                    );
                    return;
                }
            }
            if let Some((dashboard, mut sessions)) = today_now(data) {
                if sessions.is_object() {
                    phone_sessions(&mut sessions);
                }
                let sessions_opt = sessions.is_object().then_some(sessions);
                let stored = {
                    let mut recent = RECENT.lock().expect("the live readings");
                    keep_recent(&mut recent, starmeter_core::runtime::now(), &dashboard, sessions_opt.as_ref())
                };
                if stored || changed {
                    cache.seen_snapshot = mtime;
                    cache.last_snapshot_dashboard = Some(dashboard);
                    cache.last_snapshot_sessions = sessions_opt;
                }
            }
            return;
        }
        Data::Fixtures(_) => {
            if watched {
                sample(data);
            }
            return;
        }
        Data::Engine(_) => {}
    }
    let Data::Engine(engine) = data else { return };
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
    let sessions_changed = updated_sessions.is_some();
    if let Some(mut fresh) = updated_sessions {
        if fresh.is_object() {
            phone_sessions(&mut fresh);
            cache.last_sessions = Some(fresh);
        } else {
            cache.last_sessions = None;
        }
    }
    let fresh_today = engine.today_cached(cache.seen_today);
    if !watched && fresh_today.is_none() && !sessions_changed {
        return;
    }
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
fn ui_tag(app: &AppHandle) -> Option<String> {
    static TAG: OnceLock<Option<String>> = OnceLock::new();
    TAG.get_or_init(|| {
        let bytes = app.asset_resolver().get("index.html".to_string())?.bytes;
        let mut hash: u64 = 0xcbf29ce484222325;
        for &byte in &bytes {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
        Some(format!("{hash:016x}"))
    })
    .clone()
}

fn live(app: &AppHandle, history: bool) -> Value {
    let mut answer = live_from(&app.state::<Data>(), history);
    if let (Some(tag), Some(object)) = (ui_tag(app), answer.as_object_mut()) {
        object.insert("ui".into(), json!(tag));
    }
    answer
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
            let snapshot = files.snapshot_readonly();
            let interval = snapshot.get("pinInterval").and_then(Value::as_u64).unwrap_or(2).clamp(1, 5);
            files.request_live_if_stale(interval as f64);
            let sessions = sessions_of(&snapshot);
            let (turns, pending) =
                if sessions.is_object() { files.turns_and_pending() } else { (serde_json::Map::new(), Vec::new()) };
            json!({
                "at": starmeter_core::runtime::now(),
                "interval": interval,
                "dashboard": snapshot.get("dashboard").cloned().unwrap_or(Value::Null),
                "sessions": sessions,
                "turns": turns,
                "pending": pending,
            })
        }
        Data::Fixtures(fixtures) => {
            let mut fixtures = fixtures.lock().expect("the fixtures");
            let _ = fixtures.act(&["refresh-live".to_string()]);
            let snapshot = fixtures.snapshot();
            let interval = snapshot.get("pinInterval").and_then(Value::as_u64).unwrap_or(2).clamp(1, 5);
            json!({
                "at": starmeter_core::runtime::now(),
                "interval": interval,
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
        Addresses, AtomicBool, AtomicUsize, CONNECTIONS, Deadline, DomainCheck, Fixed, Kept, Keys, LinkPage, Mutex,
        Nic, Ordering, Origin, PING, Running, Slot, Slots, Via, admitted, bind, checked, domain_host, domain_port,
        host_name, lan_choice, loopback_taken, networks, new_key, nic, origin_of, read_kept, renew, same, settle,
        starting_key, tailnet_ip, valid_port, via, write_kept,
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
        // A campus's 100.64/10 Wi-Fi, a proxy's TUN leaving 100.64/10 out and a VM's bridge, Tailscale off: the way to
        // 100.100.100.100 leaves through the Wi-Fi card, still the network's, never taken for Tailscale's.
        let campus_off = [
            unix("bridge100", false, "192.168.64.1"),
            unix("en0", false, "100.69.38.146"),
            unix("utun5", true, "198.18.0.1"),
        ];
        assert_eq!(
            networks(Some(ip("198.18.0.1")), Some(ip("100.69.38.146")), &campus_off),
            Addresses { lan: Some(ip("100.69.38.146")), tailnet: None, wireguard: None }
        );
        // The same campus with Tailscale on (utun4): the Wi-Fi the network's, utun4 Tailscale's.
        let campus_on = [
            unix("bridge100", false, "192.168.64.1"),
            unix("en0", false, "100.69.38.146"),
            unix("utun4", true, "100.76.55.73"),
            unix("utun5", true, "198.18.0.1"),
        ];
        assert_eq!(
            networks(Some(ip("198.18.0.1")), Some(ip("100.76.55.73")), &campus_on),
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
        // 用户：同一网络和任何网络……互不影响. 用户：这样同一网络那里就直接打开了吧，逻辑上很不对. 用户：同一网络|反代，
        // 任何网络，就这两个分类。tailscale,等vpn, cloudflare放到任意网络里.
        use Origin::{Here, Network, Vpn};
        let (local, vpn, public) = (new_key(), new_key(), new_key());
        let keys = Mutex::new(Keys {
            local: Some(local.clone()),
            vpn: Some(vpn.clone()),
            public: Some(public.clone()),
            domain_host: None,
            domain_port: None,
        });
        // 同一个网络's key from the network and this computer, not a VPN's address.
        assert!(admitted(&keys, &local, Network, ""));
        assert!(admitted(&keys, &local, Here, ""));
        assert!(!admitted(&keys, &local, Vpn, ""));
        // The VPN page's from a VPN and this computer, not the network.
        assert!(admitted(&keys, &vpn, Vpn, ""));
        assert!(admitted(&keys, &vpn, Here, ""));
        assert!(!admitted(&keys, &vpn, Network, ""));
        // The public link's only as cloudflared hands it on, from this computer itself.
        assert!(admitted(&keys, &public, Here, ""));
        assert!(!admitted(&keys, &public, Network, ""));
        assert!(!admitted(&keys, &public, Vpn, ""));
        assert!(keys.lock().unwrap().welcome(Network) && keys.lock().unwrap().welcome(Vpn));
        // 同一个网络 stopped: the network is kept out; the others as they were.
        keys.lock().unwrap().local = None;
        assert!(!admitted(&keys, &local, Network, ""));
        assert!(!admitted(&keys, &local, Here, ""));
        assert!(!keys.lock().unwrap().welcome(Network));
        assert!(admitted(&keys, &vpn, Vpn, ""));
        // The VPN page stopped: the VPNs are kept out.
        keys.lock().unwrap().vpn = None;
        assert!(!admitted(&keys, &vpn, Vpn, ""));
        assert!(!keys.lock().unwrap().welcome(Vpn));
        assert!(keys.lock().unwrap().welcome(Here));
        assert!(admitted(&keys, &public, Here, ""));
        // Nothing empty opens anything.
        assert!(!admitted(&keys, "", Here, ""));
    }

    #[test]
    fn a_key_opens_the_page_only_by_its_own_way_in_whatever_name_the_request_came_by() {
        // R12-1: cloudflared and a proxy on this computer both hand requests on from 127.0.0.1. The name a request came
        // by tells them apart: Cloudflare's link takes its own key alone; 自有域名 (a row of 同一个网络's page, no key of
        // its own: 用户：去掉开关，改成域名一行) the same network's key alone.
        use Origin::{Here, Network, Vpn};
        let (local, vpn, public) = (new_key(), new_key(), new_key());
        let keys = Mutex::new(Keys {
            local: Some(local.clone()),
            vpn: Some(vpn.clone()),
            public: Some(public.clone()),
            domain_host: domain_host(Some("https://meter.example.com")),
            domain_port: domain_port(Some("https://meter.example.com")),
        });
        let tunnel = "bright-owl-42.trycloudflare.com";
        // Through Cloudflare's link: its key, nothing else.
        assert!(admitted(&keys, &public, Here, tunnel));
        assert!(!admitted(&keys, &local, Here, tunnel));
        assert!(!admitted(&keys, &vpn, Here, tunnel));
        // Through 自有域名 (its name, any case, with a port or a trailing dot): the same network's key, nothing else, from
        // a proxy here, on a NAS, or over a VPN.
        for name in ["meter.example.com", "METER.example.com:443", "meter.example.com."] {
            assert!(admitted(&keys, &local, Here, name), "{name}");
            assert!(!admitted(&keys, &vpn, Here, name), "{name}");
            assert!(!admitted(&keys, &public, Here, name), "{name}");
        }
        assert!(admitted(&keys, &local, Network, "meter.example.com"));
        assert!(admitted(&keys, &local, Vpn, "meter.example.com"));
        assert!(!admitted(&keys, &vpn, Vpn, "meter.example.com"));
        // By an address (the codes' own): each way's key as before.
        assert!(admitted(&keys, &local, Network, "192.168.1.20:51234"));
        assert!(admitted(&keys, &vpn, Vpn, "100.76.55.73:51234"));
        assert!(admitted(&keys, &vpn, Vpn, "[fd7a:115c:a1e0::1]:51234"));
        assert!(admitted(&keys, &public, Here, "127.0.0.1:51234"));
        // 同一个网络 stopped, the proxy still forwarding here: nothing opens through its name.
        keys.lock().unwrap().local = None;
        assert!(!admitted(&keys, &local, Here, "meter.example.com"));
        assert!(!admitted(&keys, &vpn, Here, "meter.example.com"));
        assert!(!admitted(&keys, &public, Here, "meter.example.com"));
        // No domain set: a name is taken as an address (a phone opening this computer by its .local or MagicDNS name).
        keys.lock().unwrap().local = Some(local.clone());
        keys.lock().unwrap().domain_host = None;
        assert!(admitted(&keys, &local, Network, "macbook.local:51234"));
    }

    #[test]
    fn a_hosts_name_is_its_host_alone() {
        assert_eq!(host_name("Meter.Example.com:8443"), "meter.example.com");
        assert_eq!(host_name("[FD7A::1]:80"), "fd7a::1");
        assert_eq!(host_name(" localhost "), "localhost");
        assert_eq!(domain_host(Some("http://home.lan:8080")).as_deref(), Some("home.lan"));
        assert_eq!(domain_host(None), None);
        assert_eq!(via("x.trycloudflare.com", Some("meter.example.com")), Via::Tunnel);
        assert_eq!(via("meter.example.com", Some("meter.example.com")), Via::Domain);
        assert_eq!(via("", Some("meter.example.com")), Via::Direct);
    }

    #[test]
    fn a_kept_port_the_user_could_not_give_is_no_port() {
        let folder = std::env::temp_dir().join(format!("starmeter-phone-port-{}", new_key()));
        let path = folder.join("phone.json");
        write_kept(&path, &Kept { port: Some(80), ..Kept::default() });
        assert_eq!(read_kept(&path).port, None);
        write_kept(&path, &Kept { port: Some(51234), ..Kept::default() });
        assert_eq!(read_kept(&path).port, Some(51234));
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn a_ways_link_keeps_its_key_from_one_start_to_the_next_until_a_new_link_is_asked_for() {
        // 用户：每次的内网链接都不同？那个key可不可以固定啊？用户可以手动刷新，刷新之前就不要改变。
        use Origin::{Network, Vpn};
        let folder = std::env::temp_dir().join(format!("starmeter-phone-fixed-{}", new_key()));
        let path = folder.join("phone.json");
        let mut kept = Kept::default();
        let keys = Mutex::new(Keys::default());
        // What each way's 开始 and 停止 do to the keys and to what is kept, the file written and read back between.
        let start_local = |keys: &Mutex<Keys>, kept: &mut Kept| {
            let key = starting_key(&mut keys.lock().unwrap().local, kept.local.clone(), kept.fixed.local.clone());
            kept.local = Some(key.clone());
            kept.fixed.local = Some(key.clone());
            write_kept(&path, kept);
            *kept = read_kept(&path);
            key
        };
        let stop_local = |keys: &Mutex<Keys>, kept: &mut Kept| {
            keys.lock().unwrap().local = None;
            kept.local = None;
            write_kept(&path, kept);
            *kept = read_kept(&path);
        };
        // The first 开始 makes a key; 停止 shuts the way, and the next 开始 opens it with the same key: the same link.
        let first = start_local(&keys, &mut kept);
        assert!(admitted(&keys, &first, Network, ""));
        stop_local(&keys, &mut kept);
        assert!(!admitted(&keys, &first, Network, ""));
        assert!(!keys.lock().unwrap().welcome(Network));
        assert_eq!(kept.local, None);
        assert_eq!(start_local(&keys, &mut kept), first);
        assert!(admitted(&keys, &first, Network, ""));
        // The app quit while it was off and started again (a server with no keys yet): still the same.
        stop_local(&keys, &mut kept);
        let again = Mutex::new(Keys::default());
        assert_eq!(start_local(&again, &mut kept), first);
        // 换链接 while it is on: another key at once, the one before opening nothing, kept as on with the new one.
        renew(&mut again.lock().unwrap(), &mut kept, LinkPage::Near);
        let second = again.lock().unwrap().local.clone().unwrap();
        assert_ne!(second, first);
        assert!(!admitted(&again, &first, Network, ""));
        assert!(admitted(&again, &second, Network, ""));
        assert_eq!((kept.local.as_ref(), kept.fixed.local.as_ref()), (Some(&second), Some(&second)));
        // And that one from then on, through a 停止.
        stop_local(&again, &mut kept);
        assert_eq!(start_local(&again, &mut kept), second);
        // 换链接 while it is off: the next 开始 has a new link, and nothing is turned on by it.
        stop_local(&again, &mut kept);
        renew(&mut again.lock().unwrap(), &mut kept, LinkPage::Near);
        assert_eq!((again.lock().unwrap().local.clone(), kept.local.clone()), (None, None));
        let third = start_local(&again, &mut kept);
        assert_ne!(third, second);

        // The VPN page's key is its own: 同一网络's 换链接 leaves it, its own changes it and nothing else.
        let vpn = starting_key(&mut again.lock().unwrap().vpn, kept.vpn.clone(), kept.fixed.vpn.clone());
        kept.vpn = Some(vpn.clone());
        kept.fixed.vpn = Some(vpn.clone());
        renew(&mut again.lock().unwrap(), &mut kept, LinkPage::Near);
        assert_eq!(again.lock().unwrap().vpn.as_ref(), Some(&vpn));
        let near = again.lock().unwrap().local.clone().unwrap();
        assert_ne!(near, third);
        renew(&mut again.lock().unwrap(), &mut kept, LinkPage::Vpn);
        let vpn_after = again.lock().unwrap().vpn.clone().unwrap();
        assert_ne!(vpn_after, vpn);
        assert!(admitted(&again, &vpn_after, Vpn, "") && !admitted(&again, &vpn, Vpn, ""));
        assert_eq!(again.lock().unwrap().local.as_ref(), Some(&near));
        assert_eq!(kept.fixed.vpn.as_ref(), Some(&vpn_after));

        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn a_file_kept_before_links_had_keys_of_their_own_starts_as_it_was_and_keeps_that_key() {
        // 6.0.26's file: a key only while a way is on. Left on: the same link when the app starts, and from then on.
        let folder = std::env::temp_dir().join(format!("starmeter-phone-before-{}", new_key()));
        let path = folder.join("phone.json");
        std::fs::create_dir_all(&folder).unwrap();
        let (local, vpn) = (new_key(), new_key());
        std::fs::write(&path, format!(r#"{{"port":51234,"local":"{local}","vpn":"{vpn}"}}"#)).unwrap();
        let kept = read_kept(&path);
        assert_eq!(kept.fixed, Fixed::default());
        let mut keys = Keys::default();
        assert_eq!(starting_key(&mut keys.local, kept.local.clone(), kept.fixed.local.clone()), local);
        assert_eq!(starting_key(&mut keys.vpn, kept.vpn.clone(), kept.fixed.vpn.clone()), vpn);
        // A key that is none of ours is no key; 自有域名's own key of 6.0.27 builds is let go (it opens with the same
        // network's now).
        std::fs::write(&path, r#"{"fixed":{"local":"zz","vpn":"0123","domain":"0123456789abcdef0123456789abcdef"}}"#)
            .unwrap();
        assert_eq!(read_kept(&path).fixed, Fixed::default());
        std::fs::write(
            &path,
            r#"{"domain":"https://meter.example.com","domainKey":"0123456789abcdef0123456789abcdef"}"#,
        )
        .unwrap();
        assert_eq!(read_kept(&path), Kept { domain: Some("https://meter.example.com".into()), ..Kept::default() });
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn a_port_let_go_a_moment_later_is_taken_again_not_moved_from() {
        // R12-9: a quick 停止 then 开始 keeps the address a phone's link names, the server before letting go of the
        // port within a beat of its loop.
        let before = TcpListener::bind("0.0.0.0:0").unwrap();
        let port = before.local_addr().unwrap().port();
        let letting_go = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(80));
            drop(before);
        });
        let again = bind(Some(port), false).unwrap();
        assert_eq!(again.local_addr().unwrap().port(), port);
        letting_go.join().unwrap();
    }

    #[test]
    fn the_server_stays_while_either_way_in_is_on() {
        let running = |local: bool, opening: usize| Running {
            port: 1,
            keys: Arc::new(Mutex::new(Keys { local: local.then(new_key), ..Keys::default() })),
            stop: Arc::new(AtomicBool::new(false)),
            origin: None,
            public: None,
            public_round: Arc::new(AtomicBool::new(false)),
            opening,
            opening_active: opening,
        };
        let mut on = Some(running(true, 0));
        settle(&mut on);
        assert!(on.is_some());
        // The VPN page alone on: kept for it.
        let mut vpn = Some(running(false, 0));
        vpn.as_ref().unwrap().keys.lock().unwrap().vpn = Some(new_key());
        settle(&mut vpn);
        assert!(vpn.is_some());
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
    fn a_port_another_program_listens_on_at_127_0_0_1_counts_as_taken() {
        // Another program on 127.0.0.1 alone: on macOS the server could still bind all addresses at that port, and that
        // program would then take cloudflared's and the user's proxy's connections.
        let other = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = other.local_addr().unwrap().port();
        assert!(loopback_taken(port));
        // Held (自有域名 set): said taken, not moved.
        assert_eq!(bind(Some(port), true).err().as_deref(), Some(format!("端口 {port} 被别的程序占用").as_str()));
        // Not held: another port instead.
        let moved = bind(Some(port), false).unwrap();
        assert_ne!(moved.local_addr().unwrap().port(), port);
        drop(other);
        assert!(!loopback_taken(port));
    }

    #[test]
    fn what_was_on_is_kept_for_the_next_start_and_a_stop_leaves_nothing() {
        // 用户：默认是关的，但是你要记住用户的状态。就算重新安装新版本，也要记住之前用户开没开。
        let folder = std::env::temp_dir().join(format!("starmeter-phone-kept-{}", new_key()));
        let path = folder.join("phone.json");
        assert_eq!(read_kept(&path), Kept::default());
        let on = Kept {
            port: Some(51234),
            port_chosen: true,
            local: Some(new_key()),
            vpn: Some(new_key()),
            public_until: Some(1_800_000_000.5),
            domain: Some("https://meter.example.com".into()),
            fixed: Fixed { local: Some(new_key()), vpn: Some(new_key()) },
        };
        write_kept(&path, &on);
        assert_eq!(read_kept(&path), on);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
        // A key not one of ours is left out; the rest stands.
        std::fs::write(&path, r#"{"port":51234,"local":"not a key","publicUntil":null}"#).unwrap();
        assert_eq!(read_kept(&path), Kept { port: Some(51234), ..Kept::default() });
        // An address that isn't one is left out; one as typed is kept as its origin.
        std::fs::write(&path, r#"{"port":51234,"domain":"ftp://x"}"#).unwrap();
        assert_eq!(read_kept(&path), Kept { port: Some(51234), ..Kept::default() });
        std::fs::write(&path, r#"{"port":51234,"domain":"meter.example.com"}"#).unwrap();
        assert_eq!(
            read_kept(&path),
            Kept { port: Some(51234), domain: Some("https://meter.example.com".into()), ..Kept::default() }
        );
        // A port said to be the user's with no port there is no choice.
        std::fs::write(&path, r#"{"portChosen":true}"#).unwrap();
        assert_eq!(read_kept(&path), Kept::default());
        std::fs::write(&path, r#"{"port":8080,"portChosen":true}"#).unwrap();
        assert_eq!(read_kept(&path), Kept { port: Some(8080), port_chosen: true, ..Kept::default() });
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
        let other = bind(Some(port), false).unwrap();
        assert_ne!(other.local_addr().unwrap().port(), port);
        // Held for 自有域名 (the user's proxy forwards there): not another, a reason naming it.
        let taken = bind(Some(port), true).map(|_| ()).unwrap_err();
        assert_eq!(taken, format!("端口 {port} 被别的程序占用"));
        drop(held);
        drop(other);
        // Free again: the same port, so a phone's link from before opens.
        let again = bind(Some(port), false).unwrap();
        assert_eq!(again.local_addr().unwrap().port(), port);
        drop(again);
        // Held, and let go of by a server stopping a moment later: taken once it is free.
        let stopping = std::net::TcpListener::bind(("0.0.0.0", port)).unwrap();
        let letting_go = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(200));
            drop(stopping);
        });
        assert_eq!(bind(Some(port), true).unwrap().local_addr().unwrap().port(), port);
        letting_go.join().unwrap();
        assert!(bind(None, false).unwrap().local_addr().unwrap().port() > 0);
    }

    #[test]
    fn the_domain_is_its_origin_as_typed_with_or_without_https() {
        // 自有域名 as the user types it.
        assert_eq!(origin_of("meter.example.com").unwrap(), "https://meter.example.com");
        assert_eq!(origin_of("  https://Meter.Example.com/ ").unwrap(), "https://meter.example.com");
        assert_eq!(origin_of("http://home.lan:8080").unwrap(), "http://home.lan:8080");
        assert_eq!(origin_of("https://meter.example.com:443").unwrap(), "https://meter.example.com");
        assert_eq!(origin_of("mac.tail1234.ts.net").unwrap(), "https://mac.tail1234.ts.net");
        // An IP (用户：域名那里是域名/IP): http unless https is typed, its port kept.
        assert_eq!(origin_of("203.0.113.7").unwrap(), "http://203.0.113.7");
        assert_eq!(origin_of("203.0.113.7:443").unwrap(), "http://203.0.113.7:443");
        assert_eq!(origin_of("https://203.0.113.7").unwrap(), "https://203.0.113.7");
        assert_eq!(origin_of("[2001:db8::7]:8080").unwrap(), "http://[2001:db8::7]:8080");
        assert_eq!(origin_of("nas.local:5001").unwrap(), "https://nas.local:5001");
        assert_eq!(origin_of("例子.测试").unwrap(), "https://xn--fsqu00a.xn--0zwm56d");
        // Only the address: no path, query, # or user, and a web address at all.
        for wrong in [
            "https://meter.example.com/starmeter",
            "meter.example.com/?a=1",
            "meter.example.com/#k=1",
            "https://me:pw@meter.example.com",
            "ftp://meter.example.com",
            "https://",
            "a b",
            // Not a name or address as one is written (用户：ip/域名格式不对，就不生成二维码，先检查格式啊).
            "2.2..",
            "2.2",
            "1.2.3",
            "300.1.1.1",
            "1.2.3.4.5",
            "nas",
            "meter..example.com",
            "-meter.example.com",
            "meter.example.123",
            "meter.example.com:0",
            "meter.example.com:99999",
            "meter.example.com:80a",
            "[2001:db8::zz]",
            "1.0x1",
            "ht\ttps://meter.example.com",
            "１.２",
        ] {
            assert!(origin_of(wrong).is_err(), "{wrong}");
        }
        assert_eq!(origin_of(" ").unwrap_err(), "填写你的域名或 IP，比如 starmeter.example.com");
        assert_eq!(origin_of("a b").unwrap_err(), "格式不对：填域名或 IP，比如 starmeter.example.com");
        assert!(valid_port(80).is_err());
        assert_eq!(valid_port(8765), Ok(8765));
    }

    #[test]
    fn vpn_domain_port_and_tunnel_precedence_do_not_hijack_direct_or_cloudflare_links() {
        use Origin::{Here, Vpn};
        let (local, vpn, public) = (new_key(), new_key(), new_key());
        // Setting a trycloudflare.com host as domain must not hijack Via::Tunnel.
        assert_eq!(via("abc.trycloudflare.com", Some("abc.trycloudflare.com")), Via::Tunnel);
        // Setting a VPN IP at port 443 must not hijack direct VPN connections to the server's own port 51234.
        let keys = Mutex::new(Keys {
            local: Some(local.clone()),
            vpn: Some(vpn.clone()),
            public: Some(public.clone()),
            domain_host: domain_host(Some("https://100.76.55.73")),
            domain_port: domain_port(Some("https://100.76.55.73")),
        });
        assert!(admitted(&keys, &vpn, Vpn, "100.76.55.73:51234"));
        assert!(admitted(&keys, &local, Vpn, "100.76.55.73:443"));
        assert!(admitted(&keys, &local, Vpn, "100.76.55.73"));
        // Even with VPN page off, a reverse proxy over VPN naming 自有域名 is welcomed and admitted, while a direct
        // probe to the VPN IP's own server port is rejected.
        keys.lock().unwrap().vpn = None;
        assert!(keys.lock().unwrap().welcome(Vpn));
        assert!(keys.lock().unwrap().vpn_allows_host("100.76.55.73:443"));
        assert!(!keys.lock().unwrap().vpn_allows_host("100.76.55.73:51234"));
        assert!(admitted(&keys, &public, Here, "abc.trycloudflare.com"));
    }

    #[test]
    fn the_domain_check_tells_this_server_from_another_answer() {
        use starmeter_core::http::{Response, Unreachable};
        let answer = |status: u16, body: &[u8]| Ok(Response { status, body: body.to_vec(), headers: Vec::new() });
        assert_eq!(checked(answer(200, PING)), DomainCheck { opens: true, status: None, reason: None });
        // A proxy's own page (or a 200 from something else): not this server.
        assert_eq!(checked(answer(200, b"<html>")), DomainCheck { opens: false, status: Some(200), reason: None });
        assert_eq!(checked(answer(502, b"Bad Gateway")), DomainCheck { opens: false, status: Some(502), reason: None });
        assert_eq!(
            checked(Err(Unreachable("timed out".into()))),
            DomainCheck { opens: false, status: None, reason: Some("timed out".into()) }
        );
    }
}
