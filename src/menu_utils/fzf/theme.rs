//! Theme and color configuration for FZF

/// Get Catppuccin-themed color arguments for fzf
pub(crate) fn theme_args() -> Vec<String> {
    vec![
        "--color=bg+:#313244".to_string(),
        "--color=bg:#1E1E2E".to_string(),
        "--color=spinner:#F5E0DC".to_string(),
        "--color=hl:#F38BA8".to_string(),
        "--color=fg:#CDD6F4".to_string(),
        "--color=header:#CDD6F4".to_string(),
        "--color=info:#CBA6F7".to_string(),
        "--color=pointer:#F5E0DC".to_string(),
        "--color=marker:#B4BEFE".to_string(),
        "--color=fg+:#CDD6F4".to_string(),
        "--color=prompt:#CBA6F7".to_string(),
        "--color=hl+:#F38BA8".to_string(),
        "--color=selected-bg:#45475A".to_string(),
        "--color=border:#6C7086".to_string(),
        "--color=label:#CDD6F4".to_string(),
    ]
}
