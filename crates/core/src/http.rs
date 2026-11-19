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
    let response = call.send().map_err(|error| Unreachable(describe(&error)))?;
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
