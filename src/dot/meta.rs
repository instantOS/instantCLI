use anyhow::{Context, Result};
use git2::{Repository, Signature};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::dot::config::{self, Config};
use crate::ui::prelude::*;

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

#[derive(Debug, Clone)]
pub struct ExistingRepoInfo {
    pub name: String,
    pub path: PathBuf,
    pub url: String,
}

#[derive(Debug, Clone)]
pub struct CreatedRepoInfo {
    pub name: String,
    pub path: PathBuf,
    pub metadata_name: String,
    pub metadata_path: PathBuf,
    pub config_path: PathBuf,
}

#[derive(Debug, Clone)]
pub enum InitOutcome {
    InitializedInPlace { path: PathBuf },
    CreatedDefault { info: CreatedRepoInfo },
    AlreadyConfigured { existing: Vec<ExistingRepoInfo> },
}

pub fn handle_init_command(
    config: &mut Config,
    current_dir: &Path,
    name: Option<&str>,
    non_interactive: bool,
) -> Result<()> {
    let outcome = init_or_create_default_repo(config, current_dir, name, non_interactive)?;

    match outcome {
        InitOutcome::InitializedInPlace { path } => {
            emit(
                Level::Success,
                "dot.init.initialized_in_place",
                &format!(
                    "{} Initialized instantdots.toml in {}",
                    char::from(NerdFont::Check),
                    path.display()
                ),
                None,
            );
        }
        InitOutcome::CreatedDefault { info } => {
            let CreatedRepoInfo {
                name: repo_name,
                path,
                metadata_name,
                metadata_path,
                config_path,
            } = info;

            emit(
                Level::Success,
                "dot.init.created_default_repo",
                &format!(
                    "{} Created default dotfile repository '{}'",
                    char::from(NerdFont::Check),
                    repo_name
                ),
                None,
            );

            println!("  Location: {}", path.display());
            println!("  Repo name: {}", metadata_name);
            println!("  Metadata: {}", metadata_path.display());
            println!("  Config: {}", config_path.display());
            println!("\nNext steps:");
            println!("  - Add dotfiles with `ins dot add <path>`");
            println!("  - Inspect repo with `git -C {} status`", path.display());
        }
        InitOutcome::AlreadyConfigured { existing } => {
            if let Some(first) = existing.first() {
                emit(
                    Level::Info,
                    "dot.init.already_configured",
                    &format!(
                        "{} Dotfile repository '{}' already configured at {}",
                        char::from(NerdFont::Info),
                        first.name,
                        first.path.display()
                    ),
                    None,
                );
            } else {
                emit(
                    Level::Info,
                    "dot.init.already_configured",
                    &format!(
                        "{} Dotfile repositories already configured",
                        char::from(NerdFont::Info)
                    ),
                    None,
                );
            }

            if !existing.is_empty() {
                println!("Existing repositories:");
                for repo in existing {
                    println!(
                        "  - {} at {} ({})",
                        repo.name,
                        repo.path.display(),
                        repo.url
                    );
                }
            }

            println!(
                "Run `ins dot init` inside a git repository to convert it into an instant dot repo."
            );
        }
    }

    Ok(())
}

pub fn init_or_create_default_repo(
    config: &mut Config,
    current_dir: &Path,
    name: Option<&str>,
    non_interactive: bool,
) -> Result<InitOutcome> {
    if Repository::open(current_dir).is_ok() {
        init_repo(current_dir, name, non_interactive)?;
        return Ok(InitOutcome::InitializedInPlace {
            path: current_dir.to_path_buf(),
        });
    }

    if !config.repos.is_empty() {
        let existing = config
            .repos
            .iter()
            .map(|repo| ExistingRepoInfo {
                name: repo.name.clone(),
                path: config.repos_path().join(&repo.name),
                url: repo.url.clone(),
            })
            .collect();
        return Ok(InitOutcome::AlreadyConfigured { existing });
    }

    let (repo_name, repo_path) = next_available_repo_name(config);

    if !non_interactive {
        println!(
            "Creating new dotfile repository at: {}",
            repo_path.display()
        );
        println!();
    }

    let repo = Repository::init(&repo_path)
        .with_context(|| format!("creating git repository at {}", repo_path.display()))?;

    fs::create_dir_all(repo_path.join("dots")).with_context(|| {
        format!(
            "creating dots directory at {}",
            repo_path.join("dots").display()
        )
    })?;

    init_repo(&repo_path, name, non_interactive)?;

    let gitkeep_path = repo_path.join("dots/.gitkeep");
    if !gitkeep_path.exists() {
        fs::write(&gitkeep_path, b"")
            .with_context(|| format!("creating {}", gitkeep_path.display()))?;
    }

    let mut index = repo.index().context("opening git index")?;
    index
        .add_path(Path::new("instantdots.toml"))
        .context("adding instantdots.toml to index")?;
    index
        .add_path(Path::new("dots/.gitkeep"))
        .context("adding dots/.gitkeep to index")?;
    index.write().context("writing git index")?;
    let tree_id = index.write_tree().context("writing git tree")?;
    let tree = repo.find_tree(tree_id).context("loading git tree")?;

    let signature = repo
        .signature()
        .or_else(|_| Signature::now("instantCLI", "instant@localhost"))
        .context("creating git signature")?;

    repo.commit(
        Some("HEAD"),
        &signature,
        &signature,
        "Initial instantCLI dotfile commit",
        &tree,
        &[],
    )
    .context("creating initial commit")?;

    let repo_config = config::Repo {
        url: repo_path.to_string_lossy().to_string(),
        name: repo_name.clone(),
        branch: None,
        active_subdirectories: vec!["dots".to_string()],
        enabled: true,
    };

    config.add_repo(repo_config, None)?;

    let metadata = read_meta(&repo_path)?;
    let config_path = config::config_file_path(None)?;

    let metadata_path = repo_path.join("instantdots.toml");

    Ok(InitOutcome::CreatedDefault {
        info: CreatedRepoInfo {
            name: repo_name,
            path: repo_path,
            metadata_name: metadata.name,
            metadata_path,
            config_path,
        },
    })
}

fn next_available_repo_name(config: &Config) -> (String, PathBuf) {
    let base_name = "local".to_string();
    if !name_in_use(config, &base_name) {
        let path = config.repos_path().join(&base_name);
        if !path.exists() {
            return (base_name, path);
        }
    }

    let mut counter = 2;
    loop {
        let candidate = format!("local-{}", counter);
        if !name_in_use(config, &candidate) {
            let path = config.repos_path().join(&candidate);
            if !path.exists() {
                return (candidate, path);
            }
        }
        counter += 1;
    }
}

fn name_in_use(config: &Config, name: &str) -> bool {
    config.repos.iter().any(|r| r.name == name)
}
