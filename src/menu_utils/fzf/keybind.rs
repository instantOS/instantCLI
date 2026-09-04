//! Validation, fzf configuration, and presentation for menu keybinds.

use std::collections::HashSet;
use std::process::Command;

use anyhow::{Result, bail};

use super::types::MenuKeybind;
use crate::ui::catppuccin::{colors, hex_to_ansi_bg, hex_to_ansi_fg};
use crate::ui::nerd_font::NerdFont;
use crate::ui::text::{display_width, truncate_to_width};

const ANSI_RESET: &str = "\x1b[0m";
const HINT_MAX_WIDTH: usize = 96;
const HINT_MIN_WIDTH: usize = 12;
const SEPARATOR_WIDTH: usize = 5;

pub(super) const NONE: &[MenuKeybind<()>] = &[];

struct HintSegment {
    styled: String,
    width: usize,
}

pub(super) fn validate<A>(keybinds: &[MenuKeybind<A>]) -> Result<()> {
    let mut seen = HashSet::new();
    for bind in keybinds {
        if !seen.insert(bind.key.as_str()) {
            bail!("duplicate menu keybind: {}", bind.key);
        }
    }
    Ok(())
}

/// Register one `print(token)+accept` binding per key. The token is emitted
/// before selected rows and resolved back to the caller's typed action.
pub(super) fn configure_command<A>(command: &mut Command, keybinds: &[MenuKeybind<A>]) {
    for bind in keybinds {
        command
            .arg("--bind")
            .arg(format!("{}:print({})+accept", bind.key, bind.key));
    }
}

fn normalize_label(label: &str) -> String {
    label
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn render_segment<A>(bind: &MenuKeybind<A>, max_width: usize) -> HintSegment {
    let key = truncate_to_width(&bind.key.display_name(), max_width.saturating_sub(2));
    let key_width = display_width(&key) + 2;
    let label = normalize_label(&bind.label);
    let label_gap = usize::from(!label.is_empty()) * 2;
    let label = truncate_to_width(&label, max_width.saturating_sub(key_width + label_gap));

    let keycap = format!(
        "{}{}\x1b[1m {key} {ANSI_RESET}",
        hex_to_ansi_bg(colors::SURFACE0),
        hex_to_ansi_fg(colors::MAUVE),
    );
    let styled = if label.is_empty() {
        keycap
    } else {
        format!(
            "{keycap}  {}{label}{ANSI_RESET}",
            hex_to_ansi_fg(colors::SUBTEXT0)
        )
    };

    HintSegment {
        styled,
        width: key_width + usize::from(!label.is_empty()) * 2 + display_width(&label),
    }
}

fn hint_width(responsive_layout: bool) -> usize {
    let list_width = if responsive_layout {
        super::utils::get_responsive_layout().list_width
    } else {
        super::utils::get_terminal_dimensions()
            .map(|(columns, _)| usize::from(columns).saturating_sub(4))
            .unwrap_or(76)
    };
    list_width.clamp(HINT_MIN_WIDTH, HINT_MAX_WIDTH)
}

/// Render accent keycaps and muted labels, wrapping complete bindings to the
/// estimated width of the fzf list pane.
pub(super) fn render_hint<A>(keybinds: &[MenuKeybind<A>], responsive_layout: bool) -> String {
    render_hint_at_width(keybinds, hint_width(responsive_layout))
}

fn render_hint_at_width<A>(keybinds: &[MenuKeybind<A>], max_width: usize) -> String {
    let keyboard = char::from(NerdFont::Keyboard);
    let prefix_width = display_width(&keyboard.to_string()) + 2;
    let first_prefix = format!("{}{keyboard}{ANSI_RESET}  ", hex_to_ansi_fg(colors::BLUE));
    let continuation_prefix = " ".repeat(prefix_width);
    let separator = format!("{}  •  {ANSI_RESET}", hex_to_ansi_fg(colors::SURFACE2));
    let content_width = max_width
        .saturating_sub(prefix_width)
        .max(HINT_MIN_WIDTH.saturating_sub(prefix_width));

    let mut lines = vec![first_prefix];
    let mut line_width = prefix_width;
    let mut has_segment = false;
    for segment in keybinds
        .iter()
        .map(|bind| render_segment(bind, content_width))
    {
        let separator_width = if has_segment { SEPARATOR_WIDTH } else { 0 };
        if has_segment && line_width + separator_width + segment.width > max_width {
            lines.push(continuation_prefix.clone());
            line_width = prefix_width;
            has_segment = false;
        }
        if has_segment {
            lines.last_mut().expect("line exists").push_str(&separator);
            line_width += SEPARATOR_WIDTH;
        }
        lines
            .last_mut()
            .expect("line exists")
            .push_str(&segment.styled);
        line_width += segment.width;
        has_segment = true;
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu_utils::{MenuKey, default_fzf_key};

    #[test]
    fn hints_use_keycaps_sanitize_labels_and_wrap_whole_bindings() {
        let binds = [
            MenuKeybind::new(MenuKey::new("ctrl-e").unwrap(), "edit\n  entry", 1u8),
            MenuKeybind::new(MenuKey::new("alt-d").unwrap(), "delete", 2u8),
        ];
        let rendered = render_hint_at_width(&binds, 30);
        let plain = default_fzf_key(&rendered);
        let lines = plain.lines().collect::<Vec<_>>();

        assert_eq!(lines.len(), 2);
        assert!(plain.contains(char::from(NerdFont::Keyboard)));
        assert!(plain.contains("Ctrl E   edit entry"));
        assert!(plain.contains("Alt D   delete"));
        assert!(lines.iter().all(|line| display_width(line) <= 30));
        assert!(rendered.contains(&hex_to_ansi_bg(colors::SURFACE0)));
    }

    #[test]
    fn hints_separate_bindings_when_they_fit() {
        let binds = [
            MenuKeybind::new(MenuKey::new("ctrl-e").unwrap(), "edit", 1u8),
            MenuKeybind::new(MenuKey::new("alt-d").unwrap(), "delete", 2u8),
        ];
        let plain = default_fzf_key(&render_hint_at_width(&binds, 80));

        assert_eq!(plain.lines().count(), 1);
        assert!(plain.contains("edit  •   Alt D"));
    }

    #[test]
    fn overlong_labels_are_truncated_to_the_hint_width() {
        let binds = [MenuKeybind::new(
            MenuKey::new("ctrl-e").unwrap(),
            "edit an exceptionally long entry label",
            1u8,
        )];
        let plain = default_fzf_key(&render_hint_at_width(&binds, 24));

        assert!(plain.contains('…'));
        assert!(plain.lines().all(|line| display_width(line) <= 24));
    }

    #[test]
    fn duplicate_keys_are_rejected() {
        let binds = [
            MenuKeybind::new(MenuKey::new("ctrl-e").unwrap(), "a", 1u8),
            MenuKeybind::new(MenuKey::new("ctrl-e").unwrap(), "b", 2u8),
        ];

        assert_eq!(
            validate(&binds).unwrap_err().to_string(),
            "duplicate menu keybind: ctrl-e"
        );
    }
}
