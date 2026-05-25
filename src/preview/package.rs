//! Package preview rendering

use anyhow::Result;
use duct::cmd;
use std::collections::{BTreeMap, BTreeSet, HashSet};

use crate::common::distro::OperatingSystem;
use crate::common::package::{PackageManager, detect_aur_helper};
use crate::ui::catppuccin::colors;
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewWriter;

use super::{PreviewContext, PreviewId, cache};

/// Helper to extract package from context, returning None for empty.
fn package_from_context(ctx: &PreviewContext) -> Option<&str> {
    ctx.key().filter(|k| !k.is_empty())
}

// ============================================================================
// Routing helpers
// ============================================================================

fn render_package_with(ctx: &PreviewContext, preview: &mut PreviewWriter) -> Result<()> {
    let key = ctx.key().unwrap_or_default();

    let (source, package) = if let Some((src, pkg)) = key.split_once('\t') {
        (Some(src), pkg)
    } else {
        (None, key)
    };

    if package.is_empty() {
        preview
            .header(NerdFont::Package, "Package Info")
            .subtext("Select a package to see details");
        return Ok(());
    }

    if let Some(src) = source {
        if let Ok(manager) = src.parse::<PackageManager>() {
            return render_for_manager(package, manager, preview);
        }

        preview
            .header(NerdFont::Package, package)
            .subtext(&format!("{src} Package"))
            .blank()
            .subtext("Preview not available for this package type");
        return Ok(());
    }

    let os = OperatingSystem::detect();
    if let Some(manager) = os.native_package_manager() {
        render_for_manager(package, manager, preview)
    } else {
        preview
            .header(NerdFont::Package, package)
            .subtext("Package manager not available");
        Ok(())
    }
}

fn render_installed_package_with(ctx: &PreviewContext, preview: &mut PreviewWriter) -> Result<()> {
    let package = ctx.key().unwrap_or_default();

    if package.is_empty() {
        preview
            .header(NerdFont::Package, "Package Info")
            .subtext("Select a package to see details");
        return Ok(());
    }

    if package.contains('\t')
        && let Some((src, pkg)) = package.split_once('\t')
    {
        if let Ok(manager) = src.parse::<PackageManager>() {
            return render_installed_for_manager(pkg, manager, preview);
        }

        preview
            .header(NerdFont::Package, pkg)
            .subtext(&format!("{src} Package"))
            .blank()
            .subtext("Preview not available for this package type");
        return Ok(());
    }

    let os = OperatingSystem::detect();
    if let Some(manager) = os.native_package_manager() {
        render_installed_for_manager(package, manager, preview)
    } else {
        preview
            .header(NerdFont::Package, package)
            .subtext("Package manager not available");
        Ok(())
    }
}

fn render_installed_for_manager(
    package: &str,
    manager: PackageManager,
    preview: &mut PreviewWriter,
) -> Result<()> {
    match manager {
        PackageManager::Pacman | PackageManager::Aur => {
            render_installed_pacman_impl(package, preview)
        }
        _ => render_for_manager(package, manager, preview),
    }
}

fn render_for_manager(
    package: &str,
    manager: PackageManager,
    preview: &mut PreviewWriter,
) -> Result<()> {
    match manager {
        PackageManager::Apt => render_apt_impl(package, preview),
        PackageManager::Dnf => render_dnf_impl(package, preview),
        PackageManager::Zypper => render_zypper_impl(package, preview),
        PackageManager::Pacman => render_pacman_impl(package, preview),
        PackageManager::Snap => render_snap_impl(package, preview),
        PackageManager::Pkg => render_pkg_impl(package, preview),
        PackageManager::Flatpak => render_flatpak_impl(package, preview),
        PackageManager::Aur => render_aur_impl(package, preview),
        PackageManager::Cargo => render_cargo_impl(package, preview),
    }
}

// ============================================================================
// Streaming entry points — called by handle_preview_command with a streaming
// writer so the header appears immediately while slow commands run.
// ============================================================================

pub(crate) fn render_package_preview_streaming(ctx: &PreviewContext) -> Result<()> {
    cache::render_streaming_cached(PreviewId::Package, ctx, |preview| {
        render_package_with(ctx, preview)
    })
}

pub(crate) fn render_installed_package_preview_streaming(ctx: &PreviewContext) -> Result<()> {
    cache::render_streaming_cached(PreviewId::InstalledPackage, ctx, |preview| {
        render_installed_package_with(ctx, preview)
    })
}

