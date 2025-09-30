use crate::dot::config;
use crate::dot::git::status::get_dotfile_status;
use crate::dot::git::{get_dotfile_dir_name, get_repo_name_for_dotfile, status::DotFileStatus};
use anyhow::{Context, Result};
use colored::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn diff_all(
    cfg: &config::Config,
    _debug: bool,
    path: Option<&str>,
    db: &crate::dot::db::Database,
) -> Result<()> {
    let all_dotfiles = crate::dot::get_all_dotfiles(cfg, db)?;

    if let Some(path_str) = path {
        show_path_diff(path_str, &all_dotfiles, cfg, db)?;
    } else {
        show_all_diffs(&all_dotfiles, cfg, db)?;
    }

    Ok(())
}

pub fn show_path_diff(
    path_str: &str,
    all_dotfiles: &HashMap<PathBuf, crate::dot::Dotfile>,
    cfg: &config::Config,
    db: &crate::dot::db::Database,
) -> Result<()> {
    let target_path = crate::dot::resolve_dotfile_path(path_str)?;

    if target_path.is_dir() {
        diff_directory(target_path.as_path(), all_dotfiles, cfg, db)
    } else {
        diff_file(&target_path, all_dotfiles, cfg, db)
    }
}

fn diff_directory(
    target_dir: &Path,
    all_dotfiles: &HashMap<PathBuf, crate::dot::Dotfile>,
    cfg: &config::Config,
    db: &crate::dot::db::Database,
) -> Result<()> {
    let mut matching: Vec<_> = all_dotfiles
        .iter()
        .filter(|(path, _)| path.starts_with(target_dir))
        .collect();

    if matching.is_empty() {
        println!("{} -> not tracked", target_dir.display());
        return Ok(());
    }

    matching.sort_by(|(a, _), (b, _)| a.cmp(b));

    let home = dirs::home_dir().context("Failed to get home directory")?;
    let relative_dir = target_dir.strip_prefix(&home).unwrap_or(target_dir);
    let tilde_dir = format!("~/{}", relative_dir.display());
    println!("{}", tilde_dir.as_str().yellow().bold());

    let mut showed_diff = false;

    for (path, dotfile) in matching {
        let status = get_dotfile_status(dotfile, db);

        if matches!(status, DotFileStatus::Clean) {
            continue;
        }

        showed_diff = true;

        let relative_path = path.strip_prefix(&home).unwrap_or(path);
        let tilde_path = format!("~/{}", relative_path.display());
        let repo_name = get_repo_name_for_dotfile(dotfile, cfg);
        let dotfile_dir = get_dotfile_dir_name(dotfile, cfg);

        println!(
            "  {} ({})",
            tilde_path,
            format!("{}: {}", repo_name, dotfile_dir).dimmed()
        );
        show_dotfile_diff(dotfile, &repo_name, &dotfile_dir)?;
        println!();
    }

    if !showed_diff {
        println!(
            "  {} No modified or outdated dotfiles under {}",
            "✓".green(),
            tilde_dir
        );
    }

    Ok(())
}

fn diff_file(
    target_path: &PathBuf,
    all_dotfiles: &HashMap<PathBuf, crate::dot::Dotfile>,
    cfg: &config::Config,
    db: &crate::dot::db::Database,
) -> Result<()> {
    if let Some(dotfile) = all_dotfiles.get(target_path) {
        let status = get_dotfile_status(dotfile, db);

        match status {
            DotFileStatus::Clean => {
                let home = dirs::home_dir().context("Failed to get home directory")?;
                let relative_path = target_path.strip_prefix(&home).unwrap_or(target_path);
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

pub fn show_all_diffs(
    all_dotfiles: &HashMap<PathBuf, crate::dot::Dotfile>,
    cfg: &config::Config,
    db: &crate::dot::db::Database,
) -> Result<()> {
    let (files_by_status, _) =
        crate::dot::git::status::categorize_files_and_get_summary(all_dotfiles, cfg, db);

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
    if let Some(modified_files) = files_by_status.get(&DotFileStatus::Modified)
        && !modified_files.is_empty()
    {
        println!("{}", " Modified files:".yellow().bold());
        for file_info in modified_files {
            let home = dirs::home_dir().context("Failed to get home directory")?;
            let relative_path = file_info
                .target_path
                .strip_prefix(&home)
                .unwrap_or(&file_info.target_path);
            let tilde_path = format!("~/{}", relative_path.display());
            println!(
                "  {} ({})",
                tilde_path,
                format!("{}: {}", file_info.repo_name, file_info.dotfile_dir).dimmed()
            );
            show_dotfile_diff(
                &file_info.dotfile,
                &file_info.repo_name,
                &file_info.dotfile_dir,
            )?;
            println!();
        }
    }

    // Show outdated files
    if let Some(outdated_files) = files_by_status.get(&DotFileStatus::Outdated)
        && !outdated_files.is_empty()
    {
        println!("{}", "Outdated files:".blue().bold());
        for file_info in outdated_files {
            let home = dirs::home_dir().context("Failed to get home directory")?;
            let relative_path = file_info
                .target_path
                .strip_prefix(&home)
                .unwrap_or(&file_info.target_path);
            let tilde_path = format!("~/{}", relative_path.display());
            println!(
                "  {} ({})",
                tilde_path,
                format!("{}: {}", file_info.repo_name, file_info.dotfile_dir).dimmed()
            );
            show_dotfile_diff(
                &file_info.dotfile,
                &file_info.repo_name,
                &file_info.dotfile_dir,
            )?;
            println!();
        }
    }

    Ok(())
}

fn show_dotfile_diff(
    dotfile: &crate::dot::Dotfile,
    _repo_name: &crate::dot::RepoName,
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

fn show_delta_diff(dotfile: &crate::dot::Dotfile) -> Result<()> {
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
