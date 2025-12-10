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

/// Build the full package plan to install inside the chroot in a single pacman call.
///
/// This collects:
/// - Extended/system packages (drivers, tools, DE) derived from answers and detected hardware
/// - Config-required packages (encryption, plymouth)
/// - Bootloader packages (grub, efibootmgr/os-prober)
pub fn build_package_plan(context: &InstallContext) -> Result<Vec<String>> {
    let mut packages = collect_extended_packages(context)?;
    packages.extend(crate::arch::execution::config::config_package_list(context));
    packages.extend(crate::arch::execution::bootloader::bootloader_package_list(
        context,
    ));

    dedup_preserve(&mut packages);
    Ok(packages)
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
        "vim",
        "nano",
        "git",
        "libgit2",
        "fzf",
        "gum",
        "base-devel",
    ]);

    packages.push(format!("{}-headers", kernel));

    if !minimal_mode {
        packages.extend(strings(&[
            "sway",
            "xorg-xwayland",
            "instantdepend",
            "instantos",
            "instantextra",
            "lightdm",
            "lightdm-gtk-greeter",
        ]));
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
