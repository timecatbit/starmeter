//! The standalone app's own update (应用内升级): this computer's package of the newer release, checked against the
//! SHA-256 GitHub computed for it, put in place of the running app once it has quit, and the app opened again. The
//! settings, keys and usage records live in the app's data folder and stay as they are.
//!
//! - macOS: the release's DMG for this chip, mounted read-only; its Starmeter.app is copied next to the installed one,
//!   its signature verified and its identifier and version checked, then swapped in by a small helper.
//! - Windows: the installer, run silently once the app has quit (it installs for this user, no administrator), then
//!   the app is started again. A portable folder isn't replaced: its row links to the release.
//! - Linux: an AppImage is replaced by the new one (renamed over it) and started again. A .deb isn't: its row links to
//!   the release, as replacing it needs an administrator.
//!
//! Paths go to the helper as data, in environment variables, never into its script.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::json::Json;
use crate::runtime::{self, Runtime, VERSION};
use crate::{files, object, shell, updates};

/// At most this large a package (an AppImage is the largest), downloaded within this long.
const PACKAGE_LIMIT: usize = 300 * 1024 * 1024;
const PACKAGE_TIME: f64 = 600.0;

/// What replaces the running app: the installed copy (`target`) and what takes its place.
#[derive(Debug, PartialEq)]
pub enum Prepared {
    /// A checked Starmeter.app copied next to the installed bundle.
    Mac { staged: PathBuf, target: PathBuf },
    /// The checked installer, and the program to start again once it has run.
    Windows { setup: PathBuf, target: PathBuf },
    /// A checked AppImage next to the running one.
    Linux { staged: PathBuf, target: PathBuf },
}

/// What can be replaced in place: the app bundle on macOS, the program an installer put in place on Windows (an
/// uninstaller beside it), the AppImage on Linux. None for anything else (a .deb, a portable folder, a build run from
/// a source checkout without a bundle).
pub fn replaceable(executable: &Path, appimage: Option<&str>) -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        // …/Starmeter.app/Contents/MacOS/Starmeter
        let bundle = executable.parent()?.parent()?.parent()?;
        return (bundle.extension().is_some_and(|ext| ext == "app") && bundle.is_dir()).then(|| bundle.to_path_buf());
    }
    if cfg!(windows) {
        return executable.parent()?.join("unins000.exe").is_file().then(|| executable.to_path_buf());
    }
    let image = PathBuf::from(appimage.filter(|path| !path.is_empty())?);
    image.is_file().then_some(image)
}

/// The running app's replaceable copy, if any.
pub fn this_app() -> Option<PathBuf> {
    replaceable(&std::env::current_exe().ok()?, std::env::var("APPIMAGE").ok().as_deref())
}

/// The release file an update in place of this app takes.
pub fn package(version: &str) -> String {
    if cfg!(target_os = "macos") {
        let chip = if updates::machine() == "arm64" { "apple-silicon" } else { "intel" };
        format!("starmeter-{version}-macos-{chip}.dmg")
    } else if cfg!(windows) {
        format!("starmeter-{version}-windows-x64-setup.exe")
    } else {
        format!("starmeter-{version}-linux-x64.AppImage")
    }
}

/// A field of a bundle's Info.plist.
fn info(bundle: &Path, key: &str) -> Option<String> {
    let plist = bundle.join("Contents/Info.plist");
    let text = shell::run(
        &["/usr/bin/plutil", "-extract", key, "raw", "-o", "-", &plist.to_string_lossy()],
        10.0,
        None,
        None,
    )?;
    Some(text.trim().to_string())
}

