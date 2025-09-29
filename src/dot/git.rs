use crate::common;
use crate::common::git;
use crate::dot::config;
use crate::dot::db::DotFileType;
use crate::dot::get_all_dotfiles;
use crate::dot::localrepo as repo_mod;
use crate::ui::Fa;
use anyhow::{Context, Result};
use colored::*;
use std::path::PathBuf;
use std::process::Command;

pub fn add_repo(
    config_manager: &mut config::ConfigManager,
    repo: config::Repo,
    debug: bool,
) -> Result<PathBuf> {
    let base = config_manager.config().repos_path();

    let repo_dir_name = repo.name.clone();

    let target = base.join(repo_dir_name);

    if target.exists() {
        return Err(anyhow::anyhow!(
            "Destination '{}' already exists",
            target.display()
        ));
    }

    let depth = config_manager.config().clone_depth;

    let pb = common::progress::create_spinner(format!("Cloning {}...", repo.url));

    git::clone_repo(
        &repo.url,
        &target,
        repo.branch.as_deref(),
        Some(depth as i32),
    )
    .context("Failed to clone repository")?;

    pb.finish_with_message(format!("Cloned {}", repo.url));

    // Note: config addition is now handled by the caller (add_repository function)

    // validate metadata but do not delete invalid clones; report their existence
    let local_repo = repo_mod::LocalRepo::new(&config_manager.config, repo.name.clone())?;
    let meta = &local_repo.meta;

    if debug {
        eprintln!(
            "Repo {} identified as dot repo '{}' - {}",
            local_repo.url,
            meta.name,
            meta.description.as_deref().unwrap_or("")
        );
    }

    // Initialize database with source file hashes to prevent false "modified" status
    // when identical files already exist in the home directory
    if let Ok(db) =
        crate::dot::db::Database::new(config_manager.config().database_path().to_path_buf())
        && let Ok(dotfiles) = get_all_dotfiles(config_manager.config(), &db)
    {
        for (_, dotfile) in dotfiles {
            // Only register hashes for dotfiles from this repository
            if dotfile.source_path.starts_with(&target) {
                // Register the source file hash with source_file=true
                if let Ok(source_hash) =
                    crate::dot::dotfile::Dotfile::compute_hash(&dotfile.source_path)
                {
                    db.add_hash(&source_hash, &dotfile.source_path, DotFileType::SourceFile)?; // source_file=true

                    // If the target file exists and has the same content,
                    // register it with source_file=false
                    if dotfile.target_path.exists()
                        && let Ok(target_hash) =
                            crate::dot::dotfile::Dotfile::compute_hash(&dotfile.target_path)
                        && target_hash == source_hash
                    {
                        db.add_hash(&target_hash, &dotfile.target_path, DotFileType::TargetFile)?; // source_file=false
                    }
                }
            }
        }
    }

    Ok(target)
}

pub fn update_all(cfg: &config::Config, debug: bool) -> Result<()> {
    let repos = cfg.repos.clone();
    if repos.is_empty() {
        println!("No repos configured.");
        return Ok(());
    }

    let mut any_failed = false;

    for repo in repos.iter() {
        let local_repo = repo_mod::LocalRepo::new(cfg, repo.name.clone())?;
        if let Err(e) = local_repo.update(cfg, debug) {
            eprintln!("Failed to update {}:", repo.url);
            for (i, cause) in e.chain().enumerate() {
                if i == 0 {
                    eprintln!("  {}", cause);
                } else {
                    eprintln!("  Caused by: {}", cause);
                }
            }
            any_failed = true;
        }
    }

    if any_failed {
        Err(anyhow::anyhow!(
            "One or more repositories failed to update (see error messages above for details)"
        ))
    } else {
        Ok(())
    }
}

