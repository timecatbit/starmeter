//! What the terminal can show: its width, whether it takes colour, the system's language, and how wide a piece of
//! text is on it (Chinese takes two columns).

use std::io::IsTerminal;

/// The columns to lay out in: COLUMNS when set, else the terminal's own width, else 80 (a pipe or a file).
pub fn columns() -> usize {
    if let Some(given) = std::env::var("COLUMNS").ok().and_then(|value| value.trim().parse::<usize>().ok())
        && given > 0
    {
        return given;
    }
    measured().filter(|width| *width > 0).unwrap_or(80)
}

#[cfg(unix)]
fn measured() -> Option<usize> {
    if !std::io::stdout().is_terminal() {
        return None;
    }
    // SAFETY: TIOCGWINSZ fills the winsize it is given; the struct is plain data.
    unsafe {
        let mut size: libc::winsize = std::mem::zeroed();
        (libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut size) == 0).then_some(size.ws_col as usize)
    }
}

#[cfg(windows)]
fn measured() -> Option<usize> {
    use windows_sys::Win32::System::Console::{
        CONSOLE_SCREEN_BUFFER_INFO, GetConsoleScreenBufferInfo, GetStdHandle, STD_OUTPUT_HANDLE,
    };
    // SAFETY: the console fills the buffer info it is given; a handle that isn't a console fails the call.
    unsafe {
        let mut info: CONSOLE_SCREEN_BUFFER_INFO = std::mem::zeroed();
        (GetConsoleScreenBufferInfo(GetStdHandle(STD_OUTPUT_HANDLE), &mut info) != 0)
            .then(|| (info.srWindow.Right - info.srWindow.Left + 1).max(0) as usize)
    }
}

#[cfg(not(any(unix, windows)))]
fn measured() -> Option<usize> {
    None
}

/// Whether to colour: never into a pipe, with NO_COLOR set or on a dumb terminal, unless asked for.
pub fn colour(choice: &str) -> bool {
    match choice {
        "always" => {
            enable_colour();
            true
        }
        "never" => false,
        _ => {
            std::io::stdout().is_terminal()
                && std::env::var_os("NO_COLOR").is_none_or(|value| value.is_empty())
                && std::env::var("TERM").map_or(true, |term| term != "dumb")
                && enable_colour()
        }
    }
}

#[cfg(windows)]
fn enable_colour() -> bool {
    use windows_sys::Win32::System::Console::{
        ENABLE_VIRTUAL_TERMINAL_PROCESSING, GetConsoleMode, GetStdHandle, STD_OUTPUT_HANDLE, SetConsoleMode,
    };
    // SAFETY: plain console calls on this process's own output handle.
    unsafe {
        let handle = GetStdHandle(STD_OUTPUT_HANDLE);
        let mut mode = 0;
        GetConsoleMode(handle, &mut mode) != 0 && SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING) != 0
    }
}

#[cfg(not(windows))]
fn enable_colour() -> bool {
    true
}

/// The language a locale asks for: "zh", "zh-Hant" (Taiwan, Hong Kong, Macau, or Hant written out) or "en" (anything
/// not Chinese); none for C and POSIX, which say nothing.
pub fn locale_name(locale: &str) -> Option<&'static str> {
    let name = locale.trim().to_ascii_lowercase();
    if name.is_empty() || name == "c" || name == "posix" || name.starts_with("c.") {
        return None;
    }
    if !name.starts_with("zh") {
        return Some("en");
    }
    let traditional = ["hant", "_tw", "-tw", "_hk", "-hk", "_mo", "-mo"].iter().any(|mark| name.contains(mark));
    Some(if traditional { "zh-Hant" } else { "zh" })
}

/// Whether the system asks for Chinese: STARMETER_LANG, then the locale variables in POSIX order, then the system's
/// own preference (macOS's first language, Windows's display language). Chinese when nothing says, as the panel.
pub fn system_chinese() -> bool {
    system_language() != "en"
}

