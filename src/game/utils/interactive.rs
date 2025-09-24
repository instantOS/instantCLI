// Common UI utilities for game interactions
// This module can be extended with shared interactive helpers

//TODO: so far this entire module is not useful, thin wrappers around a wrapper are not necessary
use anyhow::{Context, Result};
use crate::fzf_wrapper::FzfWrapper;

/// Display a message to the user with consistent error handling
pub fn show_message(message: &str) -> Result<()> {
    FzfWrapper::message(message).context("Failed to show message")
}

/// Display a success message to the user
pub fn show_success_message(message: &str) -> Result<()> {
    FzfWrapper::message(&format!("✓ {}", message))
        .context("Failed to show success message")
}

/// Display an error message to the user
pub fn show_error_message(message: &str) -> Result<()> {
    FzfWrapper::message(&format!("❌ {}", message))
        .context("Failed to show error message")
}
