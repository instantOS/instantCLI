use anyhow::{Context, Result};
use git2::{Repository, Signature};
use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::common::config::DocumentedConfig;
use crate::dot::config::{self, Config};
use crate::ui::prelude::*;

/// Validate that the given path is a git repository
fn ensure_git_repo(repo_path: &Path) -> Result<()> {
    Repository::open(repo_path)
        .map(|_| ())
        .with_context(|| format!("Not a git repository: {}", repo_path.display()))
}

use crate::dot::types::RepoMetaData;

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

/// Write updated metadata to instantdots.toml
pub fn update_meta(repo_path: &Path, meta: &RepoMetaData) -> Result<()> {
    let toml_path = repo_path.join("instantdots.toml");
    meta.save_with_documentation(&toml_path)
        .with_context(|| format!("writing {}", toml_path.display()))?;
    Ok(())
}

/// Add a new dotfile directory to a repository's instantdots.toml
///
/// This function:
/// 1. Reads the current instantdots.toml
/// 2. Adds the new directory name to dots_dirs if not already present
/// 3. Creates the directory on disk if it doesn't exist
/// 4. Writes the updated instantdots.toml
pub fn add_dots_dir(repo_path: &Path, new_dir: &str) -> Result<()> {
    // Validate directory name
    let new_dir = new_dir.trim();
    if new_dir.is_empty() {
        anyhow::bail!("Directory name cannot be empty");
    }
    if new_dir.contains('/') || new_dir.contains('\\') {
        anyhow::bail!("Directory name cannot contain path separators");
    }

    // Read existing metadata
    let mut meta = read_meta(repo_path)?;

    // Check if already exists
    if meta.dots_dirs.contains(&new_dir.to_string()) {
        anyhow::bail!(
            "Dotfile directory '{}' already exists in this repository",
            new_dir
        );
    }

    // Create directory if not exists
    let dir_path = repo_path.join(new_dir);
    if !dir_path.exists() {
        fs::create_dir_all(&dir_path)
            .with_context(|| format!("creating directory {}", dir_path.display()))?;
    }

    // Update metadata
    meta.dots_dirs.push(new_dir.to_string());

    // Write back
    update_meta(repo_path, &meta)?;
    Ok(())
}

/// Remove a dotfile directory from a repository's instantdots.toml
///
/// This function:
/// 1. Reads the current instantdots.toml
/// 2. Removes the directory name from dots_dirs
/// 3. Optionally deletes the directory from disk
/// 4. Writes the updated instantdots.toml
///
/// Returns the path to the removed directory (for git operations)
pub fn remove_dots_dir(repo_path: &Path, dir_name: &str, delete_files: bool) -> Result<PathBuf> {
    let dir_name = dir_name.trim();
    if dir_name.is_empty() {
        anyhow::bail!("Directory name cannot be empty");
    }

    // Read existing metadata
    let mut meta = read_meta(repo_path)?;

    // Check if it exists
    if !meta.dots_dirs.contains(&dir_name.to_string()) {
        anyhow::bail!(
            "Dotfile directory '{}' does not exist in this repository",
            dir_name
        );
    }

    // Don't allow removing the last subdir
    if meta.dots_dirs.len() == 1 {
        anyhow::bail!(
            "Cannot remove '{}' - it's the only dotfile directory in this repository",
            dir_name
        );
    }

    let dir_path = repo_path.join(dir_name);

    // Remove from metadata
    meta.dots_dirs.retain(|d| d != dir_name);

    // Optionally delete the directory
    if delete_files && dir_path.exists() {
        fs::remove_dir_all(&dir_path)
            .with_context(|| format!("deleting directory {}", dir_path.display()))?;
    }

    // Write back
    update_meta(repo_path, &meta)?;
    Ok(dir_path)
}