/// The DMG's Starmeter.app, copied next to `target` and checked: its signature intact, the same identifier as the
/// installed app, and the version asked for. Anything wrong removes the copy.
fn stage_mac(dmg: &[u8], target: &Path, version: &str, work: &Path) -> Result<PathBuf, String> {
    let image = work.join("starmeter.dmg");
    std::fs::write(&image, dmg).map_err(|_| "安装失败：写入临时文件失败".to_string())?;
    let mount = work.join("mount");
    files::make_directory(&mount, 0o700).map_err(|_| "安装失败：写入临时文件失败".to_string())?;
    let (image_text, mount_text) = (image.to_string_lossy().into_owned(), mount.to_string_lossy().into_owned());
    shell::run(
        &[
            "/usr/bin/hdiutil",
            "attach",
            "-nobrowse",
            "-readonly",
            "-noautoopen",
            "-mountpoint",
            &mount_text,
            &image_text,
        ],
        120.0,
        None,
        None,
    )
    .ok_or_else(|| "安装包打不开（DMG 挂载失败）".to_string())?;
    let parent = target.parent().ok_or_else(|| "找不到 App 所在的文件夹".to_string())?;
    let staged = parent.join(format!(".Starmeter-{version}-{}.app", runtime::random_hex(6)));
    let copied = shell::run(
        &["/usr/bin/ditto", &mount.join("Starmeter.app").to_string_lossy(), &staged.to_string_lossy()],
        300.0,
        None,
        None,
    )
    .is_some();
    let _ = shell::run(&["/usr/bin/hdiutil", "detach", &mount_text, "-quiet"], 60.0, None, None);
    if !copied {
        let _ = std::fs::remove_dir_all(&staged);
        return Err(format!("不能写入 {}，请从发布页下载安装", parent.display()));
    }
    let checked = (|| {
        shell::run(
            &["/usr/bin/codesign", "--verify", "--deep", "--strict", &staged.to_string_lossy()],
            120.0,
            None,
            None,
        )
        .ok_or_else(|| "新版本的签名校验不通过".to_string())?;
        if info(&staged, "CFBundleIdentifier").is_none()
            || info(&staged, "CFBundleIdentifier") != info(target, "CFBundleIdentifier")
        {
            return Err("安装包里的 App 不是 Starmeter".to_string());
        }
        if info(&staged, "CFBundleShortVersionString").as_deref() != Some(version) {
            return Err(format!("安装包里的版本不是 {version}"));
        }
        Ok(())
    })();
    if let Err(reason) = checked {
        let _ = std::fs::remove_dir_all(&staged);
        return Err(reason);
    }
    Ok(staged)
}

impl Runtime {
    /// The release file an update in place of this app would take, when this app can be replaced in place and the
    /// release has it; None leaves the row a link to the release.
    pub fn app_package(&self, version: &str, assets: &[Json]) -> Option<String> {
        if !(self.mac_app || self.is_desktop()) || this_app().is_none() {
            return None;
        }
        let name = package(version);
        assets.iter().any(|asset| asset.get("name") == Some(&Json::from(name.as_str()))).then_some(name)
    }

    /// Downloads and checks this app's package of `version` and readies it to replace `target`.
    pub fn prepare_app_update(&self, version: &str, target: &Path) -> Result<Prepared, String> {
        let name = package(version);
        let data = self.release_asset(version, &name, PACKAGE_LIMIT, PACKAGE_TIME)?;
        if cfg!(target_os = "macos") {
            let work = std::env::temp_dir().join(format!("starmeter-app-update-{}", runtime::random_hex(12)));
            files::make_directory(&work, 0o700).map_err(|_| "安装失败：写入临时文件失败".to_string())?;
            let staged = stage_mac(&data, target, version, &work);
            let _ = std::fs::remove_dir_all(&work);
            return staged.map(|staged| Prepared::Mac { staged, target: target.to_path_buf() });
        }
        let folder = if cfg!(windows) {
            std::env::temp_dir()
        } else {
            target.parent().map(Path::to_path_buf).ok_or_else(|| "找不到 AppImage 所在的文件夹".to_string())?
        };
        let staged = folder.join(format!(".{}-{}", runtime::random_hex(6), name));
        files::replace_atomically(&staged, &data, ".starmeter-", || Ok(()))
            .map_err(|_| format!("不能写入 {}，请从发布页下载安装", folder.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755));
        }
        Ok(if cfg!(windows) {
            Prepared::Windows { setup: staged, target: target.to_path_buf() }
        } else {
            Prepared::Linux { staged, target: target.to_path_buf() }
        })
    }

    /// The whole update in place: readied, then handed to the helper that finishes it once this process (`pid`) has
    /// quit. The caller quits the app after Ok. The state and log are the plugin's own update's.
    pub fn update_app_in_place(&self, version: &str, pid: u32) -> Result<(), String> {
        let target = this_app().ok_or_else(|| "这份 Starmeter 不能在原处更新，请从发布页下载".to_string())?;
        self.write_self_update_state(object! { "status" => "running", "version" => version });
        let outcome = self.prepare_app_update(version, &target).and_then(|prepared| finish(&prepared, pid, None));
        let log = format!("{}/logs/self-update.log", self.state_dir);
        if let Some(parent) = Path::new(&log).parent() {
            let _ = files::make_directory(parent, 0o700);
        }
        let told = match &outcome {
            Ok(()) => format!(
                "Starmeter {VERSION} → {version}\n已校验并交给更新程序，App 退出后替换并重新打开 {}\n",
                target.display()
            ),
            Err(reason) => format!("Starmeter {VERSION} → {version}\n失败：{reason}\n"),
        };
        let _ = files::replace_atomically(Path::new(&log), told.as_bytes(), ".self-update-log-", || Ok(()));
        match &outcome {
            Ok(()) => self.write_self_update_state(object! { "status" => "done", "version" => version }),
            Err(reason) => self.write_self_update_state(
                object! { "status" => "failed", "version" => version, "reason" => reason.as_str() },
            ),
        }
        outcome
    }
}

