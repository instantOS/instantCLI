//! Package preview rendering

use anyhow::Result;
use duct::cmd;

use crate::common::distro::OperatingSystem;
use crate::common::package::{PackageManager, detect_aur_helper};
use crate::ui::catppuccin::colors;
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

use super::PreviewContext;

/// Helper to extract package from context, returning None for empty.
fn package_from_context(ctx: &PreviewContext) -> Option<&str> {
    ctx.key().filter(|k| !k.is_empty())
}

/// Build a placeholder preview for when no package is selected.
fn placeholder_preview(title: &str, subtitle: &str) -> Result<String> {
    Ok(PreviewBuilder::new()
        .header(NerdFont::Package, title)
        .subtext(subtitle)
        .build_string())
}

// ============================================================================
// Routing helpers
// ============================================================================

/// Core routing logic shared by both collect and streaming paths.
fn render_package_with(ctx: &PreviewContext, builder: PreviewBuilder) -> Result<PreviewBuilder> {
    let key = ctx.key().unwrap_or_default();

    let (source, package) = if let Some((src, pkg)) = key.split_once('\t') {
        (Some(src), pkg)
    } else {
        (None, key)
    };

    if package.is_empty() {
        return Ok(builder
            .header(NerdFont::Package, "Package Info")
            .subtext("Select a package to see details"));
    }

    if let Some(src) = source {
        if let Ok(manager) = src.parse::<PackageManager>() {
            return render_for_manager(package, manager, builder);
        }
        return Ok(builder
            .header(NerdFont::Package, package)
            .subtext(&format!("{src} Package"))
            .blank()
            .subtext("Preview not available for this package type"));
    }

    let os = OperatingSystem::detect();
    if let Some(manager) = os.native_package_manager() {
        render_for_manager(package, manager, builder)
    } else {
        Ok(builder
            .header(NerdFont::Package, package)
            .subtext("Package manager not available"))
    }
}

/// Core routing logic for installed packages.
fn render_installed_package_with(
    ctx: &PreviewContext,
    builder: PreviewBuilder,
) -> Result<PreviewBuilder> {
    let package = ctx.key().unwrap_or_default();

    if package.is_empty() {
        return Ok(builder
            .header(NerdFont::Package, "Package Info")
            .subtext("Select a package to see details"));
    }

    if package.contains('\t')
        && let Some((src, pkg)) = package.split_once('\t')
    {
        if let Ok(manager) = src.parse::<PackageManager>() {
            return render_for_manager(pkg, manager, builder);
        }
        return Ok(builder
            .header(NerdFont::Package, pkg)
            .subtext(&format!("{src} Package"))
            .blank()
            .subtext("Preview not available for this package type"));
    }

    let os = OperatingSystem::detect();
    if let Some(manager) = os.native_package_manager() {
        render_for_manager(package, manager, builder)
    } else {
        Ok(builder
            .header(NerdFont::Package, package)
            .subtext("Package manager not available"))
    }
}

/// Route to the correct per-manager renderer.
fn render_for_manager(
    package: &str,
    manager: PackageManager,
    builder: PreviewBuilder,
) -> Result<PreviewBuilder> {
    match manager {
        PackageManager::Apt => render_apt_impl(package, builder),
        PackageManager::Dnf => render_dnf_impl(package, builder),
        PackageManager::Zypper => render_zypper_impl(package, builder),
        PackageManager::Pacman => render_pacman_impl(package, builder),
        PackageManager::Snap => render_snap_impl(package, builder),
        PackageManager::Pkg => render_pkg_impl(package, builder),
        PackageManager::Flatpak => render_flatpak_impl(package, builder),
        PackageManager::Aur => render_aur_impl(package, builder),
        PackageManager::Cargo => render_cargo_impl(package, builder),
    }
}

// ============================================================================
// Public entry points (collect mode — return String)
// ============================================================================

/// Render preview for a package (install context).
/// Key format: "package_name" or "source\tpackage_name" for Arch.
pub fn render_package_preview(ctx: &PreviewContext) -> Result<String> {
    Ok(render_package_with(ctx, PreviewBuilder::new())?.build_string())
}

/// Render preview for an installed package (uninstall context).
pub fn render_installed_package_preview(ctx: &PreviewContext) -> Result<String> {
    Ok(render_installed_package_with(ctx, PreviewBuilder::new())?.build_string())
}

pub fn render_apt_preview(ctx: &PreviewContext) -> Result<String> {
    match package_from_context(ctx) {
        Some(pkg) => Ok(render_apt_impl(pkg, PreviewBuilder::new())?.build_string()),
        None => placeholder_preview("APT Package", "Select a package to see details"),
    }
}

