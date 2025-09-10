use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use std::process::Command;

pub fn create_spinner(message: String) -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner} {msg}")
            .unwrap()
            .tick_chars("⠁⠁⠉⠙⠚⠒⠂⠂⠒⠲⠴⠤⠄⠄⠤⠠⠠⠤⠦⠖⠒⠐⠐⠒⠓⠋⠉⠙⠚⠒⠂⠂⠒⠲⠴⠤⠄⠄⠤⠠⠠⠤⠦⠖⠒⠐⠐⠒⠓⠋⠉⠙⠚"),
    );
    pb.set_message(message);
    pb.enable_steady_tick(std::time::Duration::from_millis(100));
    pb
}

pub fn git_clone(
    url: &str,
    target: &std::path::Path,
    branch: Option<&str>,
    depth: i32,
    debug: bool,
) -> Result<()> {
    let mut cmd = Command::new("git");
    cmd.arg("clone");
    if depth > 0 {
        cmd.arg("--depth").arg(depth.to_string());
    }
    if let Some(branch) = branch {
        cmd.arg("--branch").arg(branch);
    }
    cmd.arg(url).arg(target);

    if debug {
        eprintln!("Running: {:?}", cmd);
    }

    let output = cmd.output().context("Failed to execute git clone")?;
    if !output.status.success() {
        anyhow::bail!(
            "Git clone failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

pub fn git_command_in_dir(
    dir: &std::path::Path,
    args: &[&str],
    operation_name: &str,
) -> Result<String> {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(dir);
    for arg in args {
        cmd.arg(arg);
    }

    let output = cmd.output().context(format!("Failed to execute git {}", operation_name))?;
    if !output.status.success() {
        anyhow::bail!(
            "Git {} failed: {}",
            operation_name,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn git_command_in_dir_with_output(
    dir: &std::path::Path,
    args: &[&str],
    operation_name: &str,
) -> Result<std::process::Output> {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(dir);
    for arg in args {
        cmd.arg(arg);
    }

    let output = cmd.output().context(format!("Failed to execute git {}", operation_name))?;
    if !output.status.success() {
        anyhow::bail!(
            "Git {} failed: {}",
            operation_name,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(output)
}
