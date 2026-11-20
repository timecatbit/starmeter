//! The statistics range on screen, read from every selected Harness: time bounds, the cache each range keeps, the
//! gateways that may share requests, and the bundle that merges every source.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use indexmap::IndexMap;
use jiff::tz::TimeZone;

use crate::claude_usage::{self, Event as ClaudeEvent};
use crate::files::paths;
use crate::json::{Json, Object};
use crate::preferences::USAGE;
use crate::pricing::claude;
use crate::runtime::{self, HOURLY_RANGES, Loaded, PRICING_TABLES, RANGES, Runtime, now};
use crate::tally::{self, Carrier, GatewayLog, add, archive, local_day, offset_at};
use crate::time::{IsoTime, days_from_civil};
use crate::{claude_app, harness, object, pricing, py, shell};

/// A wall-clock time in a zone, as an aware Python datetime is: arithmetic on the wall clock, the offset looked up
/// when it becomes a moment.
#[derive(Clone)]
pub struct Wall {
    pub year: i64,
    pub month: i64,
    pub day: i64,
    pub seconds: i64,
    pub microsecond: i64,
    pub zone: TimeZone,
}

impl Wall {
    pub fn at(moment: f64, zone: &TimeZone) -> Wall {
        let local = moment + offset_at(zone, moment) as f64;
        let whole = local.floor();
        let days = (whole / 86400.0).floor() as i64;
        let (year, month, day) = py::civil_from_days(days);
        let mut seconds = whole as i64 - days * 86400;
        let mut microsecond = ((local - whole) * 1_000_000.0).round() as i64;
        if microsecond >= 1_000_000 {
            microsecond -= 1_000_000;
            seconds += 1;
        }
        Wall { year, month, day, seconds, microsecond, zone: zone.clone() }
    }

    pub fn date(year: i64, month: i64, day: i64, zone: &TimeZone) -> Wall {
        Wall { year, month, day, seconds: 0, microsecond: 0, zone: zone.clone() }
    }

    pub fn timestamp(&self) -> f64 {
        let time = IsoTime {
            year: self.year,
            month: self.month,
            day: self.day,
            hour: self.seconds / 3600,
            minute: self.seconds / 60 % 60,
            second: self.seconds % 60,
            microsecond: self.microsecond,
            offset: None,
        };
        time.timestamp(&self.zone)
    }

    pub fn adding_days(&self, count: i64) -> Wall {
        let mut copy = self.clone();
        (copy.year, copy.month, copy.day) =
            py::civil_from_days(days_from_civil(self.year, self.month, self.day) + count);
        copy
    }

    pub fn adding_seconds(&self, count: i64) -> Wall {
        let total = self.seconds + count;
        let shift = (total as f64 / 86400.0).floor() as i64;
        let mut copy = self.adding_days(shift);
        copy.seconds = total - shift * 86400;
        copy
    }

    /// Monday is 0, as datetime.weekday().
    pub fn weekday(&self) -> i64 {
        ((days_from_civil(self.year, self.month, self.day) % 7) + 7 + 3) % 7
    }
}

impl Runtime {
    /// OCX_TIMEZONE or TZ when set and known, else the system's zone.
    pub fn local_zone(&self) -> TimeZone {
        for key in ["OCX_TIMEZONE", "TZ"] {
            if let Some(name) = self.env(key) {
                if let Ok(zone) = TimeZone::get(name) {
                    return zone;
                }
                break;
            }
        }
        TimeZone::system()
    }

    pub fn selected_range(&self) -> String {
        let record = if self.is_desktop() {
            claude_app::read_object(&format!("{}/view-range.json", self.state_dir))
        } else {
            self.read_cache("view-range")
        };
        let value = record.get("range").and_then(Json::as_str).unwrap_or("today");
        if RANGES.iter().any(|(key, _)| *key == value) { value.to_string() } else { "today".into() }
    }

    pub fn range_bounds(&self, key: &str, now: f64, zone: &TimeZone) -> (f64, f64) {
        range_bounds(key, now, zone)
    }