/// The helper's script: wait for the app to quit (at most a minute), put the new copy in place (the old one back if
/// that fails), open it. Every path comes from the environment.
const MAC_HELPER: &str = r#"i=0
while kill -0 "$STARMETER_PID" 2>/dev/null && [ "$i" -lt 600 ]; do sleep 0.1; i=$((i+1)); done
old="$STARMETER_TARGET.old-$$"
if mv "$STARMETER_TARGET" "$old"; then
  if mv "$STARMETER_STAGED" "$STARMETER_TARGET"; then rm -rf "$old"; else mv "$old" "$STARMETER_TARGET"; fi
fi
rm -rf "$STARMETER_STAGED"
"$STARMETER_OPEN" "$STARMETER_TARGET""#;

const LINUX_HELPER: &str = r#"i=0
while kill -0 "$STARMETER_PID" 2>/dev/null && [ "$i" -lt 600 ]; do sleep 0.1; i=$((i+1)); done
mv -f "$STARMETER_STAGED" "$STARMETER_TARGET" || rm -f "$STARMETER_STAGED"
setsid "$STARMETER_OPEN" "$STARMETER_TARGET" >/dev/null 2>&1 < /dev/null &"#;

const WINDOWS_HELPER: &str = "try { Wait-Process -Id ([int]$env:STARMETER_PID) -Timeout 60 -ErrorAction SilentlyContinue } catch {}; \
     Start-Process -Wait -FilePath $env:STARMETER_SETUP -ArgumentList '/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART'; \
     Remove-Item -LiteralPath $env:STARMETER_SETUP -ErrorAction SilentlyContinue; \
     Start-Process -FilePath $env:STARMETER_TARGET";

