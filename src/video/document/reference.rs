use std::collections::HashSet;

use anyhow::{Context, Result, bail};

use super::time::parse_time_range;
use super::types::{TimeRange, VideoMetadata};

const DEFAULT_SOURCE_ID: &str = "a";

pub struct SegmentSourceConfig {
    pub default_source: String,
    pub known_sources: HashSet<String>,
    pub require_explicit: bool,
}

impl SegmentSourceConfig {
    pub fn from_metadata(metadata: &VideoMetadata) -> Result<Self> {
        if metadata.sources.is_empty() {
            return Ok(Self {
                default_source: DEFAULT_SOURCE_ID.to_string(),
                known_sources: HashSet::new(),
                require_explicit: false,
            });
        }

        let mut known_sources = HashSet::new();
        for source in &metadata.sources {
            known_sources.insert(source.id.clone());
        }

        let require_explicit = metadata.sources.len() > 1;
        let default_source = metadata
            .default_source
            .clone()
            .unwrap_or_else(|| metadata.sources[0].id.clone());

        if !known_sources.contains(&default_source) {
            bail!(
                "default_source `{}` is not a declared source",
                default_source
            );
        }

        Ok(Self {
            default_source,
            known_sources,
            require_explicit,
        })
    }
}

pub fn parse_segment_reference(
    input: &str,
    source_config: &SegmentSourceConfig,
    line: usize,
) -> Result<(String, TimeRange)> {
    let trimmed = input.trim();
    let mut explicit_source: Option<&str> = None;
    let mut range_str = trimmed;

    if let Some((prefix, rest)) = trimmed.split_once('@') {
        let prefix = prefix.trim();
        let rest = rest.trim();
        let prefix_valid = is_valid_source_id(prefix);
        if !source_config.known_sources.is_empty() {
            if source_config.known_sources.contains(prefix) {
                explicit_source = Some(prefix);
                range_str = rest;
            } else if prefix_valid {
                bail!("Unknown source id `{}` at line {}", prefix, line);
            }
        } else if prefix_valid {
            explicit_source = Some(prefix);
            range_str = rest;
        }
    }

    if explicit_source.is_none() && source_config.require_explicit {
        bail!("Missing source id for timestamp at line {}", line);
    }

    let source_id = explicit_source.unwrap_or(source_config.default_source.as_str());

    let source_id = source_id.trim();
    if source_id.is_empty() {
        bail!("Missing source id at line {}", line);
    }
    if source_id.contains(char::is_whitespace) {
        bail!(
            "Source id `{}` at line {} must not include whitespace",
            source_id,
            line
        );
    }

    if !source_config.known_sources.is_empty() && !source_config.known_sources.contains(source_id) {
        bail!("Unknown source id `{}` at line {}", source_id, line);
    }

    let range = parse_time_range(range_str).with_context(|| {
        format!(
            "Invalid timestamp range `{}` for source `{}`",
            range_str, source_id
        )
    })?;

    Ok((source_id.to_string(), range))
}

pub fn is_valid_source_id(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_alphabetic() {
        return false;
    }
    if value.contains(char::is_whitespace) || value.contains(':') {
        return false;
    }
    for ch in chars {
        if !(ch.is_ascii_alphanumeric() || ch == '-' || ch == '_') {
            return false;
        }
    }
    true
}

/// Checks if a code span looks like a timestamp reference.
/// A timestamp reference contains a time format with `:` and `.` (e.g., `00:01.0` or `a@00:01.0-00:02.0`).
pub fn looks_like_timestamp_reference(value: &str) -> bool {
    let trimmed = value.trim();
    let range_part = if let Some((_, rest)) = trimmed.split_once('@') {
        rest.trim()
    } else {
        trimmed
    };
    range_part.contains(':') && range_part.contains('.')
}
