//! Plain HTTP for the engine's few requests: one call at a time, a time limit, an optional ban on redirects (an
//! Authorization header must never follow one to another host), a cap on the body, no cookies, and the system's
//! proxy settings, as 5.x's URLSession used them.

use std::fmt;
use std::io::Read;
use std::sync::RwLock;
use std::time::Duration;

use reqwest::blocking::Client;
use reqwest::redirect::Policy;

use crate::json::{Json, Object};
use crate::py::FileError;

pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
    pub headers: Vec<(String, String)>,
}

impl Response {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(key, _)| key.eq_ignore_ascii_case(name)).map(|(_, value)| value.as_str())
    }
}

/// The request never got an answer: no connection, a timeout, a bad address.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unreachable(pub String);

impl fmt::Display for Unreachable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Unreachable {}

/// Tests point real hosts at a local server: "https://api.deepseek.com" → "http://127.0.0.1:8123".
static ROUTES: RwLock<Vec<(String, String)>> = RwLock::new(Vec::new());

pub fn set_routes(routes: Vec<(String, String)>) {
    *ROUTES.write().expect("routes") = routes;
}

pub struct Request<'a> {
    pub method: &'a str,
    pub headers: Vec<(&'a str, String)>,
    pub body: Option<Vec<u8>>,
    pub timeout: f64,
    pub follow_redirects: bool,
    /// Local addresses go straight there, never through a proxy.
    pub direct: bool,
    pub limit: usize,
}

impl Default for Request<'_> {
    fn default() -> Self {
        Request {
            method: "GET",
            headers: Vec::new(),
            body: None,
            timeout: 10.0,
            follow_redirects: true,
            direct: false,
            limit: 64 << 20,
        }
    }
}

pub fn send(address: &str, request: Request) -> Result<Response, Unreachable> {
    exchange(address, request, describe)
}

/// As send, the reason for no answer said the way a person can act on: the name not found, the connection turned
/// away, the certificate, the time out; else the innermost cause (reqwest's own text names only the request).
pub fn send_explained(address: &str, request: Request) -> Result<Response, Unreachable> {
    exchange(address, request, explain)
}

fn exchange(address: &str, request: Request, why: fn(&reqwest::Error) -> String) -> Result<Response, Unreachable> {
    let mut target = address.to_string();
    for (from, to) in ROUTES.read().expect("routes").iter() {
        if let Some(rest) = address.strip_prefix(from.as_str()) {
            target = format!("{to}{rest}");
            break;
        }
    }
    if !(target.starts_with("http://") || target.starts_with("https://")) {
        return Err(Unreachable("bad address".into()));
    }
    let timeout = Duration::from_secs_f64(request.timeout.max(0.1));
    let mut builder = Client::builder()
        .timeout(timeout)
        .connect_timeout(timeout)
        .redirect(if request.follow_redirects { Policy::default() } else { Policy::none() });
    if request.direct {
        builder = builder.no_proxy();
    }
    let client = builder.build().map_err(|error| Unreachable(error.to_string()))?;
    let method =
        reqwest::Method::from_bytes(request.method.as_bytes()).map_err(|_| Unreachable("bad method".into()))?;
    let mut call = client.request(method, &target);
    for (name, value) in request.headers {
        call = call.header(name, value);
    }
    if let Some(body) = request.body {
        call = call.body(body);
    }
    let response = call.send().map_err(|error| Unreachable(why(&error)))?;
    let status = response.status().as_u16();
    let headers = response
        .headers()
        .iter()
        .map(|(name, value)| (name.as_str().to_string(), String::from_utf8_lossy(value.as_bytes()).into_owned()))
        .collect();
    let mut body = Vec::new();
    response.take(request.limit as u64).read_to_end(&mut body).map_err(|error| Unreachable(error.to_string()))?;
    Ok(Response { status, body, headers })
}

fn describe(error: &reqwest::Error) -> String {
    if error.is_timeout() { "timed out".into() } else { error.to_string() }
}

fn explain(error: &reqwest::Error) -> String {
    if error.is_timeout() {
        return "等了太久没有回应".into();
    }
    let mut causes = Vec::new();
    let mut source: Option<&dyn std::error::Error> = Some(error);
    while let Some(cause) = source {
        causes.push(cause.to_string());
        source = cause.source();
    }
    explained(&causes)
}

