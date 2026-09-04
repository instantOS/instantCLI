use std::env;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result};

use crate::ui::catppuccin::colors;
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

use super::history::ClipEntry;

// PreviewBuilder emits: padding, header, description, three fields, one
// language/padding row, separator, and a blank before the payload.
const BODY_TOP: usize = 9;
const TEXT_LIMIT: usize = 256 * 1024;
const HEX_LIMIT: usize = 512;

pub fn render(entry: &ClipEntry) -> Result<()> {
    clear_preview_images();
    let bytes = entry.decode()?;
    let mut file = tempfile::Builder::new()
        .prefix("ins-clip-preview-")
        .tempfile()
        .context("Failed to create clipboard preview file")?;
    file.write_all(&bytes)?;

    let mime = mime_type(file.path(), &bytes);
    let description = file_description(file.path());
    let language = is_text(&mime, &bytes)
        .then(|| detect_language(&String::from_utf8_lossy(&bytes)))
        .flatten();
    print_header(entry, &mime, &description, bytes.len(), language.as_ref());

    if mime.starts_with("image/") {
        return preview_image(file.path());
    }
    if mime == "application/pdf" {
        return preview_pdf(file.path());
    }
    if mime.starts_with("video/") {
        return preview_video(file.path());
    }
    if mime.starts_with("audio/") {
        return preview_media_metadata(file.path());
    }
    if is_archive(&mime) {
        return preview_archive(file.path());
    }
    if is_text(&mime, &bytes) {
        if let Some(language) = language
            && preview_code(file.path(), &language)?
        {
            return Ok(());
        }
        print_text(&bytes);
        return Ok(());
    }

    print_hex(&bytes);
    Ok(())
}

fn print_header(
    entry: &ClipEntry,
    mime: &str,
    description: &str,
    size: usize,
    language: Option<&DetectedLanguage>,
) {
    let (icon, title, color) = preview_identity(mime, language);
    let mut preview = PreviewBuilder::streaming()
        .header(icon, title)
        .line(color, None, description)
        .field("ID", &entry.id)
        .field("Type", mime)
        .field("Size", &human_size(size));
    if let Some(language) = language {
        preview = preview.field("Language", language.label);
    } else {
        // Keep the body at a stable row for Kitty placement.
        preview = preview.blank();
    }
    drop(preview.separator().blank());
}

fn preview_identity(
    mime: &str,
    language: Option<&DetectedLanguage>,
) -> (NerdFont, &'static str, &'static str) {
    if let Some(language) = language {
        return (NerdFont::FileCode, language.label, colors::MAUVE);
    }
    if mime.starts_with("image/") {
        (NerdFont::Image, "Image", colors::LAVENDER)
    } else if mime == "application/pdf" {
        (NerdFont::FilePdf, "PDF document", colors::RED)
    } else if mime.starts_with("video/") {
        (NerdFont::Video, "Video", colors::PEACH)
    } else if mime.starts_with("audio/") {
        (NerdFont::Music, "Audio", colors::BLUE)
    } else if is_archive(mime) {
        (NerdFont::Archive, "Archive", colors::YELLOW)
    } else if mime.starts_with("text/") {
        (NerdFont::FileText, "Text", colors::GREEN)
    } else {
        (NerdFont::File, "Binary data", colors::SUBTEXT0)
    }
}

fn mime_type(path: &Path, bytes: &[u8]) -> String {
    command_line(
        "file",
        &["--brief", "--mime-type", path.to_string_lossy().as_ref()],
    )
    .filter(|value| !value.is_empty())
    .unwrap_or_else(|| {
        if looks_like_text(bytes) {
            "text/plain".to_string()
        } else {
            "application/octet-stream".to_string()
        }
    })
}

fn file_description(path: &Path) -> String {
    command_line("file", &["--brief", path.to_string_lossy().as_ref()])
        .unwrap_or_else(|| "Clipboard data".to_string())
}

fn preview_image(path: &Path) -> Result<()> {
    if render_terminal_image(path, BODY_TOP)? {
        return Ok(());
    }
    let mut preview = PreviewBuilder::streaming().line(
        colors::YELLOW,
        Some(NerdFont::Warning),
        "Visual preview unavailable",
    );
    if let Some(details) = command_line("identify", &["-format", "%m · %wx%h", path_str(path)]) {
        preview = preview.field("Image", &details);
    }
    drop(preview.subtext("Use a Kitty-compatible terminal or install chafa."));
    Ok(())
}

