//! Preview builder for FZF menus
//!
//! Provides a fluent API for generating styled preview text with consistent
//! formatting across all FZF-based menus.
//!
//! The builder can output:
//! - Static text via [`PreviewBuilder::build`] for inline previews
//! - Shell script bodies via [`PreviewBuilder::build_shell_script`] for `preview_command()`
//! - Streaming output via [`PreviewBuilder::streaming`] for incremental rendering

use std::io::{BufWriter, Stdout, Write};

use serde::{Deserialize, Serialize};

use crate::ui::catppuccin::{colors, hex_to_ansi_fg};
use crate::ui::nerd_font::NerdFont;

/// Preview content for FZF items.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum FzfPreview {
    Text(String),
    /// Bash-compatible script body executed by the preview runner.
    /// The selected item's key is passed as `$1` when available.
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

/// Output strategy: collect lines in memory or stream to stdout.
enum PreviewSink {
    /// Collect lines for later retrieval via `build_string()` / `build_shell_script()`.
    Collect(Vec<PreviewLine>),
    /// Write each line to stdout immediately, flushing after every line so fzf
    /// can display partial output while expensive operations run.
    Stream(BufWriter<Stdout>),
    /// Stream to stdout while also retaining lines for later caching.
    StreamAndCollect(BufWriter<Stdout>, Vec<PreviewLine>),
}

pub struct PreviewWriter {
    sink: PreviewSink,
}

/// Builder for creating styled FZF preview text.
///
/// Supports both static previews (rendered at build time) and dynamic shell-based
/// previews (executed by fzf when the preview is shown).
///
/// # Modes
///
/// - **Collect** (`PreviewBuilder::new()`): Lines are buffered and returned via
///   `build_string()`, `build()`, or `build_shell_script()`.
/// - **Streaming** (`PreviewBuilder::streaming()`): Lines are written to stdout
///   immediately. Use this for previews that perform expensive operations (network
///   calls, slow commands) so fzf can display the header while data loads.
///
/// # Example - Static Preview
///
/// ```ignore
/// let preview = PreviewBuilder::new()
///     .header(NerdFont::User, "John Doe")
///     .field("Status", "Active")
///     .bullets(&["wheel", "video", "audio"])
///     .build();
/// ```
///
/// # Example - Streaming Preview
///
/// ```ignore
/// let mut builder = PreviewBuilder::streaming()
///     .header(NerdFont::Package, "my-package")
///     .line(colors::BLUE, None, "APT Package");
///
/// // Header is already visible in fzf while this runs:
/// let info = expensive_network_call()?;
/// builder = builder.text(&info);
/// ```
pub struct PreviewBuilder {
    writer: PreviewWriter,
}

impl PreviewWriter {
    /// Create a preview writer in **collect** mode.
    ///
    /// Starts with a blank line for padding from the preview window border.
    pub fn collect() -> Self {
        Self {
            sink: PreviewSink::Collect(vec![PreviewLine::Static(String::new())]),
        }
    }

    /// Create a preview writer in **streaming** mode.
    ///
    /// Each line is written to stdout and flushed immediately, allowing fzf
    /// to display partial output while expensive operations run.
    pub fn streaming() -> Self {
        let mut writer = BufWriter::new(std::io::stdout());
        // Initial blank line matching collect()
        let _ = writeln!(writer);
        let _ = writer.flush();
        Self {
            sink: PreviewSink::Stream(writer),
        }
    }

    /// Create a preview writer that streams immediately and also retains the
    /// rendered content so callers can cache it afterward.
    pub fn streaming_cached() -> Self {
        let mut writer = BufWriter::new(std::io::stdout());
        let _ = writeln!(writer);
        let _ = writer.flush();
        Self {
            sink: PreviewSink::StreamAndCollect(writer, vec![PreviewLine::Static(String::new())]),
        }
    }

    fn push_static(&mut self, s: String) {
        match &mut self.sink {
            PreviewSink::Collect(lines) => lines.push(PreviewLine::Static(s)),
            PreviewSink::Stream(writer) => {
                let _ = writeln!(writer, "{s}");
                let _ = writer.flush();
            }
            PreviewSink::StreamAndCollect(writer, lines) => {
                let _ = writeln!(writer, "{s}");
                let _ = writer.flush();
                lines.push(PreviewLine::Static(s));
            }
        }
    }

    /// Add a styled header with icon and title.
    ///
    /// Creates a header section with:
    /// - Icon + title in accent color (mauve)
    /// - Separator line below
    /// - Blank line after
    pub fn header(&mut self, icon: NerdFont, title: &str) -> &mut Self {
        let mauve = hex_to_ansi_fg(colors::MAUVE);
        let surface = hex_to_ansi_fg(colors::SURFACE1);
        self.push_static(format!("{mauve}{}  {title}{RESET}", char::from(icon)));
        self.push_static(format!("{surface}{SEPARATOR}{RESET}"));
        self.push_static(String::new());
        self
    }

    /// Add primary text line in the standard text color.
    pub fn text(&mut self, content: &str) -> &mut Self {
        let text_color = hex_to_ansi_fg(colors::TEXT);
        self.push_static(format!("{text_color}{content}{RESET}"));
        self
    }

