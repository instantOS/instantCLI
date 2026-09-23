use std::io::Write;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, anyhow};
use serde::Serialize;

use crate::assist::utils::{copy_image_to_clipboard, copy_to_clipboard};
use crate::common::display_server::DisplayServer;

/// How clipboard changes are captured into the shared cliphist database.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ClipBackend {
    /// `wl-paste --watch cliphist store` via the packaged `cliphist.service`.
    Wayland,
    /// `ins clip watch-x11` via the generated `ins-clip-x11.service`.
    X11,
}

impl ClipBackend {
    pub fn detect() -> Result<Self> {
        match DisplayServer::detect() {
            DisplayServer::Wayland => Ok(Self::Wayland),
            DisplayServer::X11 => Ok(Self::X11),
            DisplayServer::Unknown => Err(anyhow!(
                "cannot choose a clipboard capture method outside an X11 or Wayland session"
            )),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Wayland => "cliphist (wl-paste)",
            Self::X11 => "cliphist (X11 watcher)",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum EntrySource {
    Cliphist(String),
    #[cfg(test)]
    Memory(Vec<u8>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClipEntry {
    pub id: String,
    pub summary: String,
    pub(super) source: EntrySource,
}

#[derive(Serialize)]
pub struct ClipOutputEntry {
    pub id: String,
    pub summary: String,
    pub content: String,
}

impl ClipEntry {
    fn from_cliphist_line(line: &str) -> Option<Self> {
        let (id, summary) = line.split_once('\t')?;
        if id.is_empty() {
            return None;
        }
        Some(Self {
            id: id.to_string(),
            summary: summary.to_string(),
            source: EntrySource::Cliphist(line.to_string()),
        })
    }

    pub fn preview(&self) -> Result<String> {
        Ok(String::from_utf8_lossy(&self.decode()?).into_owned())
    }

    pub fn decode(&self) -> Result<Vec<u8>> {
        match &self.source {
            EntrySource::Cliphist(line) => pipe_to_cliphist("decode", line),
            #[cfg(test)]
            EntrySource::Memory(content) => Ok(content.clone()),
        }
    }
}

pub fn load() -> Result<Vec<ClipEntry>> {
    let output = Command::new("cliphist")
        .arg("list")
        .output()
        .context("Failed to list cliphist entries")?;
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr);
        if error.contains("please store something first") {
            return Ok(Vec::new());
        }
        return Err(anyhow!("cliphist list failed: {}", error.trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(ClipEntry::from_cliphist_line)
        .collect())
}

pub fn find(id: &str) -> Result<ClipEntry> {
    find_entry(load()?, id)
}

pub fn find_entry(entries: Vec<ClipEntry>, id: &str) -> Result<ClipEntry> {
    if let Some(exact) = entries.iter().find(|entry| entry.id == id) {
        return Ok(exact.clone());
    }

    let matches: Vec<_> = entries
        .into_iter()
        .filter(|entry| entry.id.starts_with(id))
        .collect();
    match matches.as_slice() {
        [] => Err(anyhow!("clipboard entry '{id}' was not found")),
        [entry] => Ok(entry.clone()),
        _ => Err(anyhow!(
            "clipboard entry ID '{id}' is ambiguous; use more characters"
        )),
    }
}

pub fn delete(id: &str) -> Result<()> {
    let target = find(id)?;
    match &target.source {
        EntrySource::Cliphist(line) => {
            pipe_to_cliphist("delete", line)?;
            Ok(())
        }
        #[cfg(test)]
        EntrySource::Memory(_) => Err(anyhow!("cannot delete an in-memory test entry")),
    }
}

pub fn clear() -> Result<usize> {
    let count = load()?.len();
    let status = Command::new("cliphist")
        .arg("wipe")
        .status()
        .context("Failed to run cliphist wipe")?;
    anyhow::ensure!(status.success(), "cliphist wipe failed");
    Ok(count)
}

/// Put an entry back on the clipboard. Images need an explicit MIME type on
/// X11, otherwise xclip would offer the raw bytes as text.
pub fn restore(entry: &ClipEntry) -> Result<()> {
    let data = entry.decode()?;
    let display_server = DisplayServer::detect();
    match image_mime_type(&data) {
        Some(mime) => copy_image_to_clipboard(&data, mime, &display_server),
        None => copy_to_clipboard(&data, &display_server),
    }
}

fn image_mime_type(data: &[u8]) -> Option<&'static str> {
    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if data.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if data.len() >= 12 && data.starts_with(b"RIFF") && &data[8..12] == b"WEBP" {
        Some("image/webp")
    } else if data.starts_with(b"BM") && data.len() > 14 {
        Some("image/bmp")
    } else {
        None
    }
}

pub fn output_entries(entries: &[ClipEntry]) -> Result<Vec<ClipOutputEntry>> {
    entries
        .iter()
        .map(|entry| {
            Ok(ClipOutputEntry {
                id: entry.id.clone(),
                summary: entry.summary.clone(),
                content: entry.preview()?,
            })
        })
        .collect()
}

fn pipe_to_cliphist(subcommand: &str, line: &str) -> Result<Vec<u8>> {
    let mut child = Command::new("cliphist")
        .arg(subcommand)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .with_context(|| format!("Failed to run cliphist {subcommand}"))?;
    child
        .stdin
        .take()
        .context("Failed to open cliphist input")?
        .write_all(format!("{line}\n").as_bytes())?;
    let output = child.wait_with_output()?;
    anyhow::ensure!(
        output.status.success(),
        "cliphist {subcommand} failed: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(output.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cliphist_list_lines() {
        let entry = ClipEntry::from_cliphist_line("42\thello world").unwrap();
        assert_eq!(entry.id, "42");
        assert_eq!(entry.summary, "hello world");
        assert!(matches!(entry.source, EntrySource::Cliphist(_)));
        assert!(ClipEntry::from_cliphist_line("bad").is_none());
    }

    #[test]
    fn sniffs_image_mime_types() {
        assert_eq!(
            image_mime_type(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR"),
            Some("image/png")
        );
        assert_eq!(
            image_mime_type(&[0xff, 0xd8, 0xff, 0xe0]),
            Some("image/jpeg")
        );
        assert_eq!(image_mime_type(b"GIF89a...."), Some("image/gif"));
        assert_eq!(image_mime_type(b"RIFF\0\0\0\0WEBPVP8 "), Some("image/webp"));
        assert_eq!(image_mime_type(b"hello world"), None);
        assert_eq!(image_mime_type(b"BM"), None);
    }

    #[test]
    fn find_entry_prefers_exact_match_over_prefix_matches() {
        let entries = vec![
            ClipEntry {
                id: "2".into(),
                summary: "two".into(),
                source: EntrySource::Memory(b"two".to_vec()),
            },
            ClipEntry {
                id: "20".into(),
                summary: "twenty".into(),
                source: EntrySource::Memory(b"twenty".to_vec()),
            },
            ClipEntry {
                id: "21".into(),
                summary: "twenty-one".into(),
                source: EntrySource::Memory(b"twenty-one".to_vec()),
            },
        ];

        let found = find_entry(entries.clone(), "2").unwrap();
        assert_eq!(found.id, "2");
        assert_eq!(found.summary, "two");

        let found_twenty = find_entry(entries, "20").unwrap();
        assert_eq!(found_twenty.id, "20");
    }

    #[test]
    fn find_entry_handles_prefix_and_missing_matches() {
        let entries = vec![
            ClipEntry {
                id: "20".into(),
                summary: "twenty".into(),
                source: EntrySource::Memory(b"twenty".to_vec()),
            },
            ClipEntry {
                id: "21".into(),
                summary: "twenty-one".into(),
                source: EntrySource::Memory(b"twenty-one".to_vec()),
            },
        ];

        let err = find_entry(entries.clone(), "2").unwrap_err();
        assert!(err.to_string().contains("ambiguous"));

        let found = find_entry(entries.clone(), "20").unwrap();
        assert_eq!(found.id, "20");

        let err_missing = find_entry(entries, "99").unwrap_err();
        assert!(err_missing.to_string().contains("was not found"));
    }
}