pub(crate) fn render_manager_preview_streaming(
    id: PreviewId,
    ctx: &PreviewContext,
    manager_render: fn(&str, &mut PreviewWriter) -> Result<()>,
) -> Result<()> {
    cache::render_streaming_cached(id, ctx, |preview| match package_from_context(ctx) {
        Some(pkg) => manager_render(pkg, preview),
        None => {
            preview
                .header(NerdFont::Package, &format!("{id}"))
                .subtext("Select a package to see details");
            Ok(())
        }
    })
}

// ============================================================================
// Per-manager implementations
// ============================================================================

pub(crate) fn render_apt_impl(package: &str, preview: &mut PreviewWriter) -> Result<()> {
    preview
        .header(NerdFont::Package, package)
        .line(colors::BLUE, None, "APT Package");

    let output = cmd!("apt", "show", package)
        .stderr_null()
        .read()
        .unwrap_or_default();

    if output.is_empty() {
        preview.blank().subtext("No package information available");
        return Ok(());
    }

    for line in output.lines() {
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim();
            let value = value.trim();
            match key {
                "Description" | "Description-en" => {
                    preview.text(value);
                }
                "Version" => {
                    preview.field("Version", value);
                }
                "Section" => {
                    preview.field("Section", value);
                }
                "Maintainer" => {
                    preview.field("Maintainer", value);
                }
                "Homepage" => {
                    preview.field("URL", value);
                }
                "Installed-Size" | "Size" => {
                    preview.field("Size", value);
                }
                _ => {}
            }
        }
    }

    Ok(())
}

pub(crate) fn render_dnf_impl(package: &str, preview: &mut PreviewWriter) -> Result<()> {
    preview
        .header(NerdFont::Package, package)
        .line(colors::YELLOW, None, "DNF Package");

    let output = cmd!("dnf", "info", package)
        .stderr_null()
        .read()
        .unwrap_or_default();

    if output.is_empty() {
        preview.blank().subtext("No package information available");
        return Ok(());
    }

    for line in output.lines() {
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim();
            let value = value.trim();
            match key {
                "Summary" | "Description" => {
                    preview.text(value);
                }
                "Version" => {
                    preview.field("Version", value);
                }
                "Release" => {
                    preview.field("Release", value);
                }
                "Architecture" | "Arch" => {
                    preview.field("Arch", value);
                }
                "Size" => {
                    preview.field("Size", value);
                }
                "Repository" | "Repo" => {
                    preview.field("Repository", value);
                }
                "URL" => {
                    preview.field("URL", value);
                }
                "License" => {
                    preview.field("License", value);
                }
                _ => {}
            }
        }
    }

    Ok(())
}

pub(crate) fn render_zypper_impl(package: &str, preview: &mut PreviewWriter) -> Result<()> {
    preview
        .header(NerdFont::Package, package)
        .line(colors::RED, None, "Zypper Package");

    let output = cmd!("zypper", "info", package)
        .stderr_null()
        .read()
        .unwrap_or_default();

    if output.is_empty() {
        preview.blank().subtext("No package information available");
        return Ok(());
    }

    for line in output.lines() {
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim();
            let value = value.trim();
            match key {
                "Summary" | "Description" => {
                    preview.text(value);
                }
                "Version" => {
                    preview.field("Version", value);
                }
                "Repository" => {
                    preview.field("Repository", value);
                }
                "Size" => {
                    preview.field("Size", value);
                }
                _ => {}
            }
        }
    }

    Ok(())
}

pub(crate) fn render_pacman_impl(package: &str, preview: &mut PreviewWriter) -> Result<()> {
    preview
        .header(NerdFont::Package, package)
        .line(colors::GREEN, None, "Pacman Package")
        .blank();

    let output = cmd!("pacman", "-Si", package)
        .stderr_null()
        .read()
        .unwrap_or_default();

    if output.is_empty() {
        preview.subtext("No package information available");
        return Ok(());
    }

    render_pacman_fields(&output, preview);

    Ok(())
}

fn render_installed_pacman_impl(package: &str, preview: &mut PreviewWriter) -> Result<()> {
    preview
        .header(NerdFont::Package, package)
        .line(colors::GREEN, None, "Installed Pacman Package")
        .blank();

    let output = cmd!("pacman", "-Qi", package)
        .stderr_null()
        .read()
        .unwrap_or_default();

    if output.is_empty() {
        preview.subtext("No package information available");
        return Ok(());
    }

    render_pacman_fields(&output, preview);
    preview.blank();
    render_pacman_removal_cascade(package, &output, preview)?;

    Ok(())
}

