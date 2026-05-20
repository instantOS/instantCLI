use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;

use super::types::{VideoMetadata, VideoSource};

pub fn parse_metadata(front_matter: Option<&str>, source_path: &Path) -> Result<VideoMetadata> {
    let Some(fm) = front_matter else {
        return Ok(VideoMetadata {
            sources: Vec::new(),
            default_source: None,
        });
    };

    if fm.trim().is_empty() {
        return Ok(VideoMetadata {
            sources: Vec::new(),
            default_source: None,
        });
    }

    let parsed: FrontMatter = serde_yaml::from_str(fm).with_context(|| {
        format!(
            "Failed to parse YAML front matter in {}",
            source_path.display()
        )
    })?;

    let mut sources = Vec::new();
    if let Some(entries) = parsed.sources {
        for entry in entries {
            let id = entry
                .id
                .ok_or_else(|| anyhow!("Each source must include an id"))?;
            let id = id.trim().to_string();
            if id.is_empty() {
                bail!("Source id must not be empty");
            }
            if id.contains(':') || id.contains(char::is_whitespace) {
                bail!("Source id `{}` must not include ':' or whitespace", id);
            }
            let source = entry
                .source
                .ok_or_else(|| anyhow!("Source `{}` is missing `source`", id))?;
            let transcript = entry
                .transcript
                .ok_or_else(|| anyhow!("Source `{}` is missing `transcript`", id))?;
            sources.push(VideoSource {
                id,
                name: entry.name,
                source: PathBuf::from(source),
                transcript: PathBuf::from(transcript),
                audio: PathBuf::new(),
                hash: entry.hash,
            });
        }
    }

    if sources.is_empty() {
        return Ok(VideoMetadata {
            sources: Vec::new(),
            default_source: None,
        });
    }

    let mut default_source = parsed.default_source;
    if default_source.is_none() && sources.len() == 1 {
        default_source = Some(sources[0].id.clone());
    }

    if let Some(default_id) = default_source.as_ref()
        && !sources.iter().any(|source| &source.id == default_id)
    {
        bail!(
            "default_source `{}` does not match any declared source id",
            default_id
        );
    }

    let mut seen = HashSet::new();
    for source in &sources {
        if !seen.insert(source.id.clone()) {
            bail!("Duplicate source id `{}` in front matter", source.id);
        }
    }

    Ok(VideoMetadata {
        sources,
        default_source,
    })
}

#[derive(Debug, Deserialize)]
struct FrontMatter {
    sources: Option<Vec<FrontMatterSource>>,
    default_source: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FrontMatterSource {
    id: Option<String>,
    name: Option<String>,
    source: Option<String>,
    transcript: Option<String>,
    hash: Option<String>,
}
