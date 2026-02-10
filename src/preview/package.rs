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

/// Render preview for a package (install context).
/// Key format: "package_name" or "source\tpackage_name" for Arch.
pub fn render_package_preview(ctx: &PreviewContext) -> Result<String> {
    let key = ctx.key().unwrap_or_default();

    // Check if this is an Arch-style key with source prefix
    let (source, package) = if let Some((src, pkg)) = key.split_once('\t') {
        (Some(src), pkg)
    } else {
        (None, key)
    };

    if package.is_empty() {
        return placeholder_preview("Package Info", "Select a package to see details");
    }

    // For packages with source info, route directly to the specific renderer
    if let Some(src) = source {
        if let Ok(manager) = src.parse::<PackageManager>() {
            return render_for_manager(package, manager);
        } else {
            return render_generic_package(package, src);
        }
    }

    // For other distros, use native package manager
    let os = OperatingSystem::detect();
    if let Some(manager) = os.native_package_manager() {
        render_for_manager(package, manager)
    } else {
        placeholder_preview(package, "Package manager not available")
    }
}

/// Render preview for an installed package (uninstall context).
pub fn render_installed_package_preview(ctx: &PreviewContext) -> Result<String> {
    let package = ctx.key().unwrap_or_default();

    if package.is_empty() {
        return placeholder_preview("Package Info", "Select a package to see details");
    }

    // Check if this has a source prefix
    if package.contains('\t')
        && let Some((src, pkg)) = package.split_once('\t')
    {
        if let Ok(manager) = src.parse::<PackageManager>() {
            return render_for_manager(pkg, manager);
        } else {
            return render_generic_package(pkg, src);
        }
    }

    let os = OperatingSystem::detect();
    if let Some(manager) = os.native_package_manager() {
        render_for_manager(package, manager)
    } else {
        placeholder_preview(package, "Package manager not available")
    }
}

/// Render preview for a specific package manager (used by legacy generic preview).
fn render_for_manager(package: &str, manager: PackageManager) -> Result<String> {
    match manager {
        PackageManager::Apt => render_apt_impl(package),
        PackageManager::Dnf => render_dnf_impl(package),
        PackageManager::Zypper => render_zypper_impl(package),
        PackageManager::Pacman => render_pacman_impl(package),
        PackageManager::Snap => render_snap_impl(package),
        PackageManager::Pkg => render_pkg_impl(package),
        PackageManager::Flatpak => render_flatpak_impl(package),
        PackageManager::Aur => render_aur_impl(package),
        PackageManager::Cargo => render_cargo_impl(package),
    }
}

// ============================================================================
// APT
// ============================================================================

pub fn render_apt_preview(ctx: &PreviewContext) -> Result<String> {
    match package_from_context(ctx) {
        Some(pkg) => render_apt_impl(pkg),
        None => placeholder_preview("APT Package", "Select a package to see details"),
    }
}

fn render_apt_impl(package: &str) -> Result<String> {
    let output = cmd!("apt", "show", package)
        .stderr_null()
        .read()
        .unwrap_or_default();

    if output.is_empty() {
        return Ok(PreviewBuilder::new()
            .header(NerdFont::Package, package)
            .line(colors::BLUE, None, "APT Package")
            .blank()
            .subtext("No package information available")
            .build_string());
    }

    let mut builder = PreviewBuilder::new()
        .header(NerdFont::Package, package)
        .line(colors::BLUE, None, "APT Package");

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

    Ok(builder.build_string())
}

// ============================================================================
// DNF
// ============================================================================

pub fn render_dnf_preview(ctx: &PreviewContext) -> Result<String> {
    match package_from_context(ctx) {
        Some(pkg) => render_dnf_impl(pkg),
        None => placeholder_preview("DNF Package", "Select a package to see details"),
    }
}

fn render_dnf_impl(package: &str) -> Result<String> {
    let output = cmd!("dnf", "info", package)
        .stderr_null()
        .read()
        .unwrap_or_default();

    if output.is_empty() {
        return Ok(PreviewBuilder::new()
            .header(NerdFont::Package, package)
            .line(colors::YELLOW, None, "DNF Package")
            .blank()
            .subtext("No package information available")
            .build_string());
    }

    let mut builder = PreviewBuilder::new()
        .header(NerdFont::Package, package)
        .line(colors::YELLOW, None, "DNF Package");

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

    Ok(builder.build_string())
}