fn preview_pdf(path: &Path) -> Result<()> {
    let thumbnail = tempfile::Builder::new()
        .prefix("ins-clip-pdf-")
        .suffix(".png")
        .tempfile()?;
    let output_base = thumbnail.path().with_extension("");
    let rendered = Command::new("pdftoppm")
        .args(["-f", "1", "-singlefile", "-scale-to", "1600", "-png"])
        .arg(path)
        .arg(&output_base)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    let rendered_path = output_base.with_extension("png");
    if rendered && render_terminal_image(&rendered_path, BODY_TOP)? {
        let _ = fs::remove_file(rendered_path);
        return Ok(());
    }
    let _ = fs::remove_file(rendered_path);
    drop(
        PreviewBuilder::streaming()
            .line(
                colors::YELLOW,
                Some(NerdFont::Warning),
                "First-page preview unavailable",
            )
            .subtext("Install pdftoppm and use a Kitty-compatible terminal."),
    );
    Ok(())
}

fn preview_video(path: &Path) -> Result<()> {
    preview_media_metadata(path)?;
    let thumbnail = tempfile::Builder::new()
        .prefix("ins-clip-video-")
        .suffix(".png")
        .tempfile()?;
    let rendered = Command::new("ffmpeg")
        .args(["-loglevel", "quiet", "-ss", "1", "-i"])
        .arg(path)
        .args(["-frames:v", "1", "-y"])
        .arg(thumbnail.path())
        .status()
        .is_ok_and(|status| status.success());
    if rendered {
        let _ = render_terminal_image(thumbnail.path(), BODY_TOP + 7)?;
    }
    Ok(())
}

fn preview_media_metadata(path: &Path) -> Result<()> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration,format_name:stream=codec_name,codec_type,width,height",
            "-of",
            "default=noprint_wrappers=1",
        ])
        .arg(path)
        .output();
    if let Ok(output) = output
        && output.status.success()
    {
        let metadata = String::from_utf8_lossy(&output.stdout);
        let mut preview = PreviewBuilder::streaming().title(colors::BLUE, "Media details");
        for line in metadata.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let label = match key {
                "codec_name" => "Codec",
                "codec_type" => "Stream",
                "width" => "Width",
                "height" => "Height",
                "format_name" => "Container",
                "duration" => "Duration",
                _ => continue,
            };
            let value = if key == "duration" {
                format_duration(value).unwrap_or_else(|| value.to_string())
            } else {
                value.to_string()
            };
            preview = preview.field(label, &value);
        }
        drop(preview);
    }
    Ok(())
}

fn preview_archive(path: &Path) -> Result<()> {
    let output = Command::new("bsdtar").arg("-tf").arg(path).output();
    match output {
        Ok(output) if output.status.success() => {
            let listing = String::from_utf8_lossy(&output.stdout);
            let mut preview = PreviewBuilder::streaming().title(colors::YELLOW, "Archive contents");
            for line in listing
                .lines()
                .take(preview_lines().saturating_sub(BODY_TOP))
            {
                preview = preview.raw(line);
            }
            drop(preview);
        }
        _ => drop(
            PreviewBuilder::streaming()
                .line(
                    colors::YELLOW,
                    Some(NerdFont::Warning),
                    "Archive listing unavailable",
                )
                .subtext("Install bsdtar to inspect archive contents."),
        ),
    }
    Ok(())
}

/// Escape that deletes every graphic the terminal is currently showing —
/// the same sequence `kitten icat --clear` sends. `q=2` keeps the terminal
/// from replying.
pub const CLEAR_GRAPHICS: &str = "\x1b_Ga=d,d=A,q=2\x1b\\";

/// Whether previews can place graphics through the kitty graphics protocol.
pub fn terminal_graphics_supported() -> bool {
    command_exists("kitten") && kitty_graphics_available()
}

