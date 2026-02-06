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
    
    // For Arch with source info
    if let Some(src) = source {
        return render_arch_package_preview(package, src);
    }

    // For other distros, use native package manager
    if let Some(manager) = os.native_package_manager() {
        render_manager_package_preview(package, manager)
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
    
    if let Some(manager) = os.native_package_manager() {
        render_manager_package_preview(package, manager)
    } else {
        Ok(PreviewBuilder::new()
            .header(NerdFont::Package, package)
            .subtext("Package manager not available")
            .build_string())
    }
}

/// Render preview for Arch package with source info.
fn render_arch_package_preview(package: &str, source: &str) -> Result<String> {
    let (title, color, info_output) = match source {
        "aur" => {
            let helper = detect_aur_helper().unwrap_or("yay");
            let output = cmd!(helper, "-Si", package)
                .stderr_null()
                .read()
                .unwrap_or_default();
            ("AUR Package", colors::MAUVE, output)
        }
        _ => {
            let output = cmd!("pacman", "-Si", package)
                .stderr_null()
                .read()
                .unwrap_or_default();
            ("Repository Package", colors::GREEN, output)
        }
    };

    if info_output.is_empty() {
        return Ok(PreviewBuilder::new()
            .header(NerdFont::Package, package)
            .line(color, None, title)
            .blank()
            .subtext("No package information available")
            .build_string());
    }

    // Parse key fields from pacman/AUR helper output
    let mut builder = PreviewBuilder::new()
        .header(NerdFont::Package, package)
        .line(color, None, title)
        .blank();

    for line in info_output.lines() {
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

/// Render preview using a specific package manager.
fn render_manager_package_preview(package: &str, manager: PackageManager) -> Result<String> {
    let info_output = get_package_info(package, manager);

    if info_output.is_empty() {
        return Ok(PreviewBuilder::new()
            .header(NerdFont::Package, package)
            .subtext("No package information available")
            .build_string());
    }

    let mut builder = PreviewBuilder::new()
        .header(NerdFont::Package, package);

    // Parse based on package manager format
    match manager {
        PackageManager::Apt | PackageManager::Pkg => {
            for line in info_output.lines() {
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
        }
        PackageManager::Dnf => {
            for line in info_output.lines() {
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
        }
        PackageManager::Zypper => {
            for line in info_output.lines() {
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
        }
        PackageManager::Pacman => {
            for line in info_output.lines() {
                if let Some((key, value)) = line.split_once(':') {
                    let key = key.trim();
                    let value = value.trim();
                    match key {
                        "Description" => builder = builder.text(value),
                        "Version" => builder = builder.field("Version", value),
                        "Repository" => builder = builder.field("Repository", value),
                        "URL" => builder = builder.field("URL", value),
                        "Licenses" => builder = builder.field("License", value),
                        "Installed Size" => builder = builder.field("Size", value),
                        _ => {}
                    }
                }
            }
        }
        _ => {
            // Fallback: just show raw output
            builder = builder.raw(&info_output);
        }
    }

    Ok(builder.build_string())
}

/// Get package info using the appropriate command.
fn get_package_info(package: &str, manager: PackageManager) -> String {
    let result = match manager {
        PackageManager::Pacman => cmd!("pacman", "-Si", package).stderr_null().read(),
        PackageManager::Apt => cmd!("apt", "show", package).stderr_null().read(),
        PackageManager::Dnf => cmd!("dnf", "info", package).stderr_null().read(),
        PackageManager::Zypper => cmd!("zypper", "info", package).stderr_null().read(),
        PackageManager::Pkg => cmd!("pkg", "show", package).stderr_null().read(),
        _ => return String::new(),
    };

    result.unwrap_or_default()
}
