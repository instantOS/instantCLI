use anyhow::Result;
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
    env: &TestEnvironment,
    args: &[&str],
) -> Result<CommandOutput> {
    let mut cmd = Command::new("cargo");
    cmd.args(&["run", "--bin", "instant", "--"])
        .args(args)
        .env("INSTANT_TEST_HOME_DIR", env.fake_home().to_str().unwrap());
    
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
    let repo_path = env.fake_home().join(".local/share/instantos/dots").join(name);
    
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
    
    // Add files to the first subdirectory by default
    if let Some(first_subdir) = subdirs.first() {
        let subdir_path = work_dir.path().join(first_subdir);
        for (file_path, content) in files {
            let full_path = subdir_path.join(file_path);
            if let Some(parent) = full_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&full_path, content)?;
        }
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