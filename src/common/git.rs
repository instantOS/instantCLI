use anyhow::{Context, Result};
use duct::cmd;
#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;

pub fn git_clone(
    url: &str,
    target: &std::path::Path,
    branch: Option<&str>,
    depth: i32,
    debug: bool,
) -> Result<()> {
    if debug {
        eprintln!(
            "Running git clone with depth: {}, branch: {:?}, url: {}, target: {:?}",
            depth, branch, url, target
        );
    }

    if depth > 0 {
        if let Some(branch) = branch {
            cmd!(
                "git",
                "clone",
                "--depth",
                depth.to_string(),
                "--branch",
                branch,
                url,
                target
                    .to_str()
                    .ok_or_else(|| anyhow::anyhow!("Invalid target path"))?
            )
            .run()
            .context("Failed to execute git clone")?;
        } else {
            cmd!(
                "git",
                "clone",
                "--depth",
                depth.to_string(),
                url,
                target
                    .to_str()
                    .ok_or_else(|| anyhow::anyhow!("Invalid target path"))?
            )
            .run()
            .context("Failed to execute git clone")?;
        }
    } else if let Some(branch) = branch {
        cmd!(
            "git",
            "clone",
            "--branch",
            branch,
            url,
            target
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("Invalid target path"))?
        )
        .run()
        .context("Failed to execute git clone")?;
    } else {
        cmd!(
            "git",
            "clone",
            url,
            target
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("Invalid target path"))?
        )
        .run()
        .context("Failed to execute git clone")?;
    }

    Ok(())
}

pub fn git_command_in_dir(
    dir: &std::path::Path,
    args: &[&str],
    operation_name: &str,
) -> Result<String> {
    let mut cmd_args = vec![
        "-C",
        dir.to_str()
            .ok_or_else(|| anyhow::anyhow!("Invalid directory path"))?,
    ];
    cmd_args.extend(args);

    cmd("git", cmd_args)
        .read()
        .context(format!("Failed to execute git {operation_name}"))
}

pub fn git_command_in_dir_with_output(
    dir: &std::path::Path,
    args: &[&str],
    operation_name: &str,
) -> Result<std::process::Output> {
    // Build command arguments
    let mut cmd_args = vec![
        "-C",
        dir.to_str()
            .ok_or_else(|| anyhow::anyhow!("Invalid directory path"))?,
    ];
    cmd_args.extend(args);

    // Use reader() to capture output
    let mut reader = cmd("git", cmd_args)
        .unchecked() // Don't fail on non-zero exit codes
        .reader()
        .context(format!("Failed to execute git {operation_name}"))?;

    use std::io::Read;
    let mut stdout = Vec::new();

    // Read all output (duct's reader combines stdout/stderr by default)
    reader
        .read_to_end(&mut stdout)
        .context("Failed to read git output")?;

    Ok(std::process::Output {
        status: std::process::ExitStatus::from_raw(0), // Success status
        stdout,
        stderr: Vec::new(), // Combined with stdout in this approach
    })
}
