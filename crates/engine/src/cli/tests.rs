use std::collections::HashMap;

use starmeter_core::json::{Json, Object};
use starmeter_core::runtime::{now, random_hex};
use starmeter_core::{object, preferences};

use super::report::{Words, build};
use super::terminal::width;
use super::text::{Paint, View, render};
use super::*;

fn arguments(text: &str) -> Vec<String> {
    text.split_whitespace().map(String::from).collect()
}

#[test]
fn the_command_takes_the_homepages_ranges_and_groups() {
    let plain = parse(&[]).expect("no arguments");
    assert_eq!((plain.command, plain.range.as_str(), plain.group.as_str()), (Command::Overview, "today", "tool"));
    let usage = parse(&arguments("usage -r 7d --group=model -d --json --no-refresh")).expect("usage");
    assert_eq!(usage.command, Command::Usage);
    assert_eq!((usage.range.as_str(), usage.group.as_str()), ("7d", "model"));
    assert!(usage.detail && usage.json && !usage.refresh);
    let short = parse(&arguments("usage 30d project")).expect("positional range and group");
    assert_eq!((short.range.as_str(), short.group.as_str()), ("30d", "project"));
    assert_eq!(parse(&arguments("quota --lang en")).expect("quota").language.as_deref(), Some("en"));
    assert_eq!(parse(&arguments("--help")).expect("help").command, Command::Help);
    assert_eq!(parse(&arguments("-V")).expect("version").command, Command::Version);
    assert!(parse(&arguments("--range week2")).is_err());
    assert!(parse(&arguments("--group vendor")).is_err());
    assert!(parse(&arguments("--range")).is_err());
    assert!(parse(&arguments("--color sometimes")).is_err());
    assert!(parse(&arguments("--lang fr")).is_err());
    assert!(parse(&arguments("--bogus")).is_err());
}

/// The panel's fixture: its 7-day dashboard and its homepage rows.
fn fixture(range: &str) -> (Object, Vec<Json>) {
    let path = format!("{}/../../app/ui/fixtures/default.json", env!("CARGO_MANIFEST_DIR"));
    let value = Json::parse(&std::fs::read(path).expect("the fixture")).expect("JSON");
    let dashboard =
        value.get("dashboards").and_then(|d| d.get(range)).and_then(Json::as_object).cloned().expect("range");
    let entries = value.get("snapshot").and_then(|s| s.get("entries")).and_then(Json::as_array).cloned().expect("rows");
    (dashboard, entries)
}

fn visible(line: &str) -> usize {
    let mut plain = String::new();
    let mut escape = false;
    for c in line.chars() {
        match (escape, c) {
            (false, '\x1b') => escape = true,
            (true, 'm') => escape = false,
            (true, _) => {}
            (false, c) => plain.push(c),
        }
    }
    width(&plain)
}

#[test]
fn the_overview_shows_the_homepages_figures_at_every_width() {
    let (dashboard, entries) = fixture("today");
    let report = build(&dashboard, &entries, "Asia/Shanghai", Words { english: false });
    // Summary as the homepage and the share picture add it.
    assert_eq!(report.tokens, Some(61_956_680.0));
    assert_eq!(report.requests, Some(436.0));
    assert!(report.cost_partial);
    for columns in [120, 80, 64, 56, 44, 32] {
        for colour in [false, true] {
            let text = render(&report, View::Overview, columns, &Paint { colour });
            for line in text.lines() {
                assert!(visible(line) <= columns, "{columns} columns: {line:?} is {} wide", visible(line));
            }
            assert!(text.contains("61.96M"), "{columns}: {text}");
            assert!(text.contains("$21.78"), "{columns}: {text}");
            assert!(text.contains("ChatGPT 订阅"), "{columns}: {text}");
            assert_eq!(text.contains('\x1b'), colour);
        }
    }
    let wide = render(&report, View::Overview, 100, &Paint { colour: false });
    assert!(wide.contains("今日 AI 用量"), "{wide}");
    assert!(wide.contains("已知 API 估算"), "{wide}");
    assert!(wide.contains("92.0%"), "{wide}");
    assert!(wide.contains("已用 本周 38% · 5 小时 12%"), "{wide}");
    // The costliest tool first, as on the homepage.
    let claude = wide.find("Claude Code").or_else(|| wide.find("Claude")).expect("Claude's row");
    let gemini = wide.find("Gemini").expect("Gemini's row");
    assert!(claude < gemini, "{wide}");
}

