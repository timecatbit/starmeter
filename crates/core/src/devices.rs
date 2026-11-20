//! 多台电脑合并: every computer writes what Summary counts of each range into a folder the user picked on each of them
//! (iCloud Drive, Dropbox, OneDrive, Syncthing…), one file per computer, and adds the other computers' files to its
//! own figures. No server: the folder's own sync carries the files. A file holds tool and model names with counts by
//! day and by hour, never paths, projects, sessions, prompts or keys.
//!
//! The figures merged are what Summary counts on the computer that wrote them (a gateway's carried requests already
//! counted once there), added into this computer's records of the same tool by day, by hour and by model, so the
//! cards, Summary, the trend, the heatmap, the share picture and the export all take them in. A tool only the other
//! computer uses counts here once it is ticked here. A range's figures from another computer are used only while they
//! describe the same stretch of time (a day apart at most, for computers in other time zones) and the file is under
//! two days old.

use std::path::{Path, PathBuf};

use crate::dashboard::{Selected, summary_usage};
use crate::files;
use crate::json::{Json, Object};
use crate::object;
use crate::preferences::{MODULES, USAGE};
use crate::py;
use crate::runtime::{self, RANGES, Runtime, now};

/// The folder inside the one the user picked.
pub const FOLDER: &str = "starmeter-devices";
const SCHEMA: i64 = 1;
/// How often this computer's file is written.
const WRITE_EVERY: f64 = 600.0;
/// A file older than this is left out.
const STALE: f64 = 2.0 * 86400.0;
/// What a usage record keeps for another computer: its counts.
const KEPT: [&str; 5] = ["summary", "models", "days", "hours", "firstActivityAt"];

/// The calendar day a range starts on, where the computer that read it is: today's or yesterday's date, the first day
/// of 7 or 30 days, of the week or of the month. Two computers' ranges are the same when their days are, whatever their
/// time zones.
fn range_day(bundle: &Object) -> Option<String> {
    let start = bundle.get("start").and_then(Json::number)?;
    Some(crate::tally::local_day(start, &jiff::tz::TimeZone::system()))
}

/// How far another computer's range may sit from this one's and still be the same range (a file from before the
/// range's day was written down: see range_day).
fn allowed_shift(key: &str) -> f64 {
    match key {
        "24h" => 7200.0,
        "all" => f64::INFINITY,
        _ => 86400.0,
    }
}

/// Adds b to a: whole numbers stay whole.
fn add_number(a: Option<&Json>, b: &Json) -> Option<Json> {
    match (a, b) {
        (Some(Json::Int(x)), Json::Int(y)) => Some(Json::Int(x + y)),
        (Some(x), y) => match (x.number(), y.number()) {
            (Some(x), Some(y)) => Some(Json::Double(x + y)),
            _ => None,
        },
        (None, y) => y.number().is_some().then(|| y.clone()),
    }
}

/// Every number of `from` added into `into`; the coverage ratio is this computer's own and stays.
fn add_numbers(into: &mut Object, from: &Object) {
    for (name, value) in from.iter() {
        if name == "coverageRatio" || value.as_bool().is_some() {
            continue;
        }
        if let Some(sum) = add_number(into.get(name), value) {
            into.insert(name.clone(), sum);
        }
    }
}

/// Rows (by day, by hour, by model) added by the field that names them.
fn add_rows(into: &mut Vec<Json>, from: &[Json], field: &str) {
    for row in from.iter().filter_map(Json::as_object) {
        let Some(name) = row.get(field) else { continue };
        match into.iter_mut().filter_map(Json::as_object_mut).find(|mine| mine.get(field) == Some(name)) {
            Some(mine) => add_numbers(mine, row),
            None => into.push(Json::Object(row.clone())),
        }
    }
}

/// Another computer's counted record of a tool, added into this computer's record of it.
pub fn merge_usage(into: &mut Object, from: &Object) {
    if let Some(summary) = from.get("summary").and_then(Json::as_object) {
        let mut mine = into.get("summary").and_then(Json::as_object).cloned().unwrap_or_default();
        add_numbers(&mut mine, summary);
        into.insert("summary", Json::Object(mine));
    }
    for (list, field) in [("models", "model"), ("days", "date"), ("hours", "timestamp")] {
        if let Some(rows) = from.get(list).and_then(Json::as_array) {
            let mut mine = into.get(list).and_then(Json::as_array).cloned().unwrap_or_default();
            add_rows(&mut mine, rows, field);
            into.insert(list, Json::Array(mine));
        }
    }
    if let Some(first) = from.get("firstActivityAt").and_then(Json::number) {
        let earlier = into.get("firstActivityAt").and_then(Json::number).is_none_or(|mine| first < mine);
        if earlier {
            into.insert("firstActivityAt", Json::from(first));
        }
    }
}

