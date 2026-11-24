//! File chores shared by every state file: atomic replacement, keeping an unreadable file aside, and paths the
//! way Python's pathlib spells them.

use std::collections::hash_map::RandomState;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::hash::{BuildHasher, Hasher};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::json::Json;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure(pub String);

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Failure {}

/// Whether data is a JSON object in this version's schema 1.
pub fn schema_one(data: &[u8]) -> bool {
    Json::parse(data).ok().and_then(|value| value.get("schema").cloned()) == Some(Json::Int(1))
}

/// Copies path aside when it exists and `readable` says no, so a fresh write loses nothing. Returns the copy's
/// path. Fails when the copy can't be made, and the caller then doesn't write.
pub fn keep_unreadable(path: &Path, readable: impl Fn(&[u8]) -> bool) -> Result<Option<PathBuf>, Failure> {
    let data = match fs::read(path) {
        Ok(data) => data,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(Failure(format!("cannot read {}", path.display()))),
    };
    if readable(&data) {
        return Ok(None);
    }
    let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let (stem, suffix) = match name.rfind('.') {
        Some(dot) if dot > 0 => (&name[..dot], &name[dot..]),
        _ => (name.as_str(), ""),
    };
    let directory = path.parent().unwrap_or(Path::new(""));
    let stamp = local_stamp();
    for attempt in 0..100 {
        let counter = if attempt > 0 { format!("-{attempt}") } else { String::new() };
        let aside = directory.join(format!("{stem}.unreadable-{stamp}{counter}{suffix}"));
        match create_exclusively(&aside, &data) {
            Creation::Created => return Ok(Some(aside)),
            Creation::Exists => continue,
            Creation::Failed(reason) => return Err(Failure(reason)),
        }
    }
    Err(Failure(format!("no free name to keep {name} aside")))
}

/// Writes data next to path under a hidden temporary name, then moves it over path in one step. `before` runs
/// once the temporary file is written, just before the move.
pub fn replace_atomically(
    path: &Path,
    data: &[u8],
    prefix: &str,
    before: impl FnOnce() -> Result<(), Failure>,
) -> Result<(), Failure> {
    let directory = path.parent().unwrap_or(Path::new(""));
    let mut temporary = None;
    for _ in 0..100 {
        let candidate = directory.join(format!("{prefix}{}", random_name()));
        match create_exclusively(&candidate, data) {
            Creation::Created => {
                temporary = Some(candidate);
                break;
            }
            Creation::Exists => continue,
            Creation::Failed(reason) => return Err(Failure(reason)),
        }
    }
    let Some(temporary) = temporary else {
        return Err(Failure(format!("no free temporary name in {}", directory.display())));
    };
    let result = before().and_then(|()| {
        fs::rename(&temporary, path).map_err(|error| Failure(format!("cannot replace {}: {error}", path.display())))
    });
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

/// Makes a directory and its parents; new ones get `mode`, as `Path.mkdir(parents=True, mode=...)`.
pub fn make_directory(path: &Path, mode: u32) -> Result<(), Failure> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, mode);
    #[cfg(not(unix))]
    let _ = mode;
    builder.create(path).map_err(|error| Failure(format!("cannot create {}: {error}", path.display())))
}

/// Readable and writable by the owner only (0600), as os.chmod; Windows keeps its own ACLs.
pub fn owner_only(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
    #[cfg(not(unix))]
    let _ = path;
}

enum Creation {
    Created,
    Exists,
    Failed(String),
}

/// Creates a new file readable only by its owner, failing if the name is taken.
fn create_exclusively(path: &Path, contents: &[u8]) -> Creation {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => return Creation::Exists,
        Err(error) => return Creation::Failed(format!("cannot create {}: {error}", path.display())),
    };
    if file.write_all(contents).is_err() {
        drop(file);
        let _ = fs::remove_file(path);
        return Creation::Failed(format!("cannot write {}", path.display()));
    }
    Creation::Created
}

fn random_name() -> String {
    const LETTERS: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789_";
    let mut bits = RandomState::new().build_hasher().finish() ^ time_nanoseconds() as u64;
    (0..8)
        .map(|_| {
            let letter = LETTERS[(bits % LETTERS.len() as u64) as usize] as char;
            bits /= LETTERS.len() as u64;
            letter
        })
        .collect()
}

/// `time.strftime('%Y%m%d-%H%M%S')` in the local zone.
fn local_stamp() -> String {
    jiff::Zoned::now().strftime("%Y%m%d-%H%M%S").to_string()
}

/// Nanoseconds since 1970, as `time.time_ns()`.
pub fn time_nanoseconds() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_nanos() as i64)
}

/// Where things are, the way the Python reference looks them up.
pub mod paths {
    use std::path::{Path, PathBuf};

    /// The home directory `os.path.expanduser` uses: HOME (USERPROFILE on Windows) when set.
    pub fn home() -> PathBuf {
        let variable = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
        match std::env::var_os(variable) {
            Some(home) if !home.is_empty() => PathBuf::from(home),
            _ => std::env::home_dir().unwrap_or_default(),
        }
    }

    /// `Path.expanduser()`: a leading ~ or ~/ becomes the home directory.
    pub fn expand_user(path: &str) -> PathBuf {
        if path == "~" {
            return home();
        }
        let rest = path.strip_prefix("~/").or_else(|| if cfg!(windows) { path.strip_prefix("~\\") } else { None });
        match rest {
            Some(rest) => home().join(rest),
            None => PathBuf::from(path),
        }
    }

    /// `Path.resolve()` of a path that exists: absolute, with links followed. None when it doesn't resolve.
    pub fn resolve(path: &Path) -> Option<PathBuf> {
        let resolved = std::fs::canonicalize(path).ok()?;
        // Windows gives \\?\C:\..., which the Python reference never shows.
        #[cfg(windows)]
        if let Some(plain) = resolved.to_str().and_then(without_verbatim) {
            return Some(PathBuf::from(plain));
        }
        Some(resolved)
    }

    /// A Windows path without its \\?\ prefix: \\?\C:\x is C:\x, and a share (\\?\UNC\host\share, as WSL's
    /// \\wsl.localhost\Ubuntu comes back) is \\host\share again, not the relative UNC\host\share.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(crate) fn without_verbatim(text: &str) -> Option<String> {
        if let Some(share) = text.strip_prefix(r"\\?\UNC\") {
            return Some(format!(r"\\{share}"));
        }
        text.strip_prefix(r"\\?\").map(String::from)
    }
}

#[cfg(test)]
mod tests {
    use super::paths::without_verbatim;

    #[test]
    fn a_share_keeps_its_two_backslashes_without_the_verbatim_prefix() {
        assert_eq!(without_verbatim(r"\\?\C:\Users\me\x").as_deref(), Some(r"C:\Users\me\x"));
        // WSL's home, as canonicalize gives it back: a share, not the relative UNC\wsl.localhost\….
        assert_eq!(
            without_verbatim(r"\\?\UNC\wsl.localhost\Ubuntu\home\me\.claude").as_deref(),
            Some(r"\\wsl.localhost\Ubuntu\home\me\.claude")
        );
        assert_eq!(without_verbatim(r"\\?\UNC\wsl$\Ubuntu\home").as_deref(), Some(r"\\wsl$\Ubuntu\home"));
        assert_eq!(without_verbatim(r"C:\plain"), None);
    }
}
