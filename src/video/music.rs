use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use dirs::{cache_dir, data_dir, home_dir};
use sha2::{Digest, Sha256};

use super::document::MusicDirective;
use super::utils::canonicalize_existing;

pub struct MusicResolver {
    markdown_dir: PathBuf,
    cache: HashMap<String, PathBuf>,
}

impl MusicResolver {
    pub fn new(markdown_dir: &Path) -> Self {
        Self {
            markdown_dir: markdown_dir.to_path_buf(),
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

        if trimmed.starts_with("~/") {
            if let Some(home) = home_dir() {
                let candidate = home.join(trimmed.trim_start_matches("~/"));
                if candidate.exists() {
                    return Ok(candidate);
                }
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

        if relative.parent().is_none() {
            attempted.push(self.markdown_dir.join("music").join(relative));
        }
        attempted.push(self.markdown_dir.join(relative));

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
        let hash = format!("{:x}", hasher.finalize());

        let mut cache_path = cache_dir().context("Unable to resolve cache directory for music downloads")?;
        cache_path.push("instant");
        cache_path.push("music");
        fs::create_dir_all(&cache_path).with_context(|| {
            format!("Failed to create cache directory {}", cache_path.display())
        })?;

        let destination = cache_path.join(&hash);
        if destination.exists() {
            return Ok(destination);
        }

        let output = Command::new("yt-dlp")
            .arg("--no-part")
            .arg("--quiet")
            .arg("--no-warnings")
            .arg("-f")
            .arg("bestaudio/best")
            .arg("-o")
            .arg(destination.to_string_lossy().into_owned())
            .arg(url)
            .output()
            .with_context(|| format!("Failed to spawn yt-dlp for {}", url))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("yt-dlp failed to download {}: {}", url, stderr.trim());
        }

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
                if let Some(name) = file_name.to_str() {
                    if name.starts_with(&hash)
                        && !name.ends_with(".info.json")
                        && !name.ends_with(".description")
                    {
                        resolved = Some(entry.path());
                        break;
                    }
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

        Ok(destination)
    }
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
    fn returns_none_for_none_directive() {
        let temp = tempdir().unwrap();
        let mut resolver = MusicResolver::new(temp.path());
        let resolved = resolver.resolve(&MusicDirective::None).unwrap();
        assert!(resolved.is_none());
    }
}
