//! Test helper, not shipped: reads {"call": name, "args": [...]} on stdin and writes the engine's answer as JSON,
//! so tests/test_engine.py can hold each ported function against the Python reference. A call not ported yet
//! answers {"error": "unported"}, which the tests skip.

use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::path::Path;

use jiff::tz::TimeZone;
use starmeter_core::json::{Format, Json, Object, python_repr};
use starmeter_core::runtime::{self, Runtime};
use starmeter_core::time::IsoTime;
use starmeter_core::{
    accounts, alerts, background, catalog, claude_subscription, claude_usage, client_accounts, cursor, files, harness,
    http, object, opencodex, preferences, pricing, py, range_usage, sessions, tally, updates, zstd,
};

fn answer(call: &str, args: &[Json]) -> Result<Json, String> {
    let text = |index: usize| args.get(index).and_then(Json::as_str);
    match call {
        "parse_iso" => {
            let time = IsoTime::parse(text(0).unwrap_or("")).map_err(|error| error.to_string())?;
            let zone = text(1).and_then(|name| TimeZone::get(name).ok()).unwrap_or(TimeZone::UTC);
            Ok(object! {
                "wall" => format!("{}.{:06}", time.wall(), time.microsecond),
                "offset" => time.offset,
                "timestamp" => time.timestamp(&zone),
                "isoformat" => time.isoformat(),
            }
            .into())
        }
        "keep_unreadable" => {
            let kept = files::keep_unreadable(Path::new(text(0).unwrap_or("")), files::schema_one)
                .map_err(|error| error.to_string())?;
            Ok(kept.map(|path| path.to_string_lossy().into_owned()).into())
        }
        "preferences.read" => Ok(preferences::read(Path::new(text(0).unwrap_or(""))).into()),
        "preferences.update" => {
            let directory = Path::new(text(0).unwrap_or(""));
            Ok(preferences::update(directory, text(1).unwrap_or(""), text(2), text(3))
                .map_err(|error| error.to_string())?
                .into())
        }
        "json.roundtrip" => {
            // Parses text(0) and writes it back with the separators given, as json.dumps would.
            let value = Json::parse_str(text(0).unwrap_or("")).map_err(|error| error.to_string())?;
            let format = Format {
                ensure_ascii: args.get(1) == Some(&Json::Bool(true)),
                item: text(2).unwrap_or(", "),
                key: text(3).unwrap_or(": "),
                sort_keys: false,
            };
            Ok(value.serialized_with(format).into())
        }
        "python_repr" => {
            Ok(args.iter().map(|value| python_repr(value.number().unwrap_or(f64::NAN))).collect::<Vec<_>>().into())
        }
        "zstd" => Ok(args
            .iter()
            .map(|path| match zstd::decompress_file(Path::new(path.as_str().unwrap_or(""))) {
                Ok(data) => Json::from(object! { "sha256" => py::sha256_hex(&data), "size" => data.len() as i64 }),
                Err(error) => Json::from(object! { "error" => error.to_string() }),
            })
            .collect::<Vec<_>>()
            .into()),
        "sha256" => Ok(py::sha256_hex(text(0).unwrap_or("").as_bytes()).into()),
        "range.usage" => {
            let runtime = Runtime::from_process();
            let ttl = args.get(1).and_then(Json::number);
            let (payload, error) = runtime.render(|| runtime.range_usage(text(0), ttl));
            Ok(object! { "payload" => payload, "error" => error, "cache" => runtime.range_cache_name(text(0)) }.into())
        }
        "dashboard" => {
            // [data, now, range error]
            let runtime = Runtime::from_process();
            let data = args.first().and_then(Json::as_object).cloned().unwrap_or_default();
            let now = args.get(1).and_then(Json::number).unwrap_or_else(starmeter_core::runtime::now);
            Ok(runtime.render(|| runtime.dashboard_payload(&data, text(2), now)).into())
        }
        "range.bounds" => {
            let zone = text(2).and_then(|name| TimeZone::get(name).ok()).unwrap_or(TimeZone::UTC);
            let (start, end) = range_usage::range_bounds(
                text(0).unwrap_or(""),
                args.get(1).and_then(Json::number).unwrap_or(0.0),
                &zone,
            );
            Ok(Json::from(vec![start, end]))
        }
        "runtime" => {
            // [what, routes, ...]: a Runtime call against the environment this probe runs in.
            routes(args.get(1));
            let runtime = Runtime::from_process();
            let loaded = |(payload, error): (Option<Object>, Option<String>)| {
                Json::from(object! { "payload" => payload, "error" => error })
            };
            runtime.render(|| match text(0).unwrap_or("") {
                "provider_quotas" => Ok(loaded(runtime.provider_quotas())),
                "komari" => Ok(loaded(runtime.komari_snapshot())),
                "opencodex_range" => Ok(runtime
                    .opencodex_range_usage_live(
                        text(2).unwrap_or(""),
                        args.get(3).and_then(Json::number).unwrap_or(0.0),
                    )
                    .into()),
                "connection_parts" => Ok(Json::Array(runtime.connection_parts(text(2).unwrap_or("")))),
                "fingerprint" => Ok(runtime.connection_fingerprint(text(2).unwrap_or("")).into()),
                "opencodex_refresh" => {
                    runtime.opencodex_refresh();
                    let caches: Object = opencodex::ENDPOINTS
                        .iter()
                        .map(|(name, _, _)| (*name, Json::Object(runtime.read_cache(name))))
                        .collect();
                    Ok(caches.into())
                }
                "loaders" => {
                    Ok(Json::Array(runtime.external_loaders().into_iter().map(|(name, _)| Json::from(name)).collect()))
                }
                "ready" => Ok(preferences::MODULES
                    .iter()
                    .map(|module| (module.key, Json::Bool(runtime.default_ready(module.key))))
                    .collect::<Object>()
                    .into()),
                "status" => {
                    let wanted = strings(args.get(2));
                    Ok(background::service_status::read_all(&wanted, "test").into())
                }
                "fx" => Ok(loaded(runtime.exchange_rate_load())),
                "alert_notices" => Ok(Json::Array(
                    args.get(2)
                        .and_then(Json::as_array)
                        .into_iter()
                        .flatten()
                        .map(|alert| {
                            let (subtitle, body) =
                                runtime.alert_notice(&alert.as_object().cloned().unwrap_or_default(), runtime::now());
                            Json::Array(vec![subtitle.into(), body.into()])
                        })
                        .collect(),
                )),
                "alerts_due" => {
                    // [windows, balances, marks, percent, reminder, now]
                    let list = |index: usize| -> Vec<Object> {
                        args.get(index)
                            .and_then(Json::as_array)
                            .into_iter()
                            .flatten()
                            .filter_map(Json::as_object)
                            .cloned()
                            .collect()
                    };
                    let mut marks = args.get(4).and_then(Json::as_object).cloned().unwrap_or_default();
                    let due = alerts::due(
                        &list(2),
                        &list(3),
                        &mut marks,
                        args.get(5).and_then(Json::number).unwrap_or(0.0),
                        args.get(6) == Some(&Json::Bool(true)),
                        args.get(7).and_then(Json::number).unwrap_or(0.0),
                    );
                    Ok(object! { "due" => Json::Array(due.into_iter().map(Json::Object).collect()), "marks" => marks }
                        .into())
                }
                _ => Err("unported".to_string()),
            })
        }
        "accounts.account" => {
            // [source, key or null, options, environment, routes]
            routes(args.get(4));
            let mut environment: HashMap<String, String> = std::env::vars().collect();
            for (name, value) in args.get(3).and_then(Json::as_object).into_iter().flat_map(|values| values.iter()) {
                if let Some(value) = value.as_str() {
                    environment.insert(name.clone(), value.to_string());
                }
            }
            let options = args.get(2).and_then(Json::as_object).cloned().unwrap_or_default();
            Ok(accounts::account(
                text(0).unwrap_or(""),
                text(1),
                &options,
                &environment,
                starmeter_core::runtime::now(),
            )
            .into())
        }
        "claude.parse" => {
            let now = args.get(1).and_then(Json::number).unwrap_or(0.0);
            claude_subscription::parse_usage(args.first().unwrap_or(&Json::Null), now)
                .map(Json::from)
                .map_err(|error| error.0)
        }
        "claude.desktop" => {
            Ok(claude_subscription::desktop(text(0).unwrap_or(""), args.get(1).and_then(Json::number).unwrap_or(0.0))
                .into())
        }
        "claude.local" => Ok(claude_subscription::local_snapshot(
            text(0).unwrap_or(""),
            args.get(1).and_then(Json::number).unwrap_or(0.0),
        )
        .into()),
        "claude.oauth" => {
            // [state, force, requestedAt, routes]
            routes(args.get(3));
            let force = args.get(1) == Some(&Json::Bool(true));
            let requested = args.get(2).and_then(Json::number).unwrap_or(0.0);
            let (payload, error) = claude_subscription::oauth_fetch(text(0).unwrap_or(""), true, force, requested);
            Ok(object! { "payload" => payload, "error" => error }.into())
        }
        "updates.pieces" => {
            // Small pure pieces: versions, CLI output, the checker ID, appcasts and Codex's channel.
            let list = |index: usize| args.get(index).and_then(Json::as_array).cloned().unwrap_or_default();
            let versions: Vec<Json> = list(0)
                .iter()
                .map(|value| {
                    updates::release_version(Some(value)).map_or(Json::Null, |parts| Json::from(parts.to_vec()))
                })
                .collect();
            let first: Vec<Json> =
                list(1).iter().map(|text| updates::first_version(text.as_str().unwrap_or("")).into()).collect();
            let sparkle: Vec<Json> = list(2)
                .iter()
                .map(|body| {
                    let (version, build) = updates::sparkle_release(body.as_str().unwrap_or("").as_bytes());
                    Json::from(vec![Json::from(version), Json::from(build)])
                })
                .collect();
            let channels: Vec<Json> =
                list(3).iter().map(|binary| updates::codex_channel(binary.as_str().unwrap_or("")).into()).collect();
            Ok(object! {
                "versions" => Json::Array(versions), "first" => Json::Array(first),
                "uuid" => updates::uuid5_url("opencodex-swiftbar-update-check"),
                "sparkle" => Json::Array(sparkle), "channels" => Json::Array(channels),
            }
            .into())
        }
        "qoder.snapshot" => {
            let environment: HashMap<String, String> = std::env::vars().collect();
            let snapshot = client_accounts::qoder::snapshot(
                text(0).unwrap_or(""),
                &environment,
                args.get(1).and_then(Json::number).unwrap_or(0.0),
            );
            Ok(snapshot.into())
        }
        "chatgpt.account" => {
            let environment: HashMap<String, String> = std::env::vars().collect();
            Ok(client_accounts::chatgpt::account(text(0), text(1), &environment).into())
        }
        "accounts.check" => {
            routes(args.get(2));
            Ok(accounts::check_key(text(0).unwrap_or(""), text(1).unwrap_or("")).into())
        }
        "cursor.refresh" => {
            // [store or null, home, start, until, now, routes]
            routes(args.get(5));
            let environment: HashMap<String, String> = std::env::vars().collect();
            let store = args.first().and_then(Json::as_object);
            let number = |index: usize| args.get(index).and_then(Json::number).unwrap_or(0.0);
            Ok(match cursor::refresh(store, text(1).unwrap_or(""), &environment, number(2), number(3), number(4)) {
                Ok(store) => store.into(),
                Err(failure) => object! { "failure" => failure.0 }.into(),
            })
        }
        "cursor.plan" => {
            routes(args.get(2));
            let environment: HashMap<String, String> = std::env::vars().collect();
            Ok(
                match cursor::plan(
                    text(0).unwrap_or(""),
                    &environment,
                    args.get(1).and_then(Json::number).unwrap_or(0.0),
                ) {
                    Ok(plan) => plan.into(),
                    Err(failure) => object! { "failure" => failure.0 }.into(),
                },
            )
        }
        "sessions.turnLines" => {
            // [turns, unit, project]: a session's turns as the menu lists them.
            let runtime = Runtime::from_process();
            let turns: Vec<Object> = args
                .first()
                .and_then(Json::as_array)
                .map(|turns| turns.iter().filter_map(|turn| turn.as_object().cloned()).collect())
                .unwrap_or_default();
            let lines = runtime.turn_lines(&turns, args.get(1).unwrap_or(&Json::Null), text(2));
            Ok(Json::from(lines))
        }
        "sessions.active" => {
            // [claude roots, codex root, now, with prices, claude app map]
            let runtime = Runtime::from_process();
            let mut app: HashMap<String, (Option<String>, Option<String>)> = HashMap::new();
            for (key, value) in args.get(4).and_then(Json::as_object).into_iter().flat_map(|map| map.iter()) {
                let field = |name: &str| value.get(name).and_then(Json::as_str).map(String::from);
                app.insert(key.clone(), (field("title"), field("effort")));
            }
            let prices = runtime.session_prices();
            let with_prices = args.get(3) == Some(&Json::Bool(true));
            let found = sessions::active(
                &strings(args.first()),
                text(1),
                args.get(2).and_then(Json::number).unwrap_or(0.0),
                if with_prices { Some(&prices) } else { None },
                &app,
            );
            Ok(Json::Array(found))
        }
        "harness.gateway" => {
            let log = tally::gateway_requests(
                Path::new(text(0).unwrap_or("")),
                Path::new(text(1).unwrap_or("")),
                args.get(2).and_then(Json::number).unwrap_or(0.0),
                args.get(3).and_then(Json::number).unwrap_or(f64::INFINITY),
                text(4).unwrap_or("opencodex"),
            )
            .map_err(|error| error.to_string())?;
            let clients: Vec<Json> = log
                .into_iter()
                .map(|(agent, requests)| {
                    let mut splits: Vec<_> = requests.into_iter().collect();
                    splits.sort_by_key(|(split, _)| *split);
                    let splits: Vec<Json> = splits
                        .into_iter()
                        .map(|(split, times)| {
                            Json::from(vec![
                                Json::Int(split.input),
                                Json::Int(split.cached),
                                Json::Int(split.writes),
                                Json::Int(split.output),
                                Json::from(times),
                            ])
                        })
                        .collect();
                    Json::from(vec![Json::from(agent), Json::Array(splits)])
                })
                .collect();
            Ok(clients.into())
        }
        "harness.read" => {
            // [source, root, cache, start, end, zone, excludeProxy, archive, carriers]; carriers: [[name, log path or null, kind, agents or null]]
            let source = text(0).unwrap_or("");
            let zone = text(5).and_then(|name| TimeZone::get(name).ok()).unwrap_or(TimeZone::UTC);
            let start = args.get(3).and_then(Json::number).unwrap_or(0.0);
            let end = args.get(4).and_then(Json::number).unwrap_or(f64::INFINITY);
            let cache = Path::new(text(2).unwrap_or(""));
            let mut carriers: Vec<tally::Carrier> = Vec::new();
            for spec in args.get(8).and_then(Json::as_array).into_iter().flatten() {
                let fields = spec.as_array().cloned().unwrap_or_default();
                let Some(name) = fields.first().and_then(Json::as_str) else { continue };
                match fields.get(1).and_then(Json::as_str) {
                    Some(path) => {
                        let kind = fields.get(2).and_then(Json::as_str).unwrap_or("opencodex");
                        let log = tally::gateway_requests(Path::new(path), cache, start, end, kind)
                            .map_err(|error| error.to_string())?;
                        let agents: Option<HashSet<String>> = fields
                            .get(3)
                            .and_then(Json::as_array)
                            .map(|_| strings(fields.get(3)).into_iter().collect());
                        carriers.push((name.to_string(), Some(tally::requests_from(&log, agents.as_ref()))));
                    }
                    None => carriers.push((name.to_string(), None)),
                }
            }
            let exclude = args.get(6) == Some(&Json::Bool(true));
            let carried = if carriers.is_empty() { None } else { Some(carriers.as_mut_slice()) };
            let archive = text(7).map(Path::new);
            let root = text(1).unwrap_or("");
            let result = match source {
                "gemini" => harness::gemini_usage(root, cache, start, end, &zone, exclude, carried, archive),
                "opencode" => harness::opencode_usage(root, start, end, &zone, exclude, carried),
                _ => {
                    let price: Option<tally::Price> =
                        if source == "magpie" { Some(&harness::magpie_price) } else { None };
                    harness::jsonl_usage(source, root, cache, start, end, &zone, exclude, carried, price, archive)
                }
            };
            Ok(result.map_err(|error| error.to_string())?.into())
        }
        "claude.read_events" => {
            let files: Vec<String> = strings(args.first());
            let allowed: Option<HashSet<String>> =
                args.get(2).and_then(Json::as_array).map(|_| strings(args.get(2)).into_iter().collect());
            let archive = text(5).map(Path::new);
            let (events, failures) = claude_usage::read_events(
                &files,
                Path::new(text(1).unwrap_or("")),
                allowed.as_ref(),
                args.get(3).and_then(Json::number).unwrap_or(0.0),
                args.get(4).and_then(Json::number).unwrap_or(f64::INFINITY),
                archive,
            )
            .map_err(|error| error.to_string())?;
            let events: Vec<Json> = events
                .into_iter()
                .map(|event| {
                    object! {
                        "ts" => event.ts, "model" => event.model,
                        "metrics" => event.metrics.iter().map(|&n| Json::Int(n)).collect::<Vec<_>>(),
                        "cost" => event.cost, "speed" => event.speed, "geo" => event.geo, "tier" => event.tier,
                        "session" => event.session.map(|session| claude_usage::hex(&session)), "project" => event.project,
                    }
                    .into()
                })
                .collect();
            Ok(object! { "failures" => failures as i64, "events" => events }.into())
        }
        "claude.digest" => {
            Ok(claude_usage::digest(args.to_vec()).iter().map(|byte| format!("{byte:02x}")).collect::<String>().into())
        }
        "claude.price" => {
            let metrics: Vec<f64> = args
                .get(1)
                .and_then(Json::as_array)
                .map(|items| items.iter().filter_map(Json::integer).map(|n| n as f64).collect())
                .unwrap_or_default();
            let cost = pricing::claude::cost(
                text(0).unwrap_or(""),
                &metrics,
                text(2).unwrap_or("standard"),
                text(3).unwrap_or("global"),
                text(4).unwrap_or("standard"),
            );
            Ok(cost.into())
        }
        "catalog" => Ok(object! {
            "manual" => catalog::manual_subscriptions(),
            "portal" => catalog::portal_apis(),
            "balance" => catalog::balance_apis(),
            "keyOnly" => catalog::key_only_apis(),
            "labels" => catalog::SERVICES.iter().map(|s| s.label()).collect::<Vec<_>>(),
            "modules" => preferences::MODULES
                .iter()
                .map(|m| Json::from(vec![Json::from(m.key), Json::from(m.label.as_str()), Json::from(m.category)]))
                .collect::<Vec<_>>(),
            "credentials" => catalog::SERVICES.iter().map(|s| catalog::credential_names(s.key)).collect::<Vec<_>>(),
        }
        .into()),
        _ => Err("unported".to_string()),
    }
}

