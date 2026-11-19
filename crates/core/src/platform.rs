//! The few operating-system calls the engine makes, the same on macOS, Linux and Windows: file locks, links and
//! renames, process ids and whether a process is alive.

use std::fs::{self, File, OpenOptions};
use std::path::Path;
use std::time::UNIX_EPOCH;

/// An exclusive lock on a file (flock on macOS and Linux, as 5.x takes it); released when dropped, and by the
/// system if the process dies.
pub struct Lock {
    _file: File,
}

/// Takes the lock on path (created if missing); None when another process holds it and `wait` is false.
pub fn lock(path: &Path, wait: bool) -> Option<Lock> {
    let mut options = OpenOptions::new();
    options.read(true).append(true).create(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let file = options.open(path).ok()?;
    let locked = if wait { file.lock().is_ok() } else { file.try_lock().is_ok() };
    locked.then_some(Lock { _file: file })
}

/// Removes a file if it is there; a missing file or a failure is not an error.
pub fn remove(path: &Path) {
    let _ = fs::remove_file(path);
}

/// Moves source over destination in one step; false when it couldn't.
pub fn replace(source: &Path, destination: &Path) -> bool {
    fs::rename(source, destination).is_ok()
}

/// A second name for an existing file; fails if the target exists. The way to publish a file only once.
pub fn hard_link(source: &Path, target: &Path) -> bool {
    fs::hard_link(source, target).is_ok()
}

/// Owner-only permissions on POSIX; Windows keeps the folder's own access rules.
pub fn set_mode(path: &Path, mode: u32) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(mode));
    }
    #[cfg(not(unix))]
    let _ = (path, mode);
}

pub fn process_id() -> u32 {
    std::process::id()
}

/// The user part of the cache folder's name: the uid, or "user" where there is none, as Python's getuid fallback.
pub fn user_id() -> String {
    #[cfg(unix)]
    {
        // SAFETY: getuid has no preconditions and cannot fail.
        unsafe { libc::getuid() }.to_string()
    }
    #[cfg(not(unix))]
    {
        "user".to_string()
    }
}

/// Whether a process with this id is running.
pub fn alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        // SAFETY: signal 0 only checks that the process exists and may be signalled.
        unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
        use windows_sys::Win32::System::Threading::{
            GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };
        // SAFETY: the handle is checked before use and closed once.
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle.is_null() {
                return false;
            }
            let mut code = 0u32;
            let running = GetExitCodeProcess(handle, &mut code) != 0 && code == STILL_ACTIVE as u32;
            CloseHandle(handle);
            running
        }
    }
}

/// When a file was last changed, in seconds since 1970, without following a link at path itself.
pub fn modified(path: &Path) -> Option<f64> {
    let time = fs::symlink_metadata(path).ok()?.modified().ok()?;
    Some(time.duration_since(UNIX_EPOCH).ok()?.as_secs_f64())
}