pub fn status_all(
    cfg: &config::Config,
    _debug: bool,
    path: Option<&str>,
    db: &super::db::Database,
    show_all: bool,
) -> Result<()> {
    let all_dotfiles = super::get_all_dotfiles(cfg, db)?;

    if let Some(path_str) = path {
        // Show status for specific path
        show_single_file_status(path_str, &all_dotfiles, cfg, db)?;
    } else {
        // Show summary and file list
        show_status_summary(&all_dotfiles, cfg, db, show_all)?;
    }

    Ok(())
}

fn show_single_file_status(
    path_str: &str,
    all_dotfiles: &std::collections::HashMap<PathBuf, super::Dotfile>,
    cfg: &config::Config,
    db: &super::db::Database,
) -> Result<()> {
    use crate::ui::{OutputFormat, get_output_format, info_with_data};

    let target_path = super::resolve_dotfile_path(path_str)?;

    match get_output_format() {
        OutputFormat::Json => {
            if let Some(dotfile) = all_dotfiles.get(&target_path) {
                let repo_name = get_repo_name_for_dotfile(dotfile, cfg);
                let dotfile_dir = get_dotfile_dir_name(dotfile, cfg);
                let status_data = serde_json::json!({
                    "path": target_path.display().to_string(),
                    "status": get_dotfile_status(dotfile, db),
                    "source": dotfile.source_path.display().to_string(),
                    "repo": repo_name.as_str(),
                    "dotfile_dir": dotfile_dir,
                    "tracked": true
                });
                info_with_data("dot.status.file", "File status", status_data);
            } else {
                let status_data = serde_json::json!({
                    "path": target_path.display().to_string(),
                    "tracked": false
                });
                info_with_data("dot.status.file", "File not tracked", status_data);
            }
        }
        OutputFormat::Text => {
            if let Some(dotfile) = all_dotfiles.get(&target_path) {
                let repo_name = get_repo_name_for_dotfile(dotfile, cfg);
                let dotfile_dir = get_dotfile_dir_name(dotfile, cfg);
                println!(
                    "{} -> {}",
                    target_path.display(),
                    get_dotfile_status(dotfile, db)
                );
                println!("  Source: {}", dotfile.source_path.display());
                println!("  Repo: {repo_name} ({dotfile_dir})");
            } else {
                println!("{} -> not tracked", target_path.display());
            }
        }
    }

    Ok(())
}

