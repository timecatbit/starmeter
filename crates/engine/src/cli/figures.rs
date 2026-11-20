//! Figures as the panel writes them (app/ui/src/lib/format.ts and the share picture's): JavaScript's toFixed rounding,
//! token counts in K/M/B, shares, money with thousands separators.

/// JavaScript's Number.prototype.toFixed: rounded to `digits` decimals, an exact half away from zero.
pub fn to_fixed(value: f64, digits: usize) -> String {
    if !value.is_finite() {
        return if value.is_nan() { "NaN".into() } else { value.to_string() };
    }
    let negative = value < 0.0;
    let magnitude = value.abs();
    // The exact decimal expansion ends right after a tie, so its tail reads 5 then zeros.
    let exact = format!("{magnitude:.*}", digits + 30);
    let (whole, fraction) = exact.split_once('.').unwrap_or((&exact, ""));
    let tail = &fraction[digits..];
    let rounded = if tail.starts_with('5') && tail[1..].bytes().all(|byte| byte == b'0') {
        let mut kept: Vec<u8> = format!("{whole}{}", &fraction[..digits]).into_bytes();
        let mut index = kept.len();
        loop {
            if index == 0 {
                kept.insert(0, b'1');
                break;
            }
            index -= 1;
            if kept[index] == b'9' {
                kept[index] = b'0';
            } else {
                kept[index] += 1;
                break;
            }
        }
        let text = String::from_utf8(kept).unwrap_or_default();
        let split = text.len() - digits;
        if digits == 0 { text } else { format!("{}.{}", &text[..split], &text[split..]) }
    } else {
        format!("{magnitude:.digits$}")
    };
    if negative { format!("-{rounded}") } else { rounded }
}

/// 61.96M, 9.0K, 812: the panel's token counts.
pub fn tokens(value: f64) -> String {
    if value >= 1e9 {
        to_fixed(value / 1e9, 2) + "B"
    } else if value >= 1e6 {
        to_fixed(value / 1e6, 2) + "M"
    } else if value >= 1e3 {
        to_fixed(value / 1e3, 1) + "K"
    } else {
        to_fixed(value, 0)
    }
}

/// A share as a reader expects it: whole percents, a decimal near 0 and 100, never 0.0% or 100.0% for a sliver.
pub fn share(ratio: f64) -> String {
    let value = ratio * 100.0;
    if value > 0.0 && value < 0.05 {
        return "<0.1%".into();
    }
    if value > 99.95 && value < 100.0 {
        return ">99.9%".into();
    }
    let digits = if (value > 0.0 && value < 1.0) || (value > 99.0 && value < 100.0) { 1 } else { 0 };
    to_fixed(value, digits) + "%"
}

/// A cache share with one decimal, as the cards and the share picture write it.
pub fn percent(ratio: f64) -> String {
    to_fixed(ratio * 100.0, 1) + "%"
}

/// Digits grouped by thousands (1,234,567), as en-US writes them.
fn thousands(whole: &str) -> String {
    let digits: Vec<char> = whole.chars().collect();
    let mut out = String::new();
    for (index, digit) in digits.iter().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(*digit);
    }
    out
}

/// 1,234.56: two decimals with thousands separators (the share picture's dollars()).
pub fn money(value: f64) -> String {
    let fixed = to_fixed(value.abs(), 2);
    let (whole, fraction) = fixed.split_once('.').unwrap_or((&fixed, "00"));
    format!("{}{}.{fraction}", if value < 0.0 { "-" } else { "" }, thousands(whole))
}

/// 2,969: a count with thousands separators.
pub fn count(value: f64) -> String {
    let fixed = to_fixed(value.abs(), 0);
    format!("{}{}", if value < 0.0 { "-" } else { "" }, thousands(&fixed))
}

/// Credits as the cards write them: whole from 100 up, else one decimal.
pub fn credits(value: f64) -> String {
    format!("{} Credits", if value >= 100.0 { to_fixed(value, 0) } else { to_fixed(value, 1) })
}

/// Claude's family and version (claude-opus-5-5-20261101 → Opus 5.5); other names as they are.
pub fn model_name(name: &str) -> String {
    let Some(rest) = name.strip_prefix("claude-") else { return name.to_string() };
    let rest = match rest.rfind("-20") {
        Some(at) if rest.len() - at == 9 && rest[at + 1..].bytes().all(|byte| byte.is_ascii_digit()) => &rest[..at],
        _ => rest,
    };
    let parts: Vec<&str> = rest.split('-').collect();
    let family = parts[0];
    if !["opus", "sonnet", "haiku", "fable"].contains(&family) {
        return name.to_string();
    }
    let mut capital = family.chars();
    let first = capital.next().map(|c| c.to_ascii_uppercase()).unwrap_or_default();
    format!("{first}{} {}", capital.as_str(), parts[1..].join("."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn halves_round_away_from_zero_as_in_javascript() {
        assert_eq!(to_fixed(1.25, 1), "1.3");
        assert_eq!(to_fixed(2.5, 0), "3");
        assert_eq!(to_fixed(0.125, 2), "0.13");
        assert_eq!(to_fixed(9.995, 2), "9.99"); // 9.995 is just below the half in binary
        assert_eq!(to_fixed(99.95, 1), "100.0");
        assert_eq!(to_fixed(1.005, 2), "1.00");
        assert_eq!(to_fixed(-1.25, 1), "-1.3");
        assert_eq!(to_fixed(0.0, 2), "0.00");
    }

    #[test]
    fn figures_read_as_on_the_panel() {
        assert_eq!(tokens(61_956_680.0), "61.96M");
        assert_eq!(tokens(1_250.0), "1.3K");
        assert_eq!(tokens(812.0), "812");
        assert_eq!(tokens(2_505_419_755.0), "2.51B");
        assert_eq!(share(0.65), "65%");
        assert_eq!(share(0.0004), "<0.1%");
        assert_eq!(share(0.9997), ">99.9%");
        assert_eq!(share(0.005), "0.5%");
        assert_eq!(percent(0.9196491), "92.0%");
        assert_eq!(money(21.775984), "21.78");
        assert_eq!(money(1_234_567.891), "1,234,567.89");
        assert_eq!(count(2969.0), "2,969");
        assert_eq!(credits(1160.0), "1160 Credits");
        assert_eq!(credits(42.25), "42.3 Credits");
        assert_eq!(model_name("claude-opus-5-5-20261101"), "Opus 5.5");
        assert_eq!(model_name("claude-fable-5-2"), "Fable 5.2");
        assert_eq!(model_name("gpt-5.5-codex"), "gpt-5.5-codex");
        assert_eq!(model_name("claude-instant-1"), "claude-instant-1");
    }
}
