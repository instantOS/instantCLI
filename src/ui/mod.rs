use colored::*;
use serde::Serialize;
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, RwLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Text,
    Json,
}

#[derive(Debug, Clone, Copy)]
pub enum Level {
    Info,
    Success,
    Warn,
    Error,
    Debug,
}

impl Level {
    fn as_str(self) -> &'static str {
        match self {
            Level::Info => "info",
            Level::Success => "success",
            Level::Warn => "warn",
            Level::Error => "error",
            Level::Debug => "debug",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Renderer {
    pub format: OutputFormat,
    pub color: bool,
}

impl Default for Renderer {
    fn default() -> Self {
        Self {
            format: OutputFormat::Text,
            color: true,
        }
    }
}

static RENDERER: LazyLock<RwLock<Renderer>> = LazyLock::new(|| RwLock::new(Renderer::default()));

// Global debug state
static DEBUG_MODE: AtomicBool = AtomicBool::new(false);

pub fn set_debug_mode(enabled: bool) {
    DEBUG_MODE.store(enabled, Ordering::Relaxed);
}

pub fn is_debug_enabled() -> bool {
    DEBUG_MODE.load(Ordering::Relaxed)
}

pub fn init(format: OutputFormat, color: bool) {
    // Also disable styling produced directly via the `colored` crate
    // (path highlighting etc.), not just the renderer's own coloring.
    colored::control::set_override(color);
    if let Ok(mut r) = RENDERER.write() {
        r.format = format;
        r.color = color;
    }
}

// Custom nerd font icons
pub mod nerd_font;

pub mod catppuccin;

pub mod preview;

pub(crate) mod text;

// Separator characters (not in nerd_font crate)
pub const SEPARATOR_HEAVY: &str = "━";
pub const SEPARATOR_LIGHT: &str = "─";

#[derive(Serialize)]
struct Event<'a> {
    level: &'a str,
    code: &'a str,
    message: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<serde_json::Value>,
}

fn colorize(level: Level, s: &str, enable: bool) -> String {
    if !enable {
        return s.to_string();
    }
    match level {
        Level::Info => s.normal().to_string(),
        Level::Success => s.green().bold().to_string(),
        Level::Warn => s.yellow().bold().to_string(),
        Level::Error => s.red().bold().to_string(),
        Level::Debug => s.cyan().to_string(),
    }
}

pub fn emit(level: Level, code: &str, message: &str, data: Option<serde_json::Value>) {
    let r = RENDERER.read().expect("renderer poisioned").clone();
    match r.format {
        OutputFormat::Text => {
            let line = colorize(level, message, r.color);
            let mut out: Box<dyn Write> = match level {
                Level::Error | Level::Warn => Box::new(io::stderr()),
                _ => Box::new(io::stdout()),
            };
            let _ = writeln!(out, "{}", line);
        }
        OutputFormat::Json => {
            // Ensure message contains no ANSI control sequences in JSON mode
            let clean_msg = text::strip_ansi(message);
            let ev = Event {
                level: level.as_str(),
                code,
                message: &clean_msg,
                data,
            };
            let s = serde_json::to_string(&ev).expect("serialize event");
            let mut out: Box<dyn Write> = match level {
                Level::Error | Level::Warn => Box::new(io::stderr()),
                _ => Box::new(io::stdout()),
            };
            let _ = writeln!(out, "{}", s);
        }
    }
}

// Helper to get current output format
pub fn get_output_format() -> OutputFormat {
    RENDERER.read().expect("renderer poisoned").format
}

pub fn separator(light: bool) {
    let r = RENDERER.read().expect("renderer poisioned").clone();
    // In JSON mode, do not print separators to avoid breaking jq parsing
    if matches!(r.format, OutputFormat::Json) {
        return;
    }
    let glyph = if light {
        SEPARATOR_LIGHT
    } else {
        SEPARATOR_HEAVY
    };
    let line = glyph.repeat(80);
    let mut out = io::stdout();
    let _ = writeln!(
        out,
        "{}",
        if r.color {
            line.normal().to_string()
        } else {
            line
        }
    );
}

pub mod prelude {
    pub use super::nerd_font::NerdFont;
    pub use super::preview::PreviewBuilder;
    pub use super::{Level, OutputFormat, emit, get_output_format, separator};
}
