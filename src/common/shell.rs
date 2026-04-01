//! Shell utility functions
//!
//! This module provides common shell manipulation utilities used across the application.

use std::env;
use std::path::PathBuf;

/// Escape a string for use in a shell command
///
/// This function quotes the string only if necessary (i.e., if it contains characters
/// that have special meaning in the shell). It uses single quotes for safety.
///
/// # Examples
///
/// ```
/// use crate::common::shell::shell_quote;
///
/// assert_eq!(shell_quote("simple"), "simple");
/// assert_eq!(shell_quote("has spaces"), "'has spaces'");
/// assert_eq!(shell_quote("has'quote"), "'has'\\''quote'");
/// ```
pub fn shell_quote(s: &str) -> String {
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

/// Get the current executable path as a shell-quoted command string.
/// Falls back to "ins" if the path cannot be determined.
/// Resolve the persistent path to the current `ins` binary.
///
/// When running from an AppImage, `std::env::current_exe()` may point into a
/// temporary FUSE mount. `APPIMAGE` holds the real persistent AppImage path.
pub fn resolve_current_binary() -> PathBuf {
    if let Ok(appimage_path) = env::var("APPIMAGE") {
        return PathBuf::from(appimage_path);
    }
    env::current_exe().unwrap_or_else(|_| PathBuf::from("ins"))
}

pub fn current_exe_command() -> String {
    shell_quote(&resolve_current_binary().to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shell_quote() {
        assert_eq!(shell_quote(""), "''");
        assert_eq!(shell_quote("foo"), "foo");
        assert_eq!(shell_quote("foo bar"), "'foo bar'");
        assert_eq!(shell_quote("foo'bar"), "'foo'\\''bar'");
        assert_eq!(shell_quote("path/to/file"), "path/to/file");
        assert_eq!(shell_quote("--flag=value"), "--flag=value");
    }
}