    /// A range's cache: the price tables' stamps, the archive setting and the folders read are part of its name, so
    /// a change in any of them reads afresh.
    pub fn range_cache_name(&self, key: Option<&str>) -> String {
        let settings = self.settings();
        let mut pricing_parts = Vec::new();
        for name in PRICING_TABLES {
            let table = pricing::table(name);
            let source = table.get("source").and_then(Json::as_object);
            pricing_parts.push(source.and_then(|s| s.get("commit")).cloned().unwrap_or(Json::Null));
            pricing_parts.push(source.and_then(|s| s.get("committedAt")).cloned().unwrap_or(Json::Null));
        }
        let mut scope = vec![
            Json::from(vec!["collector", "ccusage-v2"]),
            Json::Array(vec!["pricing".into(), Json::Array(pricing_parts)]),
            Json::Array(vec!["keep".into(), settings.get("keepCleanedUsage").cloned().unwrap_or(Json::Bool(true))]),
        ];
        let visible = settings.get("visible").and_then(Json::as_array).cloned().unwrap_or_default();
        for key in USAGE {
            if visible.contains(&Json::from(key)) {
                scope.push(Json::from(vec![key.to_string(), self.source_path(key)]));
            }
        }
        if visible.contains(&Json::from("claude")) {
            let roots = claude_app::project_roots(&self.home, &self.environment);
            scope.push(Json::Array(vec!["claude-projects".into(), Json::from(roots)]));
        }
        let key = key.map(String::from).unwrap_or_else(|| self.selected_range());
        let digest = py::sha256_hex(py::dumps(&Json::Array(scope)).as_bytes());
        format!("range_usage_{key}_{}", &digest[..12])
    }

