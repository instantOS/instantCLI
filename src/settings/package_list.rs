use std::io::{self, BufRead, BufReader, Write};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, anyhow, bail};
use duct::cmd;

use crate::common::package::{PackageManager, detect_aur_helper};
use crate::common::shell::{current_exe_command, resolve_current_binary};
use crate::menu_utils::StreamingCommand;

pub fn available_command(manager: PackageManager) -> StreamingCommand {
    StreamingCommand::new(resolve_current_binary())
        .arg("settings")
        .arg("internal-generate-package-list")
        .arg("--manager")
        .arg(manager.as_str())
        .arg("--mode")
        .arg("available")
}

pub fn installed_command(manager: PackageManager) -> StreamingCommand {
    StreamingCommand::new(resolve_current_binary())
        .arg("settings")
        .arg("internal-generate-package-list")
        .arg("--manager")
        .arg(manager.as_str())
        .arg("--mode")
        .arg("installed")
}

pub fn arch_available_command() -> StreamingCommand {
    StreamingCommand::new(resolve_current_binary())
        .arg("settings")
        .arg("internal-generate-package-list")
        .arg("--manager")
        .arg("arch")
        .arg("--mode")
        .arg("available")
}

pub fn snap_search_command(keyword: Option<&str>) -> StreamingCommand {
    let mut command = StreamingCommand::new(resolve_current_binary())
        .arg("settings")
        .arg("internal-generate-snap-list");
    if let Some(keyword) = keyword {
        command = command.arg("--keyword").arg(keyword);
    }
    command
}

pub fn snap_search_reload_command() -> String {
    format!(
        "{} settings internal-generate-snap-list --keyword '{{q}}'",
        current_exe_command()
    )
}

pub fn generate_and_print_package_list(manager: &str, mode: &str) -> Result<()> {
    let parse_manager = || {
        manager
            .parse::<PackageManager>()
            .map_err(anyhow::Error::msg)
    };
    match (manager, mode) {
        ("arch", "available") => stream_arch_available(),
        (_, "available") => stream_available(parse_manager()?),
        (_, "installed") => stream_installed(parse_manager()?),
        _ => bail!("Unsupported package list mode: {mode}"),
    }
}

pub fn generate_and_print_snap_list(keyword: Option<&str>) -> Result<()> {
    let mut command = Command::new("snap");
    command.arg("find");
    if let Some(keyword) = keyword
        && !keyword.trim().is_empty()
    {
        command.arg(keyword);
    }

    let output = command.output().context("Failed to run snap find")?;
    if !output.status.success() {
        return Ok(());
    }

    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if line.trim().is_empty()
            || line.starts_with("Name")
            || line.starts_with("Provide a search term")
        {
            continue;
        }

        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 4 {
            continue;
        }

        let name = fields[0];
        let version = fields[1];
        let publisher = fields[2];
        let summary = fields.iter().skip(4).copied().collect::<Vec<_>>().join(" ");
        println!("{name}\t{version}\t{publisher}\t{summary}");
    }

    Ok(())
}

fn stream_available(manager: PackageManager) -> Result<()> {
    match manager {
        PackageManager::Pacman => stream_lines(Command::new("pacman").arg("-Slq"), |line| {
            Some(line.to_string())
        }),
        PackageManager::Apt => {
            stream_lines(Command::new("apt-cache").args(["search", "."]), |line| {
                line.split_once(' ').map(|(name, _)| name.to_string())
            })
        }
        PackageManager::Dnf => stream_lines(
            Command::new("dnf").args(["list", "available"]),
            parse_dnf_package_name,
        ),
        PackageManager::Zypper => stream_lines(
            Command::new("zypper").args(["se", "--available-only"]),
            parse_zypper_package_name,
        ),
        PackageManager::Pkg => stream_lines(Command::new("pkg").args(["list-all"]), |line| {
            parse_pkg_name(line)
        }),
        PackageManager::Flatpak => stream_lines(
            Command::new("flatpak").args(["remote-ls", "--app", "--columns=application"]),
            |line| Some(line.to_string()),
        ),
        PackageManager::Aur => stream_aur_available(),
        PackageManager::Cargo => stream_lines(
            Command::new("cargo").args(["search", "--limit", "1000", ""]),
            |line| line.split(' ').next().map(|name| name.to_string()),
        ),
        PackageManager::Snap => generate_and_print_snap_list(None),
    }
}

