//! Resolve compatibility once, before execution can mutate the target.

use super::services::ServiceConfiguration;
use super::{InstallInputs, SessionAnswers, SystemInfo};
use crate::arch::arch_linux_variant::ArchLinuxVariant;

/// The Plymouth theme selected by planning.
#[derive(Debug, Clone, Copy)]
pub struct PlymouthTheme {
    theme: &'static str,
}
impl PlymouthTheme {
    pub fn theme(&self) -> &'static str {
        self.theme
    }
}

#[derive(Debug, Clone, Copy)]
pub enum InitramfsConfiguration {
    Unchanged,
    Systemd {
        encryption: bool,
        btrfs: bool,
        plymouth: Option<PlymouthTheme>,
    },
    Busybox {
        encryption: bool,
        btrfs: bool,
    },
}

#[derive(Debug, Clone, Copy)]
pub enum RootUnlockMethod {
    Systemd,
    Busybox,
}

impl RootUnlockMethod {
    pub fn kernel_parameters(self, uuid: &str) -> String {
        let unlock = match self {
            Self::Systemd => format!("rd.luks.name={uuid}=cryptlvm"),
            Self::Busybox => format!("cryptdevice=UUID={uuid}:cryptlvm"),
        };
        format!("{unlock} root=/dev/mapper/instantOS-root resume=/dev/mapper/instantOS-swap")
    }
}

impl InitramfsConfiguration {
    pub(super) fn resolve(inputs: &InstallInputs, plymouth: Option<PlymouthTheme>) -> Self {
        let encryption = inputs.storage.encryption().is_some();
        let btrfs = inputs.storage.filesystem().is_btrfs();
        if !encryption && !btrfs && plymouth.is_none() {
            Self::Unchanged
        } else if inputs.linux_variant.uses_systemd_initramfs() {
            Self::Systemd {
                encryption,
                btrfs,
                plymouth,
            }
        } else {
            Self::Busybox { encryption, btrfs }
        }
    }
    /// GRUB's unlock syntax follows the selected initramfs implementation.
    pub fn encryption(&self) -> Option<RootUnlockMethod> {
        match self {
            Self::Systemd {
                encryption: true, ..
            } => Some(RootUnlockMethod::Systemd),
            Self::Busybox {
                encryption: true, ..
            } => Some(RootUnlockMethod::Busybox),
            _ => None,
        }
    }
    pub fn plymouth(&self) -> Option<&PlymouthTheme> {
        match self {
            Self::Systemd { plymouth, .. } => plymouth.as_ref(),
            Self::Busybox { .. } | Self::Unchanged => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RepositoryConfiguration {
    native_32bit: &'static str,
    additional_arch: &'static [&'static str],
}
impl RepositoryConfiguration {
    fn resolve(variant: ArchLinuxVariant, minimal: bool) -> Self {
        Self {
            native_32bit: if variant.is_artix() {
                "lib32"
            } else {
                "multilib"
            },
            additional_arch: if minimal {
                &[]
            } else {
                variant.additional_arch_repositories()
            },
        }
    }
    pub fn native_32bit(&self) -> &'static str {
        self.native_32bit
    }
    pub fn additional_arch(&self) -> &'static [&'static str] {
        self.additional_arch
    }
    pub fn arch_support(&self) -> bool {
        !self.additional_arch.is_empty()
    }
}