    /// Where a source's records live on after its tool cleans them up, while 保留已清理记录的用量 is on.
    pub fn usage_archive(&self, source: &str, root: &str) -> Option<String> {
        if root.is_empty() || !self.settings().get("keepCleanedUsage").is_none_or(Json::is_truthy) {
            return None;
        }
        let expanded = paths::expand_user(root);
        let real = paths::resolve(&expanded)
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|| runtime::absolute(&expanded.to_string_lossy()));
        let digest = py::sha256_hex(real.as_bytes());
        Some(archive::location(Path::new(&self.state_dir), source, &digest[..12]).to_string_lossy().into_owned())
    }

    /// The host part of the OpenCodex address, without IPv6 brackets.
    pub fn base_host(&self) -> String {
        reqwest::Url::parse(&self.base)
            .ok()
            .and_then(|url| url.host_str().map(|host| host.trim_start_matches('[').trim_end_matches(']').to_string()))
            .unwrap_or_default()
    }

    pub fn base_is_local(&self) -> bool {
        ["127.0.0.1", "localhost", "::1"].contains(&self.base_host().as_str())
    }

    /// OpenCodex's own log of the requests it carried, when it runs on this machine; a remote one's isn't here.
    fn opencodex_request_log(&self) -> Option<String> {
        let path = format!("{}/usage.jsonl", self.ocx_home);
        (self.base_is_local() && Path::new(&path).exists()).then_some(path)
    }

    /// What each selected gateway logged, read once per refresh: None when its log can't be read here.
    fn gateway_logs(&self, enabled: &[&str], start: f64, end: f64) -> HashMap<String, Option<GatewayLog>> {
        let mut logs = HashMap::new();
        let cache = Path::new(&self.cache_dir);
        if enabled.contains(&"opencodex") {
            let log = self
                .opencodex_request_log()
                .and_then(|log| tally::gateway_requests(Path::new(&log), cache, start, end, "opencodex").ok());
            logs.insert("OpenCodex".to_string(), log);
        }
        let magpie = format!("{}/usage.jsonl", self.source_path("magpie"));
        if enabled.contains(&"magpie")
            && Path::new(&magpie).exists()
            && let Ok(log) = tally::gateway_requests(Path::new(&magpie), cache, start, end, "magpie")
        {
            logs.insert("Magpie".to_string(), Some(log));
        }
        logs
    }

    /// Gateways that may have carried this source's requests as well. Every selected source is checked against
    /// every other selected gateway; Summary counts such a request once, from the gateway.
    fn gateway_carriers(&self, source: &str, logs: &HashMap<String, Option<GatewayLog>>) -> Vec<Carrier> {
        let mut carriers = Vec::new();
        if let Some(entry) = logs.get("OpenCodex")
            && source != "opencodex"
        {
            // OpenCodex names its client only now and then, so any selected source's requests may be in its log.
            // Without the log, the local ChatGPT card defers to the proxy; others keep telling sessions by provider name.
            match entry {
                Some(log) => carriers.push(("OpenCodex".to_string(), Some(tally::requests_from(log, None)))),
                None if source == "codex" => carriers.push(("OpenCodex".to_string(), None)),
                None => {}
            }
        }
        if let Some(Some(log)) = logs.get("Magpie")
            && let Some(agents) = runtime::magpie_agents(source)
        {
            let agents: HashSet<String> = agents.iter().map(|agent| agent.to_string()).collect();
            carriers.push(("Magpie".to_string(), Some(tally::requests_from(log, Some(&agents)))));
        }
        carriers
    }

    // Claude

    pub fn claude_usage_live(&self, range_key: &str, now: f64, zone: &TimeZone, carriers: &mut [Carrier]) -> Object {
        let root = self.source_path("claude");
        // The desktop App, VS Code, JetBrains and terminal sessions share these transcripts; all of them count.
        let (files, _) = claude_app::transcript_files(&root, &self.home, &self.environment);
        let archive = self.usage_archive("claude", &root);
        if files.is_empty() && !archive.as_ref().is_some_and(|path| Path::new(path).exists()) {
            return object! { "error" => "未找到 Claude 用量记录" };
        }
        self.claude_usage_records(&root, range_key, now, zone, &files, carriers, archive.as_deref())
            .unwrap_or_else(|_| object! { "error" => "未找到 Claude 用量记录" })
    }

    #[allow(clippy::too_many_arguments)]
    fn claude_usage_records(
        &self,
        root: &str,
        range_key: &str,
        now: f64,
        zone: &TimeZone,
        files: &[String],
        carriers: &mut [Carrier],
        archive: Option<&str>,
    ) -> crate::Fallible<Object> {
        let (start, end) = range_bounds(range_key, now, zone);
        let real = paths::resolve(Path::new(root))
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|| runtime::absolute(root));
        let database =
            format!("{}/claude-app-usage-v5-{}.sqlite3", self.cache_dir, &py::sha256_hex(real.as_bytes())[..12]);
        let (all, failures) =
            claude_usage::read_events(files, Path::new(&database), None, start, end, archive.map(Path::new))?;
        let events: Vec<ClaudeEvent> = all.into_iter().filter(|event| start <= event.ts && event.ts <= end).collect();
        let tally = |rows: &[ClaudeEvent]| claude_tally(rows, range_key, zone);
        let mut result = tally(&events);
        result.insert("source", "Claude App 与 Claude Code");
        result.insert("pricingDate", claude::PRICING_DATE);
        result.insert("day", local_day(now, zone));
        if !carriers.is_empty() {
            // Requests a gateway such as Magpie also logged count once in Summary, from the gateway.
            let mut counted: Vec<ClaudeEvent> = Vec::new();
            let mut carried: Vec<(String, Vec<ClaudeEvent>)> = Vec::new();
            for event in &events {
                let m = event.metrics;
                let view = object! {
                    "input" => m[0], "output" => m[1], "writes" => m[2] + m[3], "cached" => m[4], "timestamp" => event.ts,
                };
                let owner = carriers.iter_mut().find_map(|(name, requests)| {
                    requests
                        .as_mut()
                        .filter(|requests| !requests.is_empty())
                        .and_then(|requests| tally::carried_by(requests, &view).then(|| name.clone()))
                });
                match owner {
                    Some(owner) => match carried.iter_mut().find(|(name, _)| *name == owner) {
                        Some((_, rows)) => rows.push(event.clone()),
                        None => carried.push((owner, vec![event.clone()])),
                    },
                    None => counted.push(event.clone()),
                }
            }
            for (key, value) in summary_parts(&counted, &carried, tally) {
                result.insert(key, value);
            }
        }
        if failures > 0 {
            result.insert("partial", true);
            result.insert("warning", "部分记录无法读取");
        }
        Ok(result)
    }

    // The range bundle

    /// Reads one source for the range; a failure reads as an error, never as zero.
    fn collect(
        &self,
        source: &str,
        key: &str,
        now: f64,
        zone: &TimeZone,
        logs: &HashMap<String, Option<GatewayLog>>,
        enabled: &[&str],
    ) -> Object {
        let (start, end) = range_bounds(key, now, zone);
        let mut carriers = self.gateway_carriers(source, logs);
        match source {
            "opencodex" => {
                return if self.opencodex_configured() {
                    self.opencodex_range_usage_live(key, now)
                } else {
                    object! { "error" => "未配置 OpenCodex" }
                };
            }
            // Cursor's calls never pass a gateway; only the ones it reported to Magpie are in a gateway's log.
            "cursor" => {
                let mut magpie: Vec<Carrier> = carriers.into_iter().filter(|(name, _)| name == "Magpie").collect();
                return self.cursor_range_usage_live(start, end, now, &mut magpie);
            }
            "claude" => return self.claude_usage_live(key, now, zone, &mut carriers),
            _ => {}
        }
        let root = self.source_path(source);
        if root.is_empty() {
            return object! { "error" => "请选择记录目录" };
        }
        let kept = self.usage_archive(source, &root);
        let kept = kept.as_deref().map(Path::new);
        let exclude = enabled.contains(&"opencodex");
        let cache = Path::new(&self.cache_dir);
        let carried = Some(carriers.as_mut_slice());
        let read = match source {
            "magpie" => harness::jsonl_usage(
                source,
                &root,
                cache,
                start,
                end,
                zone,
                false,
                carried,
                Some(&harness::magpie_price),
                kept,
            ),
            "gemini" => harness::gemini_usage(&root, cache, start, end, zone, exclude, carried, kept),
            "opencode" => harness::opencode_usage(&root, start, end, zone, exclude, carried),
            _ => harness::jsonl_usage(source, &root, cache, start, end, zone, exclude, carried, None, kept),
        };
        read.unwrap_or_else(|_| object! { "error" => "记录暂不可用" })
    }

    /// Every selected Harness for one range, cached a minute (longer while the panel is closed). Each source keeps
    /// the time of its own last good read, and a failed read keeps its last good figures, marked as such.
    pub fn range_usage(&self, key: Option<&str>, ttl: Option<f64>) -> Loaded {
        let key = key.map(String::from).unwrap_or_else(|| self.selected_range());
        let name = self.range_cache_name(Some(&key));
        let ttl = ttl.unwrap_or_else(|| self.refresh_ttl(60.0));
        self.cached_fetch(&name, ttl, 60.0, || {
            let zone = self.local_zone();
            let now = now();
            let (start, end) = range_bounds(&key, now, &zone);
            let enabled: Vec<&str> = USAGE.into_iter().filter(|key| self.module_enabled(key)).collect();
            // Python gives 全部 a start of the integer 0.
            let mut bundle = object! {
                "range" => key.as_str(),
                "start" => if key == "all" { Json::Int(0) } else { Json::Double(start) },
                "end" => end,
            };
            let logs = self.gateway_logs(&enabled, start, end);
            let record = self.read_cache(&name);
            let previous = record.get("payload").and_then(Json::as_object).cloned().unwrap_or_default();
            let results: Vec<Object> = std::thread::scope(|scope| {
                let handles: Vec<_> = enabled
                    .iter()
                    .map(|source| {
                        let (logs, enabled, zone, key) = (&logs, &enabled, &zone, &key);
                        scope.spawn(move || self.collect(source, key, now, zone, logs, enabled))
                    })
                    .collect();
                handles.into_iter().map(|handle| handle.join().unwrap_or_default()).collect()
            });
            for (source, result) in enabled.iter().zip(results) {
                let last = previous.get(source).and_then(Json::as_object).cloned().unwrap_or_default();
                let failed = result.get("error").is_some_and(Json::is_truthy);
                if failed && last.get("summary").is_some_and(Json::is_truthy) {
                    let mut kept = last.clone();
                    kept.insert("refreshError", result.get("error").cloned().unwrap_or(Json::Null));
                    kept.insert(
                        "readAt",
                        py::or(last.get("readAt"), record.get("fetchedAt").cloned().unwrap_or(Json::Null)),
                    );
                    bundle.insert(*source, kept);
                } else if failed {
                    bundle.insert(*source, result);
                } else {
                    let mut fresh = result;
                    fresh.insert("readAt", runtime::now());
                    bundle.insert(*source, fresh);
                }
            }
            Ok((Some(bundle), None))
        })
    }

    /// Closed panels use a relaxed cadence; visible ones keep their normal TTL.
    pub fn refresh_ttl(&self, base: f64) -> f64 {
        let (visible, battery) = self.memoized("cadence", || self.refresh_cadence());
        if visible { base } else { base.max(if battery { 300.0 } else { 180.0 }) }
    }

    fn refresh_cadence(&self) -> (bool, bool) {
        if let Some(cadence) = &self.host_cadence {
            return cadence();
        }
        if cfg!(windows) {
            // The tray says whether its panel is open.
            return (self.environment.get("OCX_PANEL_VISIBLE").map(String::as_str) == Some("1"), false);
        }
        let state = self.read_cache("panel-state");
        // A phone looking at the homepage (在手机上看) keeps the open panel's pace while it reads.
        let watched = state.get("visible") == Some(&Json::Bool(true))
            || state.get("phoneUntil").and_then(Json::python_float).is_some_and(|until| until > now());
        if watched
            && let Some(pid) = state.get("pid").filter(|pid| pid.as_bool().is_none()).and_then(Json::integer)
            && pid > 0
            && crate::platform::alive(pid as u32)
        {
            return (true, false);
        }
        let mut power = self.read_cache("power-state");
        let now = now();
        if now - power.get("checkedAt").and_then(Json::python_float).unwrap_or(0.0) > 300.0
            && let Some(output) = shell::run(&["/usr/bin/pmset", "-g", "batt"], 2.0, None, None)
        {
            power = object! { "schema" => 1, "battery" => output.contains("'Battery Power'"), "checkedAt" => now };
            self.write_json_cache(&self.cache_path("power-state"), &Json::Object(power.clone()));
        }
        (false, power.get("battery").is_some_and(Json::is_truthy))
    }
}

