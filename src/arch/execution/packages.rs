use crate::arch::engine::{GpuKind, InstallPlan};
use anyhow::Result;
use std::collections::HashSet;

/// Small helper to turn a slice of &str into owned `String`s.
pub fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

/// Deduplicate a list of strings while preserving the first occurrence order.
pub fn dedup_preserve(vec: &mut Vec<String>) {
    let mut seen = HashSet::new();
    vec.retain(|s| seen.insert(s.clone()));
}

/// Build the standard Arch package plan for fresh installations.
///
/// This collects:
/// - Extended/system packages (drivers, tools, DE) derived from answers and detected hardware
/// - Config-required packages (encryption, plymouth)
/// - Bootloader packages (grub, efibootmgr/os-prober)
///
/// Note: instantOS packages (instantdepend, instantos, instantextra) are NOT included here.
/// They are installed separately via `build_instant_package_plan()` to allow `ins arch setup`
/// to work on existing Arch installations without reinstalling standard packages.
pub fn build_standard_package_plan(plan: &InstallPlan) -> Result<Vec<String>> {
    let mut packages = collect_extended_packages(plan)?;
    packages.extend(crate::arch::execution::config::config_package_list(plan));
    packages.extend(crate::arch::execution::bootloader::bootloader_package_list(
        plan,
    ));

    dedup_preserve(&mut packages);
    Ok(packages)
}

/// Build the instantOS package plan from the [instant] repository.
///
/// These packages are installed by both:
/// - `ins arch install` (in Post step, after [instant] repo is configured)
/// - `ins arch setup` (on existing Arch installations converting to instantOS)
pub fn build_instant_package_plan(minimal_mode: bool) -> Vec<String> {
    if minimal_mode {
        return Vec::new();
    }
    strings(&["instantdepend", "instantos", "instantextra"])
}

fn collect_language_packages(plan: &InstallPlan) -> Vec<String> {
    let mut packages = Vec::new();

    {
        let locale = plan.locale.as_str();
        let locale_lower = locale.to_lowercase();
        // Extract language and region/country
        // e.g. "de_DE.UTF-8" -> "de_de"
        let lang_and_country = locale_lower.split('.').next().unwrap_or(&locale_lower);
        // e.g. "de_de" -> "de"
        let lang_code = lang_and_country
            .split('_')
            .next()
            .unwrap_or(lang_and_country);

        // Mappings from language/locale to packages.
        // Developers can cleanly add/edit packages per language here.
        let lang_pkgs: &[&str] = match lang_code {
            "de" => &["firefox-i18n-de", "hunspell-de", "man-pages-de"],
            "fr" => &[
                "firefox-i18n-fr",
                "hunspell-fr-comprehensive",
                "man-pages-fr",
            ],
            "es" => match lang_and_country {
                "es_ar" => &["firefox-i18n-es-ar", "hunspell-es_ar", "man-pages-es"] as &[&str],
                "es_cl" => &["firefox-i18n-es-cl", "hunspell-es_cl", "man-pages-es"] as &[&str],
                "es_mx" => &["firefox-i18n-es-mx", "hunspell-es_mx", "man-pages-es"] as &[&str],
                _ => &["firefox-i18n-es-es", "hunspell-es_es", "man-pages-es"] as &[&str],
            },
            "it" => &["firefox-i18n-it", "hunspell-it", "man-pages-it"],
            "ru" => &["firefox-i18n-ru", "hunspell-ru", "man-pages-ru"],
            "ja" => &["firefox-i18n-ja"],
            "zh" => match lang_and_country {
                "zh_tw" => &["firefox-i18n-zh-tw", "man-pages-zh_tw"] as &[&str],
                _ => &["firefox-i18n-zh-cn", "man-pages-zh_cn"] as &[&str],
            },
            "pt" => match lang_and_country {
                "pt_pt" => &["firefox-i18n-pt-pt"] as &[&str],
                _ => &["firefox-i18n-pt-br", "man-pages-pt_br"] as &[&str],
            },
            "en" => match lang_and_country {
                "en_gb" => &["firefox-i18n-en-gb", "hunspell-en_gb"] as &[&str],
                "en_za" => &["firefox-i18n-en-gb", "hunspell-en_gb"] as &[&str],
                _ => &[] as &[&str],
            },
            _ => &[],
        };

        packages.extend(lang_pkgs.iter().map(|s| (*s).to_string()));
    }

    packages
}

