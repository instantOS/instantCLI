//! Core types and traits for the FZF wrapper

use anyhow::{Context, Result, anyhow, bail};
use base64::{Engine as _, engine::general_purpose};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::ffi::OsStr;
use std::fmt;
use std::process::Command;

use crate::ui::catppuccin::{colors, hex_to_ansi_fg};

pub use crate::ui::preview::FzfPreview;

const RESET: &str = "\x1b[0m";

/// Strip ANSI escape codes from a string for use as a stable key.
fn strip_ansi_escape_codes(s: &str) -> String {
    let mut result = String::new();
    let mut chars = s.chars();

    while let Some(c) = chars.next() {
        if c != '\x1b' {
            result.push(c);
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
                        '\x1b' => {
                            if matches!(iter.next(), Some('\\')) {
                                break;
                            }
                        }
                        _ => {}
                    }
                }
            }
            Some(_) | None => {}
        }
    }

    result
}

/// Default fzf key fallback for display text.
///
/// This is intentionally a fallback only. Item types that need stronger
/// identity than their rendered label should implement `fzf_key()` directly.
pub fn default_fzf_key(display_text: &str) -> String {
    strip_ansi_escape_codes(display_text)
}

/// Trait for types that can be displayed in FZF selection menus.
///
/// # Styling with ANSI Escape Codes
///
/// Both `fzf_display_text()` and `fzf_preview()` support ANSI escape codes:
///
/// - Use `format_icon_colored()` for styled icon badges
/// - Use `hex_to_ansi_fg()` for colored text
/// - Use `PreviewBuilder` for consistent preview formatting
pub trait FzfSelectable {
    /// Text shown in the FZF selection list.
    ///
    /// Supports ANSI escape codes for colored output. Use `format_icon_colored()`
    /// or `hex_to_ansi_fg()` for styling.
    fn fzf_display_text(&self) -> String;

    /// Preview content shown in the right pane.
    ///
    /// Supports ANSI escape codes for styling. Use `PreviewBuilder` for
    /// consistent formatting.
    fn fzf_preview(&self) -> FzfPreview {
        FzfPreview::None
    }

    /// Unique key for identifying this item (defaults to display text with ANSI stripped).
    fn fzf_key(&self) -> String {
        default_fzf_key(&self.fzf_display_text())
    }

    /// Optional: provide initial checked state for checklists.
    /// Default implementation returns false (unchecked).
    /// Only used by checklist dialogs.
    fn fzf_initial_checked_state(&self) -> bool {
        false
    }

    /// Optional hidden search keywords for alternative matching.
    ///
    /// These keywords are included in the fzf search but not displayed.
    /// Useful for aliases (e.g., "Sound Settings" could have keywords like "audio", "volume").
    /// Default implementation returns an empty slice.
    fn fzf_search_keywords(&self) -> &[&str] {
        &[]
    }

    /// Whether this item is selectable/navigable. Non-selectable items act as
    /// visual separators that cursor navigation skips over.
    fn fzf_is_selectable(&self) -> bool {
        true
    }
}

impl FzfSelectable for String {
    fn fzf_display_text(&self) -> String {
        self.clone()
    }
}

impl FzfSelectable for &str {
    fn fzf_display_text(&self) -> String {
        self.to_string()
    }
}

/// A menu item that can be either a selectable entry or a visual separator.
///
/// Use with `FzfBuilder::select_menu()` to build menus with grouped sections.
/// Separators are rendered as dimmed lines and navigation keys skip over them.
///
/// **Best suited for short, static menus** (e.g. home/action menus) where
/// visual grouping aids discoverability. Avoid in long, dynamically filtered
/// lists — raw mode keeps all items visible (dimmed) which clutters large sets.
#[derive(Clone, Debug)]
pub enum MenuItem<T: Clone> {
    Entry(T),
    Separator(String),
}

#[derive(Debug, Clone)]
pub struct StreamingMenuItem<T> {
    kind: String,
    key: String,
    display: String,
    preview: FzfPreview,
    preview_arg: Option<String>,
    payload: T,
}

#[derive(Debug, Clone)]
pub struct DecodedStreamingMenuItem<T> {
    pub kind: String,
    pub payload: T,
}

pub struct StreamingCommand {
    command: Command,
}

