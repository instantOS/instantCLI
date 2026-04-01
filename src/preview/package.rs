//! Package preview rendering

use anyhow::Result;
use duct::cmd;

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

/// Build a placeholder preview for when no package is selected.
fn placeholder_preview(title: &str, subtitle: &str) -> Result<String> {
    let mut preview = PreviewWriter::collect();
    preview.header(NerdFont::Package, title).subtext(subtitle);
    Ok(preview.build_string())
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
            return render_for_manager(pkg, manager, preview);
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
        render_for_manager(package, manager, preview)
    } else {
        preview
            .header(NerdFont::Package, package)
            .subtext("Package manager not available");
        Ok(())
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

fn collect_preview(render: impl FnOnce(&mut PreviewWriter) -> Result<()>) -> Result<String> {
    let mut preview = PreviewWriter::collect();
    render(&mut preview)?;
    Ok(preview.build_string())
}

// ============================================================================
// Public entry points (collect mode — return String)
// ============================================================================

/// Render preview for a package (install context).
/// Key format: "package_name" or "source\tpackage_name" for Arch.
pub fn render_package_preview(ctx: &PreviewContext) -> Result<String> {
    collect_preview(|preview| render_package_with(ctx, preview))
}

/// Render preview for an installed package (uninstall context).
pub fn render_installed_package_preview(ctx: &PreviewContext) -> Result<String> {
    collect_preview(|preview| render_installed_package_with(ctx, preview))
}

pub fn render_apt_preview(ctx: &PreviewContext) -> Result<String> {
    match package_from_context(ctx) {
        Some(pkg) => collect_preview(|preview| render_apt_impl(pkg, preview)),
        None => placeholder_preview("APT Package", "Select a package to see details"),
    }
}

pub fn render_dnf_preview(ctx: &PreviewContext) -> Result<String> {
    match package_from_context(ctx) {
        Some(pkg) => collect_preview(|preview| render_dnf_impl(pkg, preview)),
        None => placeholder_preview("DNF Package", "Select a package to see details"),
    }
}

pub fn render_zypper_preview(ctx: &PreviewContext) -> Result<String> {
    match package_from_context(ctx) {
        Some(pkg) => collect_preview(|preview| render_zypper_impl(pkg, preview)),
        None => placeholder_preview("Zypper Package", "Select a package to see details"),
    }
}

pub fn render_pacman_preview(ctx: &PreviewContext) -> Result<String> {
    match package_from_context(ctx) {
        Some(pkg) => collect_preview(|preview| render_pacman_impl(pkg, preview)),
        None => placeholder_preview("Pacman Package", "Select a package to see details"),
    }
}

pub fn render_snap_preview(ctx: &PreviewContext) -> Result<String> {
    match package_from_context(ctx) {
        Some(pkg) => collect_preview(|preview| render_snap_impl(pkg, preview)),
        None => placeholder_preview("Snap Package", "Select a Snap package to see details"),
    }
}

pub fn render_pkg_preview(ctx: &PreviewContext) -> Result<String> {
    match package_from_context(ctx) {
        Some(pkg) => collect_preview(|preview| render_pkg_impl(pkg, preview)),
        None => placeholder_preview("Pkg Package", "Select a package to see details"),
    }
}

pub fn render_flatpak_preview(ctx: &PreviewContext) -> Result<String> {
    match package_from_context(ctx) {
        Some(pkg) => collect_preview(|preview| render_flatpak_impl(pkg, preview)),
        None => placeholder_preview("Flatpak Package", "Select a Flatpak package to see details"),
    }
}

pub fn render_aur_preview(ctx: &PreviewContext) -> Result<String> {
    match package_from_context(ctx) {
        Some(pkg) => collect_preview(|preview| render_aur_impl(pkg, preview)),
        None => placeholder_preview("AUR Package", "Select an AUR package to see details"),
    }
}

pub fn render_cargo_preview(ctx: &PreviewContext) -> Result<String> {
    match package_from_context(ctx) {
        Some(pkg) => collect_preview(|preview| render_cargo_impl(pkg, preview)),
        None => placeholder_preview("Cargo Package", "Select a Cargo package to see details"),
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
    ctx: &PreviewContext,
    manager_render: fn(&str, &mut PreviewWriter) -> Result<()>,
    placeholder_title: &str,
) -> Result<()> {
    let Some(id) = preview_id_for_placeholder(placeholder_title) else {
        match package_from_context(ctx) {
            Some(pkg) => {
                let mut preview = PreviewWriter::streaming();
                manager_render(pkg, &mut preview)?;
            }
            None => {
                let mut preview = PreviewWriter::streaming();
                preview
                    .header(NerdFont::Package, placeholder_title)
                    .subtext("Select a package to see details");
            }
        }
        return Ok(());
    };

    cache::render_streaming_cached(id, ctx, |preview| match package_from_context(ctx) {
        Some(pkg) => manager_render(pkg, preview),
        None => {
            preview
                .header(NerdFont::Package, placeholder_title)
                .subtext("Select a package to see details");
            Ok(())
        }
    })
}

fn preview_id_for_placeholder(title: &str) -> Option<PreviewId> {
    match title {
        "APT Package" => Some(PreviewId::Apt),
        "DNF Package" => Some(PreviewId::Dnf),
        "Zypper Package" => Some(PreviewId::Zypper),
        "Pacman Package" => Some(PreviewId::Pacman),
        "Snap Package" => Some(PreviewId::Snap),
        "Pkg Package" => Some(PreviewId::Pkg),
        "Flatpak Package" => Some(PreviewId::Flatpak),
        "AUR Package" => Some(PreviewId::Aur),
        "Cargo Package" => Some(PreviewId::Cargo),
        _ => None,
    }
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
    let package = if package_info.contains('\t') {
        package_info.split('\t').next().unwrap_or(package_info)
    } else {
        package_info
    };

    preview
        .header(NerdFont::Package, package)
        .line(colors::PINK, None, "Flatpak Package")
        .blank();

    let local_output = cmd!("flatpak", "info", package).stderr_null().read().ok();

    let output = if local_output.is_some() {
        local_output
    } else {
        let remotes_output = cmd!("flatpak", "remotes", "--columns=name")
            .stderr_null()
            .read()
            .unwrap_or_default();

        let remotes: Vec<&str> = remotes_output.lines().collect();

        let mut remote_output = None;
        for remote in &remotes {
            if let Ok(output) = cmd!("flatpak", "remote-info", remote.trim(), package)
                .stderr_null()
                .read()
                && !output.is_empty()
            {
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