pub fn range_bounds(key: &str, now: f64, zone: &TimeZone) -> (f64, f64) {
    let wall = Wall::at(now, zone);
    let day = Wall::date(wall.year, wall.month, wall.day, zone);
    // Yesterday ends just before midnight: every reader counts both ends, and midnight belongs to today.
    if key == "yesterday" {
        return (day.adding_days(-1).timestamp(), day.timestamp() - 0.001);
    }
    let start = match key {
        "today" => Some(day.clone()),
        "24h" => Some(wall.adding_seconds(-86400)),
        "7d" => Some(day.adding_days(-6)),
        "30d" => Some(day.adding_days(-29)),
        "week" => Some(day.adding_days(-day.weekday())),
        "month" => Some(Wall::date(wall.year, wall.month, 1, zone)),
        _ => None,
    };
    // The end is this very moment; rebuilding it from the wall clock would pick the first of an hour seen twice.
    (start.map_or(0.0, |start| start.timestamp()), now)
}

fn claude_empty() -> Object {
    object! {
        "totalTokens" => 0, "inputTokens" => 0, "outputTokens" => 0, "cacheReadInputTokens" => 0, "cacheCreationInputTokens" => 0,
        "requests" => 0, "estimatedCostUsd" => 0, "unpricedRequests" => 0, "recordedCostRequests" => 0,
    }
}

