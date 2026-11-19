//! Test helper, not shipped: reads {"call": name, "args": [...]} on stdin and writes the engine's answer as JSON,
//! so tests/test_engine.py can hold each ported function against the Python reference. A call not ported yet
//! answers {"error": "unported"}, which the tests skip.

use std::io::{Read, Write};
use std::path::Path;

use jiff::tz::TimeZone;
use starmeter_core::json::{Format, Json, python_repr};
use starmeter_core::time::IsoTime;
use starmeter_core::{catalog, claude_usage, files, object, preferences, pricing, py};

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
        "sha256" => Ok(py::sha256_hex(text(0).unwrap_or("").as_bytes()).into()),
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