/// What another computer's file says of one range, when it is the same range as this computer's: the same calendar day
/// for the ranges counted by day (yesterday's today from a computer switched off at midnight is not today), the same
/// hours give or take two for the last 24, any for everything.
fn usable(entry: &Object, key: &str, start: f64, day: Option<&str>, written: f64, now: f64) -> bool {
    if now - written > STALE || written - now > 600.0 {
        return false;
    }
    if key == "all" {
        return true;
    }
    let theirs = entry.get("start").and_then(Json::number).unwrap_or(f64::NAN);
    if key == "24h" {
        return (theirs - start).abs() <= allowed_shift(key);
    }
    match (entry.get("day").and_then(Json::as_str), day) {
        (Some(theirs), Some(mine)) => theirs == mine,
        // A file from before the day was written down: within a day of this range's start, and written since it began.
        _ => (theirs - start).abs() <= allowed_shift(key) && written >= start,
    }
}

impl Runtime {
    /// Where this computer's file and the others' are, once a folder is picked and still there.
    pub fn devices_root(&self) -> Option<PathBuf> {
        let chosen = self.settings().get("syncFolder").and_then(Json::as_str).map(String::from).unwrap_or_default();
        if chosen.is_empty() || !Path::new(&chosen).is_dir() {
            return None;
        }
        Some(Path::new(&chosen).join(FOLDER))
    }

    /// This computer's lasting name in the folder: made once, kept in the settings folder.
    pub fn device_id(&self) -> String {
        let path = Path::new(&self.state_dir).join("device-id");
        if let Ok(text) = std::fs::read_to_string(&path) {
            let text = text.trim();
            if text.len() == 16 && text.chars().all(|c| c.is_ascii_hexdigit()) {
                return text.to_string();
            }
        }
        let seed = format!(
            "{}|{}|{}|{:?}",
            self.state_dir,
            std::process::id(),
            now(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos()
        );
        let id = py::sha256_hex(seed.as_bytes())[..16].to_string();
        let _ = files::make_directory(Path::new(&self.state_dir), 0o700);
        let _ = files::replace_atomically(&path, id.as_bytes(), ".device-", || Ok(()));
        id
    }

    /// The name the other computers show for this one: the user's, else the computer's own.
    pub fn device_name(&self) -> String {
        let chosen = self.settings().get("deviceName").and_then(Json::as_str).unwrap_or("").trim().to_string();
        if !chosen.is_empty() {
            return chosen;
        }
        if let Some(name) = self.env("COMPUTERNAME") {
            return name.to_string();
        }
        #[cfg(target_os = "macos")]
        if let Some(name) = crate::shell::run(&["/usr/sbin/scutil", "--get", "ComputerName"], 3.0, None, None) {
            let name = name.trim();
            if !name.is_empty() {
                return name.to_string();
            }
        }
        crate::shell::run(&["hostname"], 3.0, None, None)
            .map(|name| name.trim().trim_end_matches(".local").to_string())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "这台电脑".into())
    }

