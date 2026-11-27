//! Text helpers for names, paths, ages and picker columns.

use std::env;

/// tmux cannot target a name with a colon, and agent tools append status marks to session
/// names, so names are compared with only A-Z a-z 0-9 . _ - kept.
pub fn plain_name(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        .collect()
}

/// tmux reads an argument that ends in ; as the end of a command, and keeps \; as a ;.
pub fn tmux_arg(s: &str) -> String {
    match s.strip_suffix(';') {
        Some(rest) => format!("{rest}\\;"),
        None => s.to_owned(),
    }
}

/// Quote for sh, which fzf runs the preview command with.
pub fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

pub fn tilde(path: &str) -> String {
    if let Ok(home) = env::var("HOME")
        && !home.is_empty()
        && let Some(rest) = path.strip_prefix(&home)
        && (rest.is_empty() || rest.starts_with('/'))
    {
        return format!("~{rest}");
    }
    path.to_owned()
}

pub fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// The short form `wt list` uses in its Age column.
pub fn age(secs: i64) -> String {
    match secs {
        ..60 => "now".into(),
        60..3_600 => format!("{}m", secs / 60),
        3_600..86_400 => format!("{}h", secs / 3_600),
        86_400..604_800 => format!("{}d", secs / 86_400),
        _ => format!("{}w", secs / 604_800),
    }
}

/// Columns a terminal draws for S. East Asian wide characters and emoji take two columns,
/// and combining marks and variation selectors none.
pub fn width(s: &str) -> usize {
    s.chars().map(char_width).sum()
}

fn char_width(c: char) -> usize {
    match c as u32 {
        0 | 0x300..=0x36F | 0x200B..=0x200F | 0x20D0..=0x20FF | 0xFE00..=0xFE0F
        | 0x1F3FB..=0x1F3FF | 0xE0020..=0xE007F => 0,
        0x1100..=0x115F
        | 0x231A..=0x231B
        | 0x2329..=0x232A
        | 0x23E9..=0x23EC
        | 0x23F0
        | 0x23F3
        | 0x25FD..=0x25FE
        | 0x2614..=0x2615
        | 0x2648..=0x2653
        | 0x267F
        | 0x2693
        | 0x26A1
        | 0x26AA..=0x26AB
        | 0x26BD..=0x26BE
        | 0x26C4..=0x26C5
        | 0x26CE
        | 0x26D4
        | 0x26EA
        | 0x26F2..=0x26F3
        | 0x26F5
        | 0x26FA
        | 0x26FD
        | 0x2705
        | 0x270A..=0x270B
        | 0x2728
        | 0x274C
        | 0x274E
        | 0x2753..=0x2755
        | 0x2757
        | 0x2795..=0x2797
        | 0x27B0
        | 0x27BF
        | 0x2B1B..=0x2B1C
        | 0x2B50
        | 0x2B55
        | 0x2E80..=0x303E
        | 0x3041..=0x33FF
        | 0x3400..=0x4DBF
        | 0x4E00..=0x9FFF
        | 0xA000..=0xA4CF
        | 0xAC00..=0xD7A3
        | 0xF900..=0xFAFF
        | 0xFE30..=0xFE4F
        | 0xFF00..=0xFF60
        | 0xFFE0..=0xFFE6
        | 0x1F004
        | 0x1F0CF
        | 0x1F18E
        | 0x1F191..=0x1F19A
        | 0x1F200..=0x1F251
        | 0x1F300..=0x1F64F
        | 0x1F680..=0x1F6FF
        | 0x1F7E0..=0x1F7EB
        | 0x1F90C..=0x1F9FF
        | 0x1FA70..=0x1FAFF
        | 0x20000..=0x3FFFD => 2,
        _ => 1,
    }
}

pub fn pad_right(s: &str, w: usize) -> String {
    format!("{s}{}", " ".repeat(w.saturating_sub(width(s))))
}

pub fn pad_left(s: &str, w: usize) -> String {
    format!("{}{s}", " ".repeat(w.saturating_sub(width(s))))
}

/// Cut S to MAX columns, ending in … when it was longer.
pub fn shorten(s: &str, max: usize) -> String {
    if width(s) <= max {
        return s.to_owned();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in s.chars() {
        let w = char_width(c);
        if used + w > max - 1 {
            break;
        }
        out.push(c);
        used += w;
    }
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_name_keeps_safe_chars() {
        assert_eq!(plain_name("my.app v2⏳"), "my.appv2");
    }

    #[test]
    fn tmux_arg_escapes_a_trailing_semicolon() {
        assert_eq!(tmux_arg("make;"), "make\\;");
        assert_eq!(tmux_arg("a;b"), "a;b");
    }

    #[test]
    fn sh_quote_survives_a_single_quote() {
        assert_eq!(sh_quote("it's"), r"'it'\''s'");
    }

    #[test]
    fn age_reads_like_wt_list() {
        assert_eq!(
            [30, 120, 7_200, 172_800, 1_209_600].map(age),
            ["now", "2m", "2h", "2d", "2w"]
        );
    }

    #[test]
    fn emoji_take_two_columns_and_selectors_none() {
        assert_eq!(width("main⏳"), 6);
        assert_eq!(width("main⏳\u{FE0F}"), 6);
        assert_eq!(width("▸●"), 2);
    }

    #[test]
    fn shorten_ends_in_an_ellipsis() {
        assert_eq!(shorten("abcdefgh", 5), "abcd…");
        assert_eq!(shorten("abc", 5), "abc");
    }
}
