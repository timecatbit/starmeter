//! Test helper, not shipped: reads {"call": name, "args": [...]} on stdin and writes the engine's answer as JSON,
//! so tests/test_engine.py can hold each ported function against the Python reference. A call not ported yet
//! answers {"error": "unported"}, which the tests skip.

use std::io::{Read, Write};

use jiff::tz::TimeZone;
use serde_json::{Map, Value, json};
use starmeter_core::time::IsoTime;
use starmeter_core::{catalog, preferences};

fn answer(call: &str, args: &[Value]) -> Result<Value, String> {
    let text = |index: usize| args.get(index).and_then(Value::as_str);
    match call {
        "parse_iso" => {
            let time = IsoTime::parse(text(0).unwrap_or("")).map_err(|error| error.to_string())?;
            let zone = text(1).and_then(|name| TimeZone::get(name).ok()).unwrap_or(TimeZone::UTC);
            Ok(json!({
                "wall": format!("{}.{:06}", time.wall(), time.microsecond),
                "offset": time.offset,
                "timestamp": time.timestamp(&zone),
                "isoformat": time.isoformat(),
            }))
        }
        "catalog" => Ok(json!({
            "manual": catalog::manual_subscriptions(),
            "portal": catalog::portal_apis(),
            "balance": catalog::balance_apis(),
            "keyOnly": catalog::key_only_apis(),
            "labels": catalog::SERVICES.iter().map(|s| s.label()).collect::<Vec<_>>(),
            "modules": preferences::MODULES.iter().map(|m| json!([m.key, m.label, m.category])).collect::<Vec<_>>(),
            "credentials": catalog::SERVICES.iter().map(|s| catalog::credential_names(s.key)).collect::<Vec<_>>(),
        })),
        _ => Err("unported".to_string()),
    }
}

fn main() {
    let mut input = String::new();
    let request = std::io::stdin()
        .read_to_string(&mut input)
        .map_err(|error| error.to_string())
        .and_then(|_| serde_json::from_str::<Value>(&input).map_err(|error| error.to_string()));
    let mut output = Map::new();
    let reply = request.and_then(|request| {
        let call = request.get("call").and_then(Value::as_str).ok_or("bad request")?.to_string();
        let args = request.get("args").and_then(Value::as_array).cloned().unwrap_or_default();
        answer(&call, &args)
    });
    match reply {
        Ok(value) => output.insert("result".into(), value),
        Err(error) => output.insert("error".into(), Value::String(error)),
    };
    let mut stdout = std::io::stdout().lock();
    let _ = writeln!(stdout, "{}", Value::Object(output));
}