impl<T> StreamingMenuItem<T> {
    pub fn new(
        kind: impl Into<String>,
        key: impl Into<String>,
        display: impl Into<String>,
        payload: T,
    ) -> Self {
        Self {
            kind: kind.into(),
            key: key.into(),
            display: display.into(),
            preview: FzfPreview::None,
            preview_arg: None,
            payload,
        }
    }

    pub fn preview(mut self, preview: FzfPreview) -> Self {
        self.preview = preview;
        self
    }

    pub fn preview_arg(mut self, preview_arg: impl Into<String>) -> Self {
        self.preview_arg = Some(preview_arg.into());
        self
    }
}

impl StreamingCommand {
    pub fn new(program: impl AsRef<OsStr>) -> Self {
        Self {
            command: Command::new(program),
        }
    }

    pub fn arg(mut self, arg: impl AsRef<OsStr>) -> Self {
        self.command.arg(arg);
        self
    }

    pub(crate) fn into_command(self) -> Command {
        self.command
    }
}

impl From<Command> for StreamingCommand {
    fn from(command: Command) -> Self {
        Self { command }
    }
}

impl<T: Serialize> StreamingMenuItem<T> {
    pub fn encode(&self) -> Result<String> {
        let payload_json = serde_json::to_vec(&self.payload)?;
        let default_preview_arg = self.preview_arg.as_deref().unwrap_or(&self.key);
        let (preview_kind, preview_data) = match &self.preview {
            FzfPreview::Text(text) => ("T", general_purpose::STANDARD.encode(text.as_bytes())),
            FzfPreview::Command(command) => {
                let baked = command.replace("\"$1\"", &shell_quote(default_preview_arg));
                ("C", general_purpose::STANDARD.encode(baked.as_bytes()))
            }
            FzfPreview::None => ("N", String::new()),
        };

        Ok(format!(
            "{}\t{}\t{}\t{}\t{}\t{}",
            sanitize_streaming_field(&self.kind),
            sanitize_streaming_field(&self.key),
            sanitize_streaming_field(&self.display),
            preview_kind,
            preview_data,
            general_purpose::STANDARD.encode(payload_json),
        ))
    }
}

fn shell_quote(s: &str) -> String {
    if s.is_empty() {
        return "''".to_string();
    }
    if s.chars()
        .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '=' | '/' | '.' | ':' | ','))
    {
        return s.to_string();
    }
    format!("'{}'", s.replace('\'', r"'\''"))
}

impl<T: DeserializeOwned> DecodedStreamingMenuItem<T> {
    pub fn decode(line: &str) -> Result<Self> {
        // Streaming row format: kind\tkey\tdisplay\tpreview_kind\tpreview_b64\tpayload_b64
        // We only retain `kind` (used by callers to discriminate row types) and `payload`.
        // The remaining fields exist on the wire for the live fzf UI but are not consumed
        // after decoding.
        let mut fields = line.splitn(6, '\t');
        let kind = fields.next().unwrap_or_default().to_string();
        // Skip key, display, preview_kind, preview_b64.
        let _ = fields.nth(3);
        let payload_b64 = fields
            .next()
            .ok_or_else(|| anyhow!("Invalid streaming menu row: missing payload field"))?;

        let payload_json = general_purpose::STANDARD
            .decode(payload_b64)
            .context("Failed to decode streaming menu payload")?;
        let payload = serde_json::from_slice(&payload_json)
            .context("Failed to parse streaming menu payload")?;

        Ok(Self { kind, payload })
    }
}

pub fn streaming_preview_command() -> &'static str {
    "type=$(printf '%s' {4}); content=$(printf '%s' {5} | base64 -d 2>/dev/null); if [ \"$type\" = C ]; then eval \"$content\"; elif [ \"$type\" = T ]; then printf '%s' \"$content\"; fi"
}

fn sanitize_streaming_field(value: &str) -> String {
    value
        .chars()
        .map(|c| match c {
            '\t' | '\n' | '\r' => ' ',
            _ => c,
        })
        .collect()
}

impl<T: Clone> MenuItem<T> {
    pub fn entry(item: T) -> Self {
        MenuItem::Entry(item)
    }

    pub fn separator(label: &str) -> Self {
        MenuItem::Separator(label.to_string())
    }

    pub fn line() -> Self {
        MenuItem::Separator(String::new())
    }
}

