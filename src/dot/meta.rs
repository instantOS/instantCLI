use anyhow::{Context, Result};
use git2::Repository;
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

/// Validate that the given path is a git repository
fn ensure_git_repo(repo_path: &Path) -> Result<()> {
    Repository::open(repo_path)
        .map(|_| ())
        .with_context(|| format!("Not a git repository: {}", repo_path.display()))
}

#[derive(Deserialize, Debug, Clone)]
pub struct RepoMetaData {
    pub name: String,
    pub author: Option<String>,
    pub description: Option<String>,
    #[serde(default = "default_dots_dirs")]
    pub dots_dirs: Vec<String>,
}

fn default_dots_dirs() -> Vec<String> {
    vec!["dots".to_string()]
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
pub fn init_repo(repo_path: &Path, name: Option<&str>, non_interactive: bool) -> Result<()> {
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

    // Use name (non-interactive mode or prompt)
    let final_name = if non_interactive {
        match name {
            Some(s) if !s.trim().is_empty() => s.trim().to_string(),
            _ => inferred,
        }
    } else {
        let default_name = match name {
            Some(s) if !s.trim().is_empty() => s.trim().to_string(),
            _ => inferred,
        };

        print!("Name [{default_name}]: ");
        io::stdout().flush().ok();
        let mut input = String::new();
        io::stdin()
            .read_line(&mut input)
            .context("reading name from stdin")?;
        if input.trim().is_empty() {
            default_name
        } else {
            input.trim().to_string()
        }
    };

    // Get author and description (non-interactive mode or prompt)
    let (author, description) = if non_interactive {
        (None, None)
    } else {
        // Prompt for optional author
        print!("Author (optional): ");
        io::stdout().flush().ok();
        let mut input = String::new();
        io::stdin()
            .read_line(&mut input)
            .context("reading author from stdin")?;
        let author = match input.trim() {
            "" => None,
            s => Some(s.to_string()),
        };

        // Prompt for optional description
        print!("Description (optional): ");
        io::stdout().flush().ok();
        let mut input = String::new();
        io::stdin()
            .read_line(&mut input)
            .context("reading description from stdin")?;
        let description = match input.trim() {
            "" => None,
            s => Some(s.to_string()),
        };
        (author, description)
    };

    #[derive(Serialize)]
    struct MetaWrite {
        name: String,
        author: Option<String>,
        description: Option<String>,
        dots_dirs: Vec<String>,
    }

    let mw = MetaWrite {
        name: final_name,
        author,
        description,
        dots_dirs: vec!["dots".to_string()],
    };
    let toml = toml::to_string_pretty(&mw).context("serializing instantdots.toml")?;
    fs::write(&p, toml).with_context(|| format!("writing {}", p.display()))?;
    Ok(())
}