/// Remove images an earlier preview left in the pane.
///
/// fzf repaints only the preview pane's text, so a graphic placed by
/// `kitten icat` outlives its entry. Previews that draw an image clear via
/// `kitten icat --clear`; every other preview relies on this call.
pub fn clear_preview_images() {
    if terminal_graphics_supported() {
        print!("{CLEAR_GRAPHICS}");
        let _ = std::io::stdout().flush();
    }
}

fn render_terminal_image(path: &Path, top: usize) -> Result<bool> {
    if terminal_graphics_supported() {
        let place = format!(
            "{}x{}@0x{top}",
            preview_columns(),
            preview_lines().saturating_sub(top).max(1)
        );
        let mut kitten = Command::new("kitten")
            .args([
                "icat",
                "--clear",
                "--transfer-mode=memory",
                "--unicode-placeholder",
                "--stdin=no",
                "--place",
                &place,
            ])
            .arg(path)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .context("Failed to start kitten icat")?;
        let stdout = kitten
            .stdout
            .take()
            .context("Failed to read kitten output")?;
        let mut sed = Command::new("sed")
            .arg("$d")
            .stdin(stdout)
            .spawn()
            .context("Failed to filter kitten output")?;
        let kitten_status = kitten.wait()?;
        let sed_status = sed.wait()?;
        return Ok(kitten_status.success() && sed_status.success());
    }

    if command_exists("chafa") {
        let size = format!(
            "{}x{}",
            preview_columns(),
            preview_lines().saturating_sub(top).max(1)
        );
        return Ok(Command::new("chafa")
            .args(["--size", &size])
            .arg(path)
            .status()
            .is_ok_and(|status| status.success()));
    }
    Ok(false)
}

fn format_duration(value: &str) -> Option<String> {
    let seconds = value.parse::<f64>().ok()?;
    let total = seconds.round() as u64;
    Some(format!(
        "{:02}:{:02}:{:02}",
        total / 3600,
        total % 3600 / 60,
        total % 60
    ))
}

fn kitty_graphics_available() -> bool {
    env::var_os("KITTY_WINDOW_ID").is_some()
        || env::var("TERM").is_ok_and(|term| term.to_ascii_lowercase().contains("kitty"))
}

fn command_exists(name: &str) -> bool {
    which::which(name).is_ok()
}