/// The reason from the chain of causes, outermost first. reqwest's own outermost line names the request (its address
/// could hold any word), so only the causes under it are read; a connection cut off is told before a certificate (a
/// TLS library's line for it says SSL or TLS too).
fn explained(causes: &[String]) -> String {
    let under = if causes.len() > 1 { &causes[1..] } else { causes };
    let all = under.join(" ").to_lowercase();
    let has = |words: &[&str]| words.iter().any(|word| all.contains(word));
    if has(&["dns error", "failed to lookup", "nodename nor servname", "name or service not known", "no such host"]) {
        "找不到这个域名".into()
    } else if has(&["connection refused", "actively refused"]) {
        "连接被拒绝".into()
    } else if has(&["timed out", "time out"]) {
        "等了太久没有回应".into()
    } else if has(&["connection reset", "connection closed", "unexpected eof", "broken pipe", "forcibly closed"]) {
        "连接被中断".into()
    } else if has(&["certificate", "verify", "self signed", "self-signed", "unknown issuer", "expired"]) {
        "证书有问题".into()
    } else if has(&["no route to host", "network is unreachable", "host is unreachable"]) {
        "连不上这个地址".into()
    } else if has(&["tls", "ssl", "handshake"]) {
        "加密连接没能建立".into()
    } else {
        causes.last().cloned().unwrap_or_default()
    }
}

/// An HTTP error status, as urllib raises HTTPError: the status and the start of the body.
#[derive(Clone, Debug)]
pub struct Status {
    pub code: u16,
    pub body: Vec<u8>,
}

/// Why a JSON GET failed.
#[derive(Clone, Debug)]
pub enum JsonFailure {
    Unreachable(Unreachable),
    Status(Status),
    Invalid(FileError),
}

/// GET a JSON object with a bearer key, no redirects, and the first 2 MiB of the answer, as get_json does.
pub fn get_json(url: &str, key: &str) -> Result<Object, JsonFailure> {
    let response = send(
        url,
        Request {
            headers: vec![("Authorization", format!("Bearer {key}")), ("Accept", "application/json".into())],
            timeout: 8.0,
            follow_redirects: false,
            ..Request::default()
        },
    )
    .map_err(JsonFailure::Unreachable)?;
    if !(200..300).contains(&response.status) {
        return Err(JsonFailure::Status(Status { code: response.status, body: response.body }));
    }
    let head = &response.body[..response.body.len().min(2 * 1024 * 1024)];
    Json::parse(head)
        .ok()
        .and_then(|value| value.as_object().cloned())
        .ok_or_else(|| JsonFailure::Invalid(FileError::new("invalid account response")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_answer_is_said_in_words_a_person_can_act_on() {
        let said = |causes: &[&str]| explained(&causes.iter().map(|cause| cause.to_string()).collect::<Vec<_>>());
        assert_eq!(
            said(&["error sending request", "client error (Connect)", "dns error", "failed to lookup address"]),
            "找不到这个域名"
        );
        assert_eq!(
            said(&["error sending request", "tcp connect error", "Connection refused (os error 61)"]),
            "连接被拒绝"
        );
        assert_eq!(
            said(&["error sending request", "client error (Connect)", "The certificate was not trusted"]),
            "证书有问题"
        );
        assert_eq!(said(&["error sending request", "connection closed before message completed"]), "连接被中断");
        // TLS spoken to something that doesn't (http on 443): not a certificate, said all the same.
        assert_eq!(said(&["error sending request", "SSL routines: wrong version number"]), "加密连接没能建立");
        // A TLS library's line for a connection cut off says SSL too: still cut off, not a certificate.
        assert_eq!(said(&["error sending request", "SSL error: unexpected EOF"]), "连接被中断");
        // A word in the address (reqwest's outermost line) is no cause.
        assert_eq!(
            said(&["error sending request for url (https://tls.example.com/)", "No route to host (os error 65)"]),
            "连不上这个地址"
        );
        // Anything else: the innermost cause, not reqwest's line naming only the request.
        assert_eq!(said(&["error sending request for url (https://x/)", "something odd"]), "something odd");
        // Through a real request: a port nothing listens on. (A name no one has isn't tried: a proxy's fake-IP DNS, as
        // Clash's TUN mode keeps, answers every name and closes the connection instead.)
        let request = || Request { timeout: 5.0, direct: true, ..Request::default() };
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let why = |address: &str| send_explained(address, request()).err().map(|why| why.0);
        assert_eq!(why(&format!("http://127.0.0.1:{port}/api/ping")).as_deref(), Some("连接被拒绝"));
    }
}
