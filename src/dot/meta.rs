use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

/// Validate that the given path is a git repository
fn ensure_git_repo(repo_path: &Path) -> Result<()> {
    use std::process::Command;

    let git_check = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .arg("rev-parse")
        .arg("--is-inside-work-tree")
        .output()
        .context("checking git repository")?;
    if !git_check.status.success() {
        anyhow::bail!("current directory is not a git repository");
    }
    Ok(())
}

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
/// an instantdots.toml file with either the provided name or one prompted
/// interactively (defaults to the repo directory name if empty). Also prompts
/// for an optional description. The function verifies the directory is a git
/// repository before creating the file.
pub fn init_repo(repo_path: &Path, name: Option<&str>) -> Result<()> {
    use std::io::{self, Write};

    // ensure repo_path is a git repository
    ensure_git_repo(repo_path)?;

    let p = repo_path.join("instantdots.toml");
    if p.exists() {
        anyhow::bail!("instantdots.toml already exists at {}", p.display());
    }

    // infer default name from directory name
    let inferred = repo_path
        .file_name()
        .and_then(|os| os.to_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "dotfiles".to_string());

    // Prompt for name (use provided name as default if given)
    let default_name = match name {
        Some(s) if !s.trim().is_empty() => s.trim().to_string(),
        _ => inferred,
    };

    print!("Name [{}]: ", default_name);
    io::stdout().flush().ok();
    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .context("reading name from stdin")?;
    let final_name = if input.trim().is_empty() {
        default_name
    } else {
        input.trim().to_string()
    };

    // Prompt for optional description
    print!("Description (optional): ");
    io::stdout().flush().ok();
    input.clear();
    io::stdin()
        .read_line(&mut input)
        .context("reading description from stdin")?;
    let description = match input.trim() {
        "" => None,
        s => Some(s.to_string()),
    };

    #[derive(Serialize)]
    struct MetaWrite {
        name: String,
        description: Option<String>,
    }

    let mw = MetaWrite {
        name: final_name,
        description,
    };
    let toml = toml::to_string_pretty(&mw).context("serializing instantdots.toml")?;
    fs::write(&p, toml).with_context(|| format!("writing {}", p.display()))?;
    Ok(())
}

/// Non-interactive version of init_repo that uses the provided name and optional description without prompting.
pub fn non_interactive_init(repo_path: &Path, name: &str, description: Option<&str>) -> Result<()> {
    // ensure repo_path is a git repository
    ensure_git_repo(repo_path)?;

    let p = repo_path.join("instantdots.toml");
    if p.exists() {
        anyhow::bail!("instantdots.toml already exists at {}", p.display());
    }

    let final_name = name.to_string();
    let desc = description.map(|s| s.to_string());

    #[derive(Serialize)]
    struct MetaWrite {
        name: String,
        description: Option<String>,
    }

    let mw = MetaWrite {
        name: final_name,
        description: desc,
    };
    let toml = toml::to_string_pretty(&mw).context("serializing instantdots.toml")?;
    fs::write(&p, toml).with_context(|| format!("writing {}", p.display()))?;
    Ok(())
}