fn command_line(command: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(command).args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DetectedLanguage {
    bat_name: &'static str,
    label: &'static str,
}

fn preview_code(path: &Path, language: &DetectedLanguage) -> Result<bool> {
    if !command_exists("bat") {
        return Ok(false);
    }
    Ok(Command::new("bat")
        .args([
            "--color=always",
            "--decorations=never",
            "--paging=never",
            "--line-range=:500",
            "--language",
            language.bat_name,
        ])
        .arg(path)
        .stderr(Stdio::null())
        .status()
        .context("Failed to start bat")?
        .success())
}

/// Identify only languages with strong clipboard-visible signatures.
///
/// Clipboard contents have no filename, and short snippets are inherently
/// ambiguous. Returning `None` is preferable to misleading highlighting.
fn detect_language(text: &str) -> Option<DetectedLanguage> {
    let sample = &text[..floor_char_boundary(text, text.len().min(64 * 1024))];
    let trimmed = sample.trim_start();
    let first_line = trimmed.lines().next().unwrap_or_default();

    if let Some(language) = detect_shebang(first_line) {
        return Some(language);
    }
    if serde_json::from_str::<serde_json::Value>(trimmed).is_ok()
        && matches!(trimmed.as_bytes().first(), Some(b'{' | b'['))
    {
        return Some(language("json", "JSON"));
    }
    if trimmed.starts_with("diff --git ")
        || (trimmed.starts_with("--- ") && trimmed.lines().any(|line| line.starts_with("+++ ")))
    {
        return Some(language("diff", "Diff"));
    }
    if trimmed.starts_with("<!DOCTYPE html")
        || trimmed.starts_with("<html")
        || (trimmed.contains("</") && trimmed.contains("<body"))
    {
        return Some(language("html", "HTML"));
    }
    if trimmed.starts_with("<?xml") {
        return Some(language("xml", "XML"));
    }
    if has_known_fence(trimmed) {
        return Some(language("markdown", "Markdown"));
    }

    let candidates = [
        (
            score(
                sample,
                &["#[derive(", "fn ", "impl ", "use ", "::", "let mut "],
            ),
            language("rust", "Rust"),
        ),
        (
            score(
                sample,
                &["def ", "from ", " import ", "elif ", "self.", "__name__"],
            ),
            language("python", "Python"),
        ),
        (
            score(
                sample,
                &["interface ", "type ", " as const", "import type ", "=>"],
            ),
            language("typescript", "TypeScript"),
        ),
        (
            score(
                sample,
                &["const ", "let ", "function ", "=>", "import ", "export "],
            ),
            language("javascript", "JavaScript"),
        ),
        (
            score(sample, &["package ", "func ", "import (", " := ", "defer "]),
            language("go", "Go"),
        ),
        (
            score(
                sample,
                &["#include <", "#include \"", "std::", "int main(", "nullptr"],
            ),
            language("cpp", "C++"),
        ),
        (
            score(
                sample,
                &[
                    "SELECT ",
                    "FROM ",
                    "WHERE ",
                    "INSERT INTO ",
                    "CREATE TABLE ",
                ],
            ) + score(
                &sample.to_ascii_uppercase(),
                &[
                    "SELECT ",
                    "FROM ",
                    "WHERE ",
                    "INSERT INTO ",
                    "CREATE TABLE ",
                ],
            ),
            language("sql", "SQL"),
        ),
        (
            score(
                sample,
                &["#!/", "set -e", "case ", " esac", " then", " fi", "${"],
            ),
            language("bash", "Shell"),
        ),
        (
            score(
                sample,
                &["[package]", "[dependencies]", "[workspace]", "[profile."],
            ),
            language("toml", "TOML"),
        ),
        (
            score(
                sample,
                &["---\n", "apiVersion:", "kind:", "services:", "jobs:"],
            ),
            language("yaml", "YAML"),
        ),
        (
            score(
                sample,
                &["@media ", "@import ", "display:", "color:", "font-family:"],
            ),
            language("css", "CSS"),
        ),
    ];

    candidates
        .into_iter()
        .max_by_key(|(confidence, _)| *confidence)
        .filter(|(confidence, _)| *confidence >= 3)
        .map(|(_, language)| language)
}

fn language(bat_name: &'static str, label: &'static str) -> DetectedLanguage {
    DetectedLanguage { bat_name, label }
}

fn score(sample: &str, signals: &[&str]) -> usize {
    signals
        .iter()
        .filter(|signal| sample.contains(**signal))
        .count()
}

fn detect_shebang(first_line: &str) -> Option<DetectedLanguage> {
    if !first_line.starts_with("#!") {
        return None;
    }
    [
        ("python", language("python", "Python")),
        ("node", language("javascript", "JavaScript")),
        ("deno", language("typescript", "TypeScript")),
        ("ruby", language("ruby", "Ruby")),
        ("perl", language("perl", "Perl")),
        ("fish", language("fish", "Fish")),
        ("zsh", language("bash", "Zsh")),
        ("bash", language("bash", "Shell")),
        ("/sh", language("bash", "Shell")),
    ]
    .into_iter()
    .find(|(marker, _)| first_line.contains(marker))
    .map(|(_, language)| language)
}

fn has_known_fence(text: &str) -> bool {
    let marker = text.lines().find_map(|line| {
        line.trim_start()
            .strip_prefix("```")
            .map(str::trim)
            .filter(|value| !value.is_empty())
    });
    marker.is_some_and(|marker| {
        matches!(
            marker
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .to_ascii_lowercase()
                .as_str(),
            "rs" | "rust"
                | "py"
                | "python"
                | "js"
                | "javascript"
                | "ts"
                | "typescript"
                | "sh"
                | "shell"
                | "bash"
                | "zsh"
                | "json"
                | "yaml"
                | "yml"
                | "toml"
                | "sql"
                | "html"
                | "css"
        )
    })
}

fn floor_char_boundary(value: &str, mut index: usize) -> usize {
    while !value.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn is_text(mime: &str, bytes: &[u8]) -> bool {
    mime.starts_with("text/")
        || mime.ends_with("+json")
        || mime.ends_with("+xml")
        || matches!(
            mime,
            "application/json"
                | "application/xml"
                | "application/javascript"
                | "application/x-shellscript"
        )
        || looks_like_text(bytes)
}

fn looks_like_text(bytes: &[u8]) -> bool {
    if bytes.contains(&0) {
        return false;
    }
    let sample = &bytes[..bytes.len().min(4096)];
    let controls = sample
        .iter()
        .filter(|byte| **byte < 0x20 && !matches!(**byte, b'\n' | b'\r' | b'\t'))
        .count();
    controls * 20 <= sample.len().max(1)
}

fn print_text(bytes: &[u8]) {
    let truncated = bytes.len() > TEXT_LIMIT;
    let text = String::from_utf8_lossy(&bytes[..bytes.len().min(TEXT_LIMIT)]);
    for character in text.chars() {
        if character == '\n' || character == '\t' || !character.is_control() {
            print!("{character}");
        } else if character != '\r' {
            print!("�");
        }
    }
    if truncated {
        println!("\n\n… preview truncated at {}", human_size(TEXT_LIMIT));
    }
}

fn print_hex(bytes: &[u8]) {
    println!("No structured preview is available. First bytes:\n");
    for (offset, chunk) in bytes[..bytes.len().min(HEX_LIMIT)].chunks(16).enumerate() {
        print!("{:08x}  ", offset * 16);
        for index in 0..16 {
            if let Some(byte) = chunk.get(index) {
                print!("{byte:02x} ");
            } else {
                print!("   ");
            }
        }
        print!(" ");
        for byte in chunk {
            let character = if byte.is_ascii_graphic() || *byte == b' ' {
                char::from(*byte)
            } else {
                '.'
            };
            print!("{character}");
        }
        println!();
    }
    if bytes.len() > HEX_LIMIT {
        println!("\n… {} more", human_size(bytes.len() - HEX_LIMIT));
    }
}

fn is_archive(mime: &str) -> bool {
    matches!(
        mime,
        "application/zip"
            | "application/x-tar"
            | "application/gzip"
            | "application/x-bzip2"
            | "application/x-xz"
            | "application/x-7z-compressed"
            | "application/vnd.rar"
    )
}

fn preview_columns() -> usize {
    env_dimension("FZF_PREVIEW_COLUMNS", 80)
}

fn preview_lines() -> usize {
    env_dimension("FZF_PREVIEW_LINES", 24)
}

fn env_dimension(name: &str, fallback: usize) -> usize {
    env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|value| *value > 0)
        .unwrap_or(fallback)
}

fn path_str(path: &Path) -> &str {
    path.to_str().unwrap_or("")
}

fn human_size(bytes: usize) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinguishes_text_from_binary() {
        assert!(looks_like_text(b"hello\nworld"));
        assert!(looks_like_text("hello 🌍".as_bytes()));
        assert!(!looks_like_text(b"hello\0world"));
        assert!(!looks_like_text(&[1, 2, 3, 4, 5]));
    }

    #[test]
    fn formats_sizes() {
        assert_eq!(human_size(42), "42 B");
        assert_eq!(human_size(1536), "1.5 KiB");
        assert_eq!(format_duration("65.4").as_deref(), Some("00:01:05"));
    }

    #[test]
    fn detects_unambiguous_languages() {
        assert_eq!(
            detect_language(r#"{"hello": "world"}"#),
            Some(language("json", "JSON"))
        );
        assert_eq!(
            detect_language("#!/usr/bin/env python3\nprint('hello')"),
            Some(language("python", "Python"))
        );
        assert_eq!(
            detect_language(
                "use std::path::Path;\n#[derive(Debug)]\nfn main() {\nlet mut x = 1;\n}"
            ),
            Some(language("rust", "Rust"))
        );
        assert_eq!(
            detect_language("Some prose.\n\n```typescript\nconst value: number = 1;\n```"),
            Some(language("markdown", "Markdown"))
        );
    }

    #[test]
    fn leaves_ambiguous_snippets_plain() {
        assert_eq!(detect_language("name = \"Benjamin\""), None);
        assert_eq!(detect_language("const value = 1;"), None);
        assert_eq!(detect_language("hello world"), None);
    }
}