/// The language the system asks for, as system_chinese reads it: "zh", "zh-Hant" or "en".
pub fn system_language() -> &'static str {
    if let Some(chosen) = std::env::var("STARMETER_LANG").ok().and_then(|value| locale_name(&value)) {
        return chosen;
    }
    for key in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Some(chosen) = std::env::var(key).ok().and_then(|value| locale_name(&value)) {
            return chosen;
        }
    }
    os_language().unwrap_or("zh")
}

#[cfg(target_os = "macos")]
fn os_language() -> Option<&'static str> {
    let output = starmeter_core::shell::run(&["/usr/bin/defaults", "read", "-g", "AppleLanguages"], 2.0, None, None)?;
    let first = output.split(['(', ',', '"', '\n', ' ', ')']).find(|part| !part.trim().is_empty())?;
    locale_name(first)
}

#[cfg(windows)]
fn os_language() -> Option<&'static str> {
    // SAFETY: a plain query of the user's display language.
    let language = unsafe { windows_sys::Win32::Globalization::GetUserDefaultUILanguage() };
    // The primary language is the low ten bits (0x04 is Chinese), the region the rest: 1 Taiwan, 3 Hong Kong, 5 Macau.
    Some(match (language & 0x3ff, language >> 10) {
        (0x04, 1 | 3 | 5) => "zh-Hant",
        (0x04, _) => "zh",
        _ => "en",
    })
}

#[cfg(not(any(target_os = "macos", windows)))]
fn os_language() -> Option<&'static str> {
    None
}

/// Columns a character takes: two for East Asian wide and full-width forms, none for combining marks.
pub fn char_width(c: char) -> usize {
    let code = c as u32;
    match code {
        0 => 0,
        0x0300..=0x036f | 0x200b..=0x200f | 0xfe00..=0xfe0f => 0,
        0x1100..=0x115f
        | 0x2e80..=0x303e
        | 0x3041..=0x33ff
        | 0x3400..=0x4dbf
        | 0x4e00..=0x9fff
        | 0xa000..=0xa4cf
        | 0xac00..=0xd7a3
        | 0xf900..=0xfaff
        | 0xfe30..=0xfe4f
        | 0xff00..=0xff60
        | 0xffe0..=0xffe6
        | 0x1f300..=0x1f64f
        | 0x1f900..=0x1f9ff
        | 0x20000..=0x3fffd => 2,
        _ => 1,
    }
}

pub fn width(text: &str) -> usize {
    text.chars().map(char_width).sum()
}

/// The text cut to fit `room` columns, ending in “…” when cut.
pub fn fit(text: &str, room: usize) -> String {
    if width(text) <= room {
        return text.to_string();
    }
    if room == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in text.chars() {
        let w = char_width(c);
        if used + w > room - 1 {
            break;
        }
        out.push(c);
        used += w;
    }
    out.push('…');
    out
}

/// The text in lines of at most `room` columns: broken between words where it has spaces, anywhere between Chinese
/// characters, and inside a word only when the word alone is wider than the room.
pub fn wrap(text: &str, room: usize) -> Vec<String> {
    let room = room.max(1);
    // Pieces that stay together: a word with the spaces after it, or one wide character.
    let mut pieces: Vec<String> = Vec::new();
    for c in text.chars() {
        let continues = |last: &String| {
            // Spaces stay with the piece before them; a narrow character continues a word not yet ended.
            c.is_whitespace()
                || (char_width(c) < 2
                    && last.chars().last().is_some_and(|end| char_width(end) < 2 && !end.is_whitespace()))
        };
        match pieces.last_mut() {
            Some(last) if continues(last) => last.push(c),
            _ => pieces.push(c.to_string()),
        }
    }
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    for piece in pieces {
        let piece = if line.is_empty() { piece.trim_start().to_string() } else { piece };
        if !line.is_empty() && width(line.trim_end()) + width(piece.trim_end()) > room {
            lines.push(line.trim_end().to_string());
            line = piece.trim_start().to_string();
        } else {
            line.push_str(&piece);
        }
        while width(line.trim_end()) > room {
            let mut head = String::new();
            for c in line.chars() {
                // At least one character a line: a wide one in a room of one column takes the line on its own (it
                // stood still before, and the loop never ended).
                if !head.is_empty() && width(&head) + char_width(c) > room {
                    break;
                }
                head.push(c);
            }
            line = line[head.len()..].trim_start().to_string();
            lines.push(head.trim_end().to_string());
        }
    }
    if !line.trim().is_empty() {
        lines.push(line.trim_end().to_string());
    }
    lines
}

