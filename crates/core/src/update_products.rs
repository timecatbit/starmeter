//! What 工具更新 checks for each Harness on the homepage, product by product: an app and its CLI apart (Claude and
//! Claude Code, ChatGPT and Codex, Gemini CLI beside Antigravity's app, IDE and agy CLI, Cursor and Cursor CLI …). For
//! each: where it is installed, the channel a CLI came from, where its vendor publishes the newest version, and the
//! official command that updates it. Only public version metadata is read, with no account and no device ID.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::claude_app;
use crate::files::paths;
use crate::json::{Json, Object};
use crate::object;
use crate::py::{self, FileError};
use crate::shell;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    App,
    Cli,
}

pub struct Product {
    pub id: &'static str,
    /// The homepage Harness whose selection lists it.
    pub tool: &'static str,
    pub name: &'static str,
    /// The official download or release page.
    pub link: &'static str,
    pub kind: Kind,
    /// Its macOS app, by bundle name.
    pub apps: &'static [&'static str],
    /// Its command on the PATH (Magpie's app carries one too, read when the app isn't there).
    pub commands: &'static [&'static str],
    /// Listed even when it is not on this computer (the tool's main product); the others only where installed.
    pub always: bool,
    /// Why there is no version check: its vendor publishes no version for it.
    pub manual: Option<&'static str>,
    /// The app updates itself, so an update opens it rather than a download page.
    pub updates_itself: bool,
}

const fn app(id: &'static str, tool: &'static str, name: &'static str, link: &'static str) -> Product {
    Product {
        id,
        tool,
        name,
        link,
        kind: Kind::App,
        apps: &[],
        commands: &[],
        always: true,
        manual: None,
        updates_itself: true,
    }
}

const fn cli(id: &'static str, tool: &'static str, name: &'static str, link: &'static str) -> Product {
    Product {
        id,
        tool,
        name,
        link,
        kind: Kind::Cli,
        apps: &[],
        commands: &[],
        always: true,
        manual: None,
        updates_itself: false,
    }
}

/// In the homepage's order (by Harness), an app before its CLI. OpenCodex keeps a row of its own (more_rows.rs): it
/// updates itself through its own installer.
pub const PRODUCTS: [Product; 18] = [
    Product {
        apps: &["magpie", "Magpie"],
        commands: &["magpie"],
        updates_itself: false,
        ..app("magpie", "magpie", "Magpie", "https://github.com/yetone/magpie/releases/latest")
    },
    Product { apps: &["ChatGPT"], ..app("chatgpt", "codex", "ChatGPT", "https://chatgpt.com/download") },
    Product { commands: &["codex"], ..cli("codex-cli", "codex", "Codex", "https://github.com/openai/codex/releases") },
    Product { apps: &["Claude"], ..app("claude", "claude", "Claude", "https://claude.ai/download") },
    Product {
        commands: &["claude"],
        ..cli("claude-code", "claude", "Claude Code", "https://github.com/anthropics/claude-code/releases")
    },
    Product { apps: &["Cursor"], ..app("cursor", "cursor", "Cursor", "https://cursor.com/downloads") },
    Product {
        commands: &["cursor-agent"],
        always: false,
        ..cli("cursor-cli", "cursor", "Cursor CLI", "https://cursor.com/cli")
    },
    Product {
        commands: &["dsh"],
        ..cli(
            "deepseek-harness",
            "deepseek-harness",
            "DeepSeek Harness",
            "https://www.npmjs.com/package/@deepseek-ai/dsh",
        )
    },
    Product {
        apps: &["Gemini"],
        always: false,
        manual: Some("Google 软件更新没有公开 Gemini App 的版本接口"),
        ..app("gemini-app", "gemini", "Gemini App", "https://gemini.google/mac/")
    },
    Product {
        commands: &["gemini"],
        ..cli("gemini", "gemini", "Gemini CLI", "https://github.com/google-gemini/gemini-cli/releases")
    },
    Product {
        apps: &["Antigravity"],
        always: false,
        ..app("antigravity", "gemini", "Antigravity", "https://antigravity.google/download")
    },
    Product {
        apps: &["Antigravity IDE"],
        always: false,
        ..app("antigravity-ide", "gemini", "Antigravity IDE", "https://antigravity.google/download")
    },
    Product {
        commands: &["agy"],
        always: false,
        ..cli("antigravity-cli", "gemini", "Antigravity CLI", "https://antigravity.google/cli")
    },
    Product {
        apps: &["OpenCode"],
        always: false,
        ..app("opencode-desktop", "opencode", "OpenCode Desktop", "https://opencode.ai/download")
    },
    Product { commands: &["opencode"], ..cli("opencode", "opencode", "OpenCode", "https://opencode.ai") },
    Product {
        apps: &["Qoder CN", "QoderCN"],
        manual: Some("Qoder CN 没有公开 App 的版本接口"),
        ..app("qoder", "qoder", "Qoder", "https://qoder.cn/download")
    },
    Product {
        commands: &["qoderclicn", "qodercn"],
        always: false,
        ..cli("qoder-cli", "qoder", "Qoder CLI", "https://qoder.com.cn/cli")
    },
    Product {
        apps: &["WorkBuddy"],
        manual: Some("腾讯没有公开 WorkBuddy 的版本接口"),
        ..app("workbuddy", "workbuddy", "WorkBuddy", "https://cloud.tencent.com/product/workbuddy")
    },
];

