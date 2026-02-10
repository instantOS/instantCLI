//! Theme and color configuration for FZF

/// Get Catppuccin-themed color arguments for fzf
pub(crate) fn theme_args() -> Vec<&'static str> {
    vec![
        "--color=bg+:#313244",
        "--color=bg:#1E1E2E",
        "--color=spinner:#F5E0DC",
        "--color=hl:#F38BA8",
        "--color=fg:#CDD6F4",
        "--color=header:#CDD6F4",
        "--color=info:#CBA6F7",
        "--color=pointer:#F5E0DC",
        "--color=marker:#B4BEFE",
        "--color=fg+:#CDD6F4",
        "--color=prompt:#CBA6F7",
        "--color=hl+:#F38BA8",
        "--color=selected-bg:#45475A",
        "--color=border:#6C7086",
        "--color=label:#CDD6F4",
    ]
}