#[test]
fn english_reads_as_the_panel_writes_it() {
    let (dashboard, entries) = fixture("7d");
    let report = build(&dashboard, &entries, "Asia/Shanghai", Words { english: true });
    let text = render(&report, View::Overview, 100, &Paint { colour: false });
    assert!(text.contains("AI usage · Last 7 days"), "{text}");
    assert!(text.contains("Known API estimate"), "{text}");
    assert!(text.contains("Plans and balances"), "{text}");
    assert!(text.contains("ChatGPT plan"), "{text}");
    assert!(text.contains("Used: this week 38% · 5 hours 12%"), "{text}");
    let quota = render(&report, View::Quota, 100, &Paint { colour: false });
    assert!(quota.contains("Resets"), "{quota}");
    assert!(quota.contains("Balance ¥186.40"), "{quota}");
    // No row that does something (add a key, open a page) is a figure.
    assert!(!quota.contains("Add a key") && !quota.contains("添加 Key"), "{quota}");
}

#[test]
fn json_carries_the_figures_for_scripts() {
    let (dashboard, entries) = fixture("today");
    let report = build(&dashboard, &entries, "Asia/Shanghai", Words { english: false });
    let value = Json::parse(pretty(&report.json(&["usage", "quotas"]), 0).as_bytes()).expect("valid JSON");
    let summary = value.get("summary").expect("summary");
    assert_eq!(summary.get("tokens").and_then(Json::integer), Some(61_956_680));
    assert_eq!(summary.get("costUsd").and_then(Json::number), Some(21.775984));
    assert_eq!(summary.get("costPartial"), Some(&Json::Bool(true)));
    assert_eq!(value.get("range").and_then(|r| r.get("key")).and_then(Json::as_str), Some("today"));
    let tools = value.get("tools").and_then(Json::as_array).expect("tools");
    let counted: i64 = tools.iter().filter_map(|tool| tool.get("tokens").and_then(Json::integer)).sum();
    assert!(counted >= 61_956_680);
    assert!(value.get("quotas").and_then(Json::as_array).is_some_and(|quotas| quotas.len() >= 4));
    let usage_only = report.json(&["usage"]);
    assert!(usage_only.get("quotas").is_none());
}

/// A scratch computer: one Magpie record of 1,234 tokens now, Summary and Magpie on the homepage.
fn scratch() -> (std::path::PathBuf, HashMap<String, String>) {
    let folder = std::env::temp_dir().join(format!("starmeter-cli-{}", random_hex(8)));
    let state = folder.join("settings");
    let logs = folder.join("magpie");
    std::fs::create_dir_all(&state).expect("settings");
    std::fs::create_dir_all(&logs).expect("logs");
    let record = Json::Object(object! {
        "t" => now() - 10.0, "agent" => "codex", "provider" => "test", "model" => "gpt-5.4", "in" => 1230, "out" => 4,
    });
    std::fs::write(logs.join("usage.jsonl"), record.serialized() + "\n").expect("record");
    let mut settings = preferences::read(&state);
    settings.insert("visible", Json::Array(vec!["summary".into(), "magpie".into()]));
    settings.insert("paths", object! { "magpie" => logs.to_string_lossy().as_ref() });
    preferences::write(&state, settings).expect("preferences");
    let environment: HashMap<String, String> = [
        ("OCX_STATE_DIR", state.to_string_lossy().into_owned()),
        ("OCX_CACHE_DIR", folder.join("cache").to_string_lossy().into_owned()),
        ("OCX_MAC_CACHE_DIR", folder.join("cache").to_string_lossy().into_owned()),
        ("OCX_PRICING_UPDATES", "0".into()),
        ("OCX_SELF_UPDATES", "0".into()),
        ("PATH", std::env::var("PATH").unwrap_or_default()),
        ("HOME", std::env::var("HOME").unwrap_or_default()),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_string(), value))
    .collect();
    (folder, environment)
}