pub fn product(id: &str) -> Option<&'static Product> {
    PRODUCTS.iter().find(|product| product.id == id)
}

/// The products a Harness on the homepage brings to 工具更新.
pub fn products_of(tool: &str) -> impl Iterator<Item = &'static Product> + '_ {
    PRODUCTS.iter().filter(move |product| product.tool == tool)
}

/// Why an app's row has no check on a system other than macOS: its version is read from its bundle there only.
pub const APP_ELSEWHERE: &str = "只在 macOS 上读取 App 的版本";

// Versions

/// The prerelease after the numbers (1.2.3-rc.5 → rc.5), when the version is one release_version reads.
fn prerelease(text: &str) -> Option<String> {
    let text = text.trim();
    let text = text.strip_prefix('v').unwrap_or(text);
    let core = text.split(['-', '+']).next().unwrap_or("");
    text[core.len()..].strip_prefix('-').map(String::from)
}

/// Two versions in semantic-version order: numbers first, then a release above its prereleases, prereleases by their
/// dot-separated parts (numbers by value, below words). None when either can't be read.
pub fn release_order(a: Option<&Json>, b: Option<&Json>) -> Option<Ordering> {
    let (numbers_a, numbers_b) = (crate::updates::release_version(a)?, crate::updates::release_version(b)?);
    let order = numbers_a.cmp(&numbers_b);
    if order != Ordering::Equal {
        return Some(order);
    }
    let text = |value: Option<&Json>| py::str(Some(&py::or(value, Json::from(""))));
    Some(match (prerelease(&text(a)), prerelease(&text(b))) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(a), Some(b)) => {
            let (a, b): (Vec<&str>, Vec<&str>) = (a.split('.').collect(), b.split('.').collect());
            for (x, y) in a.iter().zip(b.iter()) {
                let order = match (x.parse::<u64>(), y.parse::<u64>()) {
                    (Ok(x), Ok(y)) => x.cmp(&y),
                    (Ok(_), Err(_)) => Ordering::Less,
                    (Err(_), Ok(_)) => Ordering::Greater,
                    (Err(_), Err(_)) => x.cmp(y),
                };
                if order != Ordering::Equal {
                    return Some(order);
                }
            }
            a.len().cmp(&b.len())
        }
    })
}

/// Whether the latest version is one this computer hasn't got. Cursor CLI's builds are a date and a commit
/// (2026.11.22-e9042b2): the install script names the newest one, so another build of the same day is newer too.
pub fn available(id: &str, latest: Option<&Json>, current: Option<&Json>) -> bool {
    if id == "cursor-cli" {
        let (a, b) = (crate::updates::release_version(latest), crate::updates::release_version(current));
        return match (a, b) {
            (Some(a), Some(b)) => a > b || (a == b && py::str(latest).trim() != py::str(current).trim()),
            _ => false,
        };
    }
    release_order(latest, current) == Some(Ordering::Greater)
}

// Channels

