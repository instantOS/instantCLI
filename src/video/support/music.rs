use std::collections::{HashMap, VecDeque};
use std::ffi::OsString;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use dirs::{cache_dir, data_dir, home_dir};
use duct::{Expression, cmd};
use sha2::{Digest, Sha256};

use super::utils::canonicalize_existing;
use crate::ui::prelude::{Level, emit};
use crate::video::document::MusicDirective;

pub struct MusicResolver {
    project_dir: PathBuf,
    cache: HashMap<String, PathBuf>,
}

impl MusicResolver {
    pub fn new(project_dir: &Path) -> Self {
        Self {
            project_dir: project_dir.to_path_buf(),
            cache: HashMap::new(),
        }
    }

    pub fn resolve(&mut self, directive: &MusicDirective) -> Result<Option<PathBuf>> {
        match directive {
            MusicDirective::None => Ok(None),
            MusicDirective::Source(value) => {
                if let Some(existing) = self.cache.get(value) {
                    return Ok(Some(existing.clone()));
                }

                let path = if is_url(value) {
                    self.download_url(value)?
                } else {
                    self.resolve_local(value)?
                };
                let canonical = canonicalize_existing(&path)?;
                self.cache.insert(value.clone(), canonical.clone());
                Ok(Some(canonical))
            }
        }
    }

    fn resolve_local(&self, value: &str) -> Result<PathBuf> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            bail!("Music reference must not be empty");
        }

        if trimmed.starts_with("~/")
            && let Some(home) = home_dir()
        {
            let candidate = home.join(trimmed.trim_start_matches("~/"));
            if candidate.exists() {
                return Ok(candidate);
            }
        }

        let direct = Path::new(trimmed);
        if direct.is_absolute() {
            if direct.exists() {
                return Ok(direct.to_path_buf());
            }
            bail!("Music file {} does not exist", direct.display());
        }

        let relative = Path::new(trimmed);

        let mut attempted = Vec::new();

        let has_subdirs = relative.iter().nth(1).is_some();
        if !has_subdirs {
            attempted.push(self.project_dir.join("music").join(relative));
        }
        attempted.push(self.project_dir.join(relative));

        if let Some(mut home) = home_dir() {
            home.push("music");
            attempted.push(home.join(relative));
        }

        if let Some(mut data) = data_dir() {
            data.push("instant");
            data.push("music");
            attempted.push(data.join(relative));
        }

        if let Some(mut cache) = cache_dir() {
            cache.push("instant");
            cache.push("music");
            attempted.push(cache.join(relative));
        }

        for candidate in &attempted {
            if candidate.exists() {
                return Ok(candidate.clone());
            }
        }

        let paths = attempted
            .into_iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(", ");

        bail!(
            "Unable to locate music file `{}` in any of: {}",
            trimmed,
            paths
        );
    }

    fn download_url(&self, url: &str) -> Result<PathBuf> {
        let mut hasher = Sha256::new();
        hasher.update(url.as_bytes());
        let hash = hex::encode(hasher.finalize());

        let mut cache_path =
            cache_dir().context("Unable to resolve cache directory for music downloads")?;
        cache_path.push("instant");
        cache_path.push("music");
        fs::create_dir_all(&cache_path).with_context(|| {
            format!("Failed to create cache directory {}", cache_path.display())
        })?;

        let destination = cache_path.join(&hash);
        if destination.exists() {
            emit(
                Level::Info,
                "video.music.cached",
                "Using cached music download",
                None,
            );
            return Ok(destination);
        }

        emit(
            Level::Info,
            "video.music.download.start",
            &format!("Downloading music with yt-dlp: {url}"),
            None,
        );
        let mut args = [
            "--no-part",
            "--newline",
            "--no-colors",
            "--progress",
            "--progress-delta",
            "1",
            "-f",
            "bestaudio/best",
            "-o",
        ]
        .map(OsString::from)
        .to_vec();
        args.push(destination.as_os_str().to_owned());
        args.push(url.into());
        let download = cmd("yt-dlp", args);
        run_music_download(download, |line| {
            let level = if line.starts_with("WARNING:") || line.starts_with("ERROR:") {
                Level::Warn
            } else {
                Level::Info
            };
            emit(
                level,
                "video.music.download.progress",
                &format!("Music download: {line}"),
                None,
            );
        })
        .with_context(|| format!("Failed to download music from {url}"))?;

        if !destination.exists() {
            let mut resolved = None;
            for entry in fs::read_dir(&cache_path).with_context(|| {
                format!("Failed to inspect cache directory {}", cache_path.display())
            })? {
                let entry = entry?;
                if !entry.file_type()?.is_file() {
                    continue;
                }
                let file_name = entry.file_name();
                if let Some(name) = file_name.to_str()
                    && name.starts_with(&hash)
                    && !name.ends_with(".info.json")
                    && !name.ends_with(".description")
                {
                    resolved = Some(entry.path());
                    break;
                }
            }

            if let Some(actual) = resolved {
                fs::rename(&actual, &destination).with_context(|| {
                    format!(
                        "Failed to rename {} to {}",
                        actual.display(),
                        destination.display()
                    )
                })?;
            }
        }

        if !destination.exists() {
            bail!(
                "yt-dlp reported success but {} was not created",
                destination.display()
            );
        }

        emit(
            Level::Success,
            "video.music.download.complete",
            "Music download complete",
            None,
        );
        Ok(destination)
    }
}

