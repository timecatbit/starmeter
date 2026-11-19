//! Windows: Claude Code and Codex run inside WSL keep their records in the Linux homes of those distributions. The
//! homes of the distributions that are running are read through Windows' own share (\\wsl.localhost, or \\wsl$ on
//! older WSL) beside the Windows ones; a distribution that isn't running is never started for it. The list is asked
//! of wsl.exe at most every five minutes.

/// The distributions `wsl.exe -l -q` lists: UTF-16 text (with or without a byte-order mark), one name a line.
pub fn distributions(output: &[u8]) -> Vec<String> {
    let units: Vec<u16> = output.as_chunks::<2>().0.iter().map(|pair| u16::from_le_bytes(*pair)).collect();
    let wide = output.starts_with(&[0xff, 0xfe]) || (output.len() >= 2 && output[1] == 0);
    let text = if wide {
        String::from_utf16_lossy(&units)
    } else {
        // Some setups (WSL_UTF8=1) answer in UTF-8.
        String::from_utf8_lossy(output).into_owned()
    };
    text.trim_start_matches('\u{feff}')
        .lines()
        .map(|line| line.trim_matches(|c: char| c.is_whitespace() || c == '\0'))
        .filter(|line| !line.is_empty())
        .map(String::from)
        .collect()
}

/// The Linux homes under one distribution's share: every folder in /home, and /root.
pub fn homes_in(share: &str) -> Vec<String> {
    let mut homes: Vec<String> = std::fs::read_dir(format!("{share}/home"))
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
                .map(|entry| format!("{share}/home/{}", entry.file_name().to_string_lossy()))
                .collect()
        })
        .unwrap_or_default();
    homes.sort();
    let root = format!("{share}/root");
    if std::path::Path::new(&root).is_dir() {
        homes.push(root);
    }
    homes
}

/// The Linux homes of the running distributions, as Windows paths; none outside Windows.
pub fn homes() -> Vec<String> {
    #[cfg(windows)]
    {
        use std::sync::Mutex;
        use std::time::{Duration, Instant};
        static CACHE: Mutex<Option<(Instant, Vec<String>)>> = Mutex::new(None);
        let mut cache = CACHE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some((at, homes)) = cache.as_ref()
            && at.elapsed() < Duration::from_secs(300)
        {
            return homes.clone();
        }
        let running = crate::shell::run_data(&["wsl.exe", "-l", "-q", "--running"], 5.0, None, None)
            .map(|output| distributions(&output))
            .unwrap_or_default();
        let mut homes = Vec::new();
        for distribution in running {
            let share = [format!(r"\\wsl.localhost\{distribution}"), format!(r"\\wsl$\{distribution}")]
                .into_iter()
                .find(|share| std::path::Path::new(share).is_dir());
            if let Some(share) = share {
                homes.extend(homes_in(&share));
            }
        }
        *cache = Some((Instant::now(), homes.clone()));
        homes
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

/// Claude Code's transcript folders inside the running distributions.
pub fn claude_projects() -> Vec<String> {
    homes()
        .into_iter()
        .flat_map(|home| [format!("{home}/.claude/projects"), format!("{home}/.config/claude/projects")])
        .filter(|folder| std::path::Path::new(folder).is_dir())
        .collect()
}

/// Codex's homes inside the running distributions.
pub fn codex_homes() -> Vec<String> {
    homes()
        .into_iter()
        .map(|home| format!("{home}/.codex"))
        .filter(|folder| std::path::Path::new(folder).is_dir())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16(text: &str, mark: bool) -> Vec<u8> {
        let mut bytes = if mark { vec![0xff, 0xfe] } else { vec![] };
        for unit in text.encode_utf16() {
            bytes.extend(unit.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn names_from_wsl_in_either_encoding() {
        assert_eq!(distributions(&utf16("Ubuntu\r\nDebian\r\n\r\n", true)), ["Ubuntu", "Debian"]);
        assert_eq!(distributions(&utf16("Ubuntu-24.04\r\n", false)), ["Ubuntu-24.04"]);
        assert_eq!(distributions(b"Ubuntu\nkali-linux\n"), ["Ubuntu", "kali-linux"]);
        assert!(distributions(&[]).is_empty());
    }

    #[test]
    fn every_home_and_root() {
        let share = std::env::temp_dir().join(format!("starmeter-wsl-{}", std::process::id()));
        for folder in ["home/ann", "home/bob", "root", "etc"] {
            std::fs::create_dir_all(share.join(folder)).unwrap();
        }
        std::fs::write(share.join("home/notes.txt"), "").unwrap();
        let share_text = share.to_string_lossy().replace('\\', "/");
        let found = homes_in(&share_text);
        let _ = std::fs::remove_dir_all(&share);
        assert_eq!(
            found,
            [format!("{share_text}/home/ann"), format!("{share_text}/home/bob"), format!("{share_text}/root")]
        );
    }
}
