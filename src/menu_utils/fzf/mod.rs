//! FZF wrapper for modern fzf versions
//!
//! This module targets fzf 0.66 or newer. Preview word wrapping is enabled when supported.
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

mod builder;
mod header;
mod keybind;
mod preview;
mod theme;
mod types;
mod utils;
mod wrapper;

// Re-export public API
pub use builder::ItemSelection;
pub use header::HeaderBuilder;
pub use types::{
    ChecklistAction, ChecklistResult, ConfirmResult, DecodedStreamingMenuItem, DialogOutcome,
    FzfSelectable, Header, MenuItem, MenuKey, MenuKeybind, MenuSelection, StreamingCommand,
    StreamingMenuItem, default_fzf_key,
};

// Re-export main user-facing types
pub use wrapper::FzfWrapper;