fn collect_extended_packages(plan: &InstallPlan) -> Result<Vec<String>> {
    let minimal_mode = plan.minimal_mode;
    let kernel = plan.kernel;

    let mut packages: Vec<String> = strings(&[
        "openssh",
        "mesa",
        "polkit",
        "networkmanager",
        "sudo",
        "zsh",
        "vim",
        "nano",
        "git",
        "fzf",
        "gum",
        "base-devel",
        "xdg-user-dirs",
    ]);

    // Kernel headers are only needed to build out-of-tree modules. Whether a
    // GPU/driver pair needs them is decided next to the driver-package list
    // in GpuKind, so this site stays oblivious to which kernels require
    // headers and which do not.
    if let Some(headers) = plan
        .system_info
        .gpus
        .iter()
        .filter_map(|gpu| gpu.get_kernel_headers(kernel))
        .next()
    {
        packages.push(headers);
    }

    // Standard Arch desktop packages
    // Note: instantOS packages are installed separately via build_instant_package_plan()
    if !minimal_mode {
        let desktop = plan.desktop;

        if desktop.requires_display_manager() {
            packages.push("xorg-xwayland".to_string());
            let dm = plan.display_manager;
            match dm {
                crate::arch::config::DisplayManager::Gdm => {
                    packages.push("gdm".to_string());
                }
                crate::arch::config::DisplayManager::Lightdm => {
                    packages.push("lightdm".to_string());
                    packages.push("lightdm-gtk-greeter".to_string());
                }
                // No display manager: the user starts their GUI manually.
                crate::arch::config::DisplayManager::None => {}
            }

            let use_xorg = plan.use_xorg;
            if use_xorg || dm == crate::arch::config::DisplayManager::Lightdm {
                packages.push("xorg-server".to_string());
            }
        }

        packages.extend(strings(desktop.package_names()));
        packages.extend(strings(desktop.font_packages()));
    }

    // GPU packages (after multilib is enabled)
    let mut seen_gpus = HashSet::new();
    for gpu in &plan.system_info.gpus {
        if !seen_gpus.insert(std::mem::discriminant(gpu)) {
            continue;
        }

        match gpu {
            GpuKind::Nvidia => println!("Detected NVIDIA GPU, adding drivers"),
            GpuKind::Amd => println!("Detected AMD GPU, adding vulkan support"),
            GpuKind::Intel => println!("Detected Intel GPU, adding vulkan support"),
            GpuKind::Other(name) => {
                println!("Detected unknown GPU: {}, adding basic mesa support", name)
            }
        }

        packages.extend(
            gpu.get_driver_packages(Some(kernel))
                .into_iter()
                .map(String::from),
        );
    }

    // VM Guest Tools
    if let Some(vm_type) = &plan.system_info.vm_type {
        println!("Detected VM: {}, adding guest tools", vm_type);
        match vm_type.as_str() {
            "kvm" | "qemu" | "bochs" => packages.push("qemu-guest-agent".to_owned()),
            "vmware" => packages.push("open-vm-tools".to_owned()),
            "oracle" => packages.push("virtualbox-guest-utils".to_owned()),
            _ => println!("No specific guest tools for VM type: {}", vm_type),
        }
    }

    // Plymouth support
    if plan.use_plymouth && !minimal_mode {
        println!("Plymouth enabled, adding plymouth package");
        packages.push("plymouth".to_owned());
    }

    // Append language-specific packages (only if GUI is installed and not in minimal mode)
    if !minimal_mode {
        let desktop = plan.desktop;
        if desktop.requires_display_manager() {
            packages.extend(collect_language_packages(plan));
        }
    }

    Ok(packages)
}

