//! Core types and traits for the FZF wrapper

use serde::{Deserialize, Serialize};

/// Preview content for FZF items
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FzfPreview {
    Text(String),
    Command(String),
    None,
}

/// Trait for types that can be displayed in FZF selection menus
pub trait FzfSelectable {
    fn fzf_display_text(&self) -> String;

    fn fzf_preview(&self) -> FzfPreview {
        FzfPreview::None
    }

    fn fzf_key(&self) -> String {
        self.fzf_display_text()
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