/// Common resolved decisions for fresh installation and existing-system setup.
#[derive(Debug, Clone)]
pub struct SystemConfiguration {
    repositories: RepositoryConfiguration,
    services: ServiceConfiguration,
    plymouth: Option<PlymouthTheme>,
    instant_packages: Vec<String>,
}
impl SystemConfiguration {
    pub(crate) fn resolve(
        variant: ArchLinuxVariant,
        session: &SessionAnswers,
        info: &SystemInfo,
    ) -> Self {
        let plymouth = (session.use_plymouth
            && !session.minimal_mode
            && variant.supports_plymouth_configuration())
        .then_some(PlymouthTheme { theme: "instantos" });
        let mut instant_packages = Vec::new();
        if !session.minimal_mode {
            instant_packages.extend(
                super::packages::INSTANT_PACKAGES
                    .iter()
                    .map(|name| (*name).to_owned()),
            );
            if session.use_xorg {
                instant_packages.push("xorg-server".to_string());
            }
            if info.has_bluetooth {
                instant_packages.push("blueman".to_string());
            }
        }
        Self {
            repositories: RepositoryConfiguration::resolve(variant, session.minimal_mode),
            services: ServiceConfiguration::resolve(
                variant,
                session.desktop,
                session.display_manager,
                session.minimal_mode,
                info.vm_type.as_deref(),
            ),
            plymouth,
            instant_packages,
        }
    }
    pub fn repositories(&self) -> &RepositoryConfiguration {
        &self.repositories
    }
    pub fn services(&self) -> &ServiceConfiguration {
        &self.services
    }
    pub fn plymouth(&self) -> Option<&PlymouthTheme> {
        self.plymouth.as_ref()
    }
    pub fn instant_packages(&self) -> &[String] {
        &self.instant_packages
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arch::arch_linux_variant::ArtixInitSystem;
    use crate::arch::config::{DesktopEnvironment, DisplayManager};
    use crate::arch::engine::{
        EncryptionPassword, EncryptionPlan, FilesystemPlan, StoragePlan, test_install_plan,
    };

    #[test]
    fn resolved_packages_boot_and_services_agree_across_variants_and_profiles() {
        for variant in [
            ArchLinuxVariant::Arch,
            ArchLinuxVariant::Artix(ArtixInitSystem::OpenRC),
            ArchLinuxVariant::Artix(ArtixInitSystem::Runit),
            ArchLinuxVariant::Artix(ArtixInitSystem::S6),
            ArchLinuxVariant::Artix(ArtixInitSystem::Dinit),
        ] {
            for minimal in [false, true] {
                for plymouth_requested in [false, true] {
                    for encrypted in [false, true] {
                        let plan = test_install_plan().with_test_inputs(|inputs| {
                            inputs.linux_variant = variant;
                            inputs.minimal_mode = minimal;
                            inputs.use_plymouth = plymouth_requested;
                            inputs.desktop = DesktopEnvironment::Sway;
                            inputs.display_manager = DisplayManager::Lightdm;
                            inputs.system_info.vm_type = Some("kvm".to_string());
                            inputs.storage = StoragePlan::Automatic {
                                disk: inputs.storage.disk().clone(),
                                filesystem: FilesystemPlan::Ext4,
                                encryption: encrypted.then(|| EncryptionPlan {
                                    password: EncryptionPassword::parse("secret").unwrap(),
                                }),
                            };
                        });
                        let boot = plan.initramfs();
                        let system = plan.setup();
                        let packages = plan.standard_packages();
                        let contains = |name: &str| packages.iter().any(|p| p == name);
                        let plymouth =
                            variant == ArchLinuxVariant::Arch && !minimal && plymouth_requested;
                        assert_eq!(boot.plymouth().is_some(), plymouth);
                        assert_eq!(system.plymouth().is_some(), plymouth);
                        assert_eq!(contains("plymouth"), plymouth);
                        assert_eq!(boot.encryption().is_some(), encrypted);
                        if let Some(encryption) = boot.encryption() {
                            let param = encryption.kernel_parameters("test-uuid");
                            match boot {
                                InitramfsConfiguration::Systemd { .. } => {
                                    assert!(param.starts_with("rd.luks.name="))
                                }
                                InitramfsConfiguration::Busybox { .. } => {
                                    assert!(param.starts_with("cryptdevice="))
                                }
                                InitramfsConfiguration::Unchanged => {
                                    panic!("encrypted root needs initramfs configuration")
                                }
                            }
                        }
                        assert_eq!(
                            system.repositories().arch_support(),
                            variant.is_artix() && !minimal
                        );
                        assert_eq!(
                            contains("artix-archlinux-support"),
                            system.repositories().arch_support()
                        );
                        assert_eq!(system.services().display_manager().is_some(), !minimal);
                        assert_eq!(contains("lightdm"), !minimal);
                        assert_eq!(system.instant_packages().is_empty(), minimal);
                        assert_eq!(contains("systemd"), variant == ArchLinuxVariant::Arch);
                        for package in system.services().daemon_packages() {
                            assert!(
                                contains(package.package_name),
                                "missing daemon package {} for {variant:?}",
                                package.package_name
                            );
                        }
                        for package in system.services().init_packages() {
                            assert!(contains(package), "missing {package} for {variant:?}");
                        }
                        assert!(
                            system
                                .services()
                                .optional()
                                .iter()
                                .any(|service| service.service_name == "qemu-guest-agent")
                        );
                        if variant.is_artix() {
                            assert!(!matches!(boot, InitramfsConfiguration::Systemd { .. }));
                            assert_eq!(system.repositories().native_32bit(), "lib32");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn changing_inputs_does_not_mutate_the_original_plan() {
        let original = test_install_plan().with_test_inputs(|inputs| {
            inputs.use_plymouth = true;
        });
        let changed = original.clone().with_test_inputs(|inputs| {
            inputs.linux_variant = ArchLinuxVariant::Artix(ArtixInitSystem::Dinit);
        });
        assert!(original.initramfs().plymouth().is_some());
        assert!(changed.initramfs().plymouth().is_none());
        assert!(original.standard_packages().iter().any(|p| p == "plymouth"));
        assert!(!changed.standard_packages().iter().any(|p| p == "plymouth"));
    }
}