/// Points real hosts at the test's local server, from a {from: to} argument.
fn routes(value: Option<&Json>) {
    let routes = value
        .and_then(Json::as_object)
        .map(|routes| routes.iter().filter_map(|(from, to)| Some((from.clone(), to.as_str()?.to_string()))).collect())
        .unwrap_or_default();
    http::set_routes(routes);
}

/// The strings in a JSON array argument.
fn strings(value: Option<&Json>) -> Vec<String> {
    value
        .and_then(Json::as_array)
        .map(|items| items.iter().filter_map(Json::as_str).map(String::from).collect())
        .unwrap_or_default()
}

fn main() {
    if let Some(folder) = std::env::var_os("STARMETER_PRICING") {
        pricing::set_folders(vec![folder.into()]);
    }
    let mut input = Vec::new();
    let request = std::io::stdin()
        .read_to_end(&mut input)
        .map_err(|error| error.to_string())
        .and_then(|_| Json::parse(&input).map_err(|error| error.to_string()));
    let reply = request.and_then(|request| {
        let call = request.get("call").and_then(Json::as_str).ok_or("bad request")?.to_string();
        let args = request.get("args").and_then(Json::as_array).cloned().unwrap_or_default();
        answer(&call, &args)
    });
    let output = match reply {
        Ok(value) => object! { "result" => value },
        Err(error) => object! { "error" => error },
    };
    let mut stdout = std::io::stdout().lock();
    let _ = writeln!(stdout, "{}", Json::Object(output));
}
