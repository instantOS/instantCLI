use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result};

use super::history::ClipEntry;

const HEADER_LINES: usize = 5;
const TEXT_LIMIT: usize = 256 * 1024;
const HEX_LIMIT: usize = 512;

pub fn render(entry: &ClipEntry) -> Result<()> {
    let bytes = entry.decode()?;
    let mut file = tempfile::Builder::new()
        .prefix("ins-clip-preview-")
        .tempfile()
        .context("Failed to create clipboard preview file")?;
    file.write_all(&bytes)?;

    let mime = mime_type(file.path(), &bytes);
    let description = file_description(file.path());
    print_header(entry, &mime, &description, bytes.len())?;

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
        print_text(&bytes);
        return Ok(());
    }

    print_hex(&bytes);
    Ok(())
}

fn print_header(entry: &ClipEntry, mime: &str, description: &str, size: usize) -> Result<()> {
    println!("Clipboard entry {}", entry.id);
    println!("{description}");
    println!("{mime} · {}", human_size(size));
    println!("────────────────────────────────────────");
    println!();
    io::stdout().flush().context("Failed to flush preview")
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
    if render_terminal_image(path)? {
        return Ok(());
    }
    if let Some(details) = command_line("identify", &["-format", "%m · %wx%h", path_str(path)]) {
        println!("{details}");
    }
    println!("Image preview requires Kitty graphics or the optional `chafa` command.");
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
    if rendered && render_terminal_image(&rendered_path)? {
        let _ = fs::remove_file(rendered_path);
        return Ok(());
    }
    let _ = fs::remove_file(rendered_path);
    println!("Install `pdftoppm` and use a Kitty-compatible terminal for a first-page preview.");
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
        let _ = render_terminal_image(thumbnail.path())?;
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
        print!("{}", String::from_utf8_lossy(&output.stdout));
    }
    Ok(())
}

fn preview_archive(path: &Path) -> Result<()> {
    let output = Command::new("bsdtar").arg("-tf").arg(path).output();
    match output {
        Ok(output) if output.status.success() => {
            let listing = String::from_utf8_lossy(&output.stdout);
            for line in listing.lines().take(preview_lines()) {
                println!("{line}");
            }
        }
        _ => println!("Install `bsdtar` to list this archive."),
    }
    Ok(())
}

fn render_terminal_image(path: &Path) -> Result<bool> {
    if command_exists("kitten") && kitty_graphics_available() {
        let place = format!(
            "{}x{}@0x{HEADER_LINES}",
            preview_columns(),
            preview_lines().saturating_sub(HEADER_LINES).max(1)
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
            preview_lines().saturating_sub(HEADER_LINES).max(1)
        );
        return Ok(Command::new("chafa")
            .args(["--size", &size])
            .arg(path)
            .status()
            .is_ok_and(|status| status.success()));
    }
    Ok(false)
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
    }
}
