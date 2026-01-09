//! Preview builder for FZF menus
//!
//! Provides a fluent API for generating styled preview text with consistent
//! formatting across all FZF-based menus.

use crate::menu_utils::FzfPreview;
use crate::ui::catppuccin::{colors, hex_to_ansi_fg};
use crate::ui::nerd_font::NerdFont;

/// ANSI reset sequence
const RESET: &str = "\x1b[0m";

/// Standard separator for preview headers
const SEPARATOR: &str = "───────────────────────────────────";

/// Light separator for subsections
const LIGHT_SEPARATOR: &str = "┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄";

/// Builder for creating styled FZF preview text.
///
/// # Example
///
/// ```ignore
/// use crate::ui::preview::PreviewBuilder;
/// use crate::ui::nerd_font::NerdFont;
///
/// let preview = PreviewBuilder::new()
///     .header(NerdFont::User, "John Doe")
///     .field("Status", "Active")
///     .field("Shell", "/bin/zsh")
///     .blank()
///     .separator()
///     .blank()
///     .subtext("Groups:")
///     .bullets(&["wheel", "video", "audio"])
///     .build();
/// ```
pub struct PreviewBuilder {
    lines: Vec<String>,
}

impl PreviewBuilder {
    /// Create a new preview builder.
    ///
    /// Starts with a blank line for padding from the preview window border.
    pub fn new() -> Self {
        Self {
            lines: vec![String::new()],
        }
    }

    /// Add a styled header with icon and title.
    ///
    /// Creates a header section with:
    /// - Icon + title in accent color (mauve)
    /// - Separator line below
    /// - Blank line after
    pub fn header(mut self, icon: NerdFont, title: &str) -> Self {
        let mauve = hex_to_ansi_fg(colors::MAUVE);
        let surface = hex_to_ansi_fg(colors::SURFACE1);
        self.lines
            .push(format!("{mauve}{}  {title}{RESET}", char::from(icon)));
        self.lines.push(format!("{surface}{SEPARATOR}{RESET}"));
        self.lines.push(String::new());
        self
    }

    /// Add primary text line in the standard text color.
    pub fn text(mut self, content: &str) -> Self {
        let text_color = hex_to_ansi_fg(colors::TEXT);
        self.lines.push(format!("{text_color}{content}{RESET}"));
        self
    }

    /// Add secondary/muted text line in subtext color.
    pub fn subtext(mut self, content: &str) -> Self {
        let subtext = hex_to_ansi_fg(colors::SUBTEXT0);
        self.lines.push(format!("{subtext}{content}{RESET}"));
        self
    }

    /// Add a labeled field line (e.g., "Status: Active").
    ///
    /// The label appears in subtext color, the value in text color.
    pub fn field(mut self, label: &str, value: &str) -> Self {
        let subtext = hex_to_ansi_fg(colors::SUBTEXT0);
        let text_color = hex_to_ansi_fg(colors::TEXT);
        self.lines.push(format!(
            "{subtext}{label}:{RESET} {text_color}{value}{RESET}"
        ));
        self
    }

    /// Add an indented field line (for nested information).
    pub fn field_indented(mut self, label: &str, value: &str) -> Self {
        let subtext = hex_to_ansi_fg(colors::SUBTEXT0);
        let text_color = hex_to_ansi_fg(colors::TEXT);
        self.lines.push(format!(
            "  {subtext}{label}:{RESET} {text_color}{value}{RESET}"
        ));
        self
    }

    /// Add an icon + colored text line.
    ///
    /// # Arguments
    /// * `color` - Hex color string (e.g., `colors::TEAL`)
    /// * `icon` - Optional NerdFont icon
    /// * `content` - The text content
    pub fn line(mut self, color: &str, icon: Option<NerdFont>, content: &str) -> Self {
        let fg = hex_to_ansi_fg(color);
        let icon_str = icon
            .map(|i| format!("{} ", char::from(i)))
            .unwrap_or_default();
        self.lines.push(format!("{fg}{icon_str}{content}{RESET}"));
        self
    }

    /// Add a light separator line.
    pub fn separator(mut self) -> Self {
        let surface = hex_to_ansi_fg(colors::SURFACE1);
        self.lines
            .push(format!("{surface}{LIGHT_SEPARATOR}{RESET}"));
        self
    }

    /// Add a blank line.
    pub fn blank(mut self) -> Self {
        self.lines.push(String::new());
        self
    }

    /// Add a bold title in the specified color.
    pub fn title(mut self, color: &str, content: &str) -> Self {
        let fg = hex_to_ansi_fg(color);
        let bold = "\x1b[1m";
        self.lines.push(format!("{bold}{fg}{content}{RESET}"));
        self
    }

    /// Add raw text without any coloring.
    pub fn raw(mut self, content: &str) -> Self {
        self.lines.push(content.to_string());
        self
    }

    /// Add an indented line with icon and color.
    pub fn indented_line(mut self, color: &str, icon: Option<NerdFont>, content: &str) -> Self {
        let fg = hex_to_ansi_fg(color);
        let icon_str = icon
            .map(|i| format!("{} ", char::from(i)))
            .unwrap_or_default();
        self.lines.push(format!("  {fg}{icon_str}{content}{RESET}"));
        self
    }

    /// Add a bullet list item.
    pub fn bullet(mut self, content: &str) -> Self {
        let text_color = hex_to_ansi_fg(colors::TEXT);
        self.lines.push(format!("{text_color}  • {content}{RESET}"));
        self
    }

    /// Add multiple bullet items from an iterator.
    pub fn bullets<I, S>(mut self, items: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        for item in items {
            self = self.bullet(item.as_ref());
        }
        self
    }

    /// Build the final FzfPreview.
    pub fn build(self) -> FzfPreview {
        FzfPreview::Text(self.lines.join("\n"))
    }

    /// Build and extract just the text content.
    ///
    /// Useful when you need a `String` instead of `FzfPreview`.
    pub fn build_string(self) -> String {
        self.lines.join("\n")
    }
}

impl Default for PreviewBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_preview() {
        let preview = PreviewBuilder::new()
            .header(NerdFont::User, "Test User")
            .field("Status", "Active")
            .build();

        match preview {
            FzfPreview::Text(text) => {
                assert!(text.contains("Test User"));
                assert!(text.contains("Status:"));
                assert!(text.contains("Active"));
            }
            _ => panic!("Expected Text preview"),
        }
    }

    #[test]
    fn test_bullets() {
        let preview = PreviewBuilder::new()
            .subtext("Groups:")
            .bullets(["wheel", "video", "audio"])
            .build();

        match preview {
            FzfPreview::Text(text) => {
                assert!(text.contains("• wheel"));
                assert!(text.contains("• video"));
                assert!(text.contains("• audio"));
            }
            _ => panic!("Expected Text preview"),
        }
    }

    #[test]
    fn test_build_string() {
        let text = PreviewBuilder::new()
            .text("Hello")
            .text("World")
            .build_string();

        assert!(text.contains("Hello"));
        assert!(text.contains("World"));
    }
}
