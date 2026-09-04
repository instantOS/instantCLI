//! Terminal-aware text helpers shared by menu renderers and structured output.

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Strip CSI and OSC ANSI escape sequences while preserving Unicode text.
pub(crate) fn strip_ansi(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars();

    while let Some(character) = chars.next() {
        if character != '\x1b' {
            result.push(character);
            continue;
        }

        match chars.next() {
            Some('[') => {
                for next in chars.by_ref() {
                    if ('@'..='~').contains(&next) {
                        break;
                    }
                }
            }
            Some(']') => {
                let iter = chars.by_ref();
                while let Some(next) = iter.next() {
                    match next {
                        '\x07' => break,
                        '\x1b' if matches!(iter.next(), Some('\\')) => break,
                        _ => {}
                    }
                }
            }
            Some(_) | None => {}
        }
    }

    result
}

/// Display width in terminal cells, accounting for wide and combining glyphs.
pub(crate) fn display_width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}

/// Truncate text to terminal-cell width, appending an ellipsis when needed.
pub(crate) fn truncate_to_width(text: &str, max_width: usize) -> String {
    if display_width(text) <= max_width {
        return text.to_string();
    }
    if max_width == 0 {
        return String::new();
    }

    let ellipsis = '…';
    let content_width = max_width.saturating_sub(UnicodeWidthChar::width(ellipsis).unwrap_or(1));
    let mut result = String::new();
    let mut width = 0;
    for character in text.chars() {
        let character_width = UnicodeWidthChar::width(character).unwrap_or(0);
        if width + character_width > content_width {
            break;
        }
        result.push(character);
        width += character_width;
    }
    result.push(ellipsis);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ansi_stripping_preserves_unicode_and_removes_csi_and_osc() {
        let input = "é\x1b[35m紫\x1b[0m\x1b]0;title\x07終";
        assert_eq!(strip_ansi(input), "é紫終");
    }

    #[test]
    fn truncation_respects_display_cells() {
        assert_eq!(truncate_to_width("ab界cd", 5), "ab界…");
        assert_eq!(display_width(&truncate_to_width("ab界cd", 5)), 5);
        assert_eq!(truncate_to_width("abc", 0), "");
    }
}
