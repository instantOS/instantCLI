//! Shared visual and interaction defaults for FZF dialogs.

use crate::ui::catppuccin::colors;

/// Return the standard instantCLI FZF styling.
///
/// Row density is deliberately not configured here. Compact and three-line
/// rows are selected independently through `MenuPresentation`.
pub(crate) fn theme_args() -> Vec<String> {
    vec![
        "--no-separator".to_string(),
        "--no-bold".to_string(),
        "--padding=1,2".to_string(),
        "--list-border=none".to_string(),
        "--input-border=none".to_string(),
        "--preview-border=left".to_string(),
        "--pointer=▌".to_string(),
        "--ignore-case".to_string(),
        format!("--color=bg:{}", colors::BASE),
        format!("--color=bg+:{}", colors::SURFACE0),
        format!("--color=fg:{}", colors::TEXT),
        format!("--color=fg+:{}", colors::TEXT),
        format!("--color=preview-bg:{}", colors::MANTLE),
        format!("--color=hl:{}", colors::YELLOW),
        format!("--color=hl+:{}", colors::YELLOW),
        format!("--color=prompt:{}", colors::TEXT),
        format!("--color=pointer:{}", colors::ROSEWATER),
        format!("--color=header:{}", colors::TEXT),
        format!("--color=border:{}", colors::SURFACE1),
        format!("--color=gutter:{}", colors::BASE),
        format!("--color=spinner:{}", colors::ROSEWATER),
        format!("--color=info:{}", colors::MAUVE),
        format!("--color=marker:{}", colors::LAVENDER),
        format!("--color=selected-bg:{}", colors::SURFACE1),
        format!("--color=label:{}", colors::TEXT),
    ]
}
