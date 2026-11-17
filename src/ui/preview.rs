//! Preview builder for FZF menus
//!
//! Provides a fluent API for generating styled preview text with consistent
//! formatting across all FZF-based menus.
//!
//! Structured content shares a text column, with icons hanging in a gutter to
//! its left. Use `field_indented`, `indented_line` and `bullet` for nested content
//! instead of adding spaces at call sites. Continuations align with their text
//! or field value, and rules follow the pane width (`FZF_PREVIEW_COLUMNS`, or
//! an explicit builder width). `raw` and `shell` deliberately bypass this layout
//! for preformatted output and terminal graphics.
//!
//! The builder can output:
//! - Static text via [`PreviewBuilder::build`] for inline previews
//! - Shell script bodies via [`PreviewBuilder::build_shell_script`] for `preview_command()`
//! - Streaming output via [`PreviewBuilder::streaming`] for incremental rendering

use std::io::{BufWriter, Stdout, Write};

use serde::{Deserialize, Serialize};

use crate::common::shell::shell_quote;
use crate::ui::catppuccin::{colors, hex_to_ansi_fg};
use crate::ui::nerd_font::NerdFont;
use crate::ui::text::{display_width, strip_ansi, wrap_ansi};

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

/// Icons hang to the left of one shared text column. Nested content adds two cells.
pub(crate) const TEXT_COLUMN: usize = 4;
const NEST_INDENT: usize = 2;

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
    columns: usize,
    line_count: usize,
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

fn preview_columns() -> usize {
    std::env::var("FZF_PREVIEW_COLUMNS")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .filter(|&columns| columns > 0)
        .map(usize::from)
        .unwrap_or(80)
}

impl PreviewWriter {
    /// Create a preview writer in **collect** mode.
    ///
    /// Starts with a blank line for padding from the preview window border.
    pub fn collect() -> Self {
        Self {
            sink: PreviewSink::Collect(vec![PreviewLine::Static(String::new())]),
            columns: preview_columns(),
            line_count: 1,
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
            columns: preview_columns(),
            line_count: 1,
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
            columns: preview_columns(),
            line_count: 1,
        }
    }

    fn push_static(&mut self, s: String) {
        self.line_count += s.split('\n').count();
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

    /// Emit structured content with a hanging prefix and aligned continuation lines.
    fn structured(&mut self, prefix: &str, content: &str, column: usize) {
        let width = self.columns.saturating_sub(column).max(1);
        let continuation = " ".repeat(column);
        let lines = wrap_ansi(content, width);
        let text = lines
            .iter()
            .enumerate()
            .map(|(index, line)| {
                format!("{}{line}", if index == 0 { prefix } else { &continuation })
            })
            .collect::<Vec<_>>()
            .join("\n");
        self.push_static(format!("{text}{RESET}"));
    }

    fn colored(&mut self, color: &str, content: &str, column: usize) {
        let prefix = format!("{}{}", " ".repeat(column), hex_to_ansi_fg(color));
        self.structured(&prefix, content, column);
    }

    fn icon_line(&mut self, color: &str, icon: Option<NerdFont>, content: &str, nested: bool) {
        let column = TEXT_COLUMN + if nested { NEST_INDENT } else { 0 };
        let start = if nested { TEXT_COLUMN } else { 0 };
        let icon = icon
            .map(char::from)
            .map(|icon| icon.to_string())
            .unwrap_or_default();
        let prefix = format!(
            "{}{}{icon}{}",
            " ".repeat(start),
            hex_to_ansi_fg(color),
            " ".repeat(column.saturating_sub(start + display_width(&icon)))
        );
        self.structured(&prefix, content, column);
    }

    /// Icon and title share the body's text column; the rule fills the available width.
    pub fn header(&mut self, icon: NerdFont, title: &str) -> &mut Self {
        self.icon_line(colors::MAUVE, Some(icon), title, false);
        self.rule('─');
        self.blank()
    }

    /// Primary text. Explicit newlines and wrapped lines keep the same text column.
    pub fn text(&mut self, content: &str) -> &mut Self {
        self.colored(colors::TEXT, content, TEXT_COLUMN);
        self
    }

    pub fn subtext(&mut self, content: &str) -> &mut Self {
        self.colored(colors::SUBTEXT0, content, TEXT_COLUMN);
        self
    }

    pub fn field(&mut self, label: &str, value: &str) -> &mut Self {
        self.field_at(label, value, TEXT_COLUMN);
        self
    }

    pub fn field_indented(&mut self, label: &str, value: &str) -> &mut Self {
        self.field_at(label, value, TEXT_COLUMN + NEST_INDENT);
        self
    }

    fn field_at(&mut self, label: &str, value: &str, column: usize) {
        let value_column = column + display_width(&strip_ansi(label)) + 2;
        if value_column + 4 > self.columns {
            self.colored(colors::SUBTEXT0, &format!("{label}:"), column);
            self.colored(colors::TEXT, value, column + NEST_INDENT);
        } else {
            let prefix = format!(
                "{}{}{label}:{RESET} {}",
                " ".repeat(column),
                hex_to_ansi_fg(colors::SUBTEXT0),
                hex_to_ansi_fg(colors::TEXT)
            );
            self.structured(&prefix, value, value_column);
        }
    }

    pub fn line(&mut self, color: &str, icon: Option<NerdFont>, content: &str) -> &mut Self {
        self.icon_line(color, icon, content, false);
        self
    }

    fn rule(&mut self, character: char) {
        let width = self.columns.saturating_sub(TEXT_COLUMN).max(1);
        self.push_static(format!(
            "{}{}{}{RESET}",
            " ".repeat(TEXT_COLUMN),
            hex_to_ansi_fg(colors::SURFACE1),
            character.to_string().repeat(width)
        ));
    }

    pub fn separator(&mut self) -> &mut Self {
        self.rule('┄');
        self
    }

    pub fn blank(&mut self) -> &mut Self {
        self.push_static(String::new());
        self
    }

    pub fn title(&mut self, color: &str, content: &str) -> &mut Self {
        let prefix = format!(
            "{}\x1b[1m{}",
            " ".repeat(TEXT_COLUMN),
            hex_to_ansi_fg(color)
        );
        self.structured(&prefix, content, TEXT_COLUMN);
        self
    }

    /// Verbatim escape hatch for preformatted output and graphics; bypasses layout.
    pub fn raw(&mut self, content: &str) -> &mut Self {
        self.push_static(content.to_string());
        self
    }

    pub fn indented_line(
        &mut self,
        color: &str,
        icon: Option<NerdFont>,
        content: &str,
    ) -> &mut Self {
        self.icon_line(color, icon, content, true);
        self
    }

    pub fn bullet(&mut self, content: &str) -> &mut Self {
        self.indented_line(colors::TEXT, Some(NerdFont::Bullet), content)
    }

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
            PreviewSink::Collect(lines) | PreviewSink::StreamAndCollect(_, lines) => lines
                .into_iter()
                .map(|line| match line {
                    PreviewLine::Static(text) => text,
                    PreviewLine::Shell(_) => "(dynamic content)".into(),
                })
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }

    /// Static lines use printf so backslashes, quotes and leading dashes remain literal.
    pub fn build_shell_script(self) -> String {
        match self.sink {
            PreviewSink::Stream(_) => String::new(),
            PreviewSink::Collect(lines) | PreviewSink::StreamAndCollect(_, lines) => lines
                .into_iter()
                .map(|line| match line {
                    PreviewLine::Static(text) => format!("printf '%s\\n' {}", shell_quote(&text)),
                    PreviewLine::Shell(command) => command,
                })
                .collect::<Vec<_>>()
                .join("\n"),
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

    /// Set the preview pane width when a renderer already has it in its context.
    pub fn width(mut self, columns: Option<usize>) -> Self {
        if let Some(columns) = columns {
            self.writer.columns = columns.max(1);
        }
        self
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

    /// Text rows emitted so far, including wrapped lines and initial padding.
    /// Dynamic shell commands are not counted. Useful for positioning graphical payloads.
    pub fn line_count(&self) -> usize {
        self.writer.line_count
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
    fn headings_and_body_share_a_text_column_and_nested_fields_are_explicit() {
        let preview = PreviewBuilder::new()
            .width(Some(48))
            .header(NerdFont::Desktop, "QtAV Player")
            .subtext("QtAV Multimedia Player")
            .line(colors::TEAL, Some(NerdFont::ChevronRight), "Details")
            .field_indented("Desktop ID", "Player.desktop")
            .field_indented("Command", "Player -f %U")
            .header(NerdFont::Video, "head.mp4")
            .field("Duration", "00:04")
            .build_string();
        let plain = strip_ansi(&preview);
        for (text, column) in [
            ("QtAV Player", 4),
            ("QtAV Multimedia Player", 4),
            ("Details", 4),
            ("Desktop ID", 6),
            ("Command", 6),
            ("head.mp4", 4),
            ("Duration", 4),
        ] {
            let line = plain.lines().find(|line| line.contains(text)).unwrap();
            assert_eq!(
                display_width(&line[..line.find(text).unwrap()]),
                column,
                "{line:?}"
            );
        }
        for line in plain.lines().filter(|line| line.contains('─')) {
            assert_eq!(display_width(line), 48);
            assert!(line.starts_with("    "));
        }
    }

    #[test]
    fn multiline_values_and_prose_keep_their_continuation_indent() {
        let preview = PreviewBuilder::new()
            .width(Some(32))
            .text("A description that spans several lines\nSecond paragraph")
            .field("Command", "Player --long-option with spaces\nnext argument")
            .bullet("A bullet which also needs to wrap across lines");
        let emitted = preview.line_count();
        let text = strip_ansi(&preview.build_string());
        assert_eq!(emitted, text.split('\n').count());
        assert!(text.lines().all(|line| display_width(line) <= 32));
        assert!(text.contains("\n             next argument"));
        assert!(text.contains("\n    Second paragraph"));
        assert!(
            text.lines()
                .filter(|line| !line.is_empty())
                .all(|line| line.starts_with("    "))
        );
    }

    #[test]
    fn narrow_fields_leave_room_for_values_and_raw_output_is_verbatim() {
        let text = strip_ansi(
            &PreviewBuilder::new()
                .width(Some(24))
                .field("A long field label", "a long value with multiple words")
                .raw("verbatim\noutput")
                .build_string(),
        );
        assert!(text.contains("\n      a long value"));
        assert!(text.ends_with("\nverbatim\noutput"));
        assert!(text.lines().all(|line| display_width(line) <= 24));
    }

    #[test]
    fn shell_and_collected_previews_render_identically() -> anyhow::Result<()> {
        let make = || {
            PreviewBuilder::new()
                .width(Some(40))
                .header(NerdFont::File, "A 'quoted' filename")
                .text(r"C:\new\test $literal `command` \n")
                .field("Value", "first\nsecond")
                .raw("-n")
        };
        let expected = format!("{}\n", make().build_string());
        let output = std::process::Command::new("bash")
            .arg("-c")
            .arg(make().build_shell_script())
            .output()?;
        assert!(output.status.success());
        assert_eq!(String::from_utf8(output.stdout)?, expected);
        Ok(())
    }

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

        assert!(script.contains("printf"));
        assert!(!script.starts_with("bash -c"));
    }
}