pub fn render_dnf_preview(ctx: &PreviewContext) -> Result<String> {
    match package_from_context(ctx) {
        Some(pkg) => Ok(render_dnf_impl(pkg, PreviewBuilder::new())?.build_string()),
        None => placeholder_preview("DNF Package", "Select a package to see details"),
    }
}

pub fn render_zypper_preview(ctx: &PreviewContext) -> Result<String> {
    match package_from_context(ctx) {
        Some(pkg) => Ok(render_zypper_impl(pkg, PreviewBuilder::new())?.build_string()),
        None => placeholder_preview("Zypper Package", "Select a package to see details"),
    }
}

pub fn render_pacman_preview(ctx: &PreviewContext) -> Result<String> {
    match package_from_context(ctx) {
        Some(pkg) => Ok(render_pacman_impl(pkg, PreviewBuilder::new())?.build_string()),
        None => placeholder_preview("Pacman Package", "Select a package to see details"),
    }
}

pub fn render_snap_preview(ctx: &PreviewContext) -> Result<String> {
    match package_from_context(ctx) {
        Some(pkg) => Ok(render_snap_impl(pkg, PreviewBuilder::new())?.build_string()),
        None => placeholder_preview("Snap Package", "Select a Snap package to see details"),
    }
}

pub fn render_pkg_preview(ctx: &PreviewContext) -> Result<String> {
    match package_from_context(ctx) {
        Some(pkg) => Ok(render_pkg_impl(pkg, PreviewBuilder::new())?.build_string()),
        None => placeholder_preview("Pkg Package", "Select a package to see details"),
    }
}

pub fn render_flatpak_preview(ctx: &PreviewContext) -> Result<String> {
    match package_from_context(ctx) {
        Some(pkg) => Ok(render_flatpak_impl(pkg, PreviewBuilder::new())?.build_string()),
        None => placeholder_preview("Flatpak Package", "Select a Flatpak package to see details"),
    }
}

pub fn render_aur_preview(ctx: &PreviewContext) -> Result<String> {
    match package_from_context(ctx) {
        Some(pkg) => Ok(render_aur_impl(pkg, PreviewBuilder::new())?.build_string()),
        None => placeholder_preview("AUR Package", "Select an AUR package to see details"),
    }
}

pub fn render_cargo_preview(ctx: &PreviewContext) -> Result<String> {
    match package_from_context(ctx) {
        Some(pkg) => Ok(render_cargo_impl(pkg, PreviewBuilder::new())?.build_string()),
        None => placeholder_preview("Cargo Package", "Select a Cargo package to see details"),
    }
}

// ============================================================================
// Streaming entry points — called by handle_preview_command with a streaming
// builder so the header appears immediately while slow commands run.
// ============================================================================

pub(crate) fn render_package_preview_streaming(ctx: &PreviewContext) -> Result<()> {
    render_package_with(ctx, PreviewBuilder::streaming())?;
    Ok(())
}

pub(crate) fn render_installed_package_preview_streaming(ctx: &PreviewContext) -> Result<()> {
    render_installed_package_with(ctx, PreviewBuilder::streaming())?;
    Ok(())
}

/// Streaming entry point for a specific per-manager preview.
pub(crate) fn render_manager_preview_streaming(
    ctx: &PreviewContext,
    manager_render: fn(&str, PreviewBuilder) -> Result<PreviewBuilder>,
    placeholder_title: &str,
) -> Result<()> {
    match package_from_context(ctx) {
        Some(pkg) => {
            manager_render(pkg, PreviewBuilder::streaming())?;
        }
        None => {
            PreviewBuilder::streaming()
                .header(NerdFont::Package, placeholder_title)
                .subtext("Select a package to see details");
        }
    }
    Ok(())
}

// ============================================================================
// Per-manager implementations (accept a PreviewBuilder, work in any mode)
// ============================================================================

pub(crate) fn render_apt_impl(package: &str, builder: PreviewBuilder) -> Result<PreviewBuilder> {
    let mut builder =
        builder
            .header(NerdFont::Package, package)
            .line(colors::BLUE, None, "APT Package");

    let output = cmd!("apt", "show", package)
        .stderr_null()
        .read()
        .unwrap_or_default();

    if output.is_empty() {
        return Ok(builder.blank().subtext("No package information available"));
    }

    for line in output.lines() {
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim();
            let value = value.trim();
            match key {
                "Description" | "Description-en" => builder = builder.text(value),
                "Version" => builder = builder.field("Version", value),
                "Section" => builder = builder.field("Section", value),
                "Maintainer" => builder = builder.field("Maintainer", value),
                "Homepage" => builder = builder.field("URL", value),
                "Installed-Size" | "Size" => builder = builder.field("Size", value),
                _ => {}
            }
        }
    }

    Ok(builder)
}