/// Input structure for gathering repository metadata from user.
///
/// # Adding New Fields to instantdots.toml
///
/// All metadata fields are centralized in this module. To add a new field:
///
/// 1. Add it to `RepoInputs` struct (here)
/// 2. Add prompting logic in `gather_repo_inputs()` function
/// 3. Add it to `MetaWrite` struct in `write_instantdots_toml()` function
/// 4. Add it to `RepoMetaData` struct (if it needs to be read back)
///
/// Example: Adding a `license` field:
/// ```rust,ignore
/// // 1. In RepoInputs:
/// struct RepoInputs {
///     name: String,
///     author: Option<String>,
///     description: Option<String>,
///     license: Option<String>,  // <-- Add here
/// }
///
/// // 2. In gather_repo_inputs():
/// print!("License (optional): ");
/// io::stdout().flush().ok();
/// let mut input = String::new();
/// io::stdin().read_line(&mut input).context("reading license from stdin")?;
/// let license = match input.trim() {
///     "" => None,
///     s => Some(s.to_string()),
/// };
///
/// Ok(RepoInputs {
///     name,
///     author,
///     description,
///     license,  // <-- Include in return
/// })
///
/// // 3. In write_instantdots_toml():
/// struct MetaWrite {
///     name: String,
///     author: Option<String>,
///     description: Option<String>,
///     license: Option<String>,  // <-- Add here
///     dots_dirs: Vec<String>,
///     default_active_subdirs: Option<Vec<String>>,
/// }
/// let meta = MetaWrite {
///     name: inputs.name.clone(),
///     author: inputs.author.clone(),
///     description: inputs.description.clone(),
///     license: inputs.license.clone(),  // <-- Include in construction
///     dots_dirs: vec!["dots".to_string()],
///     default_active_subdirs: None,
/// };
///
/// // 4. In RepoMetaData (for reading):
/// pub struct RepoMetaData {
///     pub name: String,
///     pub author: Option<String>,
///     pub description: Option<String>,
///     pub license: Option<String>,  // <-- Add here
///     #[serde(default = "default_dots_dirs")]
///     pub dots_dirs: Vec<String>,
///     #[serde(default)]
///     pub default_active_subdirs: Option<Vec<String>>,
/// }
/// ```
struct RepoInputs {
    name: String,
    author: Option<String>,
    description: Option<String>,
    read_only: bool,
    dots_dir: String,
}

use crate::menu_utils::{ConfirmResult, FzfWrapper};

/// Gather repository metadata inputs interactively or non-interactively.
/// This is the single source of truth for prompting users for repo metadata.
/// When adding new fields to instantdots.toml, add them here.
fn gather_repo_inputs(
    default_name: &str,
    non_interactive: bool,
    skip_name_prompt: bool,
    skip_readonly_prompt: bool,
) -> Result<RepoInputs> {
    if non_interactive {
        return Ok(RepoInputs {
            name: default_name.to_string(),
            author: None,
            description: None,
            read_only: false,
            dots_dir: "dots".to_string(),
        });
    }

    let name = if skip_name_prompt {
        default_name.to_string()
    } else {
        FzfWrapper::input(&format!("Name [{}]: ", default_name))
            .map(|s| {
                let trimmed = s.trim();
                if trimmed.is_empty() {
                    default_name.to_string()
                } else {
                    trimmed.to_string()
                }
            })
            .unwrap_or_else(|_| default_name.to_string())
    };

    let author = FzfWrapper::input("Author (optional): ")
        .map(|s| {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        })
        .unwrap_or(None);

    let description = FzfWrapper::input("Description (optional): ")
        .map(|s| {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        })
        .unwrap_or(None);

    let read_only = if skip_readonly_prompt {
        false
    } else {
        matches!(FzfWrapper::confirm("Read-only?"), Ok(ConfirmResult::Yes))
    };

    let dots_dir = FzfWrapper::input("Dotfiles directory [dots]: ")
        .map(|s| {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                "dots".to_string()
            } else {
                trimmed.to_string()
            }
        })
        .unwrap_or_else(|_| "dots".to_string());

    Ok(RepoInputs {
        name,
        author,
        description,
        read_only,
        dots_dir,
    })
}