// ============================================================================
// Zypper
// ============================================================================

pub fn render_zypper_preview(ctx: &PreviewContext) -> Result<String> {
    match package_from_context(ctx) {
        Some(pkg) => render_zypper_impl(pkg),
        None => placeholder_preview("Zypper Package", "Select a package to see details"),
    }
}

fn render_zypper_impl(package: &str) -> Result<String> {
    let output = cmd!("zypper", "info", package)
        .stderr_null()
        .read()
        .unwrap_or_default();

    if output.is_empty() {
        return Ok(PreviewBuilder::new()
            .header(NerdFont::Package, package)
            .line(colors::RED, None, "Zypper Package")
            .blank()
            .subtext("No package information available")
            .build_string());
    }

    let mut builder = PreviewBuilder::new()
        .header(NerdFont::Package, package)
        .line(colors::RED, None, "Zypper Package");

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

    Ok(builder.build_string())
}

// ============================================================================
// Pacman
// ============================================================================

pub fn render_pacman_preview(ctx: &PreviewContext) -> Result<String> {
    match package_from_context(ctx) {
        Some(pkg) => render_pacman_impl(pkg),
        None => placeholder_preview("Pacman Package", "Select a package to see details"),
    }
}

fn render_pacman_impl(package: &str) -> Result<String> {
    let output = cmd!("pacman", "-Si", package)
        .stderr_null()
        .read()
        .unwrap_or_default();

    if output.is_empty() {
        return Ok(PreviewBuilder::new()
            .header(NerdFont::Package, package)
            .line(colors::GREEN, None, "Pacman Package")
            .blank()
            .subtext("No package information available")
            .build_string());
    }

    let mut builder = PreviewBuilder::new()
        .header(NerdFont::Package, package)
        .line(colors::GREEN, None, "Pacman Package")
        .blank();

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

    Ok(builder.build_string())
}

// ============================================================================
// Snap
// ============================================================================

pub fn render_snap_preview(ctx: &PreviewContext) -> Result<String> {
    match package_from_context(ctx) {
        Some(pkg) => render_snap_impl(pkg),
        None => placeholder_preview("Snap Package", "Select a Snap package to see details"),
    }
}

fn render_snap_impl(package_info: &str) -> Result<String> {
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

    // Fetch detailed info from snap store (async/on-demand)
    let output = cmd!("snap", "info", name)
        .stderr_null()
        .read()
        .unwrap_or_default();

    if output.is_empty() {
        return Ok(PreviewBuilder::new()
            .header(NerdFont::Package, name)
            .line(colors::PEACH, None, "Snap Package")
            .blank()
            .subtext("No package information available")
            .build_string());
    }

    let mut builder = PreviewBuilder::new()
        .header(NerdFont::Package, name)
        .line(colors::PEACH, None, "Snap Package")
        .blank();

    let mut description = String::new();
    let mut in_description = false;
    let mut size = None;

    for line in output.lines() {
        // Check for size in channels section (e.g., "latest/stable: 1.0 (100) 50MB -")
        if line.contains("latest/stable:") && !line.starts_with("latest/stable:") {
            if let Some(size_match) = line.split_whitespace().nth(3) {
                if size_match.ends_with("B") {
                    size = Some(size_match.to_string());
                }
            }
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

    Ok(builder.build_string())
}

// ============================================================================
// Pkg (Termux)
// ============================================================================

pub fn render_pkg_preview(ctx: &PreviewContext) -> Result<String> {
    match package_from_context(ctx) {
        Some(pkg) => render_pkg_impl(pkg),
        None => placeholder_preview("Pkg Package", "Select a package to see details"),
    }
}

fn render_pkg_impl(package: &str) -> Result<String> {
    let output = cmd!("pkg", "show", package)
        .stderr_null()
        .read()
        .unwrap_or_default();

    if output.is_empty() {
        return Ok(PreviewBuilder::new()
            .header(NerdFont::Package, package)
            .line(colors::TEAL, None, "Pkg Package")
            .blank()
            .subtext("No package information available")
            .build_string());
    }

    let mut builder = PreviewBuilder::new()
        .header(NerdFont::Package, package)
        .line(colors::TEAL, None, "Pkg Package");

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

    Ok(builder.build_string())
}

// ============================================================================
// Flatpak
// ============================================================================

pub fn render_flatpak_preview(ctx: &PreviewContext) -> Result<String> {
    match package_from_context(ctx) {
        Some(pkg) => render_flatpak_impl(pkg),
        None => placeholder_preview("Flatpak Package", "Select a Flatpak package to see details"),
    }
}

fn render_flatpak_impl(package_info: &str) -> Result<String> {
    // Extract app_id from "app_id\tname\tdescription" format
    let package = if package_info.contains('\t') {
        package_info.split('\t').next().unwrap_or(package_info)
    } else {
        package_info
    };

    // Try to get remote info from any available remote
    let remotes_output = cmd!("flatpak", "remotes", "--columns=name")
        .stderr_null()
        .read()
        .unwrap_or_default();

    let remotes: Vec<&str> = remotes_output.lines().collect();

    // Try each remote to find package info
    let mut remote_output = None;
    for remote in &remotes {
        if let Ok(output) = cmd!("flatpak", "remote-info", remote.trim(), package)
            .stderr_null()
            .read()
        {
            if !output.is_empty() {
                remote_output = Some(output);
                break;
            }
        }
    }

    // Also try to get local info if installed
    let local_output = cmd!("flatpak", "info", package).stderr_null().read().ok();

    // Use remote info as primary source, fallback to local
    let output = remote_output.as_deref().or(local_output.as_deref());

    if output.is_none() {
        return Ok(PreviewBuilder::new()
            .header(NerdFont::Package, package)
            .line(colors::PINK, None, "Flatpak Package")
            .blank()
            .subtext("No package information available")
            .build_string());
    }

    let mut builder = PreviewBuilder::new()
        .header(NerdFont::Package, package)
        .line(colors::PINK, None, "Flatpak Package")
        .blank();

    for line in output.unwrap().lines() {
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
                "Installed" => builder = builder.field("Installed Size", value),
                _ => {
                    if line.starts_with("Description:") || line.starts_with("Summary:") {
                        builder = builder.text(value);
                    }
                }
            }
        }
    }

    Ok(builder.build_string())
}