/// The packages a CLI is published as: its Homebrew cask, its Homebrew formula and its npm package.
fn packages(id: &str) -> (&'static str, &'static str, &'static str) {
    match id {
        "claude-code" => ("claude-code", "", "@anthropic-ai/claude-code"),
        "codex-cli" => ("codex", "codex", "@openai/codex"),
        "gemini" => ("", "gemini-cli", "@google/gemini-cli"),
        "deepseek-harness" => ("", "", "@deepseek-ai/dsh"),
        "opencode" => ("", "opencode", "opencode-ai"),
        "qoder-cli" => ("", "", "@qodercn-ai/qoderclicn"),
        "cursor-cli" => ("cursor-cli", "", ""),
        "antigravity-cli" => ("antigravity-cli", "", ""),
        _ => ("", "", ""),
    }
}

/// agy's build for this computer, as its install script names it.
pub fn agy_platform() -> String {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "windows",
        _ => "linux",
    };
    let arch = if std::env::consts::ARCH == "aarch64" { "arm64" } else { "amd64" };
    format!("{os}_{arch}")
}

/// A CLI installed by its vendor's own installer (or its own update command): the vendor's version source.
fn own_channel(id: &str) -> Option<Object> {
    let (name, kind, url) = match id {
        "claude-code" => ("Claude 官方", "text", "https://downloads.claude.ai/claude-code-releases/latest".to_string()),
        "opencode" => ("官方安装", "github", "https://api.github.com/repos/anomalyco/opencode/releases/latest".into()),
        "qoder-cli" => {
            ("官方安装", "qoder-manifest", "https://static.qoder.com.cn/qoder-cli-cn/channels/manifest.json".into())
        }
        "cursor-cli" => ("官方安装", "cursor-script", "https://cursor.com/install".into()),
        "antigravity-cli" => (
            "官方安装",
            "agy-manifest",
            format!(
                "https://antigravity-cli-auto-updater-974169037036.us-central1.run.app/manifests/{}.json",
                agy_platform()
            ),
        ),
        _ => return None,
    };
    Some(object! { "channel" => name, "channelId" => kind, "releaseURL" => url })
}

fn brew(kind: &str, token: &str) -> Object {
    let (channel, folder) =
        if kind == "brew-cask" { ("Homebrew Cask", "cask") } else { ("Homebrew Formula", "formula") };
    object! { "channel" => channel, "channelId" => kind, "releaseURL" => format!("https://formulae.brew.sh/api/{folder}/{token}.json") }
}

fn npm(package: &str) -> Object {
    object! { "channel" => "npm", "channelId" => "npm", "releaseURL" => format!("https://registry.npmjs.org/{package}/latest") }
}

fn resolved(binary: &str) -> PathBuf {
    paths::resolve(Path::new(binary)).unwrap_or_else(|| PathBuf::from(binary))
}

fn name_of(path: &Path) -> &str {
    path.file_name().and_then(|name| name.to_str()).unwrap_or("")
}

/// The package channel a CLI came from, from its executable alone: Homebrew's Caskroom or Cellar, an npm package
/// (or Windows npm's launcher beside node_modules), else its vendor's own installer.
pub fn cli_channel(id: &str, binary: &str) -> Object {
    let (cask, formula, package) = packages(id);
    let path = resolved(binary);
    let scoped = package.split_once('/');
    for parent in path.ancestors().skip(1) {
        let name = name_of(parent);
        let above = parent.parent().map(name_of).unwrap_or("");
        if !cask.is_empty() && name == cask && above == "Caskroom" {
            let receipt = claude_app::read_object(&parent.join(".metadata/INSTALL_RECEIPT.json").to_string_lossy());
            if py::dict(receipt.get("source")).get("tap") != Some(&Json::from("homebrew/cask")) {
                return object! { "channel" => "Homebrew 自定义 Cask" };
            }
            return brew("brew-cask", cask);
        }
        if !formula.is_empty() && name == formula && above == "Cellar" {
            // The receipt is in the version's folder, the one below Cellar/<formula> on the way to the program.
            let version = path.ancestors().find(|folder| folder.parent() == Some(parent));
            let receipt = version.map_or_else(Object::new, |version| {
                claude_app::read_object(&version.join("INSTALL_RECEIPT.json").to_string_lossy())
            });
            if py::dict(receipt.get("source")).get("tap") != Some(&Json::from("homebrew/core")) {
                return object! { "channel" => "Homebrew 自定义 Formula" };
            }
            return brew("brew-formula", formula);
        }
        let named = match scoped {
            Some((scope, last)) => name == last && above == scope,
            None => !package.is_empty() && name == package && above == "node_modules",
        };
        if named
            && claude_app::read_object(&parent.join("package.json").to_string_lossy()).get("name")
                == Some(&Json::from(package))
        {
            return npm(package);
        }
    }
    // A Windows npm launcher (codex.cmd) sits beside node_modules, not linked into the package.
    let batch = path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("cmd") || extension.eq_ignore_ascii_case("bat"));
    if batch
        && !package.is_empty()
        && let Some(folder) = path.parent()
    {
        let mut manifest = folder.join("node_modules");
        for part in package.split('/') {
            manifest = manifest.join(part);
        }
        if claude_app::read_object(&manifest.join("package.json").to_string_lossy()).get("name")
            == Some(&Json::from(package))
        {
            return npm(package);
        }
    }
    own_channel(id).unwrap_or_else(|| object! { "channel" => "安装渠道待确认" })
}

