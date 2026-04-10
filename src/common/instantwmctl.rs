use anyhow::{Context, Result, bail};
use serde::de::DeserializeOwned;
use std::ffi::OsStr;
use std::process::{Command, Output};

fn format_args(args: &[String]) -> String {
    if args.is_empty() {
        "instantwmctl".to_string()
    } else {
        format!("instantwmctl {}", args.join(" "))
    }
}

pub fn output<I, S>(args: I) -> Result<Output>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args: Vec<String> = args
        .into_iter()
        .map(|arg| arg.as_ref().to_string_lossy().into_owned())
        .collect();

    let output = Command::new("instantwmctl")
        .args(&args)
        .output()
        .with_context(|| format!("Failed to execute {}", format_args(&args)))?;

    if output.status.success() {
        Ok(output)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.is_empty() {
            bail!(
                "{} failed with status {}",
                format_args(&args),
                output.status
            );
        } else {
            bail!("{} failed: {}", format_args(&args), stderr);
        }
    }
}

pub fn run<I, S>(args: I) -> Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    output(args).map(|_| ())
}

pub fn json<T, I, S>(args: I) -> Result<T>
where
    T: DeserializeOwned,
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args: Vec<String> = args
        .into_iter()
        .map(|arg| arg.as_ref().to_string_lossy().into_owned())
        .collect();

    let output = output(std::iter::once("--json").chain(args.iter().map(String::as_str)))?;
    serde_json::from_slice(&output.stdout)
        .with_context(|| format!("Failed to parse {} output", format_args(&args)))
}
