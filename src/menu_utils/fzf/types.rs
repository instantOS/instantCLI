//! Core types and traits for the FZF wrapper

use crate::ui::catppuccin::{colors, hex_to_ansi_fg};
use serde::{Deserialize, Serialize};

const RESET: &str = "\x1b[0m";

/// Preview content for FZF items
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FzfPreview {
    Text(String),
    Command(String),
    None,
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

    /// Unique key for identifying this item (defaults to display text).
    fn fzf_key(&self) -> String {
        self.fzf_display_text()
    }

    /// Optional: provide initial checked state for checklists.
    /// Default implementation returns false (unchecked).
    /// Only used by DialogType::Checklist.
    fn fzf_initial_checked_state(&self) -> bool {
        false
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

/// Result type for FZF operations
#[derive(Debug, PartialEq)]
pub enum FzfResult<T> {
    Selected(T),
    MultiSelected(Vec<T>),
    Cancelled,
    Error(String),
}

/// Result type for confirmation dialogs
#[derive(Debug, Clone, PartialEq)]
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
    /// Create a manual header (passed verbatim to fzf)
    pub fn manual(text: &str) -> Self {
        Header::Manual(text.to_string())
    }

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
                let surface =
                    crate::ui::catppuccin::hex_to_ansi_fg(crate::ui::catppuccin::colors::SURFACE1);
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

    pub fn set_checked(&mut self, checked: bool) {
        self.checked = checked;
        self.display_text = Self::format_display(&self.item, checked);
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
    pub fn confirm_key() -> String {
        "__CHECKLIST_CONFIRM__".to_string()
    }
}

impl FzfSelectable for ChecklistConfirm {
    fn fzf_display_text(&self) -> String {
        // Use ASCII arrow instead of nerd font symbol
        let blue = hex_to_ansi_fg(colors::BLUE);
        format!("{blue}→ {RESET}{}", self.text)
    }

    fn fzf_key(&self) -> String {
        Self::confirm_key()
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
}