// Where each app's newest version is published

pub fn app_source(id: &str) -> Option<(&'static str, String)> {
    let arm = std::env::consts::ARCH == "aarch64";
    Some(match id {
        "magpie" => ("github", "https://api.github.com/repos/yetone/magpie/releases/latest".into()),
        "cursor" => (
            "cursor-api",
            format!(
                "https://cursor.com/api/download?platform=darwin-{}&releaseTrack=stable",
                if arm { "arm64" } else { "x64" }
            ),
        ),
        "antigravity" => (
            "antigravity-releases",
            "https://antigravity-hub-auto-updater-974169037036.us-central1.run.app/releases".into(),
        ),
        "antigravity-ide" => (
            "ide-feed",
            format!(
                "https://antigravity-ide-auto-updater-974169037036.us-central1.run.app/api/update/{}/stable/latest",
                if arm { "darwin-arm64" } else { "darwin" }
            ),
        ),
        "opencode-desktop" => ("github", "https://api.github.com/repos/anomalyco/opencode/releases/latest".into()),
        _ => return None,
    })
}

/// The newest version in a source's answer, by the kind of source.
pub fn latest_version(kind: &str, body: &[u8]) -> Result<String, FileError> {
    let missing = || FileError::new("KeyError");
    let text = || String::from_utf8_lossy(body).into_owned();
    match kind {
        "text" => return Ok(text().trim().to_string()),
        "cursor-script" => {
            static PATTERN: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
                regex::Regex::new(r"downloads\.cursor\.com/lab/([0-9][0-9A-Za-z.-]*)/").expect("cursor pattern")
            });
            let body = text();
            return PATTERN
                .captures(&body)
                .and_then(|found| found.get(1))
                .map(|found| found.as_str().to_string())
                .ok_or_else(missing);
        }
        _ => {}
    }
    let data = Json::parse(body).map_err(|_| FileError::new("bad metadata"))?;
    let field = |name: &str| data.get(name).filter(|value| value.is_truthy()).map(|value| py::str(Some(value)));
    match kind {
        "brew-formula" => data
            .get("versions")
            .and_then(|versions| versions.get("stable"))
            .map(|v| py::str(Some(v)))
            .ok_or_else(missing),
        // A cask's version may carry its build after a comma: "2.34.0,5887025308631040".
        "brew-cask" => {
            field("version").map(|version| version.split(',').next().unwrap_or("").to_string()).ok_or_else(missing)
        }
        "github" => field("tag_name").map(|tag| tag.strip_prefix('v').unwrap_or(&tag).to_string()).ok_or_else(missing),
        "qoder-manifest" => field("latest").ok_or_else(missing),
        "antigravity-releases" => data
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|release| release.get("version"))
            .filter(|version| crate::updates::release_version(Some(version)).is_some())
            .fold(None::<&Json>, |best, version| match best {
                Some(best) if release_order(Some(version), Some(best)) != Some(Ordering::Greater) => Some(best),
                _ => Some(version),
            })
            .map(|version| py::str(Some(version)))
            .ok_or_else(missing),
        "ide-feed" => ide_release(body).map(|(version, _)| version).ok_or_else(missing),
        _ => field("version").ok_or_else(missing),
    }
}

