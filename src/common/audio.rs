use anyhow::{Context, Result};
use std::process::Command;

#[derive(Debug, Clone)]
pub struct AudioDefaults {
    pub sink: Option<String>,
    pub source: Option<String>,
}

impl AudioDefaults {}

#[derive(Debug, Clone)]
pub struct AudioSourceInfo {
    pub name: String,
    pub driver: Option<String>,
    pub sample_spec: Option<String>,
    pub channel_map: Option<String>,
    pub state: Option<String>,
}

pub fn pactl_defaults() -> Result<AudioDefaults> {
    let output = Command::new("pactl")
        .arg("info")
        .output()
        .context("Failed to run pactl info")?;

    if !output.status.success() {
        anyhow::bail!("pactl info failed");
    }

    let info = String::from_utf8_lossy(&output.stdout);
    let mut defaults = AudioDefaults {
        sink: None,
        source: None,
    };

    for line in info.lines() {
        if let Some(value) = line.strip_prefix("Default Sink:") {
            defaults.sink = Some(value.trim().to_string());
        }
        if let Some(value) = line.strip_prefix("Default Source:") {
            defaults.source = Some(value.trim().to_string());
        }
    }

    Ok(defaults)
}

pub fn list_audio_sources_short() -> Result<Vec<AudioSourceInfo>> {
    let output = Command::new("pactl")
        .args(["list", "sources", "short"])
        .output()
        .context("Failed to run pactl list sources")?;

    if !output.status.success() {
        anyhow::bail!("pactl list sources failed");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut sources = Vec::new();

    for line in stdout.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 2 {
            continue;
        }

        sources.push(AudioSourceInfo {
            name: parts[1].to_string(),
            driver: parts.get(2).map(|value| value.to_string()),
            sample_spec: parts.get(3).map(|value| value.to_string()),
            channel_map: parts.get(4).map(|value| value.to_string()),
            state: parts.get(5).map(|value| value.to_string()),
        });
    }

    Ok(sources)
}

pub fn list_audio_source_names() -> Result<Vec<String>> {
    Ok(list_audio_sources_short()?
        .into_iter()
        .map(|source| source.name)
        .collect())
}

pub fn default_source_names(defaults: &AudioDefaults, sources: &[AudioSourceInfo]) -> Vec<String> {
    let mut names = Vec::new();

    if let Some(default_output) = defaults
        .sink
        .as_ref()
        .map(|sink| format!("{}.monitor", sink))
        && sources.iter().any(|source| source.name == default_output)
    {
        names.push(default_output);
    }

    if let Some(default_input) = defaults.source.as_ref()
        && sources.iter().any(|source| &source.name == default_input)
    {
        names.push(default_input.clone());
    }

    names.sort();
    names.dedup();
    names
}
