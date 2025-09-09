use anyhow::Result;
use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;

use super::common::TestEnvironment;

pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

pub fn run_instant_command(
    _env: &TestEnvironment,
    args: &[&str],
) -> Result<CommandOutput> {
    // Build the binary first
    let build_output = Command::new("cargo")
        .args(&["build", "--bin", "instant"])
        .current_dir(env::current_dir()?) // Run from project directory
        .output()?;
    
    if !build_output.status.success() {
        return Ok(CommandOutput {
            stdout: String::new(),
            stderr: String::from_utf8_lossy(&build_output.stderr).to_string(),
            exit_code: build_output.status.code().unwrap_or(-1),
        });
    }
    
    // Get the project directory and binary path
    let project_dir = env::current_dir()?;
    let binary_path = project_dir.join("./target/debug/instant");
    
    // Run the binary directly from the project directory
    let mut cmd = Command::new(&binary_path);
    cmd.args(args)
        .current_dir(&project_dir); // Important: run from project directory
    
    let output = cmd.output()?;
    
    Ok(CommandOutput {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        exit_code: output.status.code().unwrap_or(-1),
    })
}

pub fn create_test_repo(
    env: &TestEnvironment,
    name: &str,
    files: &[(&str, &str)], // (path, content) pairs
    subdirs: &[&str],
) -> Result<std::path::PathBuf> {
    let repo_path = env.path().join(name);
    
    // Create git repository
    Command::new("git")
        .args(&["init", "--bare", repo_path.to_str().unwrap()])
        .output()?;
    
    // Create temporary working directory for adding files
    let work_dir = tempfile::tempdir()?;
    
    // Clone bare repo to working directory
    Command::new("git")
        .args(&["clone", repo_path.to_str().unwrap(), work_dir.path().to_str().unwrap()])
        .output()?;
    
    // Add files to specified subdirectories
    for subdir in subdirs {
        let subdir_path = work_dir.path().join(subdir);
        fs::create_dir_all(&subdir_path)?;
        
        // Create instantdots.toml if it doesn't exist
        let meta_path = work_dir.path().join("instantdots.toml");
        if !meta_path.exists() {
            let meta_content = if subdirs.len() > 1 {
                format!(r#"name = "{}"
dots_dirs = {:?}
"#, name, subdirs)
            } else {
                format!(r#"name = "{}"
"#, name)
            };
            fs::write(&meta_path, meta_content)?;
        }
    }
    
    // Add files to their respective subdirectories
    for (file_path, content) in files {
        // Determine which subdirectory this file belongs to
        // For this test, we'll put files with "app1" in the path into app1, etc.
        let target_subdir = if file_path.contains("app1") {
            "app1"
        } else if file_path.contains("app2") {
            "app2"
        } else {
            // Default to first subdirectory
            subdirs.first().unwrap_or(&"dots")
        };
        
        let subdir_path = work_dir.path().join(target_subdir);
        if !subdir_path.exists() {
            fs::create_dir_all(&subdir_path)?;
        }
        
        let full_path = subdir_path.join(file_path);
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&full_path, content)?;
    }
    
    // Commit and push back to bare repo
    Command::new("git")
        .args(&["add", "."])
        .current_dir(work_dir.path())
        .output()?;
    
    Command::new("git")
        .args(&["commit", "-m", "Initial commit"])
        .current_dir(work_dir.path())
        .output()?;
    
    Command::new("git")
        .args(&["push", "origin", "main"])
        .current_dir(work_dir.path())
        .output()?;
    
    Ok(repo_path)
}

pub fn file_exists(path: &Path) -> bool {
    path.exists()
}

pub fn read_file(path: &Path) -> Result<String> {
    Ok(fs::read_to_string(path)?)
}

pub fn write_file(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(fs::write(path, content)?)
}