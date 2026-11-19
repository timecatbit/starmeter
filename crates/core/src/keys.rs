//! Several named keys per API: secrets stay in private files, the index holds names and options only.

use std::path::{Path, PathBuf};

use crate::files;
use crate::json::{Json, Object};
use crate::object;
use crate::preferences::Error;

fn root(state: &Path) -> PathBuf {
    state.join("keys")
}

fn valid_label(value: &str) -> bool {
    let count = value.chars().count();
    (1..=24).contains(&count) && value.chars().all(|c| !((c as u32) < 0x20 || c as u32 == 0x7F || c == '|'))
}

fn is_id(value: &str) -> bool {
    value.len() == 8 && value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub fn load(state: &Path) -> Object {
    let empty = object! { "schema" => 1, "providers" => Object::new(), "labels" => Object::new() };
    let Some(value) = std::fs::read(root(state).join("index.json"))
        .ok()
        .and_then(|data| Json::parse(&data).ok())
        .and_then(|value| value.as_object().cloned())
        .filter(|value| value.get("schema") == Some(&Json::Int(1)))
    else {
        return empty;
    };
    let mut clean = Object::new();
    for (provider, rows) in
        value.get("providers").and_then(Json::as_object).into_iter().flat_map(|providers| providers.iter())
    {
        let kept: Vec<Json> = rows
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|row| {
                let fields = row.as_object()?;
                let id = fields.get("id")?;
                let label = fields.get("label")?.as_str()?;
                if !is_id(&crate::py::str(Some(id))) || !valid_label(label) {
                    return None;
                }
                let options = fields.get("options").and_then(Json::as_object).cloned().unwrap_or_default();
                Some(object! { "id" => id.clone(), "label" => label, "options" => options }.into())
            })
            .collect();
        clean.insert(provider.clone(), Json::Array(kept));
    }
    let mut labels = Object::new();
    for (provider, label) in value.get("labels").and_then(Json::as_object).into_iter().flat_map(|labels| labels.iter())
    {
        if label.as_str().is_some_and(valid_label) {
            labels.insert(provider.clone(), label.clone());
        }
    }
    object! { "schema" => 1, "providers" => clean, "labels" => labels }
}

fn save(state: &Path, data: Object) -> Result<(), Error> {
    let index = root(state).join("index.json");
    // load() reads a damaged index as empty; keep the original names and options before replacing it.
    files::keep_unreadable(&index, files::schema_one).map_err(Error::Write)?;
    write_private(&index, &Json::Object(data).serialized())
}

fn write_private(path: &Path, text: &str) -> Result<(), Error> {
    let folder = path.parent().unwrap_or(Path::new(""));
    files::make_directory(folder, 0o700).map_err(Error::Write)?;
    crate::platform::set_mode(folder, 0o700);
    files::replace_atomically(path, text.as_bytes(), ".key-", || Ok(())).map_err(Error::Write)
}

pub fn label(value: Option<&str>) -> Result<String, Error> {
    let text = value.unwrap_or("").trim();
    if valid_label(text) { Ok(text.to_string()) } else { Err(Error::Invalid("invalid key name")) }
}

pub fn entries(state: &Path, provider: &str) -> Vec<Object> {
    load(state)
        .get("providers")
        .and_then(|providers| providers.get(provider))
        .and_then(Json::as_array)
        .map(|rows| rows.iter().filter_map(|row| row.as_object().cloned()).collect())
        .unwrap_or_default()
}

fn secret_path(state: &Path, provider: &str, id: &str) -> Result<PathBuf, Error> {
    let valid_provider = (2..=32).contains(&provider.len())
        && provider.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
    if !valid_provider || !is_id(id) {
        return Err(Error::Invalid("invalid key id"));
    }
    Ok(root(state).join(provider).join(format!("{id}.key")))
}

pub fn secret(state: &Path, provider: &str, id: &str) -> String {
    secret_path(state, provider, id)
        .ok()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .map(|text| text.trim().to_string())
        .unwrap_or_default()
}

/// An API key as typed: 8 to 1024 printable ASCII characters.
pub fn valid_key(value: &str) -> bool {
    (8..=1024).contains(&value.len()) && value.bytes().all(|byte| (0x21..=0x7E).contains(&byte))
}

pub fn add(state: &Path, provider: &str, name: &str, value: &str, options: Object) -> Result<String, Error> {
    if !valid_key(value) {
        return Err(Error::Invalid("invalid API key"));
    }
    let mut data = load(state);
    let id = crate::runtime::random_hex(8);
    write_private(&secret_path(state, provider, &id)?, &format!("{value}\n"))?;
    let mut providers = data.get("providers").and_then(Json::as_object).cloned().unwrap_or_default();
    let mut rows = providers.get(provider).and_then(Json::as_array).cloned().unwrap_or_default();
    rows.push(object! { "id" => id.as_str(), "label" => label(Some(name))?, "options" => options }.into());
    providers.insert(provider, Json::Array(rows));
    data.insert("providers", providers);
    save(state, data)?;
    Ok(id)
}

/// The classic single connection is "default"; its name lives in the index too.
pub fn rename(state: &Path, provider: &str, id: &str, name: &str) -> Result<(), Error> {
    let mut data = load(state);
    if id == "default" {
        let mut labels = data.get("labels").and_then(Json::as_object).cloned().unwrap_or_default();
        labels.insert(provider, label(Some(name))?);
        data.insert("labels", labels);
    } else {
        let mut providers = data.get("providers").and_then(Json::as_object).cloned().unwrap_or_default();
        let mut rows = providers.get(provider).and_then(Json::as_array).cloned().unwrap_or_default();
        let Some(index) = rows.iter().position(|row| row.get("id") == Some(&Json::from(id))) else {
            return Err(Error::Invalid("unknown key"));
        };
        let Some(mut row) = rows[index].as_object().cloned() else { return Err(Error::Invalid("unknown key")) };
        row.insert("label", label(Some(name))?);
        rows[index] = row.into();
        providers.insert(provider, Json::Array(rows));
        data.insert("providers", providers);
    }
    save(state, data)
}

pub fn remove(state: &Path, provider: &str, id: &str) -> Result<(), Error> {
    let mut data = load(state);
    let mut providers = data.get("providers").and_then(Json::as_object).cloned().unwrap_or_default();
    let rows = providers.get(provider).and_then(Json::as_array).cloned().unwrap_or_default();
    if !rows.iter().any(|row| row.get("id") == Some(&Json::from(id))) {
        return Err(Error::Invalid("unknown key"));
    }
    providers
        .insert(provider, Json::Array(rows.into_iter().filter(|row| row.get("id") != Some(&Json::from(id))).collect()));
    data.insert("providers", providers);
    save(state, data)?;
    if let Ok(path) = secret_path(state, provider, id) {
        let _ = std::fs::remove_file(path);
    }
    Ok(())
}

pub fn default_label(state: &Path, provider: &str) -> String {
    load(state)
        .get("labels")
        .and_then(|labels| labels.get(provider))
        .and_then(Json::as_str)
        .unwrap_or("默认")
        .to_string()
}
