use super::system_configuration::{InitramfsConfiguration, SystemConfiguration};
use crate::arch::arch_linux_variant::ArchLinuxVariant;
use crate::arch::engine::{BootMode, GpuKind, InstallInputs, Kernel};
use std::collections::HashSet;

fn package_names(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

/// Deduplicate a list of strings while preserving the first occurrence order.
fn dedup_preserve(vec: &mut Vec<String>) {
    let mut seen = HashSet::new();
    vec.retain(|s| seen.insert(s.clone()));
}

/// The standard package batch for fresh installations.
/// This collects:
/// - Extended/system packages (drivers, tools, DE) derived from answers and detected hardware
/// - Config-required packages (encryption, plymouth)
/// - Bootloader packages (grub, efibootmgr/os-prober)
///
/// Note: instantOS packages (instantdepend, instantos, instantextra) are NOT included here.
/// They are selected separately from `INSTANT_PACKAGES` to allow `ins arch setup`
/// to work on existing Arch installations without reinstalling standard packages.
pub(super) fn standard_packages(
    inputs: &InstallInputs,
    system: &SystemConfiguration,
    initramfs: &InitramfsConfiguration,
) -> Vec<String> {
    let mut packages = collect_extended_packages(inputs, system);
    packages.extend(config_package_list(inputs, initramfs));
    packages.extend(bootloader_package_list(inputs));
    dedup_preserve(&mut packages);
    packages
}

/// Fixed instantOS packages from the [instant] repository.
/// These packages are installed by both:
/// - `ins arch install` (in Post step, after [instant] repo is configured)
/// - `ins arch setup` (on existing Arch installations converting to instantOS)
pub(super) const INSTANT_PACKAGES: &[&str] = &["instantdepend", "instantos", "instantextra"];

fn collect_language_packages(inputs: &InstallInputs) -> Vec<String> {
    let mut packages = Vec::new();

    {
        let locale = inputs.locale.as_str();
        let locale_lower = locale.to_lowercase();
        // Extract language and region/country
        // e.g. "de_DE.UTF-8" -> "de_de"
        let lang_and_country = locale_lower.split('.').next().unwrap_or(&locale_lower);
        // e.g. "de_de" -> "de"
        let lang_code = lang_and_country
            .split('_')
            .next()
            .unwrap_or(lang_and_country);

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

fn collect_extended_packages(inputs: &InstallInputs, system: &SystemConfiguration) -> Vec<String> {
    let minimal_mode = inputs.minimal_mode;
    let kernel = inputs.kernel;
    let linux_variant = inputs.linux_variant;

    let mut packages: Vec<String> = package_names(&[
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

    packages.extend(system.services().init_packages().iter().cloned());
    packages.extend(
        system
            .services()
            .daemon_packages()
            .map(|package| package.package_name.to_owned()),
    );
    // The utilities are useful even where native guest-service activation is unavailable.
    if linux_variant.is_artix() && inputs.system_info.vm_type.as_deref() == Some("oracle") {
        packages.push("virtualbox-guest-utils".to_string());
    }
    if system.repositories().arch_support() {
        packages.push("artix-archlinux-support".to_string());
    }

    // Kernel headers are only needed to build out-of-tree modules. Whether a
    // GPU/driver pair needs them is decided next to the driver-package list
    // in GpuKind, so this site stays oblivious to which kernels require
    // headers and which do not.
    if let Some(headers) = inputs
        .system_info
        .gpus
        .iter()
        .filter_map(|gpu| gpu_kernel_headers(gpu, kernel, linux_variant))
        .next()
    {
        packages.push(headers);
    }

    // Standard Arch desktop packages
    // Note: instantOS packages are selected separately from INSTANT_PACKAGES
    if !minimal_mode {
        let desktop = inputs.desktop;

        if desktop.requires_display_manager() {
            packages.push("xorg-xwayland".to_string());
            let dm = inputs.display_manager;
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

            let use_xorg = inputs.use_xorg;
            if use_xorg || dm == crate::arch::config::DisplayManager::Lightdm {
                packages.push("xorg-server".to_string());
            }
        }

        packages.extend(package_names(desktop.package_names()));
        packages.extend(package_names(desktop.font_packages()));
    }

    // GPU packages (after multilib is enabled)
    let mut seen_gpus = HashSet::new();
    for gpu in &inputs.system_info.gpus {
        if !seen_gpus.insert(std::mem::discriminant(gpu)) {
            continue;
        }

        packages.extend(gpu_packages(gpu, kernel, linux_variant));
    }

    if !minimal_mode {
        let desktop = inputs.desktop;
        if desktop.requires_display_manager() {
            packages.extend(collect_language_packages(inputs));
        }
    }

    packages
}

/// Driver packages for `gpu` on `kernel` and `linux_variant`.
///
/// Artix does not package `nvidia-open-lts`, so the LTS kernel uses the DKMS
/// module there. Arch keeps its prebuilt LTS module.
fn gpu_packages(gpu: &GpuKind, kernel: Kernel, linux_variant: ArchLinuxVariant) -> Vec<String> {
    if linux_variant.is_artix() && matches!(gpu, GpuKind::Nvidia) && kernel == Kernel::Lts {
        return package_names(&[
            "nvidia-open-dkms",
            "dkms",
            "nvidia-utils",
            "nvidia-settings",
        ]);
    }

    gpu.get_driver_packages(Some(kernel))
        .into_iter()
        .map(String::from)
        .collect()
}

/// Kernel headers required by the driver selected for `gpu`.
///
/// The DKMS fallback used for Nvidia on Artix's LTS kernel needs the LTS
/// headers, which the Arch-oriented `GpuKind` helper does not ask for.
fn gpu_kernel_headers(
    gpu: &GpuKind,
    kernel: Kernel,
    linux_variant: ArchLinuxVariant,
) -> Option<String> {
    if linux_variant.is_artix() && matches!(gpu, GpuKind::Nvidia) && kernel == Kernel::Lts {
        return Some(format!("{}-headers", kernel.label()));
    }

    gpu.get_kernel_headers(kernel)
}

/// PCI vendor IDs of common network interface controllers and the
/// `linux-firmware-*` split that ships their firmware. Sorted by vendor ID
/// for grep-friendliness; extend by adding a row.
const NIC_VENDOR_FIRMWARE: &[(&str, &str)] = &[
    ("104c", "linux-firmware-ti"),
    ("10ec", "linux-firmware-realtek"),
    ("14c3", "linux-firmware-mediatek"),
    ("14e4", "linux-firmware-broadcom"),
    ("168c", "linux-firmware-atheros"),
    ("8086", "linux-firmware-intel"),
];

/// Firmware packages matching the detected hardware.
///
/// The `linux-firmware` meta package pulls every vendor split — NVIDIA GSP
/// images, the AMD/AMDGPU stacks and all wifi vendors — which is most of a
/// GiB of download and extraction for hardware the machine does not have.
/// Instead, install the splits matching the detected GPUs, AMD CPU platform
/// firmware and network interface vendors plus the small catch-alls. When no
/// GPU was detected at all, fall back to the full meta package: shipping a
/// system whose graphics firmware is missing is worse than a slower install.
fn firmware_packages(inputs: &InstallInputs) -> Vec<String> {
    // Small and universal: the catch-all split, laptop speaker firmware and
    // the firmware index/licence file.
    let mut packages: Vec<String> = package_names(&[
        "linux-firmware-other",
        "linux-firmware-cirrus",
        "linux-firmware-whence",
    ]);

    let mut wanted: HashSet<&str> = HashSet::new();
    for gpu in &inputs.system_info.gpus {
        match gpu {
            GpuKind::Nvidia => {
                wanted.insert("linux-firmware-nvidia");
            }
            GpuKind::Amd => {
                wanted.insert("linux-firmware-amdgpu");
            }
            GpuKind::Intel => {
                wanted.insert("linux-firmware-intel");
            }
            GpuKind::Other(_) => {}
        }
    }

    // linux-firmware-amd is the AMD CPU/APU platform firmware (NPU, TEE,
    // Secure Processor). CPU microcode is a separate `amd-ucode` package
    // installed above. Keying on the CPU keeps AMD-GPU + Intel-CPU installs
    // from pulling platform firmware they will never use.
    if inputs.system_info.has_amd_cpu {
        wanted.insert("linux-firmware-amd");
    }

    for vendor in &inputs.system_info.network_vendor_ids {
        for (vid, pkg) in NIC_VENDOR_FIRMWARE {
            if vendor == vid {
                wanted.insert(pkg);
            }
        }
    }

    if inputs.system_info.gpus.is_empty() {
        return vec!["linux-firmware".to_string()];
    }

    packages.extend(wanted.into_iter().map(String::from));
    packages.sort();
    packages
}

/// Packages installed by the base bootstrap.
///
/// Artix adds the selected init, its elogind integration, and `artix-mirrorlist`,
/// which `basestrap` does not necessarily carry into the target. Arch's
/// list is unchanged.
pub(super) fn base_packages(inputs: &InstallInputs) -> Vec<String> {
    let mut packages: Vec<String> = vec!["base".to_string()];
    packages.extend(firmware_packages(inputs));

    if inputs.storage.filesystem().is_btrfs() {
        packages.push("btrfs-progs".to_string());
    }

    // Add kernel (headers are installed later alongside extra packages)
    packages.push(inputs.kernel.label().to_string());

    if inputs.system_info.has_amd_cpu {
        packages.push("amd-ucode".to_string());
    }
    if inputs.system_info.has_intel_cpu {
        packages.push("intel-ucode".to_string());
    }

    if inputs.linux_variant.is_artix() {
        packages.extend(
            inputs
                .linux_variant
                .bootstrap_init_packages()
                .iter()
                .map(|p| p.to_string()),
        );
        packages.push("artix-mirrorlist".to_string());
    }

    // GPU drivers are installed later after the native 32-bit repository is enabled,
    // allowing lib32-* packages to be installed properly.
    packages
}

/// Packages required for configuration steps (installed in a single batch elsewhere)
fn config_package_list(inputs: &InstallInputs, initramfs: &InitramfsConfiguration) -> Vec<String> {
    let mut packages = vec!["kbd".to_string()];
    if let Some(package) = inputs.console_font.legacy_package() {
        packages.push(package.to_string());
    }

    if inputs.storage.encryption().is_some() {
        packages.push("lvm2".to_string());
        packages.push("cryptsetup".to_string());
        // Artix packages the init scripts separately from the tools.
        for base in ["lvm2", "cryptsetup"] {
            if let Some(package) = inputs.linux_variant.init_service_package(base) {
                packages.push(package);
            }
        }
    }

    if initramfs.plymouth().is_some() {
        packages.push("plymouth".to_string());
    }

    packages
}

/// Packages needed for bootloader setup (installed in a single batch elsewhere)
fn bootloader_package_list(inputs: &InstallInputs) -> Vec<String> {
    let mut packages = vec!["grub".to_string(), "os-prober".to_string()];

    if matches!(
        inputs.system_info.boot_mode,
        BootMode::UEFI64 | BootMode::UEFI32
    ) {
        packages.push("efibootmgr".to_string());
    }

    packages
}

#[cfg(test)]
mod tests {
    use crate::arch::arch_linux_variant::{ArchLinuxVariant, ArtixInitSystem};
    use crate::arch::config::{DesktopEnvironment, DisplayManager};
    use crate::arch::engine::{GpuKind, InstallPlan, Kernel};

    fn base_plan() -> InstallPlan {
        crate::arch::engine::test_install_plan()
    }

    fn artix_plan(init: ArtixInitSystem) -> InstallPlan {
        let mut plan = base_plan();
        plan = plan.with_test_inputs(|inputs| {
            inputs.linux_variant = ArchLinuxVariant::Artix(init);
        });
        plan
    }

    #[test]
    fn tty_selection_skips_display_manager_packages() {
        let plan = base_plan();
        let packages = plan.standard_packages();

        assert!(!packages.iter().any(|pkg| pkg == "lightdm"));
        assert!(!packages.iter().any(|pkg| pkg == "gdm"));
        assert!(!packages.iter().any(|pkg| pkg == "sway"));
        assert!(!packages.iter().any(|pkg| pkg == "xorg-xwayland"));
    }

    #[test]
    fn hyprland_selection_adds_hyprland_and_gdm_by_default() {
        let mut plan = base_plan();
        plan = plan.with_test_inputs(|inputs| {
            inputs.desktop = DesktopEnvironment::Hyprland;
        });

        let packages = plan.standard_packages();

        assert!(packages.iter().any(|pkg| pkg == "hyprland"));
        assert!(packages.iter().any(|pkg| pkg == "gdm"));
        assert!(!packages.iter().any(|pkg| pkg == "lightdm"));
    }

    #[test]
    fn hyprland_selection_adds_hyprland_and_lightdm_when_selected() {
        let mut plan = base_plan();
        plan = plan.with_test_inputs(|inputs| {
            inputs.desktop = DesktopEnvironment::Hyprland;
        });
        plan = plan.with_test_inputs(|inputs| {
            inputs.display_manager = DisplayManager::Lightdm;
        });

        let packages = plan.standard_packages();

        assert!(packages.iter().any(|pkg| pkg == "hyprland"));
        assert!(packages.iter().any(|pkg| pkg == "lightdm"));
        assert!(packages.iter().any(|pkg| pkg == "xorg-server"));
        assert!(!packages.iter().any(|pkg| pkg == "gdm"));
    }

    #[test]
    fn hyprland_selection_with_no_display_manager_skips_dm_packages() {
        let mut plan = base_plan();
        plan = plan.with_test_inputs(|inputs| {
            inputs.desktop = DesktopEnvironment::Hyprland;
        });
        plan = plan.with_test_inputs(|inputs| {
            inputs.display_manager = DisplayManager::None;
        });

        let packages = plan.standard_packages();

        assert!(packages.iter().any(|pkg| pkg == "hyprland"));
        assert!(!packages.iter().any(|pkg| pkg == "gdm"));
        assert!(!packages.iter().any(|pkg| pkg == "lightdm"));
        assert!(!packages.iter().any(|pkg| pkg == "xorg-server"));
    }

    #[test]
    fn use_xorg_selection_adds_xorg_server() {
        let mut plan = base_plan();
        plan = plan.with_test_inputs(|inputs| {
            inputs.desktop = DesktopEnvironment::InstantWM;
        });
        plan = plan.with_test_inputs(|inputs| {
            inputs.use_xorg = true;
        });

        let packages = plan.standard_packages();

        assert!(packages.iter().any(|pkg| pkg == "xorg-server"));
        assert!(packages.iter().any(|pkg| pkg == "gdm"));
    }

    #[test]
    fn default_instantwm_skips_xorg_server() {
        let mut plan = base_plan();
        plan = plan.with_test_inputs(|inputs| {
            inputs.desktop = DesktopEnvironment::InstantWM;
        });

        let packages = plan.standard_packages();

        assert!(!packages.iter().any(|pkg| pkg == "xorg-server"));
        assert!(packages.iter().any(|pkg| pkg == "gdm"));
    }

    #[test]
    fn instantwm_installs_launcher_wallpaper_and_browser_fonts() {
        let mut plan = base_plan();
        plan = plan.with_test_inputs(|inputs| {
            inputs.desktop = DesktopEnvironment::InstantWM;
        });
        let packages = plan.standard_packages();
        for required in [
            "fuzzel",
            "swaybg",
            "noto-fonts",
            "noto-fonts-cjk",
            "noto-fonts-emoji",
            "ttf-liberation",
        ] {
            assert!(
                packages.iter().any(|pkg| pkg == required),
                "Missing {required}"
            );
        }
    }

    #[test]
    fn selecting_german_locale_adds_german_firefox_i18n_package() {
        let mut plan = base_plan();
        plan = plan.with_test_inputs(|inputs| {
            inputs.locale = crate::arch::engine::LocaleName::parse("de_DE.UTF-8").unwrap();
        });
        plan = plan.with_test_inputs(|inputs| {
            inputs.desktop = DesktopEnvironment::Sway;
        });

        let packages = plan.standard_packages();
        assert!(packages.iter().any(|pkg| pkg == "firefox-i18n-de"));
    }

    #[test]
    fn artix_plans_include_init_service_packages() {
        let plan = artix_plan(ArtixInitSystem::OpenRC);
        let packages = plan.standard_packages();

        for required in [
            "networkmanager-openrc",
            "openssh-openrc",
            "chrony",
            "chrony-openrc",
            "artix-archlinux-support",
        ] {
            assert!(
                packages.iter().any(|pkg| pkg == required),
                "Missing {required}: {packages:?}"
            );
        }
    }

    #[test]
    fn artix_minimal_mode_skips_arch_repository_support() {
        let mut plan = artix_plan(ArtixInitSystem::Runit);
        plan = plan.with_test_inputs(|inputs| {
            inputs.minimal_mode = true;
        });
        let packages = plan.standard_packages();

        assert!(!packages.iter().any(|pkg| pkg == "artix-archlinux-support"));
        assert!(packages.iter().any(|pkg| pkg == "networkmanager-runit"));
    }

    #[test]
    fn artix_display_manager_gets_its_init_package() {
        let mut plan = artix_plan(ArtixInitSystem::S6);
        plan = plan.with_test_inputs(|inputs| {
            inputs.desktop = DesktopEnvironment::Hyprland;
        });
        plan = plan.with_test_inputs(|inputs| {
            inputs.display_manager = DisplayManager::Gdm;
        });

        let packages = plan.standard_packages();

        assert!(packages.iter().any(|pkg| pkg == "gdm"));
        assert!(packages.iter().any(|pkg| pkg == "gdm-s6"));
    }

    #[test]
    fn artix_lightdm_gets_its_init_package() {
        let mut plan = artix_plan(ArtixInitSystem::Dinit);
        plan = plan.with_test_inputs(|inputs| {
            inputs.desktop = DesktopEnvironment::Sway;
        });
        plan = plan.with_test_inputs(|inputs| {
            inputs.display_manager = DisplayManager::Lightdm;
        });

        let packages = plan.standard_packages();

        assert!(packages.iter().any(|pkg| pkg == "lightdm"));
        assert!(packages.iter().any(|pkg| pkg == "lightdm-dinit"));
    }

    #[test]
    fn artix_vm_guest_agent_gets_its_init_package() {
        let mut plan = artix_plan(ArtixInitSystem::OpenRC);
        plan = plan.with_test_inputs(|inputs| {
            inputs.system_info.vm_type = Some("kvm".to_owned());
        });

        let packages = plan.standard_packages();

        assert!(packages.iter().any(|pkg| pkg == "qemu-guest-agent"));
        assert!(packages.iter().any(|pkg| pkg == "qemu-guest-agent-openrc"));
    }

    #[test]
    fn artix_virtualbox_keeps_utilities_without_native_service_activation() {
        let plan = artix_plan(ArtixInitSystem::OpenRC).with_test_inputs(|inputs| {
            inputs.system_info.vm_type = Some("oracle".to_string());
        });
        assert!(
            plan.standard_packages()
                .iter()
                .any(|package| package == "virtualbox-guest-utils")
        );
        assert!(
            !plan
                .setup()
                .services()
                .optional()
                .iter()
                .any(|service| service.service_name == "vboxservice")
        );
        assert!(
            !plan
                .setup()
                .services()
                .init_packages()
                .iter()
                .any(|package| package.starts_with("virtualbox-guest-utils-"))
        );
    }

    #[test]
    fn artix_encryption_gets_lvm_and_cryptsetup_init_packages() {
        use crate::arch::engine::{
            DiskPath, EncryptionPassword, EncryptionPlan, FilesystemPlan, StoragePlan,
        };

        let mut plan = artix_plan(ArtixInitSystem::OpenRC);
        plan = plan.with_test_inputs(|inputs| {
            inputs.storage = StoragePlan::Automatic {
                disk: DiskPath::parse("/dev/test-disk").unwrap(),
                filesystem: FilesystemPlan::Ext4,
                encryption: Some(EncryptionPlan {
                    password: EncryptionPassword::parse("test-secret").unwrap(),
                }),
            };
        });

        let packages = plan.standard_packages();

        assert!(packages.iter().any(|pkg| pkg == "lvm2-openrc"));
        assert!(packages.iter().any(|pkg| pkg == "cryptsetup-openrc"));
    }

    #[test]
    fn artix_lts_nvidia_uses_dkms_instead_of_the_missing_lts_module() {
        let mut plan = artix_plan(ArtixInitSystem::OpenRC);
        plan = plan.with_test_inputs(|inputs| {
            inputs.kernel = Kernel::Lts;
        });
        plan = plan.with_test_inputs(|inputs| {
            inputs.system_info.gpus = vec![GpuKind::Nvidia];
        });

        let packages = plan.standard_packages();

        assert!(packages.iter().any(|pkg| pkg == "nvidia-open-dkms"));
        assert!(packages.iter().any(|pkg| pkg == "dkms"));
        assert!(packages.iter().any(|pkg| pkg == "linux-lts-headers"));
        assert!(!packages.iter().any(|pkg| pkg == "nvidia-open-lts"));
    }

    #[test]
    fn arch_lts_nvidia_keeps_the_prebuilt_module() {
        let mut plan = base_plan();
        plan = plan.with_test_inputs(|inputs| {
            inputs.kernel = Kernel::Lts;
        });
        plan = plan.with_test_inputs(|inputs| {
            inputs.system_info.gpus = vec![GpuKind::Nvidia];
        });

        let packages = plan.standard_packages();

        assert!(packages.iter().any(|pkg| pkg == "nvidia-open-lts"));
        assert!(!packages.iter().any(|pkg| pkg == "nvidia-open-dkms"));
        assert!(!packages.iter().any(|pkg| pkg == "linux-lts-headers"));
    }

    #[test]
    fn artix_never_plans_plymouth() {
        let mut plan = artix_plan(ArtixInitSystem::OpenRC);
        plan = plan.with_test_inputs(|inputs| {
            inputs.use_plymouth = true;
        });

        let packages = plan.standard_packages();

        assert!(!packages.iter().any(|pkg| pkg == "plymouth"));
    }
}
#[cfg(test)]
mod firmware_tests {
    use crate::arch::engine::test_install_plan;
    use crate::arch::engine::{GpuKind, SystemInfo};

    fn plan_with(system_info: SystemInfo) -> crate::arch::engine::InstallPlan {
        let mut plan = test_install_plan();
        plan = plan.with_test_inputs(|inputs| {
            inputs.system_info = system_info;
        });
        plan
    }

    #[test]
    fn unknown_hardware_gets_the_full_meta_package() {
        // No GPU detected at all: do not gamble on missing graphics firmware.
        let plan = plan_with(SystemInfo::default());
        assert!(plan.base_packages().iter().any(|p| p == "linux-firmware"));
    }

    #[test]
    fn virtual_machine_without_passthrough_devices_skips_vendor_firmware() {
        // The e2e VM shape: a std VGA adapter and a virtio NIC, neither of
        // which maps to a firmware vendor split.
        let mut system_info = SystemInfo::default();
        system_info.gpus = vec![GpuKind::Other("QEMU Virtual Video".to_owned())];
        let plan = plan_with(system_info);

        let packages = plan.base_packages().to_vec();
        assert!(!packages.iter().any(|p| p.contains("nvidia")));
        assert!(!packages.iter().any(|p| p.contains("amd")));
        assert!(!packages.iter().any(|p| p.contains("intel")));
        assert!(packages.contains(&"linux-firmware-other".to_string()));
    }

    #[test]
    fn amd_cpu_with_amd_gpu_and_intel_nic_gets_each_split_for_its_own_reason() {
        // AMD GPU -> linux-firmware-amdgpu (graphics firmware).
        // AMD CPU  -> linux-firmware-amd (CPU/APU platform firmware).
        // Intel NIC -> linux-firmware-intel, Realtek NIC -> linux-firmware-realtek.
        let mut system_info = SystemInfo::default();
        system_info.has_amd_cpu = true;
        system_info.gpus = vec![GpuKind::Amd];
        system_info.network_vendor_ids = vec!["8086".to_owned(), "10ec".to_owned()];
        let plan = plan_with(system_info);

        let packages = plan.base_packages().to_vec();
        assert!(packages.contains(&"linux-firmware-amd".to_string()));
        assert!(packages.contains(&"linux-firmware-amdgpu".to_string()));
        assert!(packages.contains(&"linux-firmware-intel".to_string()));
        assert!(packages.contains(&"linux-firmware-realtek".to_string()));
        assert!(!packages.iter().any(|p| p.contains("nvidia")));
        assert!(!packages.iter().any(|p| p.contains("broadcom")));
    }

    #[test]
    fn amd_gpu_without_amd_cpu_skips_variant_firmware() {
        // Discrete AMD GPU + Intel CPU is a common combo. We must not pull
        // linux-firmware-amd, which is CPU/APU platform firmware.
        let mut system_info = SystemInfo::default();
        system_info.gpus = vec![GpuKind::Amd];
        let plan = plan_with(system_info);

        let packages = plan.base_packages().to_vec();
        assert!(packages.contains(&"linux-firmware-amdgpu".to_string()));
        assert!(!packages.contains(&"linux-firmware-amd".to_string()));
    }

    #[test]
    fn nvidia_gpu_gets_nvidia_firmware() {
        let mut system_info = SystemInfo::default();
        system_info.gpus = vec![GpuKind::Nvidia];
        let plan = plan_with(system_info);

        assert!(
            plan.base_packages()
                .to_vec()
                .contains(&"linux-firmware-nvidia".to_string())
        );
    }
}

#[cfg(test)]
mod base_package_tests {
    use crate::arch::arch_linux_variant::{ArchLinuxVariant, ArtixInitSystem};
    use crate::arch::engine::test_install_plan;

    #[test]
    fn arch_base_packages_are_unchanged() {
        let plan = test_install_plan();
        assert_eq!(plan.linux_variant(), ArchLinuxVariant::Arch);

        let packages = plan.base_packages().to_vec();
        assert!(packages.contains(&"base".to_string()));
        assert!(
            !packages
                .iter()
                .any(|package| package.starts_with("elogind-")
                    || package.as_str() == "artix-mirrorlist"),
            "Arch must not receive Artix packages: {packages:?}"
        );
    }

    #[test]
    fn artix_base_packages_add_the_init_and_artix_mirrorlist() {
        let mut plan = test_install_plan();
        plan = plan.with_test_inputs(|inputs| {
            inputs.linux_variant = ArchLinuxVariant::Artix(ArtixInitSystem::OpenRC);
        });

        let packages = plan.base_packages().to_vec();
        assert!(packages.contains(&"base".to_string()));
        assert!(packages.contains(&"elogind-openrc".to_string()));
        assert!(packages.contains(&"artix-mirrorlist".to_string()));
        assert!(!packages.contains(&"elogind-dinit".to_string()));
        assert_eq!(
            packages
                .iter()
                .filter(|package| package.as_str() == "artix-mirrorlist")
                .count(),
            1,
            "artix-mirrorlist must be added once: {packages:?}"
        );
    }
}