#[cfg(test)]
mod tests {
    use super::build_standard_package_plan;
    use crate::arch::config::{DesktopEnvironment, DisplayManager};
    use crate::arch::engine::InstallPlan;

    fn base_plan() -> InstallPlan {
        crate::arch::engine::test_install_plan()
    }

    #[test]
    fn tty_selection_skips_display_manager_packages() {
        let plan = base_plan();
        let packages = build_standard_package_plan(&plan).unwrap();

        assert!(!packages.iter().any(|pkg| pkg == "lightdm"));
        assert!(!packages.iter().any(|pkg| pkg == "gdm"));
        assert!(!packages.iter().any(|pkg| pkg == "sway"));
        assert!(!packages.iter().any(|pkg| pkg == "xorg-xwayland"));
    }

    #[test]
    fn hyprland_selection_adds_hyprland_and_gdm_by_default() {
        let mut plan = base_plan();
        plan.desktop = DesktopEnvironment::Hyprland;

        let packages = build_standard_package_plan(&plan).unwrap();

        assert!(packages.iter().any(|pkg| pkg == "hyprland"));
        assert!(packages.iter().any(|pkg| pkg == "gdm"));
        assert!(!packages.iter().any(|pkg| pkg == "lightdm"));
    }

    #[test]
    fn hyprland_selection_adds_hyprland_and_lightdm_when_selected() {
        let mut plan = base_plan();
        plan.desktop = DesktopEnvironment::Hyprland;
        plan.display_manager = DisplayManager::Lightdm;

        let packages = build_standard_package_plan(&plan).unwrap();

        assert!(packages.iter().any(|pkg| pkg == "hyprland"));
        assert!(packages.iter().any(|pkg| pkg == "lightdm"));
        assert!(packages.iter().any(|pkg| pkg == "xorg-server"));
        assert!(!packages.iter().any(|pkg| pkg == "gdm"));
    }

    #[test]
    fn hyprland_selection_with_no_display_manager_skips_dm_packages() {
        let mut plan = base_plan();
        plan.desktop = DesktopEnvironment::Hyprland;
        plan.display_manager = DisplayManager::None;

        let packages = build_standard_package_plan(&plan).unwrap();

        assert!(packages.iter().any(|pkg| pkg == "hyprland"));
        assert!(!packages.iter().any(|pkg| pkg == "gdm"));
        assert!(!packages.iter().any(|pkg| pkg == "lightdm"));
        assert!(!packages.iter().any(|pkg| pkg == "xorg-server"));
    }

    #[test]
    fn use_xorg_selection_adds_xorg_server() {
        let mut plan = base_plan();
        plan.desktop = DesktopEnvironment::InstantWM;
        plan.use_xorg = true;

        let packages = build_standard_package_plan(&plan).unwrap();

        assert!(packages.iter().any(|pkg| pkg == "xorg-server"));
        assert!(packages.iter().any(|pkg| pkg == "gdm"));
    }

    #[test]
    fn default_instantwm_skips_xorg_server() {
        let mut plan = base_plan();
        plan.desktop = DesktopEnvironment::InstantWM;

        let packages = build_standard_package_plan(&plan).unwrap();

        assert!(!packages.iter().any(|pkg| pkg == "xorg-server"));
        assert!(packages.iter().any(|pkg| pkg == "gdm"));
    }

    #[test]
    fn selecting_german_locale_adds_german_firefox_i18n_package() {
        let mut plan = base_plan();
        plan.locale = crate::arch::engine::LocaleName::parse("de_DE.UTF-8").unwrap();
        plan.desktop = DesktopEnvironment::Sway;

        let packages = build_standard_package_plan(&plan).unwrap();
        assert!(packages.iter().any(|pkg| pkg == "firefox-i18n-de"));
    }
}
