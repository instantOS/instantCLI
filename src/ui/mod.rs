use colored::*;
use lazy_static::lazy_static;
use serde::Serialize;
use std::io::{self, Write};
use std::sync::RwLock;

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
    fn icon(self) -> char {
        match self {
            Level::Info => char::from(Fa::InfoCircle),
            Level::Success => char::from(Fa::Check),
            Level::Warn => char::from(Fa::ExclamationCircle),
            Level::Error => char::from(Fa::TimesCircle),
            Level::Debug => char::from(Fa::Bug),
        }
    }

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

lazy_static! {
    static ref RENDERER: RwLock<Renderer> = RwLock::new(Renderer::default());
}

pub fn init(format: OutputFormat, color: bool) {
    if let Ok(mut r) = RENDERER.write() {
        r.format = format;
        r.color = color;
    }
}

// Re-export nerd font icons directly from the nerd_font crate
pub use nerd_font::categories::Fa;
pub use nerd_font::categories::Oct;

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

fn with_icon(level: Level, msg: &str, enable_color: bool) -> String {
    let icon = level.icon();
    let line = format!("{icon} {msg}");
    colorize(level, &line, enable_color)
}

fn strip_ansi(input: &str) -> String {
    // Remove common ANSI escape sequences like \x1b[0m, \x1b[1;32m, and similar
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b {
            // ESC
            if i + 1 < bytes.len() && bytes[i + 1] == b'[' {
                i += 2;
                // Skip until we hit a letter in @ A-Z [ \ ] ^ _ ` a-z
                while i < bytes.len() {
                    let b = bytes[i];
                    if (b >= b'@' && b <= b'~') {
                        i += 1; // consume the final byte of the CSI sequence
                        break;
                    }
                    i += 1;
                }
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

pub fn emit(level: Level, code: &str, message: &str, data: Option<serde_json::Value>) {
    let r = RENDERER.read().expect("renderer poisioned").clone();
    match r.format {
        OutputFormat::Text => {
            let line = with_icon(level, message, r.color);
            let mut out: Box<dyn Write> = match level {
                Level::Error | Level::Warn => Box::new(io::stderr()),
                _ => Box::new(io::stdout()),
            };
            let _ = writeln!(out, "{}", line);
        }
        OutputFormat::Json => {
            // Ensure message contains no ANSI control sequences in JSON mode
            let clean_msg = strip_ansi(message);
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

// Convenience helpers
pub fn info(code: &str, message: &str) {
    emit(Level::Info, code, message, None)
}
pub fn info_with_data(code: &str, message: &str, data: serde_json::Value) {
    emit(Level::Info, code, message, Some(data))
}
pub fn success(code: &str, message: &str) {
    emit(Level::Success, code, message, None)
}
pub fn success_with_data(code: &str, message: &str, data: serde_json::Value) {
    emit(Level::Success, code, message, Some(data))
}
pub fn warn(code: &str, message: &str) {
    emit(Level::Warn, code, message, None)
}
pub fn warn_with_data(code: &str, message: &str, data: serde_json::Value) {
    emit(Level::Warn, code, message, Some(data))
}
pub fn error(code: &str, message: &str) {
    emit(Level::Error, code, message, None)
}
pub fn error_with_data(code: &str, message: &str, data: serde_json::Value) {
    emit(Level::Error, code, message, Some(data))
}
pub fn debug(code: &str, message: &str) {
    emit(Level::Debug, code, message, None)
}
pub fn debug_with_data(code: &str, message: &str, data: serde_json::Value) {
    emit(Level::Debug, code, message, Some(data))
}

// Helper for structured data output
pub fn data(code: &str, data: serde_json::Value) {
    emit(Level::Info, code, "", Some(data))
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
    pub use super::{
        Fa, Level, Oct, OutputFormat, SEPARATOR_HEAVY, SEPARATOR_LIGHT, data, debug,
        debug_with_data, emit, error, error_with_data, get_output_format, info, info_with_data,
        separator, success, success_with_data, warn, warn_with_data,
    };
}