impl<T: FzfSelectable + Clone> FzfSelectable for MenuItem<T> {
    fn fzf_display_text(&self) -> String {
        match self {
            MenuItem::Entry(item) => item.fzf_display_text(),
            MenuItem::Separator(label) => {
                let dim = hex_to_ansi_fg(colors::OVERLAY0);
                let reset = "\x1b[0m";
                if label.is_empty() {
                    format!("{dim}╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌{reset}")
                } else {
                    format!("{dim}╌╌ {label} ╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌{reset}")
                }
            }
        }
    }

    fn fzf_key(&self) -> String {
        match self {
            MenuItem::Entry(item) => item.fzf_key(),
            MenuItem::Separator(label) => format!("__sep__{label}"),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        match self {
            MenuItem::Entry(item) => item.fzf_preview(),
            MenuItem::Separator(_) => FzfPreview::None,
        }
    }

    fn fzf_is_selectable(&self) -> bool {
        matches!(self, MenuItem::Entry(_))
    }

    fn fzf_search_keywords(&self) -> &[&str] {
        match self {
            MenuItem::Entry(item) => item.fzf_search_keywords(),
            MenuItem::Separator(_) => &[],
        }
    }
}

/// Outcome of an interaction that either submits a value or is cancelled.
///
/// Renderer, protocol, and process failures are represented by the outer
/// `anyhow::Result`, never by this type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DialogOutcome<T> {
    Submitted(T),
    Cancelled,
}

impl<T> DialogOutcome<T> {
    /// Transform a submitted value, preserving cancellation.
    pub fn map<U, F: FnOnce(T) -> U>(self, f: F) -> DialogOutcome<U> {
        match self {
            DialogOutcome::Submitted(value) => DialogOutcome::Submitted(f(value)),
            DialogOutcome::Cancelled => DialogOutcome::Cancelled,
        }
    }
}

/// What a selection menu returned.
///
/// `items` is the submitted selection set: every tab-selected row in
/// multi-select menus, or the focused row otherwise. `action` is set when
/// the user pressed one of the menu's registered [`MenuKeybind`]s instead of
/// submitting with Enter; the items the bind was pressed on ride along.
/// An action may legitimately carry no items (e.g. the bind was pressed
/// while the filtered list was empty).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuSelection<T, A = ()> {
    pub items: Vec<T>,
    pub action: Option<A>,
}

impl<T> MenuSelection<T, ()> {
    /// A plain submission without a pressed keybind.
    pub fn from_items(items: Vec<T>) -> Self {
        Self {
            items,
            action: None,
        }
    }
}

impl<T, A> MenuSelection<T, A> {
    /// Take the submitted items, discarding any pressed keybind action.
    pub fn into_items(self) -> Vec<T> {
        self.items
    }

    /// The single submitted item.
    ///
    /// Errors when the menu did not submit exactly one item (e.g. a keybind
    /// fired with an empty filtered list, or a multi-selection was returned
    /// to a single-pick terminal).
    pub fn into_single(self) -> Result<T> {
        if self.items.len() != 1 {
            bail!(
                "expected exactly one selected item, got {}",
                self.items.len()
            );
        }
        Ok(self.items.into_iter().next().expect("checked length"))
    }
}

/// A validated fzf key name for a menu keybind (e.g. `"ctrl-e"`).
///
/// Construction rejects malformed names and keys that would collide with
/// built-in navigation or make the menu impossible to dismiss (scrolling,
/// separator navigation, tab multi-select, and fzf's abort keys).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuKey(&'static str);

impl MenuKey {
    /// Keys that would break menu navigation or trap the user in the menu.
    const RESERVED: &[&str] = &[
        // Abort / dismissal.
        "esc",
        "ctrl-c",
        "ctrl-g",
        "ctrl-q",
        // Submit (enter and its aliases).
        "enter",
        "ctrl-m",
        // Multi-select toggles.
        "tab",
        "btab",
        "shift-tab",
        // Cursor navigation (including fzf's emacs-mode defaults).
        "up",
        "down",
        "ctrl-p",
        "ctrl-n",
        "ctrl-k",
        "ctrl-j",
    ];

    /// Validate a static fzf key name such as `"ctrl-e"`, `"alt-s"`, or
    /// `"f3"`.
    pub fn new(key: &'static str) -> Result<Self> {
        if key.is_empty() {
            bail!("menu keybind name cannot be empty");
        }
        if !key
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            bail!(
                "menu keybind {key:?} must be a lowercase fzf key name like \"ctrl-e\" or \"alt-s\""
            );
        }
        if Self::RESERVED.contains(&key) {
            bail!("menu keybind {key:?} is reserved for navigation or dismissal");
        }
        Ok(Self(key))
    }

    pub fn as_str(&self) -> &'static str {
        self.0
    }
}