/// Spaces to bring the text to `room` columns, on the left (right-aligned) or the right.
pub fn pad(text: &str, room: usize, right_align: bool) -> String {
    let gap = " ".repeat(room.saturating_sub(width(text)));
    if right_align { format!("{gap}{text}") } else { format!("{text}{gap}") }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_room_of_one_column_still_wraps_wide_characters() {
        assert_eq!(super::wrap("中文 ok", 1), vec!["中", "文", "o", "k"]);
        assert_eq!(super::wrap("全角，字", 0), vec!["全", "角", "，", "字"]);
    }

    use super::*;

    #[test]
    fn chinese_takes_two_columns() {
        assert_eq!(width("Claude 订阅"), 11);
        assert_eq!(width("近 7 天"), 7);
        assert_eq!(width("▁▂█ · …"), 7);
        assert_eq!(fit("DeepSeek Harness", 10), "DeepSeek …");
        assert_eq!(fit("未标注项目", 7), "未标注…");
        assert_eq!(width(&fit("未标注项目", 6)), 5);
        assert_eq!(pad("模型", 6, true), "  模型");
    }

    #[test]
    fn notes_wrap_between_words_and_between_chinese_characters() {
        let english = "No plans or APIs are on the homepage yet: choose them under Home modules in the Starmeter panel";
        let lines = wrap(english, 38);
        assert!(lines.iter().all(|line| width(line) <= 38), "{lines:?}");
        assert_eq!(lines.join(" "), english);
        assert_eq!(lines[0], "No plans or APIs are on the homepage");
        let chinese = "主页还没有订阅或 API：在 Starmeter 面板的“主页模块”里勾选";
        let lines = wrap(chinese, 20);
        assert!(lines.iter().all(|line| width(line) <= 20), "{lines:?}");
        assert_eq!(lines.concat().replace(' ', ""), chinese.replace(' ', ""));
        assert_eq!(wrap("abcdefghij", 4), vec!["abcd", "efgh", "ij"]);
        assert_eq!(wrap("short", 40), vec!["short"]);
        assert!(wrap("", 10).is_empty());
    }

    #[test]
    fn a_locale_says_chinese_or_not() {
        assert_eq!(locale_name("zh_CN.UTF-8"), Some("zh"));
        assert_eq!(locale_name("zh-Hans-CN"), Some("zh"));
        assert_eq!(locale_name("en_US.UTF-8"), Some("en"));
        assert_eq!(locale_name("ja_JP.UTF-8"), Some("en"));
        assert_eq!(locale_name("C.UTF-8"), None);
        assert_eq!(locale_name("POSIX"), None);
        assert_eq!(locale_name(""), None);
        assert_eq!(locale_name("zh_TW.UTF-8"), Some("zh-Hant"));
        assert_eq!(locale_name("zh-Hant-HK"), Some("zh-Hant"));
        assert_eq!(locale_name("zh_HK"), Some("zh-Hant"));
        assert_eq!(locale_name("zh-Hans-CN"), Some("zh"));
        assert_eq!(locale_name("zh_SG.UTF-8"), Some("zh"));
        assert_eq!(locale_name("fr_FR.UTF-8"), Some("en"));
    }
}
