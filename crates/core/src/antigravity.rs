//! Antigravity's own quota for Gemini 订阅, the one its app shows under Models & Usage: each group of models (Gemini's,
//! and Claude's with GPT's) has a five-hour and a weekly limit. The app runs a language server on this computer and
//! notes in ~/.gemini/antigravity/daemon/hub_<port>.json the process, the ports it listens on and the token it asks
//! for; the quota is asked of it there, on 127.0.0.1, and it answers from its own sign-in. Nothing else is sent
//! anywhere, the token is used only for that request and never kept, and with the app closed there is no one to ask.

use std::path::Path;

use crate::http::{self, Request};
use crate::json::{Json, Object};
use crate::{object, platform};

/// Said when no Antigravity is running to ask: the last figures stay, marked with when they were read.
pub const NOT_RUNNING: &str = "Antigravity 没有运行";

/// The language servers Antigravity's app noted, the newest first: (its process, its plain HTTP port, its token).
fn hubs(home: &str) -> Vec<(u32, i64, String)> {
    let folder = Path::new(home).join(".gemini/antigravity/daemon");
    let Ok(entries) = std::fs::read_dir(&folder) else { return Vec::new() };
    let mut found: Vec<(std::time::SystemTime, u32, i64, String)> = entries
        .filter_map(Result::ok)
        .filter(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            name.starts_with("hub_") && name.ends_with(".json")
        })
        .filter_map(|entry| {
            let changed = entry.metadata().and_then(|metadata| metadata.modified()).ok()?;
            let note = Json::parse(&std::fs::read(entry.path()).ok()?).ok()?;
            let pid = u32::try_from(note.get("pid")?.integer()?).ok()?;
            let port = note.get("httpPort")?.integer().filter(|port| (1..65536).contains(port))?;
            let token = note.get("csrfToken")?.as_str()?.to_string();
            Some((changed, pid, port, token))
        })
        .collect();
    found.sort_by_key(|hub| std::cmp::Reverse(hub.0));
    found.into_iter().map(|(_, pid, port, token)| (pid, port, token)).collect()
}

/// Whether Antigravity's app has run here and noted a language server, running now or not.
pub fn present(home: &str) -> bool {
    !hubs(home).is_empty()
}

/// The quota now, as {"windows": [{"group", "groupName", "window" (5h or weekly), "remaining" (percent), "resetsAt"}]},
/// Gemini's first; NOT_RUNNING when no noted server is alive, or why the one asked gave nothing.
pub fn quota(home: &str) -> Result<Object, String> {
    let mut failure = NOT_RUNNING.to_string();
    for (pid, port, token) in hubs(home) {
        if !platform::alive(pid) {
            continue;
        }
        let body = object! {
            "metadata" => object! { "ideName" => "antigravity", "extensionName" => "antigravity", "locale" => "en", "ideVersion" => "unknown" },
        };
        let answer = http::send(
            &format!("http://127.0.0.1:{port}/exa.language_server_pb.LanguageServerService/RetrieveUserQuotaSummary"),
            Request {
                method: "POST",
                headers: vec![
                    ("Content-Type", "application/json".into()),
                    ("Connect-Protocol-Version", "1".into()),
                    ("X-Codeium-Csrf-Token", token),
                ],
                body: Some(Json::Object(body).serialized().into_bytes()),
                timeout: 5.0,
                follow_redirects: false,
                direct: true,
                limit: 1 << 20,
            },
        );
        match answer {
            Ok(response) if response.status == 200 => {
                let windows = Json::parse(&response.body).ok().map(|summary| windows(&summary)).unwrap_or_default();
                if windows.is_empty() {
                    failure = "Antigravity 没有给出额度".into();
                    continue;
                }
                return Ok(object! { "windows" => Json::Array(windows) });
            }
            Ok(response) => failure = format!("Antigravity 没有给出额度（HTTP {}）", response.status),
            // A note left by a server gone (its process id now another's) refuses the connection: what a live one
            // answered before it stays the reason, not 没有运行.
            Err(_) => {}
        }
    }
    Err(failure)
}