fn show_status_summary(
    all_dotfiles: &std::collections::HashMap<PathBuf, super::Dotfile>,
    cfg: &config::Config,
    db: &super::db::Database,
    show_all: bool,
) -> Result<()> {
    use crate::ui::{OutputFormat, get_output_format, info_with_data};

    let home = dirs::home_dir().context("Failed to get home directory")?;

    // Categorize files by status
    let (files_by_status, _) = categorize_files_and_collect_stats(all_dotfiles, cfg, db);

    let total_files = all_dotfiles.len();
    let clean_count = files_by_status
        .get(&DotFileStatus::Clean)
        .map_or(0, |v| v.len());
    let modified_count = files_by_status
        .get(&DotFileStatus::Modified)
        .map_or(0, |v| v.len());
    let outdated_count = files_by_status
        .get(&DotFileStatus::Outdated)
        .map_or(0, |v| v.len());

    match get_output_format() {
        OutputFormat::Json => {
            let modified_files: Vec<_> = files_by_status
                .get(&DotFileStatus::Modified)
                .unwrap_or(&vec![])
                .iter()
                .map(|(target_path, _dotfile, repo_name, dotfile_dir)| {
                    let relative_path = target_path.strip_prefix(&home).unwrap_or(target_path);
                    serde_json::json!({
                        "path": format!("~/{}", relative_path.display()),
                        "status": "modified",
                        "repo": repo_name.as_str(),
                        "dotfile_dir": dotfile_dir
                    })
                })
                .collect();

            let outdated_files: Vec<_> = files_by_status
                .get(&DotFileStatus::Outdated)
                .unwrap_or(&vec![])
                .iter()
                .map(|(target_path, _dotfile, repo_name, dotfile_dir)| {
                    let relative_path = target_path.strip_prefix(&home).unwrap_or(target_path);
                    serde_json::json!({
                        "path": format!("~/{}", relative_path.display()),
                        "status": "outdated",
                        "repo": repo_name.as_str(),
                        "dotfile_dir": dotfile_dir
                    })
                })
                .collect();

            let clean_files: Vec<_> = if show_all {
                files_by_status
                    .get(&DotFileStatus::Clean)
                    .unwrap_or(&vec![])
                    .iter()
                    .map(|(target_path, _dotfile, repo_name, dotfile_dir)| {
                        let relative_path = target_path.strip_prefix(&home).unwrap_or(target_path);
                        serde_json::json!({
                            "path": format!("~/{}", relative_path.display()),
                            "status": "clean",
                            "repo": repo_name.as_str(),
                            "dotfile_dir": dotfile_dir
                        })
                    })
                    .collect()
            } else {
                vec![]
            };

            let status_data = serde_json::json!({
                "total_files": total_files,
                "clean_count": clean_count,
                "modified_count": modified_count,
                "outdated_count": outdated_count,
                "modified_files": modified_files,
                "outdated_files": outdated_files,
                "clean_files": clean_files,
                "show_all": show_all
            });

            info_with_data("dot.status.summary", "Dotfile status summary", status_data);
        }
        OutputFormat::Text => {
            println!("Total tracked: {total_files} files");
            println!("{} Clean: {} files", "✓".green(), clean_count);

            if modified_count > 0 {
                println!(
                    "{} Modified: {} files",
                    format!("{}", char::from(Fa::ExclamationCircle)).yellow(),
                    modified_count
                );
            }

            if outdated_count > 0 {
                println!("{} Outdated: {} files", "↓".blue(), outdated_count);
            }

            // Show files with issues
            if modified_count > 0 || outdated_count > 0 {
                println!();

                if let Some(modified_files) = files_by_status.get(&DotFileStatus::Modified) {
                    println!("{}", "Modified files:".yellow().bold());
                    for (target_path, _dotfile, repo_name, dotfile_dir) in modified_files {
                        let relative_path = target_path.strip_prefix(&home).unwrap_or(target_path);
                        let tilde_path = format!("~/{}", relative_path.display());
                        println!(
                            "  {} -> {} ({}: {})",
                            tilde_path,
                            "modified".yellow(),
                            repo_name,
                            dotfile_dir
                        );
                    }
                    println!();
                }

                if let Some(outdated_files) = files_by_status.get(&DotFileStatus::Outdated) {
                    println!("{}", "Outdated files:".blue().bold());
                    for (target_path, _dotfile, repo_name, dotfile_dir) in outdated_files {
                        let relative_path = target_path.strip_prefix(&home).unwrap_or(target_path);
                        let tilde_path = format!("~/{}", relative_path.display());
                        println!(
                            "  {} -> {} ({}: {})",
                            tilde_path,
                            "outdated".blue(),
                            repo_name,
                            dotfile_dir
                        );
                    }
                    println!();
                }
            }

            // Show all files if requested
            if show_all && clean_count > 0 {
                println!("{}", "Clean files:".green().bold());
                for (target_path, _dotfile, repo_name, dotfile_dir) in files_by_status
                    .get(&DotFileStatus::Clean)
                    .unwrap_or(&vec![])
                {
                    let relative_path = target_path.strip_prefix(&home).unwrap_or(target_path);
                    let tilde_path = format!("~/{}", relative_path.display());
                    println!(
                        "  {} -> {} ({}: {})",
                        tilde_path,
                        "clean".green(),
                        repo_name,
                        dotfile_dir
                    );
                }
                println!();
            }

            // Show action suggestions (only in text mode, JSON mode handles it internally)
            show_action_suggestions(modified_count, outdated_count, clean_count);
        }
    }

    Ok(())
}

#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq, serde::Serialize)]
enum DotFileStatus {
    Modified,
    Outdated,
    Clean,
}

impl std::fmt::Display for DotFileStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DotFileStatus::Modified => write!(f, "{}", "modified".yellow()),
            DotFileStatus::Outdated => write!(f, "{}", "outdated".blue()),
            DotFileStatus::Clean => write!(f, "{}", "clean".green()),
        }
    }
}

