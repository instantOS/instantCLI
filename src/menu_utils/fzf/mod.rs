//! FZF wrapper for modern fzf versions
//!
//! This module provides a wrapper around fzf targeting version 0.66.x or newer.
//!
//! ## Version Requirements
//!
//! If fzf fails with "unknown option" or similar errors indicating an old version,
//! the program will exit with a message directing the user to upgrade fzf.
//! We recommend using `mise` for managing fzf versions.
//!
//! ## Environment Handling
//!
//! All fzf invocations clear `FZF_DEFAULT_OPTS` to avoid conflicts with user/system-wide
//! settings that may contain unsupported options.

mod types;
mod builder;
mod wrapper;
mod preview;
mod theme;
mod utils;

// Re-export public API for backward compatibility
pub use types::{ConfirmResult, FzfPreview, FzfResult, FzfSelectable, Header};

// Re-export main user-facing types
pub use builder::FzfBuilder;
pub use wrapper::FzfWrapper;

// Internal use (pub(crate) for other modules)
pub(crate) use types::InitialCursor;
pub(crate) use preview::{PreviewStrategy, PreviewUtils};
pub(crate) use utils::{check_for_old_fzf_and_exit, extract_icon_padding, log_fzf_failure};
