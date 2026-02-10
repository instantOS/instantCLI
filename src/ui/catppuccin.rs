use crate::ui::nerd_font::NerdFont;

/// Catppuccin Mocha color palette.
///
/// Values are hex RGB strings in the `#RRGGBB` format.
#[allow(dead_code)]
pub mod colors {
    // Accent colors
    pub const ROSEWATER: &str = "#f5e0dc";
    pub const FLAMINGO: &str = "#f2cdcd";
    pub const PINK: &str = "#f5c2e7";
    pub const MAUVE: &str = "#cba6f7";
    pub const RED: &str = "#f38ba8";
    pub const MAROON: &str = "#eba0ac";
    pub const PEACH: &str = "#fab387";
    pub const YELLOW: &str = "#f9e2af";
    pub const GREEN: &str = "#a6e3a1";
    pub const TEAL: &str = "#94e2d5";
    pub const SKY: &str = "#89dceb";
    pub const SAPPHIRE: &str = "#74c7ec";
    pub const BLUE: &str = "#89b4fa";
    pub const LAVENDER: &str = "#b4befe";

    // Surface colors
    pub const SURFACE0: &str = "#313244";
    pub const SURFACE1: &str = "#45475a";
    pub const SURFACE2: &str = "#585b70";

    // Overlay colors
    pub const OVERLAY0: &str = "#6c7086";
    pub const OVERLAY1: &str = "#7f849c";
    pub const OVERLAY2: &str = "#9399b2";

    // Text colors
    pub const SUBTEXT0: &str = "#a6adc8";
    pub const SUBTEXT1: &str = "#bac2de";
    pub const TEXT: &str = "#cdd6f4";

    // Base colors (backgrounds)
    pub const BASE: &str = "#1e1e2e";
    pub const MANTLE: &str = "#181825";
    pub const CRUST: &str = "#11111b";
}

fn parse_hex_rgb(hex: &str) -> Option<(u8, u8, u8)> {
    let hex = hex.trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }

    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;

    Some((r, g, b))
}

/// Convert hex color (`#RRGGBB`) to a 24-bit true color foreground escape.
pub fn hex_to_ansi_fg(hex: &str) -> String {
    let Some((r, g, b)) = parse_hex_rgb(hex) else {
        return String::new();
    };

    format!("\x1b[38;2;{r};{g};{b}m")
}

/// Convert hex color (`#RRGGBB`) to a 24-bit true color background escape.
pub fn hex_to_ansi_bg(hex: &str) -> String {
    let Some((r, g, b)) = parse_hex_rgb(hex) else {
        return String::new();
    };

    format!("\x1b[48;2;{r};{g};{b}m")
}

/// Format an icon with colored background badge (uses Catppuccin Blue by default).
pub fn format_icon(icon: NerdFont) -> String {
    format_icon_colored(icon, colors::BLUE)
}

/// Format an icon with a colored background badge (hex format like "#89b4fa").
/// Creates a pill-shaped badge with dark text on colored background.
/// Uses targeted ANSI reset (not \x1b[0m) to preserve FZF color compatibility.
pub fn format_icon_colored(icon: NerdFont, bg_color: &str) -> String {
    let bg = hex_to_ansi_bg(bg_color);
    let fg = hex_to_ansi_fg(colors::CRUST);

    // Reset background (49) and set foreground to match FZF's text color.
    // Using \x1b[49m resets only background; \x1b[39m uses default foreground.
    let reset = "\x1b[49;39m";

    // Padding inside the colored badge
    format!("{bg}{fg}   {}   {reset} ", char::from(icon))
}

/// Format the back button icon with a neutral color.
pub fn format_back_icon() -> String {
    format_icon_colored(NerdFont::ArrowLeft, colors::OVERLAY1)
}

/// Format the search icon with its own color.
pub fn format_search_icon() -> String {
    format_icon_colored(NerdFont::Search, colors::MAUVE)
}

/// Format text with a foreground color (hex format like "#89b4fa").
/// Uses targeted ANSI reset (\x1b[39m) to preserve FZF color compatibility.
pub fn format_with_color(text: &str, color: &str) -> String {
    let fg = hex_to_ansi_fg(color);
    let reset = "\x1b[39m";
    format!("{fg}{text}{reset}")
}

pub fn fzf_mocha_args() -> Vec<String> {
    vec![
        // Visual styling
        "--no-separator".to_string(),
        "--no-bold".to_string(),
        "--padding=1,2".to_string(),
        "--list-border=none".to_string(),
        "--input-border=none".to_string(),
        "--preview-border=left".to_string(),
        "--pointer=▌".to_string(),
        // Search behavior
        "--ignore-case".to_string(),
        // Catppuccin Mocha color scheme
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
    ]
}