impl fmt::Display for MenuKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

/// A global keybind registered on a selection menu.
///
/// Pressing `key` terminates fzf and returns `action` alongside the current
/// selection set; `label` is shown as a dimmed hint line in the menu header.
/// Keybinds are always global for the whole menu (fzf has no per-item
/// bindings); per-item semantics are the caller's dispatch on the returned
/// `(action, items)` pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuKeybind<A> {
    pub key: MenuKey,
    pub label: String,
    pub action: A,
}

impl<A> MenuKeybind<A> {
    pub fn new(key: MenuKey, label: impl Into<String>, action: A) -> Self {
        Self {
            key,
            label: label.into(),
            action,
        }
    }
}

/// Visual treatment of rows in a selection menu.
///
/// This affects presentation only. Preview delivery and execution semantics are
/// identical for every presentation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MenuPresentation {
    #[default]
    Compact,
    /// Add vertical breathing room and icon-aware shadows around each row.
    Padded,
}

/// Result type for checklist dialogs
#[derive(Debug, Clone, PartialEq)]
pub enum ChecklistResult<T> {
    Confirmed(Vec<T>),
    Action(ChecklistAction),
    Cancelled,
}

/// Result type for confirmation dialogs
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ConfirmResult {
    Yes,
    No,
    Cancelled,
}

/// Initial cursor position for FZF menus
#[derive(Debug, Clone)]
pub(crate) enum InitialCursor {
    Index(usize),
}

/// Header type for FZF menus with different padding and styling options.
#[derive(Debug, Clone)]
pub enum Header {
    /// Manual header - passed verbatim to fzf (no modifications)
    Manual(String),
    /// Default header - adds standard wrapper padding (\n{text}\n )
    Default(String),
    /// Fancy header - styled with separators and colors
    Fancy(String),
}

impl Header {
    /// Create a default header (with standard wrapper padding)
    pub fn default(text: &str) -> Self {
        Header::Default(text.to_string())
    }

    /// Create a fancy header (styled with separators and colors)
    pub fn fancy(text: &str) -> Self {
        Header::Fancy(text.to_string())
    }

    /// Render to fzf-compatible string with appropriate padding/formatting
    pub(crate) fn to_fzf_string(&self) -> String {
        match self {
            Header::Manual(text) => text.clone(),
            Header::Default(text) => format!("\n{}\n ", text),
            Header::Fancy(text) => {
                // Inline the fancy header styling (moved from format_styled_header)
                let reset = "\x1b[0m";
                let surface = hex_to_ansi_fg(colors::SURFACE1);
                let separator = "──────────────────────────────────────";
                format!("\n{surface}{separator}{reset}\n{text}\n{surface}{separator}{reset}\n ")
            }
        }
    }
}

// Convenience implementations allow strings to be passed directly to .header() method
impl From<&str> for Header {
    fn from(s: &str) -> Self {
        Header::Default(s.to_string())
    }
}

impl From<String> for Header {
    fn from(s: String) -> Self {
        Header::Default(s)
    }
}

impl From<&String> for Header {
    fn from(s: &String) -> Self {
        Header::Default(s.clone())
    }
}

/// Wrapper for items in a checklist dialog with checkbox state.
#[derive(Clone)]
pub struct ChecklistItem<T> {
    /// The underlying item
    pub item: T,
    /// Current checked state
    pub checked: bool,
    display_text: String,
}

impl<T: FzfSelectable> ChecklistItem<T> {
    pub fn new(item: T) -> Self {
        let checked = item.fzf_initial_checked_state();
        Self {
            display_text: Self::format_display(&item, checked),
            item,
            checked,
        }
    }

    pub fn toggle(&mut self) {
        self.checked = !self.checked;
        self.display_text = Self::format_display(&self.item, self.checked);
    }

    fn format_display(item: &T, checked: bool) -> String {
        // Use ASCII-only checkbox with ANSI colors
        // [ ] in dimmed color for unchecked, [x] in green for checked
        let checkbox = if checked {
            let green = hex_to_ansi_fg(colors::GREEN);
            format!("{green}[x]{RESET} ")
        } else {
            let subtext = hex_to_ansi_fg(colors::SUBTEXT0);
            format!("{subtext}[ ]{RESET} ")
        };
        format!("{}{}", checkbox, item.fzf_display_text())
    }
}