/// Starts the helper, detached, to finish `prepared` once process `pid` has quit. `open` is what opens the app again
/// (macOS's open, the AppImage itself on Linux): a test gives its own.
pub fn finish(prepared: &Prepared, pid: u32, open: Option<&str>) -> Result<(), String> {
    let pid = pid.to_string();
    let mut command = match prepared {
        Prepared::Mac { staged, target } => {
            let mut command = Command::new("/bin/sh");
            command
                .args(["-c", MAC_HELPER])
                .envs([("STARMETER_STAGED", staged.as_os_str()), ("STARMETER_TARGET", target.as_os_str())]);
            command.env("STARMETER_OPEN", open.unwrap_or("/usr/bin/open"));
            command
        }
        Prepared::Linux { staged, target } => {
            let mut command = Command::new("/bin/sh");
            command
                .args(["-c", LINUX_HELPER])
                .envs([("STARMETER_STAGED", staged.as_os_str()), ("STARMETER_TARGET", target.as_os_str())]);
            command.env("STARMETER_OPEN", open.unwrap_or("env"));
            command
        }
        Prepared::Windows { setup, target } => {
            let mut command = Command::new("powershell.exe");
            command
                .args([
                    "-NoProfile",
                    "-ExecutionPolicy",
                    "Bypass",
                    "-WindowStyle",
                    "Hidden",
                    "-Command",
                    WINDOWS_HELPER,
                ])
                .envs([("STARMETER_SETUP", setup.as_os_str()), ("STARMETER_TARGET", target.as_os_str())]);
            command
        }
    };
    command.env("STARMETER_PID", &pid).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW
        command.creation_flags(0x0000_0008 | 0x0000_0200 | 0x0800_0000);
    }
    command.spawn().map(|_| ()).map_err(|_| "更新程序没能启动".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(tag: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("starmeter-app-update-test-{tag}-{}", runtime::random_hex(6)));
        std::fs::create_dir_all(&folder).unwrap();
        folder
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_bundle_is_what_an_app_inside_it_replaces() {
        let root = folder("bundle");
        let program = root.join("Starmeter.app/Contents/MacOS/Starmeter");
        std::fs::create_dir_all(program.parent().unwrap()).unwrap();
        std::fs::write(&program, "").unwrap();
        assert_eq!(replaceable(&program, None), Some(root.join("Starmeter.app")));
        // A build run from the source checkout has no bundle around it.
        assert_eq!(replaceable(&root.join("target/debug/starmeter-app"), None), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn the_helper_puts_the_new_copy_in_place_once_the_app_has_quit_and_opens_it() {
        let root = folder("helper");
        let (target, staged) = (root.join("Starmeter.app"), root.join(".Starmeter-new.app"));
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("version"), "old").unwrap();
        std::fs::create_dir_all(&staged).unwrap();
        std::fs::write(staged.join("version"), "new").unwrap();
        // The app: a process that quits a moment later. The opener: a script that notes what it was asked to open.
        let mut app = Command::new("/bin/sleep").arg("1").spawn().unwrap();
        let opened = root.join("opened");
        let opener = root.join("open");
        std::fs::write(&opener, format!("#!/bin/sh\necho \"$1\" > '{}'\n", opened.display())).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&opener, std::fs::Permissions::from_mode(0o755)).unwrap();
        let prepared = if cfg!(target_os = "macos") {
            Prepared::Mac { staged: staged.clone(), target: target.clone() }
        } else {
            Prepared::Linux { staged: staged.clone(), target: target.clone() }
        };
        finish(&prepared, app.id(), Some(&opener.to_string_lossy())).expect("the helper starts");
        // Not before the app has quit.
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert_eq!(std::fs::read_to_string(target.join("version")).unwrap(), "old");
        let _ = app.wait();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !opened.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        assert_eq!(std::fs::read_to_string(target.join("version")).unwrap(), "new");
        assert!(!staged.exists());
        assert_eq!(std::fs::read_to_string(&opened).unwrap().trim(), target.to_string_lossy());
        let leftovers: Vec<_> = std::fs::read_dir(&root)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().contains(".old-"))
            .collect();
        assert!(leftovers.is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The real thing, over the network (run with --ignored): this chip's DMG of v6.0.14 from GitHub, checked against
    /// GitHub's SHA-256, mounted, its app copied next to a copy of the installed app (marked 6.0.13), its signature,
    /// identifier and version checked, and put in place by the helper once a stand-in for the app has quit.
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore]
    fn the_release_dmg_replaces_a_copy_of_the_app() {
        let installed = Path::new("/Applications/Starmeter.app");
        if !installed.is_dir() {
            return;
        }
        let version = "6.0.14";
        let answer = crate::http::send(
            &format!("https://api.github.com/repos/timecatbit/starmeter/releases/tags/v{version}"),
            crate::http::Request {
                headers: vec![("User-Agent", "Starmeter-test".into())],
                timeout: 30.0,
                limit: 4 << 20,
                ..Default::default()
            },
        )
        .expect("GitHub answers");
        let release = Json::parse(&answer.body).expect("a release");
        let assets: Vec<Json> = release
            .get("assets")
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
            .map(|asset| {
                object! {
                    "name" => asset.get("name").cloned().unwrap_or(Json::Null),
                    "url" => asset.get("browser_download_url").cloned().unwrap_or(Json::Null),
                    "digest" => asset.get("digest").cloned().unwrap_or(Json::Null),
                }
                .into()
            })
            .collect();
        let release = object! { "latest" => version, "assets" => Json::Array(assets) };
        let data = crate::self_update::checked_asset(&release, version, &package(version), PACKAGE_LIMIT, PACKAGE_TIME)
            .expect("downloaded and checked");
        let root = folder("real");
        let target = root.join("Starmeter.app");
        shell::run(&["/usr/bin/ditto", &installed.to_string_lossy(), &target.to_string_lossy()], 120.0, None, None)
            .expect("a copy of the installed app");
        let plist = target.join("Contents/Info.plist").to_string_lossy().into_owned();
        shell::run(
            &["/usr/bin/plutil", "-replace", "CFBundleShortVersionString", "-string", "6.0.13", &plist],
            10.0,
            None,
            None,
        )
        .expect("marked older");
        let work = root.join("work");
        std::fs::create_dir_all(&work).unwrap();
        let staged = stage_mac(&data, &target, version, &work).expect("staged and checked");
        assert_eq!(info(&staged, "CFBundleShortVersionString").as_deref(), Some(version));
        let mut app = Command::new("/bin/sleep").arg("1").spawn().unwrap();
        let opened = root.join("opened");
        let opener = root.join("open");
        std::fs::write(&opener, format!("#!/bin/sh\necho \"$1\" > '{}'\n", opened.display())).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&opener, std::fs::Permissions::from_mode(0o755)).unwrap();
        finish(&Prepared::Mac { staged, target: target.clone() }, app.id(), Some(&opener.to_string_lossy()))
            .expect("helper");
        let _ = app.wait();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while !opened.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        assert_eq!(info(&target, "CFBundleShortVersionString").as_deref(), Some(version));
        assert!(
            shell::run(
                &["/usr/bin/codesign", "--verify", "--deep", "--strict", &target.to_string_lossy()],
                120.0,
                None,
                None
            )
            .is_some()
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn each_platform_takes_its_own_package() {
        let name = package("6.1.0");
        if cfg!(target_os = "macos") {
            assert!(name == "starmeter-6.1.0-macos-apple-silicon.dmg" || name == "starmeter-6.1.0-macos-intel.dmg");
        } else if cfg!(windows) {
            assert_eq!(name, "starmeter-6.1.0-windows-x64-setup.exe");
        } else {
            assert_eq!(name, "starmeter-6.1.0-linux-x64.AppImage");
        }
    }
}
