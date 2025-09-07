use anyhow::{Context, Result};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

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
    let s = fs::read_to_string(&p).with_context(|| format!("reading {}", p.display()))?;
    let meta: RepoMetaData = toml::from_str(&s).context("parsing instantdots.toml")?;

    // ensure required fields
    if meta.name.trim().is_empty() {
        anyhow::bail!("instantdots.toml missing required 'name' field or it's empty");
    }

    Ok(meta)
}

/// Initialize the given repository path as an instantdots repo by creating
/// an instantdots.toml file with the provided or inferred name.
pub fn init_repo(repo_path: &Path, name: Option<&str>) -> Result<()> {
    let p = repo_path.join("instantdots.toml");
    if p.exists() {
        anyhow::bail!("instantdots.toml already exists at {}", p.display());
    }

    // determine name: use provided, otherwise infer from directory name
    let name_str = match name {
        Some(s) if !s.trim().is_empty() => s.trim().to_string(),
        _ => repo_path
            .file_name()
            .and_then(|os| os.to_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "dotfiles".to_string()),
    };

    #[derive(Serialize)]
    struct MetaWrite<'a> {
        name: &'a str,
        description: Option<&'a str>,
    }

    let mw = MetaWrite { name: &name_str, description: None };
    let toml = toml::to_string_pretty(&mw).context("serializing instantdots.toml")?;
    fs::write(&p, toml).with_context(|| format!("writing {}", p.display()))?;
    Ok(())
}