/// Claude's card totals: tokens with cache reads and writes in the input, recorded costs first, hours only for the
/// ranges whose chart is hourly.
pub fn claude_tally(rows: &[ClaudeEvent], range_key: &str, zone: &TimeZone) -> Object {
    let mut summary = claude_empty();
    let mut days: IndexMap<String, Object> = IndexMap::new();
    let mut hours: IndexMap<i64, Object> = IndexMap::new();
    let mut models: IndexMap<String, Object> = IndexMap::new();
    let mut projects: IndexMap<String, Object> = IndexMap::new();
    let mut sessions: HashSet<Vec<u8>> = HashSet::new();
    let mut first: Option<f64> = None;
    let hourly = HOURLY_RANGES.contains(&range_key);
    for row in rows {
        let [i, o, w5, w1, r] = row.metrics;
        let metrics = row.metrics.map(|count| count as f64);
        let cost = row.cost.or_else(|| {
            claude::cost(
                &row.model,
                &metrics,
                row.speed.as_deref().unwrap_or("standard"),
                row.geo.as_deref().unwrap_or("global"),
                row.tier.as_deref().unwrap_or("standard"),
            )
        });
        first = Some(first.map_or(row.ts, |first| first.min(row.ts)));
        let day = local_day(row.ts, zone);
        sessions.insert(row.session.clone().unwrap_or_default());
        let count = |target: &mut Object| {
            let mut bump = |key: &str, amount: Json| {
                let current = target.get(key).cloned().unwrap_or(Json::Int(0));
                target.insert(key, add(&current, &amount));
            };
            bump("totalTokens", Json::Int(i + o + w5 + w1 + r));
            bump("inputTokens", Json::Int(i + w5 + w1 + r));
            bump("outputTokens", Json::Int(o));
            bump("cacheReadInputTokens", Json::Int(r));
            bump("cacheCreationInputTokens", Json::Int(w5 + w1));
            bump("requests", Json::Int(1));
            match cost {
                Some(cost) => bump("estimatedCostUsd", Json::Double(cost)),
                None => bump("unpricedRequests", Json::Int(1)),
            }
            if row.cost.is_some() {
                bump("recordedCostRequests", Json::Int(1));
            }
        };
        count(&mut summary);
        count(models.entry(row.model.clone()).or_insert_with(claude_empty));
        let project = row.project.clone().filter(|project| !project.is_empty()).unwrap_or_default();
        count(projects.entry(project).or_insert_with(claude_empty));
        count(days.entry(day).or_insert_with(claude_empty));
        if hourly {
            let hour = (row.ts / 3600.0).floor() as i64 * 3600;
            count(hours.entry(hour).or_insert_with(claude_empty));
        }
    }
    summary.insert("sessions", sessions.len() as i64);
    let requests = summary.get("requests").and_then(Json::number).unwrap_or(0.0);
    let unpriced = summary.get("unpricedRequests").and_then(Json::number).unwrap_or(0.0);
    summary.insert("coverageRatio", (requests - unpriced) / requests.max(1.0));
    let named = |rows: IndexMap<String, Object>, field: &str| -> Json {
        Json::Array(
            rows.into_iter()
                .map(|(key, mut row)| {
                    row.insert(field, key);
                    row.into()
                })
                .collect(),
        )
    };
    days.sort_keys();
    hours.sort_keys();
    object! {
        "summary" => summary,
        "models" => named(models, "model"),
        "projects" => named(projects, "project"),
        "days" => named(days, "date"),
        "hours" => Json::Array(hours.into_iter().map(|(hour, mut row)| { row.insert("timestamp", hour); row.into() }).collect()),
        "firstActivityAt" => first,
    }
}

/// Summary's share of a source, plus each gateway's share.
pub fn summary_parts<T>(counted: &[T], carried: &[(String, Vec<T>)], tally: impl Fn(&[T]) -> Object) -> Object {
    let part = |rows: &[T]| -> Json {
        let result = tally(rows);
        let kept: Object = ["summary", "models", "days", "hours", "firstActivityAt"]
            .into_iter()
            .map(|key| (key, result.get(key).cloned().unwrap_or(Json::Null)))
            .collect();
        kept.into()
    };
    let counts: Object = carried.iter().map(|(name, rows)| (name.clone(), Json::Int(rows.len() as i64))).collect();
    let parts: Object = carried.iter().map(|(name, rows)| (name.clone(), part(rows))).collect();
    object! { "counted" => part(counted), "carried" => counts, "carriedParts" => parts }
}
