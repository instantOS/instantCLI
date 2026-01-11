//! Utility functions for FZF wrapper

use crossterm::terminal;

/// Get terminal dimensions (columns, rows) using crossterm.
pub(crate) fn get_terminal_dimensions() -> Option<(u16, u16)> {
    match terminal::size() {
        Ok((cols, rows)) if cols > 0 && rows > 0 => Some((cols, rows)),
        _ => {
            // Fallback to environment variables
            let cols = std::env::var("COLUMNS").ok()?.parse::<u16>().ok()?;
            let rows = std::env::var("LINES").ok()?.parse::<u16>().ok()?;
            if cols > 0 && rows > 0 {
                Some((cols, rows))
            } else {
                None
            }
        }
    }
}

/// Responsive layout settings for fzf based on terminal dimensions.
pub(crate) struct ResponsiveLayout {
    /// The preview window argument (e.g., "--preview-window=down:50%")
    pub preview_window: &'static str,
    /// The margin argument value (e.g., "2%,2%")
    pub margin: &'static str,
}

/// Get the responsive layout settings based on terminal dimensions.
/// Returns appropriate preview window and margin settings:
/// - For narrow (<60 cols) or square-ish (aspect ratio <2:1) terminals:
///   - Preview at bottom, reduced vertical margins (2%) to maximize space
/// - For wide terminals with aspect ratio >=2:1:
///   - Preview on right, larger vertical margins (10%) for visual balance
/// - Falls back to bottom layout if terminal size cannot be detected
pub(crate) fn get_responsive_layout() -> ResponsiveLayout {
    if let Some((cols, rows)) = get_terminal_dimensions() {
        let ratio = cols as f32 / rows as f32;
        // Use bottom if: too narrow (<60 cols) OR aspect ratio <2:1
        if cols < 60 || ratio < 2.0 {
            ResponsiveLayout {
                preview_window: "--preview-window=down:50%",
                margin: "2%,2%", // Minimal vertical margins for stacked layout
            }
        } else {
            ResponsiveLayout {
                preview_window: "--preview-window=right:50%",
                margin: "10%,2%", // More vertical margin when side-by-side layout allows it
            }
        }
    } else {
        // Fallback to down if terminal size can't be detected - safer for narrow terminals
        ResponsiveLayout {
            preview_window: "--preview-window=down:50%",
            margin: "2%,2%",
        }
    }
}

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