    /// Add secondary/muted text line in subtext color.
    pub fn subtext(&mut self, content: &str) -> &mut Self {
        let subtext = hex_to_ansi_fg(colors::SUBTEXT0);
        self.push_static(format!("{subtext}{content}{RESET}"));
        self
    }

    /// Add a labeled field line (e.g., "Status: Active").
    ///
    /// The label appears in subtext color, the value in text color.
    pub fn field(&mut self, label: &str, value: &str) -> &mut Self {
        let subtext = hex_to_ansi_fg(colors::SUBTEXT0);
        let text_color = hex_to_ansi_fg(colors::TEXT);
        self.push_static(format!(
            "{subtext}{label}:{RESET} {text_color}{value}{RESET}"
        ));
        self
    }

    /// Add an indented field line (for nested information).
    pub fn field_indented(&mut self, label: &str, value: &str) -> &mut Self {
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
    pub fn line(&mut self, color: &str, icon: Option<NerdFont>, content: &str) -> &mut Self {
        let fg = hex_to_ansi_fg(color);
        let icon_str = icon
            .map(|i| format!("{}  ", char::from(i)))
            .unwrap_or_default();
        self.push_static(format!("{fg}{icon_str}{content}{RESET}"));
        self
    }

    /// Add a light separator line.
    pub fn separator(&mut self) -> &mut Self {
        let surface = hex_to_ansi_fg(colors::SURFACE1);
        self.push_static(format!("{surface}{LIGHT_SEPARATOR}{RESET}"));
        self
    }

    /// Add a blank line.
    pub fn blank(&mut self) -> &mut Self {
        self.push_static(String::new());
        self
    }

    /// Add a bold title in the specified color.
    pub fn title(&mut self, color: &str, content: &str) -> &mut Self {
        let fg = hex_to_ansi_fg(color);
        let bold = "\x1b[1m";
        self.push_static(format!("{bold}{fg}{content}{RESET}"));
        self
    }

    /// Add raw text without any coloring.
    pub fn raw(&mut self, content: &str) -> &mut Self {
        self.push_static(content.to_string());
        self
    }

    /// Add an indented line with icon and color.
    pub fn indented_line(
        &mut self,
        color: &str,
        icon: Option<NerdFont>,
        content: &str,
    ) -> &mut Self {
        let fg = hex_to_ansi_fg(color);
        let icon_str = icon
            .map(|i| format!("{} ", char::from(i)))
            .unwrap_or_default();
        self.push_static(format!("  {fg}{icon_str}{content}{RESET}"));
        self
    }

    /// Add a bullet list item.
    pub fn bullet(&mut self, content: &str) -> &mut Self {
        let text_color = hex_to_ansi_fg(colors::TEXT);
        let bullet = char::from(NerdFont::Bullet);
        self.push_static(format!("{text_color}  {bullet} {content}{RESET}"));
        self
    }

    /// Add multiple bullet items from an iterator.
    pub fn bullets<I, S>(&mut self, items: I) -> &mut Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        for item in items {
            self.bullet(item.as_ref());
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
    /// In streaming mode, shell commands are not supported and are ignored.
    pub fn shell(&mut self, command: &str) -> &mut Self {
        match &mut self.sink {
            PreviewSink::Collect(lines) | PreviewSink::StreamAndCollect(_, lines) => {
                lines.push(PreviewLine::Shell(command.to_string()));
            }
            PreviewSink::Stream(_) => {}
        }
        self
    }

    // ========================================================================
    // Build methods
    // ========================================================================

    /// Build the final FzfPreview (static text).
    ///
    /// Shell commands are rendered as placeholders.
    /// In streaming mode, returns `FzfPreview::None` (output already written).
    pub fn build(self) -> FzfPreview {
        match &self.sink {
            PreviewSink::Stream(_) => FzfPreview::None,
            PreviewSink::Collect(_) | PreviewSink::StreamAndCollect(_, _) => {
                FzfPreview::Text(self.build_string())
            }
        }
    }

    /// Build and extract just the text content.
    ///
    /// Shell commands are rendered as placeholders.
    /// In streaming mode, returns an empty string (output already written).
    pub fn build_string(self) -> String {
        match self.sink {
            PreviewSink::Stream(_) => String::new(),
            PreviewSink::Collect(lines) => lines
                .into_iter()
                .map(|line| match line {
                    PreviewLine::Static(s) => s,
                    PreviewLine::Shell(_) => "(dynamic content)".to_string(),
                })
                .collect::<Vec<_>>()
                .join("\n"),
            PreviewSink::StreamAndCollect(_, lines) => lines
                .into_iter()
                .map(|line| match line {
                    PreviewLine::Static(s) => s,
                    PreviewLine::Shell(_) => "(dynamic content)".to_string(),
                })
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }

    /// Build a bash-compatible script body for use with `preview_command()`.
    ///
    /// Static lines are converted to echo statements with proper escaping.
    /// Shell commands are included directly. The preview runner will execute
    /// the returned script with bash and pass the item's key as `$1`.
    ///
    /// Not meaningful in streaming mode (returns empty string).
    pub fn build_shell_script(self) -> String {
        match self.sink {
            PreviewSink::Stream(_) => String::new(),
            PreviewSink::Collect(lines) => {
                let commands: Vec<String> = lines
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

                commands.join("\n")
            }
            PreviewSink::StreamAndCollect(_, lines) => {
                let commands: Vec<String> = lines
                    .into_iter()
                    .map(|line| match line {
                        PreviewLine::Static(s) => {
                            if s.is_empty() {
                                "echo".to_string()
                            } else {
                                let shell_escaped = s
                                    .replace('\\', "\\\\")
                                    .replace('"', "\\\"")
                                    .replace('$', "\\$")
                                    .replace('`', "\\`")
                                    .replace('\x1b', "\\e");
                                format!("echo -e \"{shell_escaped}\"")
                            }
                        }
                        PreviewLine::Shell(cmd) => cmd,
                    })
                    .collect();

                commands.join("\n")
            }
        }
    }
}

impl PreviewBuilder {
    /// Create a new preview builder in **collect** mode.
    ///
    /// Starts with a blank line for padding from the preview window border.
    pub fn new() -> Self {
        Self {
            writer: PreviewWriter::collect(),
        }
    }

    /// Create a new preview builder in **streaming** mode.
    ///
    /// Each line is written to stdout and flushed immediately, allowing fzf
    /// to display partial output while expensive operations run.
    pub fn streaming() -> Self {
        Self {
            writer: PreviewWriter::streaming(),
        }
    }

    /// Add a styled header with icon and title. See [`PreviewWriter::header`].
    pub fn header(mut self, icon: NerdFont, title: &str) -> Self {
        self.writer.header(icon, title);
        self
    }

    /// Add primary text line in the standard text color. See [`PreviewWriter::text`].
    pub fn text(mut self, content: &str) -> Self {
        self.writer.text(content);
        self
    }

    /// Add secondary/muted text line in subtext color. See [`PreviewWriter::subtext`].
    pub fn subtext(mut self, content: &str) -> Self {
        self.writer.subtext(content);
        self
    }

    /// Add a labeled field line (e.g., "Status: Active"). See [`PreviewWriter::field`].
    pub fn field(mut self, label: &str, value: &str) -> Self {
        self.writer.field(label, value);
        self
    }

    /// Add an indented field line (for nested information). See [`PreviewWriter::field_indented`].
    pub fn field_indented(mut self, label: &str, value: &str) -> Self {
        self.writer.field_indented(label, value);
        self
    }

    /// Add an icon + colored text line. See [`PreviewWriter::line`].
    pub fn line(mut self, color: &str, icon: Option<NerdFont>, content: &str) -> Self {
        self.writer.line(color, icon, content);
        self
    }

    /// Add a light separator line. See [`PreviewWriter::separator`].
    pub fn separator(mut self) -> Self {
        self.writer.separator();
        self
    }

    /// Add a blank line. See [`PreviewWriter::blank`].
    pub fn blank(mut self) -> Self {
        self.writer.blank();
        self
    }

    /// Add a bold title in the specified color. See [`PreviewWriter::title`].
    pub fn title(mut self, color: &str, content: &str) -> Self {
        self.writer.title(color, content);
        self
    }

    /// Add raw text without any coloring. See [`PreviewWriter::raw`].
    pub fn raw(mut self, content: &str) -> Self {
        self.writer.raw(content);
        self
    }

    /// Add an indented line with icon and color. See [`PreviewWriter::indented_line`].
    pub fn indented_line(mut self, color: &str, icon: Option<NerdFont>, content: &str) -> Self {
        self.writer.indented_line(color, icon, content);
        self
    }

    /// Add a bullet list item. See [`PreviewWriter::bullet`].
    pub fn bullet(mut self, content: &str) -> Self {
        self.writer.bullet(content);
        self
    }

    /// Add multiple bullet items from an iterator. See [`PreviewWriter::bullets`].
    pub fn bullets<I, S>(mut self, items: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.writer.bullets(items);
        self
    }

    /// Add raw shell command(s) for dynamic content. See [`PreviewWriter::shell`].
    pub fn shell(mut self, command: &str) -> Self {
        self.writer.shell(command);
        self
    }

    /// Build the final FzfPreview (static text). See [`PreviewWriter::build`].
    pub fn build(self) -> FzfPreview {
        self.writer.build()
    }

    /// Build and extract just the text content. See [`PreviewWriter::build_string`].
    pub fn build_string(self) -> String {
        self.writer.build_string()
    }

    /// Build a bash-compatible script body. See [`PreviewWriter::build_shell_script`].
    pub fn build_shell_script(self) -> String {
        self.writer.build_shell_script()
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

    #[test]
    fn test_build_shell_script() {
        let script = PreviewBuilder::new()
            .text("Hello")
            .blank()
            .text("World")
            .build_shell_script();

        assert!(script.contains("echo -e"));
        assert!(!script.starts_with("bash -c"));
    }
}
