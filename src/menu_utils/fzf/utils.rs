//! Utility functions for FZF wrapper

use crate::ui::{self, Level};

/// Check if the error indicates an old fzf version and exit if so
pub(crate) fn check_for_old_fzf_and_exit(stderr: &[u8]) {
    let stderr_str = String::from_utf8_lossy(stderr);
    if stderr_str.contains("unknown option")
        || stderr_str.contains("invalid option")
        || stderr_str.contains("invalid color specification")
        || stderr_str.contains("unrecognized option")
    {
        eprintln!("\n{}\n", "=".repeat(70));
        eprintln!("ERROR: Your fzf version is too old");
        eprintln!("{}\n", "=".repeat(70));
        eprintln!("This program requires fzf 0.66.x or newer.");
        eprintln!("Your current fzf version does not support required options.\n");
        eprintln!("To upgrade fzf, we recommend using mise:");
        eprintln!("  https://mise.jdx.dev/\n");
        eprintln!("Install mise and then run:");
        eprintln!("  mise use -g fzf@latest\n");
        eprintln!("Error details: {}", stderr_str.trim());
        eprintln!("{}\n", "=".repeat(70));
        std::process::exit(1);
    }
}

pub(crate) fn log_fzf_failure(stderr: &[u8], exit_code: Option<i32>) {
    if crate::ui::is_debug_enabled() {
        let stderr_str = String::from_utf8_lossy(stderr);
        let code_str = exit_code
            .map(|c| format!("exit code {}", c))
            .unwrap_or_else(|| "unknown".to_string());

        crate::ui::emit(
            crate::ui::Level::Debug,
            "fzf.execution_failed",
            &format!("FZF execution failed ({}): {}", code_str, stderr_str.trim()),
            None,
        );
    }
}

/// Extract the icon's colored background from display text and create matching padding.
/// The icon format is: \x1b[48;2;R;G;Bm\x1b[38;2;r;g;bm  {icon}  \x1b[49;39m ...
/// Returns (top_padding, bottom_padding_with_shadow) where the bottom padding has a
/// subtle darkened shadow effect using a Unicode lower block character.
pub(crate) fn extract_icon_padding(display: &str) -> (String, String) {
    // Look for ANSI 24-bit background color code: \x1b[48;2;R;G;Bm
    if let Some(start) = display.find("\x1b[48;2;") {
        // Find the end of the color code (the 'm')
        if let Some(end_offset) = display[start..].find('m') {
            let bg_code = &display[start..start + end_offset + 1];
            // Parse RGB values to create a darkened version for the shadow
            let rgb_part = &display[start + 7..start + end_offset]; // "R;G;B"
            let parts: Vec<&str> = rgb_part.split(';').collect();

            let reset = "\x1b[49;39m";
            let top_padding = format!("  {bg_code}       {reset}");

            // Create shadow effect using lower block character with darkened foreground
            if parts.len() == 3
                && let (Ok(r), Ok(g), Ok(b)) = (
                    parts[0].parse::<u8>(),
                    parts[1].parse::<u8>(),
                    parts[2].parse::<u8>(),
                )
            {
                let dark_r = r / 2;
                let dark_g = g / 2;
                let dark_b = b / 2;
                // Use lower one-quarter block (▂) with darkened foreground on same background
                let shadow_fg = format!("\x1b[38;2;{};{};{}m", dark_r, dark_g, dark_b);
                // The shadow character is at the bottom, creating a subtle border effect
                let bottom_with_shadow = format!("  {bg_code}{shadow_fg}▂▂▂▂▂▂▂{reset}");
                return (top_padding, bottom_with_shadow);
            }

            // Fallback if RGB parsing fails
            return (top_padding.clone(), top_padding);
        }
    }
    // Fallback: just return spaces for padding
    (" ".to_string(), " ".to_string())
}

// UNUSED: Consider removing - not used anywhere in the codebase
/// Strip ANSI escape codes from a string
pub(crate) fn strip_ansi_codes(s: &str) -> String {
    let mut result = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Skip until we find 'm' (end of color code)
            while let Some(&next) = chars.peek() {
                chars.next();
                if next == 'm' {
                    break;
                }
            }
        } else {
            result.push(c);
        }
    }
    result
}