fn render_pacman_fields(output: &str, preview: &mut PreviewWriter) {
    for line in output.lines() {
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim();
            let value = value.trim();
            match key {
                "Description" => {
                    preview.text(value);
                }
                "Version" => {
                    preview.field("Version", value);
                }
                "Repository" => {
                    preview.field("Repository", value);
                }
                "URL" => {
                    preview.field("URL", value);
                }
                "Licenses" => {
                    preview.field("License", value);
                }
                "Installed Size" | "Download Size" => {
                    preview.field(key, value);
                }
                _ => {}
            }
        }
    }
}

fn render_pacman_removal_cascade(
    package: &str,
    package_info: &str,
    preview: &mut PreviewWriter,
) -> Result<()> {
    preview.title(colors::PEACH, "Removal Cascade");

    let dependents = pacman_dependent_closure(package, package_info)?;
    if dependents.is_empty() {
        preview.subtext("No installed packages depend on this package.");
        return Ok(());
    }

    preview
        .line(
            colors::YELLOW,
            Some(NerdFont::Warning),
            "Uninstalling this package also requires uninstalling these packages.",
        )
        .bullets(dependents);

    Ok(())
}

fn pacman_dependent_closure(package: &str, package_info: &str) -> Result<Vec<String>> {
    let direct_dependents = parse_pacman_required_by(package_info);
    if direct_dependents.is_empty() {
        return Ok(Vec::new());
    }

    let all_required_by = pacman_required_by_map()?;
    let mut seen = BTreeSet::from([package.to_string()]);
    let mut dependents = BTreeSet::new();
    let mut queue = direct_dependents;

    while let Some(dependent) = queue.pop() {
        if !seen.insert(dependent.clone()) {
            continue;
        }

        dependents.insert(dependent.clone());
        if let Some(next_dependents) = all_required_by.get(&dependent) {
            queue.extend(next_dependents.iter().cloned());
        }
    }

    Ok(dependents.into_iter().collect())
}

fn pacman_required_by_map() -> Result<BTreeMap<String, Vec<String>>> {
    let output = cmd!("pacman", "-Qi").stderr_null().read()?;
    Ok(parse_pacman_required_by_map(&output))
}

fn parse_pacman_required_by_map(output: &str) -> BTreeMap<String, Vec<String>> {
    output
        .split("\n\n")
        .filter_map(|block| {
            let name = parse_pacman_field(block, "Name")?;
            Some((name, parse_pacman_required_by(block)))
        })
        .collect()
}

fn parse_pacman_field(output: &str, field: &str) -> Option<String> {
    output.lines().find_map(|line| {
        line.strip_prefix(field)
            .and_then(|value| value.split_once(':'))
            .map(|(_, value)| value.trim().to_string())
            .filter(|value| !value.is_empty())
    })
}