pub(crate) fn render_dnf_impl(package: &str, builder: PreviewBuilder) -> Result<PreviewBuilder> {
    let mut builder =
        builder
            .header(NerdFont::Package, package)
            .line(colors::YELLOW, None, "DNF Package");

    let output = cmd!("dnf", "info", package)
        .stderr_null()
        .read()
        .unwrap_or_default();

    if output.is_empty() {
        return Ok(builder.blank().subtext("No package information available"));
    }

    for line in output.lines() {
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim();
            let value = value.trim();
            match key {
                "Summary" | "Description" => builder = builder.text(value),
                "Version" => builder = builder.field("Version", value),
                "Release" => builder = builder.field("Release", value),
                "Architecture" | "Arch" => builder = builder.field("Arch", value),
                "Size" => builder = builder.field("Size", value),
                "Repository" | "Repo" => builder = builder.field("Repository", value),
                "URL" => builder = builder.field("URL", value),
                "License" => builder = builder.field("License", value),
                _ => {}
            }
        }
    }

    Ok(builder)
}

pub(crate) fn render_zypper_impl(package: &str, builder: PreviewBuilder) -> Result<PreviewBuilder> {
    let mut builder =
        builder
            .header(NerdFont::Package, package)
            .line(colors::RED, None, "Zypper Package");

    let output = cmd!("zypper", "info", package)
        .stderr_null()
        .read()
        .unwrap_or_default();

    if output.is_empty() {
        return Ok(builder.blank().subtext("No package information available"));
    }

    for line in output.lines() {
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim();
            let value = value.trim();
            match key {
                "Summary" | "Description" => builder = builder.text(value),
                "Version" => builder = builder.field("Version", value),
                "Repository" => builder = builder.field("Repository", value),
                "Size" => builder = builder.field("Size", value),
                _ => {}
            }
        }
    }

    Ok(builder)
}

pub(crate) fn render_pacman_impl(package: &str, builder: PreviewBuilder) -> Result<PreviewBuilder> {
    let mut builder = builder
        .header(NerdFont::Package, package)
        .line(colors::GREEN, None, "Pacman Package")
        .blank();

    let output = cmd!("pacman", "-Si", package)
        .stderr_null()
        .read()
        .unwrap_or_default();

    if output.is_empty() {
        return Ok(builder.subtext("No package information available"));
    }

    for line in output.lines() {
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim();
            let value = value.trim();
            match key {
                "Description" => builder = builder.text(value),
                "Version" => builder = builder.field("Version", value),
                "Repository" => builder = builder.field("Repository", value),
                "URL" => builder = builder.field("URL", value),
                "Licenses" => builder = builder.field("License", value),
                "Installed Size" | "Download Size" => builder = builder.field(key, value),
                _ => {}
            }
        }
    }

    Ok(builder)
}

pub(crate) fn render_snap_impl(
    package_info: &str,
    builder: PreviewBuilder,
) -> Result<PreviewBuilder> {
    let parts: Vec<&str> = package_info.split('\t').collect();

    // Extract package name from either tab-separated format or plain name
    let name = if parts.len() >= 4 {
        parts[0]
    } else {
        package_info
            .split_whitespace()
            .next()
            .unwrap_or(package_info)
    };

    let mut builder = builder
        .header(NerdFont::Package, name)
        .line(colors::PEACH, None, "Snap Package")
        .blank();

    // Fetch detailed info from snap store (may be slow/network)
    let output = cmd!("snap", "info", name)
        .stderr_null()
        .read()
        .unwrap_or_default();

    if output.is_empty() {
        return Ok(builder.subtext("No package information available"));
    }

    let mut description = String::new();
    let mut in_description = false;
    let mut size = None;

    for line in output.lines() {
        // Check for size in channels section (e.g., "latest/stable: 1.0 (100) 50MB -")
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

            // Handle multi-line description
            if key == "description" {
                in_description = true;
                if !value.is_empty() {
                    description.push_str(value);
                }
                continue;
            }

            in_description = false;

            match key {
                "summary" | "Summary" => builder = builder.text(value),
                "version" | "Version" => builder = builder.field("Version", value),
                "publisher" | "Publisher" => builder = builder.field("Publisher", value),
                "license" | "License" => builder = builder.field("License", value),
                "store-url" => builder = builder.field("Store URL", value),
                _ => {}
            }
        } else if in_description && !line.starts_with("channels:") {
            description.push('\n');
            description.push_str(line);
        }
    }

    // Add size if found
    if let Some(s) = size {
        builder = builder.field("Size", &s);
    }

    // Add description if we collected one
    if !description.is_empty() {
        builder = builder.blank().text(description.trim());
    }

    Ok(builder)
}