#[test]
fn the_figures_are_the_ones_the_app_reads_for_the_range() {
    let (folder, environment) = scratch();
    let runtime = runtime(environment);
    let words = Words { english: false };
    for (range, group) in [("today", "tool"), ("7d", "model"), ("all", "project")] {
        let options = parse(&arguments(&format!("usage -r {range} -g {group} --no-refresh"))).expect("options");
        let (report, failed) = read(&runtime, &options, words);
        assert!(!failed);
        // The app's own read of the same range (its phone view and range switch use it).
        let app =
            runtime.render(|| runtime.range_dashboard(range, Some(group), runtime.range_with_devices(range), now()));
        let app_tokens: f64 = app
            .get("providers")
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
            .filter_map(|card| card.get("tokens").and_then(Json::number))
            .sum();
        assert_eq!(app_tokens, 1234.0, "{range}");
        assert_eq!(report.tokens, Some(1234.0), "{range}");
        assert_eq!(report.tools.iter().map(|row| row.tokens).sum::<f64>(), app_tokens, "{range}");
        assert_eq!(report.grouping, group);
        if group == "model" {
            let groups = report.groups.as_ref().expect("model cards");
            assert_eq!(groups.len(), 1);
            assert_eq!(groups[0].tokens, 1234.0);
        }
        let text = render(&report, View::Usage { detail: true }, 90, &Paint { colour: false });
        assert!(text.contains("1.2K"), "{text}");
    }
    let _ = std::fs::remove_dir_all(folder);
}

/// The fixture's dashboard for a range with its cards grouped by model or project, as the core gives them.
fn grouped(range: &str, grouping: &str) -> (Object, Vec<Json>) {
    let (mut dashboard, entries) = fixture(range);
    let path = format!("{}/../../app/ui/fixtures/default.json", env!("CARGO_MANIFEST_DIR"));
    let value = Json::parse(&std::fs::read(path).expect("the fixture")).expect("JSON");
    let cards = value.get("cards").and_then(|c| c.get(range)).and_then(|c| c.get(grouping)).cloned().expect("cards");
    dashboard.insert("grouping", grouping);
    dashboard.insert("cards", cards);
    (dashboard, entries)
}

#[test]
fn the_overview_draws_the_grouping_asked_for_and_keeps_the_plans() {
    // CLI-1: starmeter -g model printed the tools.
    let (dashboard, entries) = grouped("7d", "model");
    let report = build(&dashboard, &entries, "Asia/Shanghai", Words { english: false });
    let first =
        report.groups.as_ref().and_then(|groups| groups.first()).map(|row| row.name.clone()).expect("a model card");
    let text = render(&report, View::Overview, 100, &Paint { colour: false });
    let table = text.split("订阅与额度").next().expect("the cards");
    assert!(table.contains("  模型"), "{text}");
    assert!(table.contains(&first), "{text}");
    assert!(!table.contains("  工具 "), "{text}");
    assert!(text.contains("ChatGPT 订阅"), "{text}");
    let (dashboard, entries) = grouped("7d", "project");
    let report = build(&dashboard, &entries, "Asia/Shanghai", Words { english: true });
    let text = render(&report, View::Overview, 100, &Paint { colour: false });
    assert!(
        text.contains("  Project") && text.contains("Clients' own logs") && text.contains("Plans and balances"),
        "{text}"
    );
}

#[test]
fn an_empty_table_says_what_the_range_lacks() {
    // CLI-2: only a homepage with no tools is told to choose some.
    let words = Words { english: false };
    let say = |dashboard: &Object, view: View| {
        render(&build(dashboard, &[], "Asia/Shanghai", words), view, 100, &Paint { colour: false })
    };
    let usage = View::Usage { detail: false };
    let (mut projects, _) = grouped("today", "project");
    projects.insert("cards", Json::Array(vec![]));
    let text = say(&projects, usage);
    assert!(text.contains("这段时间的用量都没有记录项目"), "{text}");
    assert!(!text.contains("勾选"), "{text}");
    let (mut models, _) = grouped("today", "model");
    models.insert("cards", Json::Array(vec![]));
    let quiet: Vec<Json> = models
        .get("providers")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
        .map(|card| {
            let mut card = card.as_object().cloned().unwrap_or_default();
            for field in ["tokens", "requests", "cost"] {
                card.insert(field, 0);
            }
            card.remove("counted");
            card.remove("credits");
            Json::Object(card)
        })
        .collect();
    models.insert("providers", Json::Array(quiet.clone()));
    let text = say(&models, usage);
    assert!(text.contains("此时段暂无模型调用") && !text.contains("勾选"), "{text}");
    projects.insert("providers", Json::Array(quiet));
    let text = say(&projects, usage);
    assert!(text.contains("此时段暂无调用") && !text.contains("记录项目"), "{text}");
    let (mut nothing, _) = fixture("today");
    nothing.insert("providers", Json::Array(vec![]));
    assert!(say(&nothing, View::Overview).contains("主页还没有勾选工具"));
}