/// Categorize files by status and collect repository statistics
fn categorize_files_and_collect_stats<'a>(
    all_dotfiles: &'a std::collections::HashMap<PathBuf, super::Dotfile>,
    cfg: &'a config::Config,
    db: &'a super::db::Database,
) -> (
    std::collections::HashMap<
        DotFileStatus,
        Vec<(PathBuf, &'a super::Dotfile, super::RepoName, String)>,
    >,
    std::collections::HashMap<super::RepoName, std::collections::HashMap<String, usize>>,
) {
    let mut files_by_status = std::collections::HashMap::new();
    let mut repo_stats = std::collections::HashMap::new();

    for (target_path, dotfile) in all_dotfiles {
        let status = get_dotfile_status(dotfile, db);
        let repo_name = get_repo_name_for_dotfile(dotfile, cfg);
        let dotfile_dir = get_dotfile_dir_name(dotfile, cfg);

        // Store file info for later display
        files_by_status
            .entry(status)
            .or_insert_with(Vec::new)
            .push((
                target_path.clone(),
                dotfile,
                repo_name.clone(),
                dotfile_dir.clone(),
            ));

        // Update repo statistics
        let repo_entry = repo_stats
            .entry(repo_name.clone())
            .or_insert_with(std::collections::HashMap::new);
        *repo_entry.entry(dotfile_dir.clone()).or_insert(0) += 1;
    }

    (files_by_status, repo_stats)
}

/// Show action suggestions based on file status counts
fn show_action_suggestions(modified_count: usize, outdated_count: usize, clean_count: usize) {
    use crate::ui::{OutputFormat, get_output_format, info_with_data};

    match get_output_format() {
        OutputFormat::Json => {
            let bin = env!("CARGO_BIN_NAME");
            let mut suggestions = Vec::new();

            if modified_count > 0 || outdated_count > 0 {
                if modified_count > 0 {
                    suggestions.push(format!(
                        "Use '{bin} dot apply' to apply changes from repositories"
                    ));
                    suggestions.push(format!(
                        "Use '{bin} dot fetch' to save your modifications to repositories"
                    ));
                }
                if outdated_count > 0 {
                    suggestions.push(format!(
                        "Use '{bin} dot reset <path>' to restore files to their original state"
                    ));
                }
                suggestions.push(format!(
                    "Use '{bin} dot status --all' to see all tracked files including clean ones"
                ));
            } else if clean_count > 0 {
                info_with_data(
                    "dot.status.message",
                    "All dotfiles are clean and up to date",
                    serde_json::json!({
                        "status": "clean",
                        "message": "All dotfiles are clean and up to date!"
                    }),
                );
                return;
            } else {
                suggestions.push(format!(
                    "Use '{bin} dot repo add <url>' to add a repository"
                ));
            }

            let suggestion_data = serde_json::json!({
                "has_issues": modified_count > 0 || outdated_count > 0,
                "suggestions": suggestions
            });

            info_with_data(
                "dot.status.suggestions",
                "Action suggestions",
                suggestion_data,
            );
        }
        OutputFormat::Text => {
            let bin = env!("CARGO_BIN_NAME");
            if modified_count > 0 || outdated_count > 0 {
                println!("{}", "Suggested actions:".bold());
                if modified_count > 0 {
                    println!("  Use '{bin} dot apply' to apply changes from repositories");
                    println!("  Use '{bin} dot fetch' to save your modifications to repositories");
                }
                if outdated_count > 0 {
                    println!(
                        "  Use '{bin} dot reset <path>' to restore files to their original state"
                    );
                }
                println!(
                    "  Use '{bin} dot status --all' to see all tracked files including clean ones"
                );
            } else if clean_count > 0 {
                println!("✓ All dotfiles are clean and up to date!");
            } else {
                println!("No dotfiles found. Use '{bin} dot repo add <url>' to add a repository.");
            }
        }
    }
}

fn get_dotfile_status(dotfile: &super::Dotfile, db: &super::db::Database) -> DotFileStatus {
    if !dotfile.is_target_unmodified(db).unwrap_or(false) {
        DotFileStatus::Modified
    } else if dotfile.is_outdated(db) {
        DotFileStatus::Outdated
    } else {
        DotFileStatus::Clean
    }
}

// Get the dotfile directory name for a dotfile
fn get_dotfile_dir_name(dotfile: &super::Dotfile, cfg: &config::Config) -> String {
    // Find which repository this dotfile comes from
    for repo_config in &cfg.repos {
        let repo_path = cfg.repos_path().join(&repo_config.name);
        if dotfile.source_path.starts_with(&repo_path) {
            // Extract the dotfile directory name from the source path
            // Source path format: {repo_path}/{dotfile_dir}/{relative_path}
            if let Ok(relative) = dotfile.source_path.strip_prefix(&repo_path)
                && let Some(dotfile_dir) = relative.components().next()
            {
                return dotfile_dir.as_os_str().to_string_lossy().to_string();
            }
            return "dots".to_string(); // default
        }
    }
    "unknown".to_string()
}

// Get the repository name for a dotfile (improved version)
fn get_repo_name_for_dotfile(dotfile: &super::Dotfile, cfg: &config::Config) -> super::RepoName {
    // Find which repository this dotfile comes from
    for repo_config in &cfg.repos {
        if dotfile
            .source_path
            .starts_with(cfg.repos_path().join(&repo_config.name))
        {
            return super::RepoName::new(repo_config.name.clone());
        }
    }
    super::RepoName::new("unknown".to_string())
}

/// Show differences between modified dotfiles and their source
pub fn diff_all(
    cfg: &config::Config,
    _debug: bool,
    path: Option<&str>,
    db: &super::db::Database,
) -> Result<()> {
    let all_dotfiles = super::get_all_dotfiles(cfg, db)?;

    if let Some(path_str) = path {
        // Show diff for specific path
        show_single_file_diff(path_str, &all_dotfiles, cfg, db)?;
    } else {
        // Show diffs for all modified files
        show_all_diffs(&all_dotfiles, cfg, db)?;
    }

    Ok(())
}

fn show_single_file_diff(
    path_str: &str,
    all_dotfiles: &std::collections::HashMap<PathBuf, super::Dotfile>,
    cfg: &config::Config,
    db: &super::db::Database,
) -> Result<()> {
    let target_path = super::resolve_dotfile_path(path_str)?;

    if let Some(dotfile) = all_dotfiles.get(&target_path) {
        let status = get_dotfile_status(dotfile, db);

        match status {
            DotFileStatus::Clean => {
                let home = dirs::home_dir().context("Failed to get home directory")?;
                let relative_path = target_path.strip_prefix(&home).unwrap_or(&target_path);
                let tilde_path = format!("~/{}", relative_path.display());
                println!("{} {} is unmodified", "✓".green(), tilde_path.green());
            }
            DotFileStatus::Modified | DotFileStatus::Outdated => {
                let repo_name = get_repo_name_for_dotfile(dotfile, cfg);
                let dotfile_dir = get_dotfile_dir_name(dotfile, cfg);
                show_dotfile_diff(dotfile, &repo_name, &dotfile_dir)?;
            }
        }
    } else {
        println!("{} -> not tracked", target_path.display());
    }

    Ok(())
}

fn show_all_diffs(
    all_dotfiles: &std::collections::HashMap<PathBuf, super::Dotfile>,
    cfg: &config::Config,
    db: &super::db::Database,
) -> Result<()> {
    let (files_by_status, _) = categorize_files_and_collect_stats(all_dotfiles, cfg, db);

    let modified_count = files_by_status
        .get(&DotFileStatus::Modified)
        .map_or(0, |v| v.len());
    let outdated_count = files_by_status
        .get(&DotFileStatus::Outdated)
        .map_or(0, |v| v.len());

    if modified_count == 0 && outdated_count == 0 {
        println!("{}", "✓ All dotfiles are clean and up to date!".green());
        return Ok(());
    }

    // Show modified files
    if let Some(modified_files) = files_by_status.get(&DotFileStatus::Modified) {
        if !modified_files.is_empty() {
            println!("{}", "Modified files:".yellow().bold());
            for (target_path, dotfile, repo_name, dotfile_dir) in modified_files {
                let home = dirs::home_dir().context("Failed to get home directory")?;
                let relative_path = target_path.strip_prefix(&home).unwrap_or(target_path);
                let tilde_path = format!("~/{}", relative_path.display());
                println!(
                    "  {} ({})",
                    tilde_path,
                    format!("{repo_name}: {dotfile_dir}").dimmed()
                );
                show_dotfile_diff(dotfile, repo_name, dotfile_dir)?;
                println!();
            }
        }
    }

    // Show outdated files
    if let Some(outdated_files) = files_by_status.get(&DotFileStatus::Outdated) {
        if !outdated_files.is_empty() {
            println!("{}", "Outdated files:".blue().bold());
            for (target_path, dotfile, repo_name, dotfile_dir) in outdated_files {
                let home = dirs::home_dir().context("Failed to get home directory")?;
                let relative_path = target_path.strip_prefix(&home).unwrap_or(target_path);
                let tilde_path = format!("~/{}", relative_path.display());
                println!(
                    "  {} ({})",
                    tilde_path,
                    format!("{repo_name}: {dotfile_dir}").dimmed()
                );
                show_dotfile_diff(dotfile, repo_name, dotfile_dir)?;
                println!();
            }
        }
    }

    Ok(())
}

fn show_dotfile_diff(
    dotfile: &super::Dotfile,
    _repo_name: &super::RepoName,
    _dotfile_dir: &str,
) -> Result<()> {
    // Check if delta is available
    if Command::new("delta").arg("--help").output().is_ok() {
        show_delta_diff(dotfile)?;
    } else {
        return Err(anyhow::anyhow!(
            "delta command not found. Please install delta to use the diff command.\n\
             Install with: cargo install git-delta\n\
             Or visit: https://github.com/dandavison/delta"
        ));
    }

    Ok(())
}

fn show_delta_diff(dotfile: &super::Dotfile) -> Result<()> {
    if !dotfile.source_path.exists() && !dotfile.target_path.exists() {
        println!("  {}", "Both source and target files are missing".red());
        return Ok(());
    }

    if !dotfile.source_path.exists() {
        println!(
            "  {} was removed from repository",
            dotfile.target_path.display().to_string().red()
        );
        return Ok(());
    }

    if !dotfile.target_path.exists() {
        println!(
            "  {} has not been applied yet",
            dotfile.target_path.display().to_string().blue()
        );
        return Ok(());
    }

    // Check if files are binary
    if is_binary_file(&dotfile.source_path)? || is_binary_file(&dotfile.target_path)? {
        println!("  {}", "Binary files differ".yellow());
        return Ok(());
    }

    // Use delta for direct file comparison (not git mode)
    let mut child = Command::new("delta")
        .arg(&dotfile.source_path)
        .arg(&dotfile.target_path)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;

    // Read and print output in real-time
    if let Some(stdout) = child.stdout.take() {
        use std::io::{BufRead, BufReader};
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            match line {
                Ok(line) => println!("  {line}"),
                Err(e) => eprintln!("  Error reading delta output: {e}"),
            }
        }
    }

    // Wait for the process to complete
    let output = child.wait_with_output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stderr.trim().is_empty() {
            eprintln!("  Delta error: {stderr}");
        }
    }

    Ok(())
}

fn is_binary_file(path: &PathBuf) -> Result<bool> {
    use std::fs::File;
    use std::io::Read;

    let mut file = File::open(path)?;
    let mut buffer = [0; 1024];
    let bytes_read = file.read(&mut buffer)?;

    // Check for null bytes in the first buffer
    Ok(buffer[..bytes_read].contains(&0))
}