impl<T: FzfSelectable> FzfSelectable for ChecklistItem<T> {
    fn fzf_display_text(&self) -> String {
        self.display_text.clone()
    }

    fn fzf_preview(&self) -> FzfPreview {
        self.item.fzf_preview()
    }

    fn fzf_key(&self) -> String {
        self.item.fzf_key()
    }

    fn fzf_initial_checked_state(&self) -> bool {
        self.checked
    }
}

/// Special marker item for checklist confirm action.
/// Appears at the bottom of the checklist as a distinct option.
#[derive(Clone)]
pub struct ChecklistConfirm {
    pub text: String,
}

impl ChecklistConfirm {
    pub fn new(text: &str) -> Self {
        Self {
            text: text.to_string(),
        }
    }

    /// Special key that identifies this as the confirm action.
    /// This unique prefix ensures it doesn't collide with real item keys.
    pub fn confirm_key() -> &'static str {
        "__CHECKLIST_CONFIRM__"
    }
}

impl FzfSelectable for ChecklistConfirm {
    fn fzf_display_text(&self) -> String {
        // Use ASCII arrow instead of nerd font symbol
        let blue = hex_to_ansi_fg(colors::BLUE);
        format!("{blue}→ {RESET}{}", self.text)
    }

    fn fzf_key(&self) -> String {
        Self::confirm_key().to_string()
    }

    fn fzf_initial_checked_state(&self) -> bool {
        false
    }
}

/// Non-checkbox action item for checklists (e.g., "Auto defaults")
#[derive(Clone, Debug, PartialEq)]
pub struct ChecklistAction {
    pub key: String,
    pub text: String,
    pub preview: FzfPreview,
    pub color: &'static str,
}

impl ChecklistAction {
    pub fn new<K: Into<String>, T: Into<String>>(key: K, text: T) -> Self {
        Self {
            key: key.into(),
            text: text.into(),
            preview: FzfPreview::None,
            color: colors::BLUE,
        }
    }

    pub fn with_preview(mut self, preview: FzfPreview) -> Self {
        self.preview = preview;
        self
    }

    pub fn with_color(mut self, color: &'static str) -> Self {
        self.color = color;
        self
    }
}

impl FzfSelectable for ChecklistAction {
    fn fzf_display_text(&self) -> String {
        let color = hex_to_ansi_fg(self.color);
        format!("{color}→ {RESET}{}", self.text)
    }

    fn fzf_preview(&self) -> FzfPreview {
        self.preview.clone()
    }

    fn fzf_key(&self) -> String {
        self.key.clone()
    }

    fn fzf_initial_checked_state(&self) -> bool {
        false
    }
}

/// Intermediate result from a single checklist iteration.
/// Used internally during the loop/reload pattern.
pub(crate) enum ChecklistSelection {
    Cancelled,      // User pressed Esc/Ctrl-C
    EmptyQuery,     // User pressed Enter with empty query (should ask to discard)
    NotFound,       // User typed a query that doesn't match any item
    Toggled(usize), // Index of item that was toggled
    Confirmed,      // User selected confirm option
    Action(String), // Selected action key
}

/// Display data for an FZF menu item.
///
/// Contains all information needed to render an item in fzf and map
/// user selections back to the original item.
#[derive(Clone, Debug)]
pub struct ItemDisplayData {
    /// The visible text shown in the fzf list (may contain ANSI codes)
    pub display_text: String,
    /// Unique key for identifying this item
    pub key: String,
    /// Hidden search keywords for fuzzy matching (space-separated)
    pub keywords: String,
    /// Whether this item is selectable (false for separators)
    pub is_selectable: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_command_is_stable() {
        assert!(streaming_preview_command().contains("base64 -d"));
    }

    #[test]
    fn default_fzf_key_strips_csi_sequences() {
        let display = "\x1b[32mAdd\x1b[0m";
        assert_eq!(default_fzf_key(display), "Add");
    }

    #[test]
    fn default_fzf_key_strips_osc_sequences() {
        let display = "\x1b]8;;https://example.com\x1b\\Open\x1b]8;;\x1b\\";
        assert_eq!(default_fzf_key(display), "Open");
    }

    #[test]
    fn default_fzf_key_preserves_plain_text() {
        let display = "Plain item";
        assert_eq!(default_fzf_key(display), "Plain item");
    }
}
