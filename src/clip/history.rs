use std::cmp::Reverse;
use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, anyhow};
use nix::fcntl::{Flock, FlockArg};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::common::display_server::DisplayServer;

const CACHE_VERSION: u8 = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ClipBackend {
    Cliphist,
    Clipmenu,
}

impl ClipBackend {
    pub fn detect() -> Result<Self> {
        match DisplayServer::detect() {
            DisplayServer::Wayland => Ok(Self::Cliphist),
            DisplayServer::X11 => Ok(Self::Clipmenu),
            DisplayServer::Unknown => Err(anyhow!(
                "cannot choose a clipboard history backend outside an X11 or Wayland session"
            )),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Cliphist => "cliphist",
            Self::Clipmenu => "clipmenu",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum EntrySource {
    Cliphist(String),
    Clipmenu(PathBuf),
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
    fn from_clipmenu_cache_line(cache_dir: &Path, cache_line: &str) -> Result<Option<Self>> {
        let Some((_, summary)) = cache_line.split_once(' ') else {
            return Ok(None);
        };
        let backing_file = cache_dir.join(clipmenu_checksum(summary)?);
        let content = match fs::read(&backing_file) {
            Ok(content) => content,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("Failed to read {}", backing_file.display()));
            }
        };
        Ok(Some(Self {
            id: short_id(&content),
            summary: summary.to_string(),
            source: EntrySource::Clipmenu(backing_file),
        }))
    }

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
            EntrySource::Clipmenu(path) => {
                fs::read(path).with_context(|| format!("Failed to read {}", path.display()))
            }
            EntrySource::Cliphist(line) => pipe_to_cliphist("decode", line),
            #[cfg(test)]
            EntrySource::Memory(content) => Ok(content.clone()),
        }
    }
}

pub fn load(backend: ClipBackend) -> Result<Vec<ClipEntry>> {
    match backend {
        ClipBackend::Cliphist => load_cliphist(),
        ClipBackend::Clipmenu => load_clipmenu(),
    }
}

fn load_cliphist() -> Result<Vec<ClipEntry>> {
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

fn load_clipmenu() -> Result<Vec<ClipEntry>> {
    let dir = clipmenu_cache_dir();
    let path = dir.join("line_cache");
    let cache = match fs::read_to_string(&path) {
        Ok(cache) => cache,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(error).with_context(|| format!("Failed to read {}", path.display()));
        }
    };

    let mut lines: Vec<&str> = cache.lines().filter(|line| !line.is_empty()).collect();
    lines.sort_by_key(|line| Reverse(timestamp(line)));

    let mut seen = HashSet::new();
    let mut entries = Vec::new();
    for line in lines {
        let Some(entry) = ClipEntry::from_clipmenu_cache_line(&dir, line)? else {
            continue;
        };
        if seen.insert(entry.summary.clone()) {
            entries.push(entry);
        }
    }
    Ok(entries)
}

pub fn find(backend: ClipBackend, id: &str) -> Result<ClipEntry> {
    find_entry(load(backend)?, id)
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

pub fn delete(backend: ClipBackend, id: &str) -> Result<()> {
    let target = find(backend, id)?;
    match &target.source {
        EntrySource::Cliphist(line) => {
            pipe_to_cliphist("delete", line)?;
            Ok(())
        }
        EntrySource::Clipmenu(backing_file) => delete_clipmenu(&target.summary, backing_file),
        #[cfg(test)]
        EntrySource::Memory(_) => Err(anyhow!("cannot delete an in-memory test entry")),
    }
}

fn delete_clipmenu(summary: &str, backing_file: &Path) -> Result<()> {
    let dir = clipmenu_cache_dir();
    fs::create_dir_all(&dir)?;
    let lock_file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(dir.join("lock"))?;
    let _lock = Flock::lock(lock_file, FlockArg::LockExclusive)
        .map_err(|(_, error)| error)
        .context("Failed to lock clipboard history")?;

    let cache_path = dir.join("line_cache");
    let cache = fs::read_to_string(&cache_path).unwrap_or_default();
    let retained: Vec<&str> = cache
        .lines()
        .filter(|line| {
            line.split_once(' ')
                .is_none_or(|(_, candidate)| candidate != summary)
        })
        .collect();
    atomic_write(&cache_path, retained.join("\n").as_bytes())?;

    match fs::remove_file(backing_file) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).context("Failed to remove clipboard entry data"),
    }
    Ok(())
}

pub fn clear(backend: ClipBackend) -> Result<usize> {
    let count = load(backend)?.len();
    match backend {
        ClipBackend::Cliphist => {
            let status = Command::new("cliphist")
                .arg("wipe")
                .status()
                .context("Failed to run cliphist wipe")?;
            anyhow::ensure!(status.success(), "cliphist wipe failed");
        }
        ClipBackend::Clipmenu => {
            let dir = clipmenu_cache_dir();
            if !dir.exists() {
                return Ok(0);
            }
            let status = Command::new("clipdel")
                .args(["-d", ".*"])
                .status()
                .context("Failed to run clipdel")?;
            anyhow::ensure!(status.success(), "clipdel failed with {status}");
        }
    }
    Ok(count)
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

fn clipmenu_cache_dir() -> PathBuf {
    let base = std::env::var_os("CM_DIR")
        .or_else(|| std::env::var_os("XDG_RUNTIME_DIR"))
        .or_else(|| std::env::var_os("TMPDIR"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    let user = std::env::var("USER").unwrap_or_else(|_| {
        Command::new("id")
            .arg("-un")
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "unknown".to_string())
    });
    base.join(format!("clipmenu.{CACHE_VERSION}.{user}"))
}

fn timestamp(line: &str) -> u128 {
    line.split_once(' ')
        .and_then(|(value, _)| value.parse().ok())
        .unwrap_or_default()
}

fn short_id(content: &[u8]) -> String {
    let digest = Sha256::digest(content);
    hex::encode(&digest[..6])
}

fn clipmenu_checksum(summary: &str) -> Result<String> {
    let mut child = Command::new("cksum")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .context("Failed to run cksum")?;
    child
        .stdin
        .take()
        .context("Failed to open cksum input")?
        .write_all(format!("{summary}\n").as_bytes())?;
    let output = child.wait_with_output()?;
    anyhow::ensure!(output.status.success(), "cksum failed");
    Ok(String::from_utf8(output.stdout)?.trim().to_string())
}

fn atomic_write(path: &Path, data: &[u8]) -> Result<()> {
    let parent = path.parent().context("Clipboard cache has no parent")?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(data)?;
    if !data.is_empty() {
        temp.write_all(b"\n")?;
    }
    temp.persist(path)
        .map_err(|error| error.error)
        .with_context(|| format!("Failed to update {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_stable_and_content_sensitive() {
        assert_eq!(short_id(b"hello"), short_id(b"hello"));
        assert_ne!(short_id(b"hello"), short_id(b"world"));
        assert_eq!(short_id(b"hello").len(), 12);
    }

    #[test]
    fn parses_cliphist_list_lines() {
        let entry = ClipEntry::from_cliphist_line("42\thello world").unwrap();
        assert_eq!(entry.id, "42");
        assert_eq!(entry.summary, "hello world");
        assert!(matches!(entry.source, EntrySource::Cliphist(_)));
        assert!(ClipEntry::from_cliphist_line("bad").is_none());
    }

    #[test]
    fn timestamp_parser_tolerates_bad_lines() {
        assert_eq!(timestamp("123 example"), 123);
        assert_eq!(timestamp("bad example"), 0);
        assert_eq!(timestamp("missing"), 0);
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
