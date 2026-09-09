use std::{
    io::{IsTerminal, Write},
    path::{Path, PathBuf},
    process::Command,
    sync::{LazyLock, Once},
};

use crate::ui::nerd_font::NerdFont;

/// Standard 16-color ANSI terminal color definitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnsiColor {
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
    BrightBlack,
    BrightRed,
    BrightGreen,
    BrightYellow,
    BrightBlue,
    BrightMagenta,
    BrightCyan,
    BrightWhite,
}

impl AnsiColor {
    /// ANSI SGR escape code for foreground text.
    pub const fn fg_code(self) -> u8 {
        match self {
            Self::Black => 30,
            Self::Red => 31,
            Self::Green => 32,
            Self::Yellow => 33,
            Self::Blue => 34,
            Self::Magenta => 35,
            Self::Cyan => 36,
            Self::White => 37,
            Self::BrightBlack => 90,
            Self::BrightRed => 91,
            Self::BrightGreen => 92,
            Self::BrightYellow => 93,
            Self::BrightBlue => 94,
            Self::BrightMagenta => 95,
            Self::BrightCyan => 96,
            Self::BrightWhite => 97,
        }
    }

    /// ANSI SGR escape code for background color.
    pub const fn bg_code(self) -> u8 {
        match self {
            Self::Black => 40,
            Self::Red => 41,
            Self::Green => 42,
            Self::Yellow => 43,
            Self::Blue => 44,
            Self::Magenta => 45,
            Self::Cyan => 46,
            Self::White => 47,
            Self::BrightBlack => 100,
            Self::BrightRed => 101,
            Self::BrightGreen => 102,
            Self::BrightYellow => 103,
            Self::BrightBlue => 104,
            Self::BrightMagenta => 105,
            Self::BrightCyan => 106,
            Self::BrightWhite => 107,
        }
    }

    /// Color name understood by fzf's `--color` option.
    pub const fn fzf_name(self) -> &'static str {
        match self {
            Self::Black => "black",
            Self::Red => "red",
            Self::Green => "green",
            Self::Yellow => "yellow",
            Self::Blue => "blue",
            Self::Magenta => "magenta",
            Self::Cyan => "cyan",
            Self::White => "white",
            Self::BrightBlack => "bright-black",
            Self::BrightRed => "bright-red",
            Self::BrightGreen => "bright-green",
            Self::BrightYellow => "bright-yellow",
            Self::BrightBlue => "bright-blue",
            Self::BrightMagenta => "bright-magenta",
            Self::BrightCyan => "bright-cyan",
            Self::BrightWhite => "bright-white",
        }
    }

    pub fn to_fg_escape(self) -> String {
        format!("\x1b[{}m", self.fg_code())
    }

    pub fn to_bg_escape(self) -> String {
        format!("\x1b[{}m", self.bg_code())
    }
}

/// Terminal color capability modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorMode {
    TrueColor,
    Ansi16,
}

static LINUX_CONSOLE_DEVICE: LazyLock<Option<PathBuf>> = LazyLock::new(detect_linux_console_device);

static DETECTED_COLOR_MODE: LazyLock<ColorMode> = LazyLock::new(|| {
    if let Ok(val) = std::env::var("INS_COLOR_MODE") {
        if val == "16" {
            return ColorMode::Ansi16;
        }
        if val.eq_ignore_ascii_case("truecolor") || val == "24" {
            return ColorMode::TrueColor;
        }
    }
    if LINUX_CONSOLE_DEVICE.is_some() {
        return ColorMode::Ansi16;
    }
    ColorMode::TrueColor
});