fn parse_pacman_required_by(output: &str) -> Vec<String> {
    output
        .lines()
        .find_map(|line| {
            line.strip_prefix("Required By")
                .and_then(|value| value.split_once(':'))
        })
        .map(|(_, packages)| {
            packages
                .split_whitespace()
                .filter(|package| *package != "None")
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn render_snap_impl(package_info: &str, preview: &mut PreviewWriter) -> Result<()> {
    let parts: Vec<&str> = package_info.split('\t').collect();
    let name = if parts.len() >= 4 {
        parts[0]
    } else {
        package_info
            .split_whitespace()
            .next()
            .unwrap_or(package_info)
    };

    preview
        .header(NerdFont::Package, name)
        .line(colors::PEACH, None, "Snap Package")
        .blank();

    let output = cmd!("snap", "info", name)
        .stderr_null()
        .read()
        .unwrap_or_default();

    if output.is_empty() {
        preview.subtext("No package information available");
        return Ok(());
    }

    let mut description = String::new();
    let mut in_description = false;
    let mut size = None;

    for line in output.lines() {
        if line.contains("latest/stable:")
            && !line.starts_with("latest/stable:")
            && let Some(size_match) = line.split_whitespace().nth(3)
            && size_match.ends_with("B")
        {
            size = Some(size_match.to_string());
        }

        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim();
            let value = value.trim();

            if key == "description" {
                in_description = true;
                if !value.is_empty() {
                    description.push_str(value);
                }
                continue;
            }

            in_description = false;

            match key {
                "summary" | "Summary" => {
                    preview.text(value);
                }
                "version" | "Version" => {
                    preview.field("Version", value);
                }
                "publisher" | "Publisher" => {
                    preview.field("Publisher", value);
                }
                "license" | "License" => {
                    preview.field("License", value);
                }
                "store-url" => {
                    preview.field("Store URL", value);
                }
                _ => {}
            }
        } else if in_description && !line.starts_with("channels:") {
            description.push('\n');
            description.push_str(line);
        }
    }

    if let Some(s) = size {
        preview.field("Size", &s);
    }

    if !description.is_empty() {
        preview.blank().text(description.trim());
    }

    Ok(())
}

pub(crate) fn render_pkg_impl(package: &str, preview: &mut PreviewWriter) -> Result<()> {
    preview
        .header(NerdFont::Package, package)
        .line(colors::TEAL, None, "Pkg Package");

    let output = cmd!("pkg", "show", package)
        .stderr_null()
        .read()
        .unwrap_or_default();

    if output.is_empty() {
        preview.blank().subtext("No package information available");
        return Ok(());
    }

    for line in output.lines() {
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim();
            let value = value.trim();
            match key {
                "Description" | "Description-en" => {
                    preview.text(value);
                }
                "Version" => {
                    preview.field("Version", value);
                }
                "Section" => {
                    preview.field("Section", value);
                }
                "Maintainer" => {
                    preview.field("Maintainer", value);
                }
                "Homepage" => {
                    preview.field("URL", value);
                }
                "Installed-Size" | "Size" => {
                    preview.field("Size", value);
                }
                _ => {}
            }
        }
    }

    Ok(())
}

pub(crate) fn render_flatpak_impl(package_info: &str, preview: &mut PreviewWriter) -> Result<()> {
    let (installation_hint, remote_hint, package) = parse_flatpak_preview_arg(package_info);

    preview
        .header(NerdFont::Package, package)
        .line(colors::PINK, None, "Flatpak Package")
        .blank();

    let local_output = cmd!("flatpak", "info", package).stderr_null().read().ok();

    let output = if local_output.is_some() {
        local_output
    } else if let Some(remote) = remote_hint {
        flatpak_remote_info(package, remote, installation_hint)
    } else {
        let remotes_output = cmd!("flatpak", "remotes", "--columns=name")
            .stderr_null()
            .read()
            .unwrap_or_default();

        let mut seen = HashSet::new();
        let remotes: Vec<&str> = remotes_output
            .lines()
            .map(str::trim)
            .filter(|remote| !remote.is_empty())
            .filter(|remote| seen.insert((*remote).to_string()))
            .collect();

        let mut remote_output = None;
        for remote in &remotes {
            if let Some(output) = flatpak_remote_info(package, remote.trim(), None) {
                remote_output = Some(output);
                break;
            }
        }
        remote_output
    };

    let Some(output) = output.as_deref() else {
        preview.subtext("No package information available");
        return Ok(());
    };

    for line in output.lines() {
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim();
            let value = value.trim();
            match key {
                "ID" | "Ref" => {
                    preview.field("ID", value);
                }
                "Arch" | "Architecture" => {
                    preview.field("Architecture", value);
                }
                "Branch" => {
                    preview.field("Branch", value);
                }
                "Origin" => {
                    preview.field("Origin", value);
                }
                "Installation" => {
                    preview.field("Installation", value);
                }
                "Installed" => {
                    preview.field("Installed", value);
                }
                "Runtime" => {
                    preview.field("Runtime", value);
                }
                "Version" => {
                    preview.field("Version", value);
                }
                "License" => {
                    preview.field("License", value);
                }
                "Download" => {
                    preview.field("Download Size", value);
                }
                "Installed Size" => {
                    preview.field("Installed Size", value);
                }
                _ => {
                    if line.starts_with("Description:") || line.starts_with("Summary:") {
                        preview.text(value);
                    }
                }
            }
        }
    }

    Ok(())
}

fn parse_flatpak_preview_arg(arg: &str) -> (Option<&str>, Option<&str>, &str) {
    let mut parts = arg.splitn(3, '|');
    let first = parts.next().unwrap_or_default();
    let second = parts.next();
    let third = parts.next();

    if let (Some(remote), Some(package)) = (second, third)
        && !first.is_empty()
        && !remote.is_empty()
        && !package.is_empty()
    {
        return (Some(first), Some(remote), package);
    }

    if let Some(package) = second
        && !first.is_empty()
        && !package.is_empty()
    {
        return (None, Some(first), package);
    }

    if let Some((remote, package)) = arg.split_once('\t')
        && !remote.is_empty()
        && !package.is_empty()
    {
        return (None, Some(remote), package);
    }

    (None, None, arg)
}

