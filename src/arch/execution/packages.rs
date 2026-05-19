use crate::arch::engine::{GpuKind, InstallContext, QuestionId};
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
pub fn build_standard_package_plan(context: &InstallContext) -> Result<Vec<String>> {
    let mut packages = collect_extended_packages(context)?;
    packages.extend(crate::arch::execution::config::config_package_list(context));
    packages.extend(crate::arch::execution::bootloader::bootloader_package_list(
        context,
    ));

    dedup_preserve(&mut packages);
    Ok(packages)
}

/// Build the instantOS package plan from the [instant] repository.
///
/// These packages are installed by both:
/// - `ins arch install` (in Post step, after [instant] repo is configured)
/// - `ins arch setup` (on existing Arch installations converting to instantOS)
pub fn build_instant_package_plan(context: &InstallContext) -> Vec<String> {
    if context.get_answer_bool(QuestionId::MinimalMode) {
        return Vec::new();
    }
    strings(&["instantdepend", "instantos", "instantextra"])
}

fn collect_extended_packages(context: &InstallContext) -> Result<Vec<String>> {
    let minimal_mode = context.get_answer_bool(QuestionId::MinimalMode);

    let kernel = context
        .get_answer(&QuestionId::Kernel)
        .map(|s| s.as_str())
        .unwrap_or("linux");

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
    ]);

    packages.push(format!("{}-headers", kernel));

    // Standard Arch desktop packages
    // Note: instantOS packages are installed separately via build_instant_package_plan()
    if !minimal_mode {
        let desktop = crate::arch::config::DesktopEnvironment::from_context(context);

        if desktop.requires_display_manager() {
            packages.extend(strings(&[
                "xorg-xwayland",
                "lightdm",
                "lightdm-gtk-greeter",
            ]));
        }

        packages.extend(strings(desktop.package_names()));
    }

    // GPU packages (after multilib is enabled)
    let mut seen_gpus = HashSet::new();
    for gpu in &context.system_info.gpus {
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
    if let Some(vm_type) = &context.system_info.vm_type {
        println!("Detected VM: {}, adding guest tools", vm_type);
        match vm_type.as_str() {
            "kvm" | "qemu" | "bochs" => packages.push("qemu-guest-agent".to_owned()),
            "vmware" => packages.push("open-vm-tools".to_owned()),
            "oracle" => packages.push("virtualbox-guest-utils".to_owned()),
            _ => println!("No specific guest tools for VM type: {}", vm_type),
        }
    }

    // Plymouth support
    if context.get_answer_bool(QuestionId::UsePlymouth) && !minimal_mode {
        println!("Plymouth enabled, adding plymouth package");
        packages.push("plymouth".to_owned());
    }

    Ok(packages)
}

#[cfg(test)]
mod tests {
    use super::build_standard_package_plan;
    use crate::arch::config::DesktopEnvironment;
    use crate::arch::engine::{BootMode, InstallContext, QuestionId};

    fn base_context() -> InstallContext {
        let mut context = InstallContext::new();
        context.system_info.boot_mode = BootMode::UEFI64;
        context.set_answer(QuestionId::Kernel, "linux".to_string());
        context.set_answer(
            QuestionId::DesktopEnvironment,
            DesktopEnvironment::Tty.answer_value().to_string(),
        );
        context
    }

    #[test]
    fn tty_selection_skips_display_manager_packages() {
        let context = base_context();
        let packages = build_standard_package_plan(&context).unwrap();

        assert!(!packages.iter().any(|pkg| pkg == "lightdm"));
        assert!(!packages.iter().any(|pkg| pkg == "sway"));
        assert!(!packages.iter().any(|pkg| pkg == "xorg-xwayland"));
    }

    #[test]
    fn hyprland_selection_adds_hyprland_and_lightdm() {
        let mut context = base_context();
        context.set_answer(
            QuestionId::DesktopEnvironment,
            DesktopEnvironment::Hyprland.answer_value().to_string(),
        );

        let packages = build_standard_package_plan(&context).unwrap();

        assert!(packages.iter().any(|pkg| pkg == "hyprland"));
        assert!(packages.iter().any(|pkg| pkg == "lightdm"));
    }
}
