//! Package preview rendering

use anyhow::Result;
use duct::cmd;

use crate::common::distro::OperatingSystem;
use crate::common::package::{PackageManager, detect_aur_helper};
use crate::ui::catppuccin::colors;
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

use super::PreviewContext;

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
        return Ok(PreviewBuilder::new()
            .header(NerdFont::Package, "Package Info")
            .subtext("Select a package to see details")
            .build_string());
    }

    let os = OperatingSystem::detect();

    // For packages with source info
    if let Some(src) = source {
        if let Ok(manager) = src.parse::<PackageManager>() {
            return render_package_manager_preview(package, manager);
        } else {
            return render_generic_package_preview(package, src);
        }
    }

    // For other distros, use native package manager
    if let Some(manager) = os.native_package_manager() {
        render_package_manager_preview(package, manager)
    } else {
        Ok(PreviewBuilder::new()
            .header(NerdFont::Package, package)
            .subtext("Package manager not available")
            .build_string())
    }
}

/// Render preview for an installed package (uninstall context).
pub fn render_installed_package_preview(ctx: &PreviewContext) -> Result<String> {
    let package = ctx.key().unwrap_or_default();

    if package.is_empty() {
        return Ok(PreviewBuilder::new()
            .header(NerdFont::Package, "Package Info")
            .subtext("Select a package to see details")
            .build_string());
    }

    let os = OperatingSystem::detect();

    // Check if this has a source prefix
    if package.contains('\t') {
        if let Some((src, pkg)) = package.split_once('\t') {
            if let Ok(manager) = src.parse::<PackageManager>() {
                return render_package_manager_preview(pkg, manager);
            } else {
                return render_generic_package_preview(pkg, src);
            }
        }
    }

    if let Some(manager) = os.native_package_manager() {
        render_package_manager_preview(package, manager)
    } else {
        Ok(PreviewBuilder::new()
            .header(NerdFont::Package, package)
            .subtext("Package manager not available")
            .build_string())
    }
}

/// Render preview for a snap package.
fn render_snap_package_preview(package_info: &str) -> Result<String> {
    // package_info can be "name\tversion\tpublisher\tsummary" or just a line starting with name
    let parts: Vec<&str> = package_info.split('\t').collect();

    if parts.len() >= 4 {
        let name = parts[0];
        let version = parts[1];
        let publisher = parts[2];
        let summary = parts[3];

        return Ok(PreviewBuilder::new()
            .header(NerdFont::Package, name)
            .line(colors::PEACH, None, "Snap Package")
            .blank()
            .field("Version", version)
            .field("Publisher", publisher)
            .blank()
            .text(summary)
            .build_string());
    }

    // If we only have the name (or a line starting with it), use the full snap info
    let name = package_info
        .split_whitespace()
        .next()
        .unwrap_or(package_info);
    render_package_manager_preview(name, PackageManager::Snap)
}

/// Render preview for AUR package.
fn render_aur_package_preview(package: &str) -> Result<String> {
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

    // Parse key fields from AUR helper output
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

/// Render preview for pacman package.
fn render_pacman_package_preview(package: &str) -> Result<String> {
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

    // Parse key fields from pacman output
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

/// Render preview for apt package.
fn render_apt_package_preview(package: &str) -> Result<String> {
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

/// Render preview for dnf package.
fn render_dnf_package_preview(package: &str) -> Result<String> {
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

/// Render preview for zypper package.
fn render_zypper_package_preview(package: &str) -> Result<String> {
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

/// Render preview using a specific package manager.
fn render_package_manager_preview(package: &str, manager: PackageManager) -> Result<String> {
    match manager {
        PackageManager::Apt => render_apt_package_preview(package),
        PackageManager::Dnf => render_dnf_package_preview(package),
        PackageManager::Zypper => render_zypper_package_preview(package),
        PackageManager::Pacman => render_pacman_package_preview(package),
        PackageManager::Snap => render_snap_package_preview(package),
        PackageManager::Pkg => render_pkg_package_preview(package),
        PackageManager::Flatpak => render_flatpak_package_preview(package),
        PackageManager::Aur => render_aur_package_preview(package), // This might duplicate the aur source prefix path
        PackageManager::Cargo => render_cargo_package_preview(package),
    }
}

/// Render preview for pkg package (Termux).
fn render_pkg_package_preview(package: &str) -> Result<String> {
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

/// Render preview for flatpak package.
fn render_flatpak_package_preview(package: &str) -> Result<String> {
    let output = cmd!("flatpak", "info", package)
        .stderr_null()
        .read()
        .unwrap_or_default();

    if output.is_empty() {
        return Ok(PreviewBuilder::new()
            .header(NerdFont::Package, package)
            .line(colors::PINK, None, "Flatpak Package")
            .blank()
            .subtext("No package information available")
            .build_string());
    }

    let mut builder = PreviewBuilder::new()
        .header(NerdFont::Package, package)
        .line(colors::PINK, None, "Flatpak Package");

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
                _ => {
                    // For description-like fields that don't have a colon
                    if line.starts_with("Description:") || line.starts_with("Summary:") {
                        builder = builder.text(value);
                    }
                }
            }
        }
    }

    Ok(builder.build_string())
}

/// Render preview for cargo package.
fn render_cargo_package_preview(package: &str) -> Result<String> {
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

/// Render preview for a generic package with custom source.
fn render_generic_package_preview(package: &str, source: &str) -> Result<String> {
    Ok(PreviewBuilder::new()
        .header(NerdFont::Package, package)
        .subtext(&format!("{} Package", source))
        .blank()
        .subtext("Preview not available for this package type")
        .build_string())
}
