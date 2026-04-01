use std::io::{self, BufRead, BufReader, Write};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, anyhow, bail};
use duct::cmd;
use serde::{Deserialize, Serialize};

use crate::common::package::{PackageManager, detect_aur_helper};
use crate::common::shell::{current_exe_command, resolve_current_binary};
use crate::menu_utils::{FzfPreview, StreamingCommand, StreamingMenuItem};
use crate::preview::{PreviewId, preview_command};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageSelectionPayload {
    pub manager: String,
    pub package: String,
}

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
        print_package_row(
            PackageManager::Snap,
            name,
            name,
            format!("{name}\t{version}\t{publisher}\t{summary}"),
            preview_command(PreviewId::Snap),
        )?;
    }

    Ok(())
}

fn stream_available(manager: PackageManager) -> Result<()> {
    let preview_id = preview_id_for_manager(manager);
    match manager {
        PackageManager::Pacman => stream_and_print(
            Command::new("pacman").arg("-Slq"),
            |line| Some(line.to_string()),
            |pkg| print_package_row(manager, &pkg, &pkg, pkg.clone(), preview_command(preview_id)),
        ),
        PackageManager::Apt => stream_and_print(
            Command::new("apt-cache").args(["search", "."]),
            |line| line.split_once(' ').map(|(name, _)| name.to_string()),
            |pkg| print_package_row(manager, &pkg, &pkg, pkg.clone(), preview_command(preview_id)),
        ),
        PackageManager::Dnf => stream_and_print(
            Command::new("dnf").args(["list", "available"]),
            parse_dnf_package_name,
            |pkg| print_package_row(manager, &pkg, &pkg, pkg.clone(), preview_command(preview_id)),
        ),
        PackageManager::Zypper => stream_and_print(
            Command::new("zypper").args(["se", "--available-only"]),
            parse_zypper_package_name,
            |pkg| print_package_row(manager, &pkg, &pkg, pkg.clone(), preview_command(preview_id)),
        ),
        PackageManager::Pkg => stream_and_print(
            Command::new("pkg").args(["list-all"]),
            parse_pkg_name,
            |pkg| print_package_row(manager, &pkg, &pkg, pkg.clone(), preview_command(preview_id)),
        ),
        PackageManager::Flatpak => stream_and_print(
            Command::new("flatpak").args(["remote-ls", "--app", "--columns=application"]),
            |line| Some(line.to_string()),
            |pkg| print_package_row(manager, &pkg, &pkg, pkg.clone(), preview_command(preview_id)),
        ),
        PackageManager::Aur => stream_aur_available(),
        PackageManager::Cargo => stream_and_print(
            Command::new("cargo").args(["search", "--limit", "1000", ""]),
            |line| line.split(' ').next().map(|name| name.to_string()),
            |pkg| print_package_row(manager, &pkg, &pkg, pkg.clone(), preview_command(preview_id)),
        ),
        PackageManager::Snap => generate_and_print_snap_list(None),
    }
}

fn stream_installed(manager: PackageManager) -> Result<()> {
    match manager {
        PackageManager::Pacman => stream_and_print(
            Command::new("pacman").arg("-Qq"),
            |line| Some(line.to_string()),
            |pkg| print_installed_package_row(manager, &pkg, &pkg),
        ),
        PackageManager::Apt => stream_and_print(
            Command::new("dpkg-query").args(["-W", "-f=${Package}\\n"]),
            |line| Some(line.to_string()),
            |pkg| print_installed_package_row(manager, &pkg, &pkg),
        ),
        PackageManager::Dnf => stream_and_print(
            Command::new("dnf").args(["list", "installed"]),
            parse_dnf_package_name,
            |pkg| print_installed_package_row(manager, &pkg, &pkg),
        ),
        PackageManager::Zypper => stream_and_print(
            Command::new("zypper").args(["se", "--installed-only"]),
            parse_zypper_package_name,
            |pkg| print_installed_package_row(manager, &pkg, &pkg),
        ),
        PackageManager::Pkg => stream_and_print(
            Command::new("pkg").arg("list-installed"),
            parse_pkg_name,
            |pkg| print_installed_package_row(manager, &pkg, &pkg),
        ),
        PackageManager::Flatpak => stream_and_print(
            Command::new("flatpak").args(["list", "--app", "--columns=application"]),
            |line| Some(line.to_string()),
            |pkg| print_installed_package_row(manager, &pkg, &pkg),
        ),
        PackageManager::Aur => stream_and_print(
            Command::new("pacman").arg("-Qm"),
            |line| Some(line.to_string()),
            |pkg| print_installed_package_row(manager, &pkg, &pkg),
        ),
        PackageManager::Cargo => stream_and_print(
            Command::new("cargo").args(["install", "--list"]),
            parse_cargo_installed_name,
            |pkg| print_installed_package_row(manager, &pkg, &pkg),
        ),
        PackageManager::Snap => stream_installed_snaps(),
    }
}

