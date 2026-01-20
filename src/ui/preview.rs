//! Preview builder for FZF menus
//!
//! Provides a fluent API for generating styled preview text with consistent
//! formatting across all FZF-based menus.
//!
//! The builder can output:
//! - Static text via [`PreviewBuilder::build`] for inline previews
//! - Shell scripts via [`PreviewBuilder::build_shell_script`] for `preview_command()`

use serde::{Deserialize, Serialize};

use crate::ui::catppuccin::{colors, hex_to_ansi_fg};
use crate::ui::nerd_font::NerdFont;

/// Preview content for FZF items.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum FzfPreview {
    Text(String),
    Command(String),
    None,
}

/// ANSI reset sequence
const RESET: &str = "\x1b[0m";

/// Standard separator for preview headers
const SEPARATOR: &str = "───────────────────────────────────";

/// Light separator for subsections
const LIGHT_SEPARATOR: &str = "┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄";

/// A line in the preview - either static text or a shell command for dynamic content.
#[derive(Clone)]
enum PreviewLine {
    /// Static text line (pre-formatted with ANSI codes)
    Static(String),
    /// Shell command that outputs dynamic content
    Shell(String),
}

/// Builder for creating styled FZF preview text.
///
/// Supports both static previews (rendered at build time) and dynamic shell-based
/// previews (executed by fzf when the preview is shown).
///
/// # Example - Static Preview
///
/// ```ignore
/// use crate::ui::preview::PreviewBuilder;
/// use crate::ui::nerd_font::NerdFont;
///
/// let preview = PreviewBuilder::new()
///     .header(NerdFont::User, "John Doe")
///     .field("Status", "Active")
///     .bullets(&["wheel", "video", "audio"])
///     .build();
/// ```
///
/// # Example - Shell Script Preview (for preview_command)
///
/// ```ignore
/// let script = PreviewBuilder::new()
///     .header(NerdFont::Image, "Image Viewer")
///     .subtext("Configure default image viewer")
///     .blank()
///     .shell_loop(
///         "mime",
///         &["image/png", "image/jpeg", "image/gif"],
///         r#"echo "  • $mime""#,
///     )
///     .build_shell_script();
/// ```
pub struct PreviewBuilder {
    lines: Vec<PreviewLine>,
}

impl PreviewBuilder {
    /// Create a new preview builder.
    ///
    /// Starts with a blank line for padding from the preview window border.
    pub fn new() -> Self {
        Self {
            lines: vec![PreviewLine::Static(String::new())],
        }
    }

    fn push_static(&mut self, s: String) {
        self.lines.push(PreviewLine::Static(s));
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
        self.push_static(format!("{mauve}{}  {title}{RESET}", char::from(icon)));
        self.push_static(format!("{surface}{SEPARATOR}{RESET}"));
        self.push_static(String::new());
        self
    }

    /// Add primary text line in the standard text color.
    pub fn text(mut self, content: &str) -> Self {
        let text_color = hex_to_ansi_fg(colors::TEXT);
        self.push_static(format!("{text_color}{content}{RESET}"));
        self
    }

    /// Add secondary/muted text line in subtext color.
    pub fn subtext(mut self, content: &str) -> Self {
        let subtext = hex_to_ansi_fg(colors::SUBTEXT0);
        self.push_static(format!("{subtext}{content}{RESET}"));
        self
    }

    /// Add a labeled field line (e.g., "Status: Active").
    ///
    /// The label appears in subtext color, the value in text color.
    pub fn field(mut self, label: &str, value: &str) -> Self {
        let subtext = hex_to_ansi_fg(colors::SUBTEXT0);
        let text_color = hex_to_ansi_fg(colors::TEXT);
        self.push_static(format!(
            "{subtext}{label}:{RESET} {text_color}{value}{RESET}"
        ));
        self
    }

