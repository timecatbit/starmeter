//! Running a helper program the engine relies on (pmset, security, the codex CLI…), with a time limit.

use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

/// The program's standard output, or None when it could not run, failed, or ran past the timeout.
pub fn run(
    arguments: &[&str],
    timeout: f64,
    environment: Option<&[(String, String)]>,
    input: Option<&[u8]>,
) -> Option<String> {
    run_data(arguments, timeout, environment, input).map(|data| String::from_utf8_lossy(&data).into_owned())
}

pub fn run_data(
    arguments: &[&str],
    timeout: f64,
    environment: Option<&[(String, String)]>,
    input: Option<&[u8]>,
) -> Option<Vec<u8>> {
    let (program, rest) = arguments.split_first()?;
    let mut command = Command::new(program);
    command.args(rest).stdout(Stdio::piped()).stderr(Stdio::null());
    command.stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() });
    // On Windows a console program started from the app (a window, no console of its own) would flash a console
    // window: CREATE_NO_WINDOW.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    if let Some(environment) = environment {
        command.env_clear().envs(environment.iter().map(|(key, value)| (key, value)));
    }
    let mut child = command.spawn().ok()?;
    if let (Some(input), Some(mut stdin)) = (input, child.stdin.take()) {
        let input = input.to_vec();
        std::thread::spawn(move || {
            let _ = stdin.write_all(&input);
        });
    }
    let mut stdout = child.stdout.take()?;
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut data = Vec::new();
        let _ = stdout.read_to_end(&mut data);
        let _ = sender.send(data);
    });
    let Ok(data) = receiver.recv_timeout(Duration::from_secs_f64(timeout.max(0.0))) else {
        let _ = child.kill();
        let _ = child.wait();
        return None;
    };
    child.wait().ok()?.success().then_some(data)
}

/// Starts a program in its own session with no terminal attached, as `Popen(start_new_session=True)` does; it
/// outlives this run.
pub fn detach(arguments: &[&str], environment: Option<&[(String, String)]>) {
    let Some((program, rest)) = arguments.split_first() else { return };
    let mut command = Command::new(program);
    command.args(rest).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    if let Some(environment) = environment {
        command.env_clear().envs(environment.iter().map(|(key, value)| (key, value)));
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: setsid is async-signal-safe, which is all pre_exec allows.
        unsafe {
            command.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW
        command.creation_flags(0x0000_0008 | 0x0000_0200 | 0x0800_0000);
    }
    let _ = command.spawn();
}

/// The first executable of that name on PATH, as shutil.which.
pub fn which(name: &str, environment: &std::collections::HashMap<String, String>) -> Option<String> {
    which_on(name, environment, cfg!(windows))
}

/// On Windows the variable is as often Path as PATH (the system reads names in any case), and only a file with a
/// program's extension runs: npm puts a POSIX script `codex` beside `codex.cmd`, and the script, found first, could
/// not run, so the version read failed; nor was a CLI on Path found at all.
fn which_on(name: &str, environment: &std::collections::HashMap<String, String>, windows: bool) -> Option<String> {
    let (separator, suffixes): (char, &[&str]) = if windows { (';', &[".exe", ".cmd", ".bat"]) } else { (':', &[""]) };
    let named = |key: &&String| if windows { key.eq_ignore_ascii_case("PATH") } else { *key == "PATH" };
    let path = environment.iter().find(|(key, _)| named(key)).map_or("", |(_, value)| value.as_str());
    for folder in path.split(separator).filter(|folder| !folder.is_empty()) {
        for suffix in suffixes {
            let candidate = format!("{folder}/{name}{suffix}");
            if executable(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

/// A file (not a folder) this user may run.
pub fn is_executable(path: &std::path::Path) -> bool {
    executable(&path.to_string_lossy())
}

fn executable(path: &str) -> bool {
    std::fs::metadata(path).is_ok_and(|metadata| !metadata.is_dir() && runnable(&metadata))
}

#[cfg(unix)]
fn runnable(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn runnable(_metadata: &std::fs::Metadata) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    /// A folder with the files npm leaves for a CLI on Windows: the POSIX script and the batch file beside it.
    fn npm_folder(tag: &str) -> std::path::PathBuf {
        let folder = std::env::temp_dir().join(format!("starmeter-which-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        for name in ["codex", "codex.cmd"] {
            let file = folder.join(name);
            std::fs::write(&file, "").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
        folder
    }

    #[test]
    fn on_windows_the_batch_file_is_found_beside_the_script_on_path_in_any_case() {
        let folder = npm_folder("windows");
        let at = folder.to_string_lossy().into_owned();
        let found = format!("{at}/codex.cmd");
        for key in ["PATH", "Path", "path"] {
            let environment = HashMap::from([(key.to_string(), format!("C:\\nothing;{at}"))]);
            assert_eq!(super::which_on("codex", &environment, true), Some(found.clone()), "{key}");
        }
        assert_eq!(super::which_on("codex", &HashMap::new(), true), None);
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// Unix only: its PATH separator is the colon a Windows folder starts with.
    #[cfg(unix)]
    #[test]
    fn elsewhere_the_name_itself_runs_and_only_path_is_read() {
        let folder = npm_folder("unix");
        let at = folder.to_string_lossy().into_owned();
        let environment = HashMap::from([("PATH".to_string(), format!("/nothing:{at}"))]);
        assert_eq!(super::which_on("codex", &environment, false), Some(format!("{at}/codex")));
        let environment = HashMap::from([("Path".to_string(), at.clone())]);
        assert_eq!(super::which_on("codex", &environment, false), None);
        let _ = std::fs::remove_dir_all(&folder);
    }
}