fn run_music_download(download: Expression, mut on_line: impl FnMut(&str)) -> Result<()> {
    // Merge before reading so neither stream can fill its pipe and stall the
    // download. Route through emit rather than inheriting output (JSON mode).
    let handle = download
        .stdin_null()
        .stderr_to_stdout()
        .unchecked()
        .reader()
        .context("Failed to spawn yt-dlp")?;
    let mut reader = BufReader::new(handle);
    let mut tail = VecDeque::new();
    let mut buffer = Vec::new();
    loop {
        buffer.clear();
        let read = match reader.read_until(b'\n', &mut buffer) {
            Ok(read) => read,
            Err(error) => {
                let _ = reader.get_ref().kill();
                return Err(error).context("Failed to read yt-dlp progress");
            }
        };
        if read == 0 {
            break;
        }
        let line = String::from_utf8_lossy(&buffer);
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        on_line(line);
        if tail.len() == 24 {
            tail.pop_front();
        }
        tail.push_back(line.chars().take(1024).collect::<String>());
    }
    // Duct guarantees an exit status after the reader reaches EOF.
    let output = reader
        .get_ref()
        .try_wait()?
        .context("yt-dlp exited without a status")?;
    if !output.status.success() {
        bail!(
            "yt-dlp exited with {}:\n{}",
            output.status,
            tail.into_iter().collect::<Vec<_>>().join("\n")
        );
    }
    Ok(())
}

fn is_url(value: &str) -> bool {
    let lower = value.trim().to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::video::document::MusicDirective;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn resolves_music_from_markdown_music_directory() {
        let temp = tempdir().unwrap();
        let music_dir = temp.path().join("music");
        fs::create_dir_all(&music_dir).unwrap();
        let file = music_dir.join("track.mp3");
        fs::write(&file, b"dummy").unwrap();

        let mut resolver = MusicResolver::new(temp.path());
        let resolved = resolver
            .resolve(&MusicDirective::Source("track.mp3".to_string()))
            .unwrap()
            .unwrap();

        assert_eq!(resolved, canonicalize_existing(&file).unwrap());
    }

    #[test]
    #[cfg(unix)]
    fn streams_download_metadata_progress_and_postprocessing() {
        let download = cmd(
            "sh",
            [
                "-c",
                "printf '[youtube] Extracting metadata\\n'; printf '[download] 25%% ETA 00:03\\n' >&2; printf '[download] 100%%\\n'; printf '[FixupM4a] Processing\\n' >&2",
            ],
        );
        let mut lines = Vec::new();
        run_music_download(download, |line| lines.push(line.to_string())).unwrap();
        assert_eq!(
            lines,
            [
                "[youtube] Extracting metadata",
                "[download] 25% ETA 00:03",
                "[download] 100%",
                "[FixupM4a] Processing",
            ]
        );
    }

    #[test]
    #[cfg(unix)]
    fn download_failures_keep_diagnostics_and_report_live_output() {
        let download = cmd(
            "sh",
            [
                "-c",
                "printf '[download] 20%%\\n'; printf 'ERROR: unavailable\\n' >&2; exit 7",
            ],
        );
        let mut lines = Vec::new();
        let error = run_music_download(download, |line| lines.push(line.to_string())).unwrap_err();
        assert_eq!(lines, ["[download] 20%", "ERROR: unavailable"]);
        assert!(error.to_string().contains("ERROR: unavailable"));
        assert!(error.to_string().contains("7"));
    }

    #[test]
    #[cfg(unix)]
    fn download_diagnostics_are_bounded_and_lossy_utf8_is_supported() {
        let download = cmd(
            "sh",
            [
                "-c",
                "printf 'old diagnostic\\n'; i=0; while [ \"$i\" -lt 30 ]; do printf 'progress\\n' >&2; i=$((i + 1)); done; printf 'ERROR: \\377failure' >&2; exit 1",
            ],
        );
        let error = run_music_download(download, |_| {})
            .unwrap_err()
            .to_string();
        assert!(!error.contains("old diagnostic"));
        assert!(error.contains("failure"));
        assert!(error.lines().count() <= 25);
    }

    #[test]
    fn returns_none_for_none_directive() {
        let temp = tempdir().unwrap();
        let mut resolver = MusicResolver::new(temp.path());
        let resolved = resolver.resolve(&MusicDirective::None).unwrap();
        assert!(resolved.is_none());
    }
}