    /// Add an indented field line (for nested information).
    pub fn field_indented(mut self, label: &str, value: &str) -> Self {
        let subtext = hex_to_ansi_fg(colors::SUBTEXT0);
        let text_color = hex_to_ansi_fg(colors::TEXT);
        self.push_static(format!(
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
        self.push_static(format!("{fg}{icon_str}{content}{RESET}"));
        self
    }

    /// Add a light separator line.
    pub fn separator(mut self) -> Self {
        let surface = hex_to_ansi_fg(colors::SURFACE1);
        self.push_static(format!("{surface}{LIGHT_SEPARATOR}{RESET}"));
        self
    }

    /// Add a blank line.
    pub fn blank(mut self) -> Self {
        self.push_static(String::new());
        self
    }

    /// Add a bold title in the specified color.
    pub fn title(mut self, color: &str, content: &str) -> Self {
        let fg = hex_to_ansi_fg(color);
        let bold = "\x1b[1m";
        self.push_static(format!("{bold}{fg}{content}{RESET}"));
        self
    }

    /// Add raw text without any coloring.
    pub fn raw(mut self, content: &str) -> Self {
        self.push_static(content.to_string());
        self
    }

    /// Add an indented line with icon and color.
    pub fn indented_line(mut self, color: &str, icon: Option<NerdFont>, content: &str) -> Self {
        let fg = hex_to_ansi_fg(color);
        let icon_str = icon
            .map(|i| format!("{} ", char::from(i)))
            .unwrap_or_default();
        self.push_static(format!("  {fg}{icon_str}{content}{RESET}"));
        self
    }

    /// Add a bullet list item.
    pub fn bullet(mut self, content: &str) -> Self {
        let text_color = hex_to_ansi_fg(colors::TEXT);
        self.push_static(format!("{text_color}  • {content}{RESET}"));
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

    // ========================================================================
    // Shell/Dynamic content methods (for build_shell_script)
    // ========================================================================

    /// Add raw shell command(s) for dynamic content.
    ///
    /// Only used when building with `build_shell_script()`.
    /// For static builds, this is converted to a placeholder.
    pub fn shell(mut self, command: &str) -> Self {
        self.lines.push(PreviewLine::Shell(command.to_string()));
        self
    }

    /// Add a shell loop that iterates over items.
    ///
    /// # Arguments
    /// * `var` - Loop variable name (e.g., "mime")
    /// * `items` - Items to iterate over
    /// * `body` - Shell commands to run for each item (can reference $var)
    pub fn shell_loop<I, S>(mut self, var: &str, items: I, body: &str) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let item_list: Vec<_> = items.into_iter().map(|s| s.as_ref().to_string()).collect();
        let items_str = item_list.join(" ");
        let cmd = format!("for {var} in {items_str}; do\n{body}\ndone");
        self.lines.push(PreviewLine::Shell(cmd));
        self
    }

    /// Add a MIME type status display loop.
    ///
    /// Generates a shell loop that queries xdg-mime for each type's default app
    /// and displays it with appropriate coloring.
    pub fn mime_defaults<I, S>(mut self, mime_types: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let types: Vec<_> = mime_types
            .into_iter()
            .map(|s| s.as_ref().to_string())
            .collect();
        let mime_list = types.join(" ");

        let green = hex_to_shell_escape(colors::GREEN);
        let yellow = hex_to_shell_escape(colors::YELLOW);
        let subtext = hex_to_shell_escape(colors::SUBTEXT0);
        let reset = "\\033[0m";

        let cmd = format!(
            r#"for mime in {mime_list}; do
    app=$(xdg-mime query default "$mime" 2>/dev/null)
    if [ -n "$app" ]; then
        name=""
        for dir in "$HOME/.local/share/applications" "/usr/share/applications" "/var/lib/flatpak/exports/share/applications" "$HOME/.local/share/flatpak/exports/share/applications"; do
            if [ -f "$dir/$app" ]; then
                name=$(grep "^Name=" "$dir/$app" 2>/dev/null | head -1 | cut -d= -f2)
                break
            fi
        done
        if [ -n "$name" ]; then
            echo -e "  {subtext}$mime:{reset} {green}$name{reset}"
        else
            echo -e "  {subtext}$mime:{reset} {green}$app{reset}"
        fi
    else
        echo -e "  {subtext}$mime:{reset} {yellow}(not set){reset}"
    fi
done"#
        );
        self.lines.push(PreviewLine::Shell(cmd));
        self
    }

    // ========================================================================
    // Build methods
    // ========================================================================

    /// Build the final FzfPreview (static text).
    ///
    /// Shell commands are rendered as placeholders.
    pub fn build(self) -> FzfPreview {
        FzfPreview::Text(self.build_string())
    }

    /// Build and extract just the text content.
    ///
    /// Shell commands are rendered as placeholders.
    pub fn build_string(self) -> String {
        self.lines
            .into_iter()
            .map(|line| match line {
                PreviewLine::Static(s) => s,
                PreviewLine::Shell(_) => "(dynamic content)".to_string(),
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Build a bash script for use with `preview_command()`.
    ///
    /// Static lines are converted to echo statements with proper escaping.
    /// Shell commands are included directly.
    pub fn build_shell_script(self) -> String {
        let commands: Vec<String> = self
            .lines
            .into_iter()
            .map(|line| match line {
                PreviewLine::Static(s) => {
                    if s.is_empty() {
                        "echo".to_string()
                    } else {
                        // Convert ANSI escapes (\x1b) to shell format (\e)
                        // Use double quotes for echo - escape $ ` \ " for shell
                        let shell_escaped = s
                            .replace('\\', "\\\\") // Escape backslashes
                            .replace('"', "\\\"") // Escape double quotes
                            .replace('$', "\\$") // Escape dollar signs
                            .replace('`', "\\`") // Escape backticks
                            .replace('\x1b', "\\e"); // Convert ANSI escapes to \e
                        format!("echo -e \"{shell_escaped}\"")
                    }
                }
                PreviewLine::Shell(cmd) => cmd,
            })
            .collect();

        let script = commands.join("\n");
        format!("bash -c '\n{script}\n'")
    }
}

/// Convert hex color to shell escape sequence for use in echo -e
fn hex_to_shell_escape(hex: &str) -> String {
    let hex = hex.trim_start_matches('#');
    if hex.len() != 6 {
        return String::new();
    }
    let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0);
    let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0);
    let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0);
    format!("\\033[38;2;{r};{g};{b}m")
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

    #[test]
    fn test_build_shell_script() {
        let script = PreviewBuilder::new()
            .text("Hello")
            .blank()
            .text("World")
            .build_shell_script();

        assert!(script.starts_with("bash -c '"));
        assert!(script.contains("echo -e"));
    }
}