/// Write instantdots.toml metadata file.
/// This is the single source of truth for the metadata file structure.
/// When adding new fields to instantdots.toml, update the MetaWrite struct here.
fn write_instantdots_toml(repo_path: &Path, inputs: &RepoInputs) -> Result<()> {
    let toml_path = repo_path.join("instantdots.toml");

    let meta = RepoMetaData {
        name: inputs.name.clone(),
        author: inputs.author.clone(),
        description: inputs.description.clone(),
        read_only: if inputs.read_only { Some(true) } else { None },
        dots_dirs: vec![inputs.dots_dir.clone()],
        default_active_subdirs: None,
        units: Vec::new(),
    };

    meta.save_with_documentation(&toml_path)
        .with_context(|| format!("writing {}", toml_path.display()))?;
    Ok(())
}

/// Initialize the given repository path as an instantdots repo by creating
/// an instantdots.toml file with either the provided name or one prompted
/// interactively (defaults to the repo directory name if empty). Also prompts
/// for an optional description. The function verifies the directory is a git
/// repository before creating the file.
pub fn init_repo(repo_path: &Path, name: Option<&str>, non_interactive: bool) -> Result<()> {
    ensure_git_repo(repo_path)?;

    let toml_path = repo_path.join("instantdots.toml");
    if toml_path.exists() {
        anyhow::bail!("instantdots.toml already exists at {}", toml_path.display());
    }

    let default_name = repo_path
        .file_name()
        .and_then(|os| os.to_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "dotfiles".to_string());

    let default_name = name
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.trim().to_string())
        .unwrap_or(default_name);

    let mut inputs = gather_repo_inputs(&default_name, non_interactive, false, false)?;

    // Check if dots_dir exists, offer to create or rename
    if !non_interactive {
        loop {
            let dots_path = repo_path.join(&inputs.dots_dir);
            if dots_path.exists() {
                break;
            }

            println!(
                "{} Directory '{}' does not exist",
                char::from(NerdFont::Warning),
                inputs.dots_dir
            );

            match FzfWrapper::confirm(&format!("Create '{}'?", inputs.dots_dir)) {
                Ok(ConfirmResult::Yes) => {
                    fs::create_dir_all(&dots_path)
                        .with_context(|| format!("creating directory {}", dots_path.display()))?;
                    println!(
                        "{} Created directory '{}'",
                        char::from(NerdFont::Check),
                        inputs.dots_dir
                    );
                    break;
                }
                _ => {
                    // Let user enter a different name
                    inputs.dots_dir = FzfWrapper::input("Dotfiles directory [dots]: ")
                        .map(|s| {
                            let trimmed = s.trim();
                            if trimmed.is_empty() {
                                "dots".to_string()
                            } else {
                                trimmed.to_string()
                            }
                        })
                        .unwrap_or_else(|_| "dots".to_string());
                }
            }
        }
    }

    write_instantdots_toml(repo_path, &inputs)?;
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
                    "{} Added instantCLI dotfile metadata to existing repository",
                    char::from(NerdFont::Check)
                ),
                None,
            );
            println!("  Location: {}", path.display());
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
    if let Some(outcome) = handle_existing_git_repo(current_dir, name, non_interactive)? {
        return Ok(outcome);
    }

    if let Some(outcome) = check_already_configured(config) {
        return Ok(outcome);
    }

    create_local_repo(config, name, non_interactive, false, false)
}

fn handle_existing_git_repo(
    current_dir: &Path,
    name: Option<&str>,
    non_interactive: bool,
) -> Result<Option<InitOutcome>> {
    if Repository::open(current_dir).is_err() {
        return Ok(None);
    }

    if !non_interactive {
        println!("Adding instantCLI dotfile metadata to existing git repository");
        println!("Location: {}", current_dir.display());
        println!();
    }

    // Just create instantdots.toml - don't add to global config
    // User should clone/add the repo separately if they want it tracked
    init_repo(current_dir, name, non_interactive)?;

    Ok(Some(InitOutcome::InitializedInPlace {
        path: current_dir.to_path_buf(),
    }))
}