/// Antigravity IDE's feed (VS Code's update service): the release's version, from its download folder
/// (…/stable/2.8.0-<build>/…) or else its product version, and the commit it was built from, which tells it apart.
pub fn ide_release(body: &[u8]) -> Option<(String, String)> {
    static FOLDER: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"/stable/(\d+(?:\.\d+){1,3})-").expect("ide pattern"));
    let data = Json::parse(body).ok()?;
    let commit = data.get("version").and_then(Json::as_str).filter(|commit| !commit.is_empty())?.to_string();
    let url = data.get("url").and_then(Json::as_str).unwrap_or("");
    let version = FOLDER
        .captures(url)
        .and_then(|found| found.get(1))
        .map(|found| found.as_str().to_string())
        .or_else(|| data.get("productVersion").and_then(Json::as_str).map(String::from))?;
    Some((version, commit))
}

/// The commit an installed VS Code-style app was built from, from its product.json.
pub fn app_commit(app: &str) -> Option<String> {
    claude_app::read_object(&format!("{app}/Contents/Resources/app/product.json"))
        .get("commit")
        .and_then(Json::as_str)
        .filter(|commit| !commit.is_empty())
        .map(String::from)
}

// Updating a CLI

/// The package manager beside a CLI's package: Homebrew's brew in the prefix above Caskroom or Cellar, npm in the
/// prefix above node_modules (its bin folder, or the prefix itself on Windows); else the one on the PATH.
fn manager(binary: &str, name: &str, environment: &HashMap<String, String>) -> Option<String> {
    let path = resolved(binary);
    let folders: &[&str] = if name == "brew" { &["Caskroom", "Cellar"] } else { &["node_modules"] };
    let candidates: Vec<PathBuf> = path
        .ancestors()
        .find(|folder| folders.contains(&name_of(folder)))
        .and_then(Path::parent)
        .map(|prefix| {
            if name == "brew" {
                vec![prefix.join("bin/brew")]
            } else if name_of(prefix) == "lib" {
                prefix.parent().map(|root| vec![root.join("bin/npm")]).unwrap_or_default()
            } else {
                vec![prefix.join("npm.cmd"), prefix.join("bin/npm")]
            }
        })
        .unwrap_or_default();
    candidates
        .into_iter()
        .find(|candidate| candidate.is_file())
        .map(|candidate| candidate.to_string_lossy().into_owned())
        .or_else(|| shell::which(name, environment))
}

/// The CLI's own update command, used when it came from its vendor's installer (or from somewhere unknown, which the
/// CLI's own update tells apart itself).
fn own_update(id: &str) -> Option<&'static str> {
    match id {
        "claude-code" | "codex-cli" | "cursor-cli" | "antigravity-cli" | "qoder-cli" => Some("update"),
        "opencode" => Some("upgrade"),
        _ => None,
    }
}

/// The official command that updates a CLI by its channel, program first: brew upgrade for Homebrew, npm install -g
/// for npm, else the CLI's own update. None for a custom Homebrew tap, and for a CLI without an update of its own
/// from somewhere unknown (Gemini CLI, DeepSeek Harness).
pub fn update_plan(
    id: &str,
    binary: &str,
    channel: &Object,
    environment: &HashMap<String, String>,
) -> Option<Vec<String>> {
    let (cask, formula, package) = packages(id);
    let kind = channel.get("channelId").and_then(Json::as_str).unwrap_or("");
    let words = |program: String, args: &[&str]| -> Vec<String> {
        std::iter::once(program).chain(args.iter().map(|arg| arg.to_string())).collect()
    };
    match kind {
        "brew-cask" => Some(words(manager(binary, "brew", environment)?, &["upgrade", "--cask", cask])),
        "brew-formula" => Some(words(manager(binary, "brew", environment)?, &["upgrade", formula])),
        "npm" => Some(words(manager(binary, "npm", environment)?, &["install", "-g", &format!("{package}@latest")])),
        _ if channel.get("channel").and_then(Json::as_str).is_some_and(|name| name.starts_with("Homebrew")) => None,
        _ => own_update(id).map(|command| words(binary.to_string(), &[command])),
    }
}

