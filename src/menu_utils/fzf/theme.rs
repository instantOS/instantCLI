//! Shared visual and interaction defaults for FZF dialogs.

use crate::ui::catppuccin::{AnsiColor, colors, ensure_tty_palette, is_console_mode};

/// Return the standard instantCLI FZF styling.
///
/// Row density is deliberately not configured here. Compact and three-line
/// rows are selected independently through `MenuPresentation`.
pub(crate) fn theme_args() -> Vec<String> {
    let console_mode = is_console_mode();
    if console_mode {
        ensure_tty_palette();
    }

    let color_args = if console_mode {
        [
            ("bg", AnsiColor::Black.fzf_name()),
            ("bg+", AnsiColor::BrightBlack.fzf_name()),
            ("fg", AnsiColor::BrightWhite.fzf_name()),
            ("fg+", AnsiColor::BrightWhite.fzf_name()),
            ("preview-bg", AnsiColor::Black.fzf_name()),
            ("hl", AnsiColor::Yellow.fzf_name()),
            ("hl+", AnsiColor::Yellow.fzf_name()),
            ("prompt", AnsiColor::BrightWhite.fzf_name()),
            ("pointer", AnsiColor::Blue.fzf_name()),
            ("header", AnsiColor::White.fzf_name()),
            ("border", AnsiColor::BrightBlack.fzf_name()),
            ("gutter", AnsiColor::Black.fzf_name()),
            ("spinner", AnsiColor::BrightMagenta.fzf_name()),
            ("info", AnsiColor::Magenta.fzf_name()),
            ("marker", AnsiColor::BrightBlue.fzf_name()),
            ("selected-bg", AnsiColor::BrightBlack.fzf_name()),
            ("label", AnsiColor::BrightWhite.fzf_name()),
        ]
    } else {
        [
            ("bg", colors::BASE),
            ("bg+", colors::SURFACE0),
            ("fg", colors::TEXT),
            ("fg+", colors::TEXT),
            ("preview-bg", colors::MANTLE),
            ("hl", colors::YELLOW),
            ("hl+", colors::YELLOW),
            ("prompt", colors::TEXT),
            ("pointer", colors::ROSEWATER),
            ("header", colors::TEXT),
            ("border", colors::SURFACE1),
            ("gutter", colors::BASE),
            ("spinner", colors::ROSEWATER),
            ("info", colors::MAUVE),
            ("marker", colors::LAVENDER),
            ("selected-bg", colors::SURFACE1),
            ("label", colors::TEXT),
        ]
    };

    let mut args = vec![
        "--no-separator".to_string(),
        "--no-bold".to_string(),
        "--padding=1,2".to_string(),
        "--list-border=none".to_string(),
        "--input-border=none".to_string(),
        "--preview-border=left".to_string(),
        "--pointer=▌".to_string(),
        "--ignore-case".to_string(),
    ];
    args.extend(
        color_args
            .into_iter()
            .map(|(name, value)| format!("--color={name}:{value}")),
    );
    args
}
