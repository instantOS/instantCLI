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

/// Word-wrap terminal text without splitting ANSI sequences or wide glyphs.
/// Existing newlines and whitespace inside a line are preserved.
pub(crate) fn wrap_ansi(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut output: Vec<String> = Vec::new();
    for paragraph in text.split('\n') {
        let paragraph_start = output.len();
        let mut units: Vec<(&str, usize)> = Vec::new();
        let mut cells = 0;
        let mut offset = 0;
        while offset < paragraph.len() {
            let start = offset;
            let character = paragraph[offset..].chars().next().unwrap_or_default();
            offset += character.len_utf8();
            if character == '\x1b' {
                let mut chars = paragraph[offset..].char_indices();
                match chars.next() {
                    Some((_, '[')) => {
                        offset += 1;
                        for (_, next) in chars {
                            offset += next.len_utf8();
                            if ('@'..='~').contains(&next) {
                                break;
                            }
                        }
                    }
                    Some((_, ']')) => {
                        offset += 1;
                        let mut previous = '\0';
                        for (_, next) in chars {
                            offset += next.len_utf8();
                            if next == '\x07' || (previous == '\x1b' && next == '\\') {
                                break;
                            }
                            previous = next;
                        }
                    }
                    Some((_, next)) => offset += next.len_utf8(),
                    None => {}
                }
                units.push((&paragraph[start..offset], 0));
                continue;
            }
            if character == ' ' && cells >= width {
                output.push(units.iter().map(|(text, _)| *text).collect());
                units.clear();
                cells = 0;
                continue;
            }
            let character_width = UnicodeWidthChar::width(character).unwrap_or(0);
            while cells + character_width > width && cells > 0 {
                let split = units.iter().rposition(|(unit, _)| *unit == " ");
                if let Some(split) =
                    split.filter(|&index| units[..index].iter().any(|(_, size)| *size > 0))
                {
                    output.push(units[..split].iter().map(|(text, _)| *text).collect());
                    units.drain(..=split);
                    cells = units.iter().map(|(_, size)| size).sum();
                } else if let Some(split) = units
                    .iter()
                    .rposition(|(unit, _)| matches!(*unit, "/" | "\\"))
                {
                    // Keep path/URL segments intact when a long token has no spaces.
                    output.push(units[..=split].iter().map(|(text, _)| *text).collect());
                    units.drain(..=split);
                    cells = units.iter().map(|(_, size)| size).sum();
                } else {
                    output.push(units.iter().map(|(text, _)| *text).collect());
                    units.clear();
                    cells = 0;
                }
            }
            units.push((&paragraph[start..offset], character_width));
            cells += character_width;
        }
        let tail: String = units.iter().map(|(text, _)| *text).collect();
        if cells == 0 && output.len() > paragraph_start {
            if let Some(last) = output.last_mut() {
                last.push_str(&tail);
            }
        } else {
            output.push(tail);
        }
    }
    output
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
    fn wrapping_preserves_ansi_and_unicode_and_explicit_newlines() {
        assert_eq!(wrap_ansi("hello world", 5), ["hello", "world"]);
        assert_eq!(
            wrap_ansi("/tmp/long-directory/head.mp4", 20),
            ["/tmp/long-directory/", "head.mp4"]
        );
        assert_eq!(wrap_ansi("first\nsecond\n", 20), ["first", "second", ""]);
        let text = "\x1b[31m界界 abcd\x1b[0m";
        let lines = wrap_ansi(text, 4);
        assert_eq!(
            lines
                .iter()
                .map(|line| strip_ansi(line))
                .collect::<Vec<_>>(),
            ["界界", "abcd"]
        );
        assert!(lines[0].starts_with("\x1b[31m"));
        assert!(lines[1].ends_with("\x1b[0m"));
        let link = "\x1b]8;;https://example.com\x07link\x1b]8;;\x07";
        assert_eq!(wrap_ansi(link, 4), [link]);
    }

    #[test]
    fn truncation_respects_display_cells() {
        assert_eq!(truncate_to_width("ab界cd", 5), "ab界…");
        assert_eq!(display_width(&truncate_to_width("ab界cd", 5)), 5);
        assert_eq!(truncate_to_width("abc", 0), "");
    }
}