fn stream_installed(manager: PackageManager) -> Result<()> {
    match manager {
        PackageManager::Pacman => stream_lines(Command::new("pacman").arg("-Qq"), |line| {
            Some(line.to_string())
        }),
        PackageManager::Apt => stream_lines(
            Command::new("dpkg-query").args(["-W", "-f=${Package}\\n"]),
            |line| Some(line.to_string()),
        ),
        PackageManager::Dnf => stream_lines(
            Command::new("dnf").args(["list", "installed"]),
            parse_dnf_package_name,
        ),
        PackageManager::Zypper => stream_lines(
            Command::new("zypper").args(["se", "--installed-only"]),
            parse_zypper_package_name,
        ),
        PackageManager::Pkg => {
            stream_lines(Command::new("pkg").arg("list-installed"), parse_pkg_name)
        }
        PackageManager::Flatpak => stream_lines(
            Command::new("flatpak").args(["list", "--app", "--columns=application"]),
            |line| Some(line.to_string()),
        ),
        PackageManager::Aur => stream_lines(Command::new("pacman").arg("-Qm"), |line| {
            Some(line.to_string())
        }),
        PackageManager::Cargo => stream_lines(
            Command::new("cargo").args(["install", "--list"]),
            parse_cargo_installed_name,
        ),
        PackageManager::Snap => stream_installed_snaps(),
    }
}

fn stream_arch_available() -> Result<()> {
    stream_lines(Command::new("pacman").arg("-Slq"), |line| {
        Some(format!("{}\t{}", PackageManager::Pacman.as_str(), line))
    })?;

    if detect_aur_helper().is_some() {
        stream_aur_available_with_prefix()?;
    }

    Ok(())
}

fn stream_aur_available() -> Result<()> {
    stream_aur_packages(None)
}

fn stream_aur_available_with_prefix() -> Result<()> {
    stream_aur_packages(Some(PackageManager::Aur.as_str()))
}

fn stream_aur_packages(prefix: Option<&str>) -> Result<()> {
    let output = cmd!("curl", "-sL", "https://aur.archlinux.org/packages.gz")
        .pipe(cmd!("gunzip"))
        .read()
        .context("Failed to fetch AUR package list")?;

    for line in output.lines().filter(|line| !line.trim().is_empty()) {
        if let Some(prefix) = prefix {
            println!("{prefix}\t{line}");
        } else {
            println!("{line}");
        }
    }

    Ok(())
}

fn stream_installed_snaps() -> Result<()> {
    let output = Command::new("snap")
        .arg("list")
        .output()
        .context("Failed to run snap list")?;

    if !output.status.success() {
        return Ok(());
    }

    for line in String::from_utf8_lossy(&output.stdout).lines().skip(1) {
        if line.trim().is_empty() {
            continue;
        }
        let name = line.split_whitespace().next().unwrap_or_default();
        if !name.is_empty() {
            println!("snap\t{name}\t{line}");
        }
    }

    Ok(())
}

fn stream_lines<F>(command: &mut Command, mut map_line: F) -> Result<()>
where
    F: FnMut(&str) -> Option<String>,
{
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("Failed to capture package list stdout"))?;
    let reader = BufReader::new(stdout);
    let mut out = io::BufWriter::new(io::stdout());

    for line in reader.lines() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(mapped) = map_line(line)
            && !mapped.trim().is_empty()
        {
            writeln!(out, "{mapped}")?;
        }
    }

    child.wait()?;
    out.flush()?;
    Ok(())
}

fn parse_dnf_package_name(line: &str) -> Option<String> {
    if line.starts_with("Available Packages") || line.starts_with("Installed Packages") {
        return None;
    }
    let name = line.split_whitespace().next()?;
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

fn parse_zypper_package_name(line: &str) -> Option<String> {
    if !line.contains('|') || line.starts_with("S |") || line.starts_with("--") {
        return None;
    }
    let mut fields = line.split('|').map(str::trim);
    let _status = fields.next()?;
    let name = fields.next()?;
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

fn parse_pkg_name(line: &str) -> Option<String> {
    if line.starts_with("Listing") {
        return None;
    }
    line.split('/').next().map(ToString::to_string)
}

fn parse_cargo_installed_name(line: &str) -> Option<String> {
    let first = line.split_whitespace().next()?;
    if first
        .chars()
        .next()
        .map(|c| c.is_ascii_alphabetic())
        .unwrap_or(false)
    {
        Some(first.trim_end_matches(':').to_string())
    } else {
        None
    }
}
