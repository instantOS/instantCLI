use std::process::{Command, Output, Stdio};

use anyhow::{Context, Result, bail};

fn run_output<const N: usize>(args: [&str; N]) -> Result<Output> {
    run_output_with_input(args, None)
}

fn run_output_with_input<const N: usize>(args: [&str; N], input: Option<&[u8]>) -> Result<Output> {
    let display_cmd = args.join(" ");
    let mut command = Command::new("pass");
    command.args(args);
    if input.is_some() {
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
    }

    let output = if let Some(input) = input {
        let mut child = command
            .spawn()
            .with_context(|| format!("Failed to run `pass {display_cmd}`"))?;

        if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write;
            stdin
                .write_all(input)
                .with_context(|| format!("Failed to write to `pass {display_cmd}`"))?;
            if !input.ends_with(b"\n") {
                stdin
                    .write_all(b"\n")
                    .with_context(|| format!("Failed to write to `pass {display_cmd}`"))?;
            }
        }

        child
            .wait_with_output()
            .with_context(|| format!("Failed to run `pass {display_cmd}`"))?
    } else {
        command
            .output()
            .with_context(|| format!("Failed to run `pass {display_cmd}`"))?
    };

    if !output.status.success() {
        // pass-otp can include the supplied URI and seed in its error output.
        if input.is_some() {
            bail!("`pass {display_cmd}` failed with status {}", output.status);
        }
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.is_empty() {
            bail!("`pass {display_cmd}` failed");
        } else {
            bail!("`pass {display_cmd}` failed: {stderr}");
        }
    }

    Ok(output)
}

pub(super) fn run_stdout<const N: usize>(args: [&str; N]) -> Result<Vec<u8>> {
    Ok(run_output(args)?.stdout)
}

pub(super) fn run_status<const N: usize>(args: [&str; N]) -> Result<()> {
    run_output(args)?;
    Ok(())
}

pub(super) fn run_status_with_input<const N: usize>(args: [&str; N], input: &str) -> Result<()> {
    run_output_with_input(args, Some(input.as_bytes()))?;
    Ok(())
}
