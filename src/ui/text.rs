//! Width-aware text truncation and path display.

use std::env;
use std::path::{Path, PathBuf};

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// Show `path` relative to the home directory when it is inside it.
pub fn display_path(path: &Path) -> String {
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty());

    match home
        .as_deref()
        .and_then(|home| path.strip_prefix(home).ok())
    {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

/// Split a display path into its parent head (including the trailing `/`)
/// and its final segment. Returns an empty head when there is no `/`.
pub fn split_last_segment(display: &str) -> (&str, &str) {
    match display.rfind('/') {
        Some(index) => display.split_at(index + 1),
        None => ("", display),
    }
}

/// Truncate `text` to `max_width` cells, ending with `…` when shortened.
pub fn truncate_right(text: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    if text.width() <= max_width {
        return text.to_owned();
    }
    if max_width == 1 {
        return "…".to_owned();
    }

    let mut shortened = String::new();
    let mut width = 0;
    for grapheme in text.graphemes(true) {
        let grapheme_width = grapheme.width();
        if width + grapheme_width > max_width - 1 {
            break;
        }
        shortened.push_str(grapheme);
        width += grapheme_width;
    }
    shortened.push('…');
    shortened
}

/// Truncate `text` to `max_width` cells from the left, starting with `…`.
pub fn truncate_left(text: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    if text.width() <= max_width {
        return text.to_owned();
    }
    if max_width == 1 {
        return "…".to_owned();
    }

    let mut tail = Vec::new();
    let mut width = 0;
    for grapheme in text.graphemes(true).rev() {
        let grapheme_width = grapheme.width();
        if width + grapheme_width > max_width - 1 {
            break;
        }
        tail.push(grapheme);
        width += grapheme_width;
    }

    let mut shortened = String::from("…");
    for grapheme in tail.into_iter().rev() {
        shortened.push_str(grapheme);
    }
    shortened
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_text_is_unchanged() {
        assert_eq!(truncate_right("hello", 8), "hello");
        assert_eq!(truncate_left("hello", 8), "hello");
    }

    #[test]
    fn truncates_from_the_right() {
        assert_eq!(truncate_right("hello", 4), "hel…");
        assert_eq!(truncate_right("hello", 1), "…");
        assert_eq!(truncate_right("hello", 0), "");
    }

    #[test]
    fn truncates_from_the_left() {
        assert_eq!(truncate_left("abcdef", 4), "…def");
        assert_eq!(truncate_left("abcdef", 1), "…");
        assert_eq!(truncate_left("abcdef", 0), "");
    }

    #[test]
    fn respects_wide_graphemes() {
        assert_eq!(truncate_right("日本語", 4), "日…");
        assert_eq!(truncate_left("日本語", 4), "…語");
    }

    #[test]
    fn splits_off_the_final_path_segment() {
        assert_eq!(
            split_last_segment("~/Documents/naskah-rust/note.md"),
            ("~/Documents/naskah-rust/", "note.md")
        );
        assert_eq!(split_last_segment("note.md"), ("", "note.md"));
    }
}