/// A plan as the row shows it: the programs by name (brew upgrade --cask claude-code, claude update).
pub fn plan_text(plan: &[String]) -> String {
    let mut words = plan.iter();
    let program = words.next().map(|program| {
        let name = name_of(Path::new(program)).to_string();
        name.strip_suffix(".cmd").or_else(|| name.strip_suffix(".exe")).unwrap_or(&name).to_string()
    });
    program.into_iter().chain(words.cloned()).collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(text: &str) -> Json {
        Json::from(text)
    }

    #[test]
    fn prereleases_come_before_their_release_and_in_order() {
        let newer = |a: &str, b: &str| available("deepseek-harness", Some(&version(a)), Some(&version(b)));
        assert!(newer("0.2.2-rc.5", "0.2.2-rc.4"));
        assert!(newer("0.2.2", "0.2.2-rc.5"));
        assert!(!newer("0.2.2-rc.5", "0.2.2"));
        assert!(newer("0.2.2-rc.10", "0.2.2-rc.9"), "parts by value");
        assert!(newer("1.0.0-beta", "1.0.0-alpha.1"));
        assert!(!newer("1.2.3+build", "1.2.3"), "build metadata isn't a newer release");
        assert!(newer("2.1.330", "2.1.311"));
        assert!(!newer("garbage", "2.1.311"));
        // Cursor CLI: the same day's other build is newer, an older day isn't.
        let cursor = |a: &str, b: &str| available("cursor-cli", Some(&version(a)), Some(&version(b)));
        assert!(cursor("2026.11.22-e9042b2", "2026.11.13-eacade1"));
        assert!(cursor("2026.11.13-e9042b2", "2026.11.13-eacade1"));
        assert!(!cursor("2026.11.13-eacade1", "2026.11.13-eacade1"));
        assert!(!cursor("2026.11.12-e9042b2", "2026.11.13-eacade1"));
    }

    #[test]
    fn each_source_names_its_newest_version() {
        assert_eq!(latest_version("npm", br#"{"version": "0.70.0"}"#).unwrap(), "0.70.0");
        assert_eq!(latest_version("brew-formula", br#"{"versions": {"stable": "0.70.0"}}"#).unwrap(), "0.70.0");
        assert_eq!(latest_version("brew-cask", br#"{"version": "1.5.14,5302690847260672"}"#).unwrap(), "1.5.14");
        assert_eq!(latest_version("github", br#"{"tag_name": "v1.19.6"}"#).unwrap(), "1.19.6");
        assert_eq!(latest_version("text", b"2.1.330\n").unwrap(), "2.1.330");
        assert_eq!(latest_version("qoder-manifest", br#"{"latest": "1.1.95", "files": []}"#).unwrap(), "1.1.95");
        assert_eq!(latest_version("agy-manifest", br#"{"version": "1.5.14", "url": "x"}"#).unwrap(), "1.5.14");
        assert_eq!(latest_version("cursor-api", br#"{"version": "3.31.15", "commitSha": "3eb8"}"#).unwrap(), "3.31.15");
        let script = b"DOWNLOAD_URL=\"https://downloads.cursor.com/lab/2026.11.22-e9042b2/${OS}/${ARCH}/agent-cli-package.tar.gz\"";
        assert_eq!(latest_version("cursor-script", script).unwrap(), "2026.11.22-e9042b2");
        let releases = br#"[{"version": "2.33.1"}, {"version": "2.34.0"}, {"version": "x"}, {"version": "2.9.0"}]"#;
        assert_eq!(latest_version("antigravity-releases", releases).unwrap(), "2.34.0");
        let feed = br#"{"version": "8dc5315a", "productVersion": "1.107.0", "url": "https://edgedl.me.gvt1.com/x/antigravity/stable/2.8.0-6733682/Antigravity.zip"}"#;
        assert_eq!(ide_release(feed), Some(("2.8.0".into(), "8dc5315a".into())));
        assert_eq!(
            latest_version("ide-feed", br#"{"version": "8dc5", "productVersion": "1.107.0"}"#).unwrap(),
            "1.107.0"
        );
        assert!(latest_version("npm", b"<html>").is_err());
        assert!(latest_version("github", br#"{"message": "Not Found"}"#).is_err());
    }

    #[test]
    fn a_cli_is_updated_by_the_channel_it_came_from() {
        let root = std::env::temp_dir().join(format!("starmeter-channels-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let write = |path: &Path, text: &str| {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        };
        let environment = HashMap::new();
        // Homebrew's cask, with brew in its prefix.
        let claude = root.join("Caskroom/claude-code/2.1.311/claude");
        write(&claude, "");
        write(
            &root.join("Caskroom/claude-code/.metadata/INSTALL_RECEIPT.json"),
            r#"{"source": {"tap": "homebrew/cask"}}"#,
        );
        write(&root.join("bin/brew"), "");
        let channel = cli_channel("claude-code", &claude.to_string_lossy());
        assert_eq!(
            channel.get("releaseURL").and_then(Json::as_str),
            Some("https://formulae.brew.sh/api/cask/claude-code.json")
        );
        let plan = update_plan("claude-code", &claude.to_string_lossy(), &channel, &environment).unwrap();
        assert_eq!(plan_text(&plan), "brew upgrade --cask claude-code");
        assert_eq!(Path::new(&plan[0]), std::fs::canonicalize(root.join("bin/brew")).unwrap());
        // npm, scoped and not, with npm in the prefix's bin.
        let gemini = root.join("lib/node_modules/@google/gemini-cli/dist/index.js");
        write(&gemini, "");
        write(&root.join("lib/node_modules/@google/gemini-cli/package.json"), r#"{"name": "@google/gemini-cli"}"#);
        write(&root.join("bin/npm"), "");
        let channel = cli_channel("gemini", &gemini.to_string_lossy());
        assert_eq!(channel.get("channelId").and_then(Json::as_str), Some("npm"));
        let plan = update_plan("gemini", &gemini.to_string_lossy(), &channel, &environment).unwrap();
        assert_eq!(plan_text(&plan), "npm install -g @google/gemini-cli@latest");
        let opencode = root.join("lib/node_modules/opencode-ai/bin/opencode");
        write(&opencode, "");
        write(&root.join("lib/node_modules/opencode-ai/package.json"), r#"{"name": "opencode-ai"}"#);
        assert_eq!(
            cli_channel("opencode", &opencode.to_string_lossy()).get("channel").and_then(Json::as_str),
            Some("npm")
        );
        // The vendor's installer: its own update command, its own version source.
        let loose = root.join("home/.local/bin/cursor-agent");
        write(&loose, "");
        let channel = cli_channel("cursor-cli", &loose.to_string_lossy());
        assert_eq!(channel.get("channelId").and_then(Json::as_str), Some("cursor-script"));
        assert_eq!(
            update_plan("cursor-cli", &loose.to_string_lossy(), &channel, &environment).unwrap(),
            vec![loose.to_string_lossy().into_owned(), "update".into()]
        );
        let channel = cli_channel("opencode", &root.join("home/.opencode/bin/opencode").to_string_lossy());
        assert_eq!(channel.get("channelId").and_then(Json::as_str), Some("github"));
        // Gemini CLI from nowhere known: no channel, no command.
        let channel = cli_channel("gemini", &loose.to_string_lossy());
        assert_eq!(channel.get("channel").and_then(Json::as_str), Some("安装渠道待确认"));
        assert_eq!(update_plan("gemini", &loose.to_string_lossy(), &channel, &environment), None);
        // A Homebrew formula from another tap is told apart and has no command.
        let tapped = root.join("Cellar/opencode/2.3.2/bin/opencode");
        write(&tapped, "");
        write(&root.join("Cellar/opencode/2.3.2/INSTALL_RECEIPT.json"), r#"{"source": {"tap": "sst/tap"}}"#);
        let channel = cli_channel("opencode", &tapped.to_string_lossy());
        assert_eq!(channel.get("channel").and_then(Json::as_str), Some("Homebrew 自定义 Formula"));
        assert_eq!(update_plan("opencode", &tapped.to_string_lossy(), &channel, &environment), None);
        let _ = std::fs::remove_dir_all(&root);
    }
}