    /// This computer's file: what Summary counts of every range read so far, written at most every ten minutes
    /// (`force` writes now). The other computers' figures are never written back.
    pub fn publish_device(&self, force: bool) -> bool {
        let Some(root) = self.devices_root() else { return false };
        let id = self.device_id();
        let path = root.join(format!("{id}.json"));
        let now = now();
        if !force
            && let Some(written) = read_object(&path).and_then(|file| file.get("updatedAt").and_then(Json::number))
            && now - written < WRITE_EVERY
            && written <= now
        {
            return false;
        }
        let mut ranges = Object::new();
        for (key, _) in RANGES {
            let record = self.read_cache(&self.range_cache_name(Some(key)));
            let Some(bundle) = record.get("payload").and_then(Json::as_object) else { continue };
            if bundle.get("range") != Some(&Json::from(key)) {
                continue;
            }
            let selected: Vec<Selected> = MODULES
                .iter()
                .filter(|module| USAGE.contains(&module.key) && self.module_enabled(module.key))
                .map(|module| {
                    (
                        module.key.to_string(),
                        module.label.clone(),
                        bundle.get(module.key).and_then(Json::as_object).cloned().unwrap_or_default(),
                    )
                })
                .collect();
            let counted = summary_usage(&selected);
            let mut sources = Object::new();
            for (source, _, usage) in &selected {
                if usage.get("error").is_some_and(Json::is_truthy) {
                    continue;
                }
                let Some(part) = counted.get(source).and_then(|part| part.get("usage")).and_then(Json::as_object)
                else {
                    continue;
                };
                let summary = part.get("summary").and_then(Json::as_object).cloned().unwrap_or_default();
                let used = summary.get("requests").and_then(Json::number).unwrap_or(0.0) > 0.0
                    || summary.get("totalTokens").and_then(Json::number).unwrap_or(0.0) > 0.0;
                if !used {
                    continue;
                }
                let kept: Object = KEPT
                    .iter()
                    .filter_map(|name| part.get(name).map(|value| ((*name).to_string(), value.clone())))
                    .collect();
                sources.insert(source.clone(), Json::Object(kept));
            }
            ranges.insert(
                key,
                object! {
                    "start" => bundle.get("start").cloned().unwrap_or(Json::Null),
                    "end" => bundle.get("end").cloned().unwrap_or(Json::Null),
                    "day" => range_day(bundle).map_or(Json::Null, Json::from),
                    "sources" => sources,
                },
            );
        }
        let file = object! {
            "schema" => SCHEMA, "device" => id.as_str(), "name" => self.device_name(), "version" => runtime::VERSION,
            "updatedAt" => now, "ranges" => ranges,
        };
        if files::make_directory(&root, 0o700).is_err() {
            return false;
        }
        files::replace_atomically(&path, Json::Object(file).serialized().as_bytes(), ".device-", || Ok(())).is_ok()
    }

    /// The other computers' files, newest first; this computer's own is left out.
    pub fn other_devices(&self) -> Vec<Object> {
        let Some(root) = self.devices_root() else { return vec![] };
        let mine = format!("{}.json", self.device_id());
        let mut found: Vec<Object> = std::fs::read_dir(&root)
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .filter(|entry| {
                        let name = entry.file_name().to_string_lossy().into_owned();
                        name.ends_with(".json") && !name.starts_with('.') && name != mine
                    })
                    .filter_map(|entry| read_object(&entry.path()))
                    .filter(|file| {
                        file.get("schema") == Some(&Json::Int(SCHEMA))
                            && file.get("ranges").and_then(Json::as_object).is_some()
                            && file.get("updatedAt").and_then(Json::number).is_some()
                    })
                    .collect()
            })
            .unwrap_or_default();
        found.sort_by(|a, b| {
            let at = |file: &Object| file.get("updatedAt").and_then(Json::number).unwrap_or(0.0);
            at(b).total_cmp(&at(a))
        });
        found
    }

    /// A cached range record with the other computers' figures for the same range added in.
    pub fn with_devices(&self, key: &str, mut record: Object) -> Object {
        let devices = self.other_devices();
        if devices.is_empty() {
            return record;
        }
        let Some(mut bundle) = record.get("payload").and_then(Json::as_object).cloned() else { return record };
        if bundle.get("range") != Some(&Json::from(key)) {
            return record;
        }
        let start = bundle.get("start").and_then(Json::number).unwrap_or(0.0);
        let day = range_day(&bundle);
        let now = now();
        let mut merged = 0;
        for device in &devices {
            let written = device.get("updatedAt").and_then(Json::number).unwrap_or(0.0);
            let Some(entry) = device.get("ranges").and_then(|ranges| ranges.get(key)).and_then(Json::as_object) else {
                continue;
            };
            if !usable(entry, key, start, day.as_deref(), written, now) {
                continue;
            }
            merged += 1;
            for (source, usage) in entry.get("sources").and_then(Json::as_object).into_iter().flat_map(Object::iter) {
                let Some(theirs) = usage.as_object() else { continue };
                if !USAGE.contains(&source.as_str()) {
                    continue;
                }
                let mut mine = bundle.get(source).and_then(Json::as_object).cloned().unwrap_or_default();
                merge_usage(&mut mine, theirs);
                // A record a gateway partly counted keeps its counted part too: the other computer's is all counted.
                if let Some(mut counted) = mine.get("counted").and_then(Json::as_object).cloned() {
                    merge_usage(&mut counted, theirs);
                    mine.insert("counted", Json::Object(counted));
                }
                bundle.insert(source.clone(), Json::Object(mine));
            }
        }
        if merged > 0 {
            bundle.insert("devices", Json::Int(merged));
            record.insert("payload", Json::Object(bundle));
        }
        record
    }

    /// Stops merging: this computer's file leaves the folder, so the others stop counting it, and the folder is
    /// forgotten here.
    pub fn leave_devices(&self) {
        if let Some(root) = self.devices_root() {
            let _ = std::fs::remove_file(root.join(format!("{}.json", self.device_id())));
        }
    }
}