fn detect_linux_console_device() -> Option<PathBuf> {
    if !std::io::stdout().is_terminal() {
        return None;
    }

    if std::env::var("TERM").is_ok_and(|term| term.starts_with("linux")) {
        return Some(PathBuf::from("/dev/tty"));
    }

    std::env::var_os("TMUX")?;
    let output = Command::new("tmux")
        .args(["display-message", "-p", "#{client_termname}\t#{client_tty}"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let details = String::from_utf8(output.stdout).ok()?;
    let (term, tty) = details.trim().split_once('\t')?;
    let tty = Path::new(tty);
    if term.starts_with("linux") && is_virtual_console_path(tty) {
        Some(tty.to_path_buf())
    } else {
        None
    }
}

fn is_virtual_console_path(path: &Path) -> bool {
    path.to_str()
        .and_then(|path| path.strip_prefix("/dev/tty"))
        .is_some_and(|number| {
            !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
        })
}

pub fn color_mode() -> ColorMode {
    *DETECTED_COLOR_MODE
}

pub fn is_console_mode() -> bool {
    color_mode() == ColorMode::Ansi16
}

/// Linux virtual console DAC palette mapping for Catppuccin Mocha.
#[allow(dead_code)]
pub struct ConsolePaletteEntry {
    pub ansi: AnsiColor,
    pub hex: &'static str,
}

pub const CATPPUCCIN_CONSOLE_PALETTE: [ConsolePaletteEntry; 16] = [
    ConsolePaletteEntry {
        ansi: AnsiColor::Black,
        hex: "1e1e2e",
    }, // Base (terminal background)
    ConsolePaletteEntry {
        ansi: AnsiColor::Red,
        hex: "f38ba8",
    }, // Red
    ConsolePaletteEntry {
        ansi: AnsiColor::Green,
        hex: "a6e3a1",
    }, // Green
    ConsolePaletteEntry {
        ansi: AnsiColor::Yellow,
        hex: "f9e2af",
    }, // Yellow
    ConsolePaletteEntry {
        ansi: AnsiColor::Blue,
        hex: "89b4fa",
    }, // Blue
    ConsolePaletteEntry {
        ansi: AnsiColor::Magenta,
        hex: "cba6f7",
    }, // Mauve
    ConsolePaletteEntry {
        ansi: AnsiColor::Cyan,
        hex: "94e2d5",
    }, // Teal
    ConsolePaletteEntry {
        ansi: AnsiColor::White,
        hex: "bac2de",
    }, // Subtext1
    ConsolePaletteEntry {
        ansi: AnsiColor::BrightBlack,
        hex: "585b70",
    }, // Surface2
    ConsolePaletteEntry {
        ansi: AnsiColor::BrightRed,
        hex: "f38ba8",
    },
    ConsolePaletteEntry {
        ansi: AnsiColor::BrightGreen,
        hex: "a6e3a1",
    },
    ConsolePaletteEntry {
        ansi: AnsiColor::BrightYellow,
        hex: "f9e2af",
    },
    ConsolePaletteEntry {
        ansi: AnsiColor::BrightBlue,
        hex: "89b4fa",
    },
    ConsolePaletteEntry {
        ansi: AnsiColor::BrightMagenta,
        hex: "f5c2e7",
    }, // Pink
    ConsolePaletteEntry {
        ansi: AnsiColor::BrightCyan,
        hex: "89dceb",
    }, // Sky
    ConsolePaletteEntry {
        ansi: AnsiColor::BrightWhite,
        hex: "cdd6f4",
    }, // Text (terminal foreground)
];

static PALETTE_INITIALIZED: Once = Once::new();

/// Reprogram the Linux virtual console DAC palette to Catppuccin Mocha.
///
/// Executes at most once per process and only when in 16-color console mode.
pub fn ensure_tty_palette() {
    if !is_console_mode() {
        return;
    }
    PALETTE_INITIALIZED.call_once(|| {
        let mut sequences = String::with_capacity(16 * 10);
        for (i, entry) in CATPPUCCIN_CONSOLE_PALETTE.iter().enumerate() {
            sequences.push_str(&format!("\x1b]P{:x}{}", i, entry.hex));
        }

        if let Some(device) = LINUX_CONSOLE_DEVICE.as_deref()
            && let Ok(mut tty) = std::fs::OpenOptions::new().write(true).open(device)
        {
            let _ = tty.write_all(sequences.as_bytes());
            let _ = tty.flush();
        }
    });
}

/// Map a Catppuccin hex color to its corresponding 16-color ANSI representation.
pub fn hex_to_ansi_color(hex: &str) -> Option<AnsiColor> {
    let clean = hex.trim_start_matches('#').to_ascii_lowercase();
    let color = match clean.as_str() {
        // Base backgrounds -> Black
        "1e1e2e" | "181825" | "11111b" => AnsiColor::Black,
        // Reds
        "f38ba8" | "eba0ac" => AnsiColor::Red,
        // Greens
        "a6e3a1" => AnsiColor::Green,
        // Yellows
        "f9e2af" | "fab387" => AnsiColor::Yellow,
        // Blues
        "89b4fa" | "74c7ec" => AnsiColor::Blue,
        // Mauve / Magenta
        "cba6f7" => AnsiColor::Magenta,
        // Teal / Cyan
        "94e2d5" => AnsiColor::Cyan,
        // Subtext / White
        "bac2de" | "a6adc8" => AnsiColor::White,
        // Surfaces / Overlays -> BrightBlack (selection & borders)
        "585b70" | "45475a" | "313244" | "6c7086" | "7f849c" | "9399b2" => AnsiColor::BrightBlack,
        // Lavender -> BrightBlue
        "b4befe" => AnsiColor::BrightBlue,
        // Pink / Rosewater -> BrightMagenta
        "f5c2e7" | "f5e0dc" | "f2cdcd" => AnsiColor::BrightMagenta,
        // Sky -> BrightCyan
        "89dceb" => AnsiColor::BrightCyan,
        // Text -> BrightWhite
        "cdd6f4" => AnsiColor::BrightWhite,
        _ => nearest_console_color(parse_hex_rgb(&clean)?),
    };
    Some(color)
}

fn nearest_console_color((red, green, blue): (u8, u8, u8)) -> AnsiColor {
    CATPPUCCIN_CONSOLE_PALETTE
        .iter()
        .filter_map(|entry| {
            let (palette_red, palette_green, palette_blue) = parse_hex_rgb(entry.hex)?;
            let distance = i32::from(red).abs_diff(i32::from(palette_red)).pow(2)
                + i32::from(green).abs_diff(i32::from(palette_green)).pow(2)
                + i32::from(blue).abs_diff(i32::from(palette_blue)).pow(2);
            Some((distance, entry.ansi))
        })
        .min_by_key(|(distance, _)| *distance)
        .map_or(AnsiColor::BrightWhite, |(_, ansi)| ansi)
}

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

/// Convert hex color (`#RRGGBB`) to a foreground escape.
///
/// Emits 24-bit TrueColor escapes in GUI terminals and standard 16-color ANSI escapes
/// in Linux virtual consoles.
pub fn hex_to_ansi_fg(hex: &str) -> String {
    if is_console_mode() {
        return hex_to_ansi_color(hex).map_or_else(String::new, AnsiColor::to_fg_escape);
    }

    let Some((r, g, b)) = parse_hex_rgb(hex) else {
        return String::new();
    };

    format!("\x1b[38;2;{r};{g};{b}m")
}

/// Convert hex color (`#RRGGBB`) to a background escape.
///
/// Emits 24-bit TrueColor escapes in GUI terminals and standard 16-color ANSI escapes
/// in Linux virtual consoles.
pub fn hex_to_ansi_bg(hex: &str) -> String {
    if is_console_mode() {
        return hex_to_ansi_color(hex).map_or_else(String::new, AnsiColor::to_bg_escape);
    }

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

/// Render text as bold for fzf rows. Uses SGR 1 + 22 (disable bold) so any
/// surrounding styling (e.g. ANSI color codes from `format_with_color`)
/// isn't disturbed by a blanket `\x1b[0m` reset.
pub fn format_bold(text: &str) -> String {
    format!("\x1b[1m{text}\x1b[22m")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn console_palette_has_16_entries() {
        assert_eq!(CATPPUCCIN_CONSOLE_PALETTE.len(), 16);
        for entry in &CATPPUCCIN_CONSOLE_PALETTE {
            assert_eq!(entry.hex.len(), 6);
        }
    }

    #[test]
    fn ansi_color_codes_are_valid() {
        assert_eq!(AnsiColor::Black.fg_code(), 30);
        assert_eq!(AnsiColor::BrightWhite.fg_code(), 97);
        assert_eq!(AnsiColor::Black.bg_code(), 40);
        assert_eq!(AnsiColor::BrightWhite.bg_code(), 107);
    }

    #[test]
    fn hex_to_ansi_color_mappings() {
        assert_eq!(hex_to_ansi_color(colors::BASE), Some(AnsiColor::Black));
        assert_eq!(
            hex_to_ansi_color(colors::TEXT),
            Some(AnsiColor::BrightWhite)
        );
        assert_eq!(hex_to_ansi_color(colors::RED), Some(AnsiColor::Red));
        assert_eq!(hex_to_ansi_color(colors::GREEN), Some(AnsiColor::Green));
        assert_eq!(hex_to_ansi_color(colors::YELLOW), Some(AnsiColor::Yellow));
        assert_eq!(hex_to_ansi_color(colors::BLUE), Some(AnsiColor::Blue));
        assert_eq!(hex_to_ansi_color(colors::MAUVE), Some(AnsiColor::Magenta));
        assert_eq!(hex_to_ansi_color(colors::TEAL), Some(AnsiColor::Cyan));
    }

    #[test]
    fn hex_to_ansi_color_accepts_uppercase_and_custom_colors() {
        assert_eq!(hex_to_ansi_color("#89B4FA"), Some(AnsiColor::Blue));
        assert_eq!(hex_to_ansi_color("#1f1f2f"), Some(AnsiColor::Black));
        assert_eq!(hex_to_ansi_color("not-a-color"), None);
    }

    #[test]
    fn virtual_console_paths_are_strictly_validated() {
        assert!(is_virtual_console_path(Path::new("/dev/tty1")));
        assert!(is_virtual_console_path(Path::new("/dev/tty12")));
        assert!(!is_virtual_console_path(Path::new("/dev/tty")));
        assert!(!is_virtual_console_path(Path::new("/dev/ttyS0")));
        assert!(!is_virtual_console_path(Path::new("/dev/pts/1")));
        assert!(!is_virtual_console_path(Path::new("/dev/tty1/other")));
    }
}