fn flatpak_remote_info(
    package: &str,
    remote: &str,
    installation_hint: Option<&str>,
) -> Option<String> {
    for scope in flatpak_scope_candidates(installation_hint) {
        let mut command = std::process::Command::new("flatpak");
        command.arg("remote-info");
        if let Some(scope) = scope {
            command.arg(format!("--{scope}"));
        }
        command.arg(remote).arg(package);

        let output = command.output().ok()?;
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            if !stdout.trim().is_empty() {
                return Some(stdout);
            }
        }
    }

    None
}

fn flatpak_scope_candidates(installation_hint: Option<&str>) -> [Option<&str>; 3] {
    match installation_hint {
        Some("user") => [Some("user"), Some("system"), None],
        Some("system") => [Some("system"), Some("user"), None],
        _ => [Some("user"), Some("system"), None],
    }
}

pub(crate) fn render_aur_impl(package: &str, preview: &mut PreviewWriter) -> Result<()> {
    let helper = detect_aur_helper().unwrap_or("yay");

    preview
        .header(NerdFont::Package, package)
        .line(colors::MAUVE, None, "AUR Package")
        .blank();

    let output = cmd!(helper, "-Si", package)
        .stderr_null()
        .read()
        .unwrap_or_default();

    if output.is_empty() {
        preview.subtext("No package information available");
        return Ok(());
    }

    for line in output.lines() {
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim();
            let value = value.trim();
            match key {
                "Description" => {
                    preview.text(value);
                }
                "Version" => {
                    preview.field("Version", value);
                }
                "Repository" => {
                    preview.field("Repository", value);
                }
                "URL" => {
                    preview.field("URL", value);
                }
                "Licenses" => {
                    preview.field("License", value);
                }
                "Installed Size" | "Download Size" => {
                    preview.field(key, value);
                }
                _ => {}
            }
        }
    }

    Ok(())
}

pub(crate) fn render_cargo_impl(package: &str, preview: &mut PreviewWriter) -> Result<()> {
    preview
        .header(NerdFont::Package, package)
        .line(colors::MAROON, None, "Cargo Package");

    let output = cmd!("cargo", "show", package)
        .stderr_null()
        .read()
        .unwrap_or_default();

    if output.is_empty() {
        preview.blank().subtext("No package information available");
        return Ok(());
    }

    for line in output.lines() {
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim();
            let value = value.trim();
            match key {
                "name" | "Name" => {
                    preview.field("Name", value);
                }
                "version" | "Version" => {
                    preview.field("Version", value);
                }
                "description" | "Description" => {
                    preview.text(value);
                }
                "homepage" | "Homepage" => {
                    preview.field("Homepage", value);
                }
                "repository" | "Repository" => {
                    preview.field("Repository", value);
                }
                "keywords" | "Keywords" => {
                    preview.field("Keywords", value);
                }
                "license" | "License" => {
                    preview.field("License", value);
                }
                _ => {}
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pacman_required_by_for_preview_cascade() {
        let output = "\
Name            : libfoo
Required By     : app-one  app-two
Optional For    : None
";

        assert_eq!(
            parse_pacman_required_by(output),
            vec!["app-one".to_string(), "app-two".to_string()]
        );
    }

    #[test]
    fn ignores_pacman_required_by_none_for_preview_cascade() {
        let output = "\
Name            : leaf-package
Required By     : None
Optional For    : app-one
";

        assert!(parse_pacman_required_by(output).is_empty());
    }

    #[test]
    fn parses_pacman_required_by_map_from_full_package_dump() {
        let output = "\
Name            : libfoo
Required By     : app-one  app-two

Name            : app-one
Required By     : shell-one

Name            : app-two
Required By     : None
";

        let map = parse_pacman_required_by_map(output);

        assert_eq!(
            map.get("libfoo"),
            Some(&vec!["app-one".to_string(), "app-two".to_string()])
        );
        assert_eq!(map.get("app-one"), Some(&vec!["shell-one".to_string()]));
        assert_eq!(map.get("app-two"), Some(&Vec::new()));
    }
}