fn check_already_configured(config: &Config) -> Option<InitOutcome> {
    // Filter out read-only repositories
    let writable_repos = config.get_writable_repos();

    if !writable_repos.is_empty() {
        let existing = writable_repos
            .iter()
            .map(|repo| ExistingRepoInfo {
                name: repo.name.clone(),
                path: config.repos_path().join(&repo.name),
                url: repo.url.clone(),
            })
            .collect();
        return Some(InitOutcome::AlreadyConfigured { existing });
    }
    None
}

pub fn create_local_repo(
    config: &mut Config,
    name: Option<&str>,
    non_interactive: bool,
    skip_name_prompt: bool,
    skip_readonly_prompt: bool,
) -> Result<InitOutcome> {
    let default_name = name
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "local".to_string());

    if !non_interactive {
        println!("Creating new dotfile repository");
        println!();
    }

    let inputs = gather_repo_inputs(
        &default_name,
        non_interactive,
        skip_name_prompt,
        skip_readonly_prompt,
    )?;

    let (repo_name, repo_path) = determine_repo_path(config, &inputs.name);

    if !non_interactive {
        println!();
        println!("Location: {}", repo_path.display());
        println!();
    }

    let cleanup_on_error = || {
        if repo_path.exists() {
            let _ = fs::remove_dir_all(&repo_path);
        }
    };

    let repo = Repository::init(&repo_path).with_context(|| {
        cleanup_on_error();
        format!("creating git repository at {}", repo_path.display())
    })?;

    fs::create_dir_all(repo_path.join("dots")).with_context(|| {
        cleanup_on_error();
        format!(
            "creating dots directory at {}",
            repo_path.join("dots").display()
        )
    })?;

    write_instantdots_toml(&repo_path, &inputs).with_context(|| {
        cleanup_on_error();
        "writing instantdots.toml"
    })?;

    let gitkeep_path = repo_path.join("dots/.gitkeep");
    fs::write(&gitkeep_path, b"").with_context(|| {
        cleanup_on_error();
        format!("creating {}", gitkeep_path.display())
    })?;

    let mut index = repo.index().with_context(|| {
        cleanup_on_error();
        "opening git index"
    })?;
    index
        .add_path(Path::new("instantdots.toml"))
        .with_context(|| {
            cleanup_on_error();
            "adding instantdots.toml to index"
        })?;
    index
        .add_path(Path::new("dots/.gitkeep"))
        .with_context(|| {
            cleanup_on_error();
            "adding dots/.gitkeep to index"
        })?;
    index.write().with_context(|| {
        cleanup_on_error();
        "writing git index"
    })?;
    let tree_id = index.write_tree().with_context(|| {
        cleanup_on_error();
        "writing git tree"
    })?;
    let tree = repo.find_tree(tree_id).with_context(|| {
        cleanup_on_error();
        "loading git tree"
    })?;

    let signature = repo
        .signature()
        .or_else(|_| Signature::now("instantCLI", "instant@localhost"))
        .with_context(|| {
            cleanup_on_error();
            "creating git signature"
        })?;

    repo.commit(
        Some("HEAD"),
        &signature,
        &signature,
        "Initial instantCLI dotfile commit",
        &tree,
        &[],
    )
    .with_context(|| {
        cleanup_on_error();
        "creating initial commit"
    })?;

    let repo_config = config::Repo {
        url: repo_path.to_string_lossy().to_string(),
        name: repo_name.clone(),
        branch: None,
        active_subdirectories: None,
        enabled: true,
        read_only: false,
        metadata: None,
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

fn name_in_use(config: &Config, name: &str) -> bool {
    config.repos.iter().any(|r| r.name == name)
}

fn determine_repo_path(config: &Config, desired_name: &str) -> (String, PathBuf) {
    let sanitized = desired_name.trim().to_string();

    if !name_in_use(config, &sanitized) {
        let path = config.repos_path().join(&sanitized);
        if !path.exists() {
            return (sanitized, path);
        }
    }

    let mut counter = 2;
    loop {
        let candidate = format!("{}-{}", sanitized, counter);
        if !name_in_use(config, &candidate) {
            let path = config.repos_path().join(&candidate);
            if !path.exists() {
                return (candidate, path);
            }
        }
        counter += 1;
    }
}
