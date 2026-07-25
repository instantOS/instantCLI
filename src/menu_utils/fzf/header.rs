//! Styled header builder for FZF menus.

use crate::ui::catppuccin::{colors, hex_to_ansi_fg};
use crate::ui::nerd_font::NerdFont;

use super::types::Header;

const RESET: &str = "\x1b[0m";

/// Builds a structured, consistently styled FZF menu header.
///
/// Use this for headers that carry context or status in addition to a title.
/// A simple one-line menu label can continue to use [`Header::fancy`].
///
/// # Example
///
/// ```ignore
/// let header = HeaderBuilder::new(NerdFont::Bell, "Notification Center")
///     .status(NerdFont::EnvelopeOpen, "2 unread", colors::YELLOW)
///     .build();
/// ```
#[derive(Debug, Clone)]
pub struct HeaderBuilder {
    lines: Vec<String>,
}

impl HeaderBuilder {
    /// Start a header with an accent-colored icon and title.
    pub fn new(icon: NerdFont, title: impl AsRef<str>) -> Self {
        let accent = hex_to_ansi_fg(colors::MAUVE);
        let title = title.as_ref();
        Self {
            lines: vec![format!("{accent}{}  {title}{RESET}", char::from(icon))],
        }
    }

    /// Add secondary guidance or explanatory text.
    pub fn subtitle(mut self, text: impl AsRef<str>) -> Self {
        let subtext = hex_to_ansi_fg(colors::SUBTEXT0);
        self.lines
            .push(format!("{subtext}{}{RESET}", text.as_ref()));
        self
    }

    /// Add a labeled contextual value.
    pub fn field(mut self, label: impl AsRef<str>, value: impl AsRef<str>) -> Self {
        let subtext = hex_to_ansi_fg(colors::SUBTEXT0);
        let text = hex_to_ansi_fg(colors::TEXT);
        self.lines.push(format!(
            "{subtext}{}:{RESET} {text}{}{RESET}",
            label.as_ref(),
            value.as_ref()
        ));
        self
    }

    /// Add a colored status line with an icon.
    pub fn status(mut self, icon: NerdFont, text: impl AsRef<str>, color: &str) -> Self {
        let color = hex_to_ansi_fg(color);
        self.lines.push(format!(
            "{color}{}  {}{RESET}",
            char::from(icon),
            text.as_ref()
        ));
        self
    }

    /// Finish the structured header using the standard fancy frame.
    pub fn build(self) -> Header {
        Header::Fancy(self.lines.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_lines_in_call_order() {
        let header = HeaderBuilder::new(NerdFont::Bell, "Notifications")
            .subtitle("Recent events")
            .field("Application", "Bluetooth")
            .status(NerdFont::EnvelopeOpen, "2 unread", colors::YELLOW)
            .build();

        let Header::Fancy(content) = header else {
            panic!("header builder must produce a fancy header");
        };

        let title = content.find("Notifications").unwrap();
        let subtitle = content.find("Recent events").unwrap();
        let field = content.find("Application:").unwrap();
        let status = content.find("2 unread").unwrap();
        assert!(title < subtitle && subtitle < field && field < status);
        assert!(content.contains(char::from(NerdFont::Bell)));
        assert!(content.contains(char::from(NerdFont::EnvelopeOpen)));
    }

    #[test]
    fn every_builder_line_resets_ansi_styling() {
        let header = HeaderBuilder::new(NerdFont::Info, "Title")
            .subtitle("Subtitle")
            .field("Label", "Value")
            .status(NerdFont::Check, "Ready", colors::GREEN)
            .build();

        let Header::Fancy(content) = header else {
            panic!("header builder must produce a fancy header");
        };

        assert!(content.lines().all(|line| line.ends_with(RESET)));
    }
}