/// The summary's buckets as windows: each group's five-hour and weekly limit, in the order the app lists them, a
/// bucket without a share left out.
fn windows(summary: &Json) -> Vec<Json> {
    let groups = summary.get("response").and_then(|response| response.get("groups")).and_then(Json::as_array);
    let mut found = Vec::new();
    for group in groups.into_iter().flatten() {
        let name = group.get("displayName").and_then(Json::as_str).unwrap_or("").to_string();
        for bucket in group.get("buckets").and_then(Json::as_array).into_iter().flatten() {
            let id = bucket.get("bucketId").and_then(Json::as_str).unwrap_or("");
            let window =
                bucket.get("window").and_then(Json::as_str).unwrap_or_else(|| id.rsplit('-').next().unwrap_or(""));
            let Some(left) = bucket.get("remainingFraction").and_then(Json::number) else { continue };
            if !["5h", "weekly"].contains(&window) {
                continue;
            }
            let resets = bucket
                .get("resetTime")
                .and_then(Json::as_str)
                .and_then(|text| crate::time::IsoTime::parse(text).ok())
                .map(|time| time.timestamp(&jiff::tz::TimeZone::UTC));
            found.push(
                object! {
                    "group" => id.split('-').next().unwrap_or(""), "groupName" => name.as_str(), "window" => window,
                    "remaining" => (left.clamp(0.0, 1.0) * 1000.0).round() / 10.0,
                    "resetsAt" => resets.map_or(Json::Null, Json::Double),
                }
                .into(),
            );
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::{NOT_RUNNING, quota, windows};
    use crate::json::Json;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    #[test]
    fn a_live_servers_answer_is_the_reason_not_a_gone_ones_refusal() {
        // The newer note's server is up and answers 500; an older note's process id is alive (another's now) but
        // nothing listens on its port. The reason is the 500, not 没有运行.
        let server = TcpListener::bind("127.0.0.1:0").unwrap();
        let live = server.local_addr().unwrap().port();
        let gone = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (mut stream, _) = server.accept().unwrap();
            let mut request = [0u8; 4096];
            let _ = stream.read(&mut request);
            let _ = stream
                .write_all(b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        });
        let home = std::env::temp_dir().join(format!("starmeter-agy-hubs-{}", crate::runtime::random_hex(6)));
        let daemon = home.join(".gemini/antigravity/daemon");
        std::fs::create_dir_all(&daemon).unwrap();
        let note = |port: u16| format!(r#"{{"pid":{},"httpPort":{port},"csrfToken":"t"}}"#, std::process::id());
        std::fs::write(daemon.join("hub_1.json"), note(gone)).unwrap();
        std::fs::write(daemon.join("hub_2.json"), note(live)).unwrap();
        let earlier = std::time::SystemTime::now() - std::time::Duration::from_secs(600);
        std::fs::File::options().write(true).open(daemon.join("hub_1.json")).unwrap().set_modified(earlier).unwrap();
        let home_text = home.to_string_lossy().into_owned();
        assert_eq!(quota(&home_text).err().as_deref(), Some("Antigravity 没有给出额度（HTTP 500）"));
        // With only the gone one noted, it is 没有运行.
        std::fs::remove_file(daemon.join("hub_2.json")).unwrap();
        assert_eq!(quota(&home_text).err().as_deref(), Some(NOT_RUNNING));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn the_summary_reads_as_each_groups_two_windows() {
        let summary = Json::parse_str(
            r#"{"response":{"groups":[
                {"displayName":"Gemini Models","buckets":[
                    {"bucketId":"gemini-weekly","window":"weekly","remainingFraction":0.996072,"resetTime":"2026-11-29T04:57:39Z"},
                    {"bucketId":"gemini-5h","window":"5h","remainingFraction":0.984288,"resetTime":"2026-11-22T09:57:39Z"}]},
                {"displayName":"Claude and GPT models","buckets":[
                    {"bucketId":"3p-weekly","window":"weekly","remainingFraction":1,"resetTime":"2026-11-29T05:21:24Z"},
                    {"bucketId":"3p-5h","remainingFraction":1},
                    {"bucketId":"3p-daily","window":"daily","remainingFraction":0.5},
                    {"bucketId":"3p-other","window":"5h"}]}]}}"#,
        )
        .unwrap();
        let found = windows(&summary);
        let read = |index: usize, field: &str| found[index].get(field).cloned().unwrap_or(Json::Null);
        assert_eq!(found.len(), 4, "a daily window and one without a share are left out");
        assert_eq!(
            (read(0, "group"), read(0, "window"), read(0, "remaining")),
            ("gemini".into(), "weekly".into(), Json::Double(99.6))
        );
        assert_eq!(read(1, "remaining"), Json::Double(98.4));
        assert_eq!(read(1, "resetsAt").number(), Some(1795341459.0));
        assert_eq!((read(3, "group"), read(3, "window"), read(3, "resetsAt")), ("3p".into(), "5h".into(), Json::Null));
    }
}