fn read_object(path: &Path) -> Option<Object> {
    std::fs::read(path).ok().and_then(|data| Json::parse(&data).ok()).and_then(|value| value.as_object().cloned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Object {
        Json::parse(text.as_bytes()).unwrap().as_object().cloned().unwrap()
    }

    #[test]
    fn another_computers_counts_add_in_by_day_hour_and_model() {
        let mut mine = parse(
            r#"{"summary": {"totalTokens": 100, "requests": 2, "estimatedCostUsd": 0.5, "coverageRatio": 1.0},
                "models": [{"model": "opus", "totalTokens": 100, "requests": 2}],
                "days": [{"date": "2026-11-19", "totalTokens": 100, "requests": 2}],
                "hours": [{"timestamp": 1000, "totalTokens": 100}], "firstActivityAt": 50,
                "projects": [{"name": "starmeter"}]}"#,
        );
        let theirs = parse(
            r#"{"summary": {"totalTokens": 40, "requests": 1, "estimatedCostUsd": 0.25, "coverageRatio": 0.5},
                "models": [{"model": "opus", "totalTokens": 30, "requests": 1}, {"model": "haiku", "totalTokens": 10}],
                "days": [{"date": "2026-11-19", "totalTokens": 40, "requests": 1}],
                "hours": [{"timestamp": 4600, "totalTokens": 40}], "firstActivityAt": 20}"#,
        );
        merge_usage(&mut mine, &theirs);
        let summary = mine.get("summary").unwrap();
        assert_eq!(summary.get("totalTokens"), Some(&Json::Int(140)));
        assert_eq!(summary.get("requests"), Some(&Json::Int(3)));
        assert_eq!(summary.get("estimatedCostUsd").and_then(Json::number), Some(0.75));
        assert_eq!(summary.get("coverageRatio").and_then(Json::number), Some(1.0), "this computer's own ratio stays");
        let models = mine.get("models").and_then(Json::as_array).unwrap();
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].get("totalTokens"), Some(&Json::Int(130)));
        assert_eq!(mine.get("days").and_then(Json::as_array).unwrap()[0].get("requests"), Some(&Json::Int(3)));
        assert_eq!(mine.get("hours").and_then(Json::as_array).unwrap().len(), 2);
        assert_eq!(mine.get("firstActivityAt").and_then(Json::number), Some(20.0));
        assert!(mine.get("projects").is_some(), "this computer's projects stay; the other's never come");
    }

    /// Two computers sharing a folder: each writes its own file, reads the other's, and adds it in; leaving takes the
    /// file away.
    #[test]
    fn two_computers_share_a_folder() {
        use std::collections::HashMap;
        let base = std::env::temp_dir().join(format!("starmeter-devices-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let folder = base.join("sync");
        std::fs::create_dir_all(&folder).unwrap();
        let computer = |name: &str, tokens: i64| {
            let home = base.join(name);
            let state = home.join("state");
            std::fs::create_dir_all(&state).unwrap();
            let settings = object! {
                "schema" => 1, "visible" => Json::from(vec!["summary".to_string(), "claude".to_string()]),
                "syncFolder" => folder.to_string_lossy().into_owned(), "deviceName" => name,
            };
            std::fs::write(state.join("preferences.json"), Json::Object(settings).serialized()).unwrap();
            let environment: HashMap<String, String> = [
                ("HOME", home.to_string_lossy().into_owned()),
                ("OCX_STATE_DIR", state.to_string_lossy().into_owned()),
                ("OCX_CACHE_DIR", home.join("cache").to_string_lossy().into_owned()),
            ]
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect();
            let runtime = Runtime::new(environment, "starmeter");
            let moment = now();
            let usage = object! {
                "summary" => object! { "totalTokens" => tokens, "requests" => 1, "outputTokens" => 5 },
                "days" => Json::Array(vec![object! { "date" => runtime::today(), "totalTokens" => tokens, "requests" => 1 }.into()]),
                "models" => Json::Array(vec![object! { "model" => "opus", "totalTokens" => tokens }.into()]),
            };
            let bundle = object! { "range" => "today", "start" => moment - 3600.0, "end" => moment, "claude" => usage };
            let record =
                object! { "schema" => 1, "fetchedAt" => moment, "day" => runtime::today(), "payload" => bundle };
            runtime
                .write_json_cache(&runtime.cache_path(&runtime.range_cache_name(Some("today"))), &Json::Object(record));
            runtime
        };
        let (a, b) = (computer("Mac A", 100), computer("Mac B", 40));
        assert!(a.publish_device(true));
        assert!(b.other_devices().len() == 1 && a.other_devices().is_empty(), "each reads only the other's file");
        let merged = b.with_devices("today", b.read_cache(&b.range_cache_name(Some("today"))));
        let claude = merged.get("payload").and_then(|payload| payload.get("claude")).unwrap();
        assert_eq!(claude.get("summary").and_then(|summary| summary.get("totalTokens")), Some(&Json::Int(140)));
        assert_eq!(
            merged.get("payload").and_then(|payload| payload.get("devices")),
            Some(&Json::Int(1)),
            "said beside the update time"
        );
        assert!(b.publish_device(true));
        assert_eq!(a.other_devices()[0].get("name").and_then(Json::as_str), Some("Mac B"));
        // Its own figures only: B's file says 40, not the 140 it shows.
        let written = &a.other_devices()[0];
        let theirs = written
            .get("ranges")
            .and_then(|r| r.get("today"))
            .and_then(|t| t.get("sources"))
            .and_then(|s| s.get("claude"));
        assert_eq!(theirs.and_then(|c| c.get("summary")).and_then(|s| s.get("totalTokens")), Some(&Json::Int(40)));
        b.leave_devices();
        assert!(a.other_devices().is_empty(), "leaving takes the file away");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn only_the_same_stretch_of_time_and_a_recent_file() {
        let day = 86400.0;
        // Ranges by day: the same calendar day, wherever the computers are.
        let entry = parse(r#"{"start": 1000.0, "day": "2026-11-19"}"#);
        let (today, written) = (1000.0 + 8.0 * 3600.0, 1000.0 + 9.0 * 3600.0);
        assert!(
            usable(&entry, "today", today, Some("2026-11-19"), written, written + 100.0),
            "a time zone apart, the same day"
        );
        // Switched off a moment before midnight: its today is yesterday here, not today.
        let late = parse(r#"{"start": 1000.0, "day": "2026-11-19"}"#);
        assert!(!usable(&late, "today", 1000.0 + day, Some("2026-11-20"), 1000.0 + day - 2.0, 1000.0 + day + 3600.0));
        assert!(
            !usable(&entry, "today", 1000.0 + 2.0 * day, Some("2026-11-21"), written, written + 100.0),
            "another day is not"
        );
        // A file from before the day was written down: within a day, and written since this range began.
        let old = parse(r#"{"start": 1000.0}"#);
        assert!(usable(&old, "today", today, Some("2026-11-19"), written, written + 100.0));
        assert!(!usable(&old, "today", 1000.0 + day, Some("2026-11-20"), 1000.0 + day - 2.0, 1000.0 + day + 3600.0));
        assert!(!usable(&entry, "24h", 1000.0 + 3.0 * 3600.0, None, written, written + 100.0));
        assert!(usable(&entry, "all", 0.0, None, 5000.0, 5100.0));
        assert!(!usable(&entry, "all", 0.0, None, 5000.0, 5000.0 + 3.0 * day), "a file two days old is left out");
    }
}