fn stream_arch_available() -> Result<()> {
    stream_and_print(
        Command::new("pacman").arg("-Slq"),
        |line| Some(line.to_string()),
        |pkg| print_arch_package_row(PackageManager::Pacman, pkg),
    )?;

    if detect_aur_helper().is_some() {
        stream_aur_available_with_prefix()?;
    }

    Ok(())
}

fn stream_aur_available() -> Result<()> {
    stream_aur_packages(None)
}

fn stream_aur_available_with_prefix() -> Result<()> {
    stream_aur_packages(Some(PackageManager::Aur))
}

fn stream_aur_packages(prefix: Option<PackageManager>) -> Result<()> {
    let output = cmd!("curl", "-sL", "https://aur.archlinux.org/packages.gz")
        .pipe(cmd!("gunzip"))
        .read()
        .context("Failed to fetch AUR package list")?;

    for line in output.lines().filter(|line| !line.trim().is_empty()) {
        if let Some(manager) = prefix {
            print_arch_package_row(manager, line.to_string())?;
        } else {
            print_package_row(
                PackageManager::Aur,
                line,
                line,
                line.to_string(),
                preview_command(PreviewId::Aur),
            )?;
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
            print_installed_package_row(PackageManager::Snap, name, line)?;
        }
    }

    Ok(())
}

fn stream_and_print<F, P>(
    command: &mut Command,
    mut map_line: F,
    mut print_row: P,
) -> Result<()>
where
    F: FnMut(&str) -> Option<String>,
    P: FnMut(String) -> Result<()>,
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

    for line in reader.lines() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(mapped) = map_line(line)
            && !mapped.trim().is_empty()
        {
            print_row(mapped)?;
        }
    }

    child.wait()?;
    Ok(())
}

fn preview_id_for_manager(manager: PackageManager) -> PreviewId {
    match manager {
        PackageManager::Apt => PreviewId::Apt,
        PackageManager::Dnf => PreviewId::Dnf,
        PackageManager::Zypper => PreviewId::Zypper,
        PackageManager::Pacman => PreviewId::Pacman,
        PackageManager::Snap => PreviewId::Snap,
        PackageManager::Pkg => PreviewId::Pkg,
        PackageManager::Flatpak => PreviewId::Flatpak,
        PackageManager::Aur => PreviewId::Aur,
        PackageManager::Cargo => PreviewId::Cargo,
    }
}

fn print_installed_package_row(
    manager: PackageManager,
    package: &str,
    display: &str,
) -> Result<()> {
    print_package_row(
        manager,
        package,
        format!("{}\t{}", manager.as_str(), package),
        display.to_string(),
        preview_command(PreviewId::InstalledPackage),
    )
}

fn print_arch_package_row(manager: PackageManager, package: String) -> Result<()> {
    let preview_arg = format!("{}\t{}", manager.as_str(), package);
    print_package_row(
        manager,
        &package.clone(),
        preview_arg,
        package,
        preview_command(PreviewId::Package),
    )
}

fn print_package_row(
    manager: PackageManager,
    key: &str,
    preview_arg: impl Into<String>,
    display: String,
    preview_command: String,
) -> Result<()> {
    let payload = PackageSelectionPayload {
        manager: manager.as_str().to_string(),
        package: key.to_string(),
    };
    let row = StreamingMenuItem::new("package", key, display, payload)
        .preview(FzfPreview::Command(preview_command))
        .preview_arg(preview_arg)
        .encode()?;
    let mut out = io::BufWriter::new(io::stdout());
    writeln!(out, "{row}")?;
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