pub(crate) fn render_pkg_impl(package: &str, builder: PreviewBuilder) -> Result<PreviewBuilder> {
    let mut builder =
        builder
            .header(NerdFont::Package, package)
            .line(colors::TEAL, None, "Pkg Package");

    let output = cmd!("pkg", "show", package)
        .stderr_null()
        .read()
        .unwrap_or_default();

    if output.is_empty() {
        return Ok(builder.blank().subtext("No package information available"));
    }

    for line in output.lines() {
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim();
            let value = value.trim();
            match key {
                "Description" | "Description-en" => builder = builder.text(value),
                "Version" => builder = builder.field("Version", value),
                "Section" => builder = builder.field("Section", value),
                "Maintainer" => builder = builder.field("Maintainer", value),
                "Homepage" => builder = builder.field("URL", value),
                "Installed-Size" | "Size" => builder = builder.field("Size", value),
                _ => {}
            }
        }
    }

    Ok(builder)
}

pub(crate) fn render_flatpak_impl(
    package_info: &str,
    builder: PreviewBuilder,
) -> Result<PreviewBuilder> {
    // Extract app_id from "app_id\tname\tdescription" format
    let package = if package_info.contains('\t') {
        package_info.split('\t').next().unwrap_or(package_info)
    } else {
        package_info
    };

    let mut builder = builder
        .header(NerdFont::Package, package)
        .line(colors::PINK, None, "Flatpak Package")
        .blank();

    // Try local info first (faster for installed apps)
    let local_output = cmd!("flatpak", "info", package).stderr_null().read().ok();

    // If not installed locally, try remotes (potentially slow/network)
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
        return Ok(builder.subtext("No package information available"));
    };

    for line in output.lines() {
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim();
            let value = value.trim();
            match key {
                "ID" | "Ref" => builder = builder.field("ID", value),
                "Arch" | "Architecture" => builder = builder.field("Architecture", value),
                "Branch" => builder = builder.field("Branch", value),
                "Origin" => builder = builder.field("Origin", value),
                "Installation" => builder = builder.field("Installation", value),
                "Installed" => builder = builder.field("Installed", value),
                "Runtime" => builder = builder.field("Runtime", value),
                "Version" => builder = builder.field("Version", value),
                "License" => builder = builder.field("License", value),
                // Include size fields from remote-info
                "Download" => builder = builder.field("Download Size", value),
                "Installed Size" => builder = builder.field("Installed Size", value),
                _ => {
                    if line.starts_with("Description:") || line.starts_with("Summary:") {
                        builder = builder.text(value);
                    }
                }
            }
        }
    }

    Ok(builder)
}

pub(crate) fn render_aur_impl(package: &str, builder: PreviewBuilder) -> Result<PreviewBuilder> {
    let helper = detect_aur_helper().unwrap_or("yay");
    let mut builder = builder
        .header(NerdFont::Package, package)
        .line(colors::MAUVE, None, "AUR Package")
        .blank();

    let output = cmd!(helper, "-Si", package)
        .stderr_null()
        .read()
        .unwrap_or_default();

    if output.is_empty() {
        return Ok(builder.subtext("No package information available"));
    }

    for line in output.lines() {
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim();
            let value = value.trim();
            match key {
                "Description" => builder = builder.text(value),
                "Version" => builder = builder.field("Version", value),
                "Repository" => builder = builder.field("Repository", value),
                "URL" => builder = builder.field("URL", value),
                "Licenses" => builder = builder.field("License", value),
                "Installed Size" | "Download Size" => builder = builder.field(key, value),
                _ => {}
            }
        }
    }

    Ok(builder)
}

pub(crate) fn render_cargo_impl(package: &str, builder: PreviewBuilder) -> Result<PreviewBuilder> {
    let mut builder =
        builder
            .header(NerdFont::Package, package)
            .line(colors::MAROON, None, "Cargo Package");

    let output = cmd!("cargo", "show", package)
        .stderr_null()
        .read()
        .unwrap_or_default();

    if output.is_empty() {
        return Ok(builder.blank().subtext("No package information available"));
    }

    for line in output.lines() {
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim();
            let value = value.trim();
            match key {
                "name" | "Name" => builder = builder.field("Name", value),
                "version" | "Version" => builder = builder.field("Version", value),
                "description" | "Description" => builder = builder.text(value),
                "homepage" | "Homepage" => builder = builder.field("Homepage", value),
                "repository" | "Repository" => builder = builder.field("Repository", value),
                "keywords" | "Keywords" => builder = builder.field("Keywords", value),
                "license" | "License" => builder = builder.field("License", value),
                _ => {}
            }
        }
    }

    Ok(builder)
}
