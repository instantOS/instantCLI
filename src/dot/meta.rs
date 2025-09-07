use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;

#[derive(Deserialize, Debug)]
pub struct RepoMetaData {
    pub name: String,
    pub description: Option<String>,
}

pub fn read_meta(repo_path: &Path) -> Result<RepoMetaData> {
    let p = repo_path.join("instantdots.toml");
    if !p.exists() {
        anyhow::bail!("missing instantdots.toml");
    }
    let s = std::fs::read_to_string(&p).with_context(|| format!("reading {}", p.display()))?;
    let meta: RepoMetaData = toml::from_str(&s).context("parsing instantdots.toml")?;

    // ensure required fields
    if meta.name.trim().is_empty() {
        anyhow::bail!("instantdots.toml missing required 'name' field or it's empty");
    }

    Ok(meta)
}