// ============================================================================
// AUR
// ============================================================================

pub fn render_aur_preview(ctx: &PreviewContext) -> Result<String> {
    match package_from_context(ctx) {
        Some(pkg) => render_aur_impl(pkg),
        None => placeholder_preview("AUR Package", "Select an AUR package to see details"),
    }
}

fn render_aur_impl(package: &str) -> Result<String> {
    let helper = detect_aur_helper().unwrap_or("yay");
    let output = cmd!(helper, "-Si", package)
        .stderr_null()
        .read()
        .unwrap_or_default();

    if output.is_empty() {
        return Ok(PreviewBuilder::new()
            .header(NerdFont::Package, package)
            .line(colors::MAUVE, None, "AUR Package")
            .blank()
            .subtext("No package information available")
            .build_string());
    }

    let mut builder = PreviewBuilder::new()
        .header(NerdFont::Package, package)
        .line(colors::MAUVE, None, "AUR Package")
        .blank();

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

    Ok(builder.build_string())
}

// ============================================================================
// Cargo
// ============================================================================

pub fn render_cargo_preview(ctx: &PreviewContext) -> Result<String> {
    match package_from_context(ctx) {
        Some(pkg) => render_cargo_impl(pkg),
        None => placeholder_preview("Cargo Package", "Select a Cargo package to see details"),
    }
}

fn render_cargo_impl(package: &str) -> Result<String> {
    let output = cmd!("cargo", "show", package)
        .stderr_null()
        .read()
        .unwrap_or_default();

    if output.is_empty() {
        return Ok(PreviewBuilder::new()
            .header(NerdFont::Package, package)
            .line(colors::MAROON, None, "Cargo Package")
            .blank()
            .subtext("No package information available")
            .build_string());
    }

    let mut builder = PreviewBuilder::new()
        .header(NerdFont::Package, package)
        .line(colors::MAROON, None, "Cargo Package");

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

    Ok(builder.build_string())
}

// ============================================================================
// Generic fallback
// ============================================================================

fn render_generic_package(package: &str, source: &str) -> Result<String> {
    Ok(PreviewBuilder::new()
        .header(NerdFont::Package, package)
        .subtext(&format!("{} Package", source))
        .blank()
        .subtext("Preview not available for this package type")
        .build_string())
}
