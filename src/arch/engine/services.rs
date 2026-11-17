use crate::arch::arch_linux_variant::ArchLinuxVariant;
use crate::arch::config::{DesktopEnvironment, DisplayManager};
use crate::common::package::{PackageDefinition, PackageManager};

/// A daemon and its separately packaged Artix init integration.
#[derive(Debug, Clone, Copy)]
pub struct SystemService {
    pub service_name: &'static str,
    daemon_package: PackageDefinition,
}

fn required_system_services(linux_variant: ArchLinuxVariant) -> [SystemService; 3] {
    [
        SystemService {
            service_name: "NetworkManager",
            daemon_package: PackageDefinition::new("networkmanager", PackageManager::Pacman),
        },
        SystemService {
            service_name: "sshd",
            daemon_package: PackageDefinition::new("openssh", PackageManager::Pacman),
        },
        if linux_variant.is_systemd() {
            SystemService {
                service_name: "systemd-timesyncd",
                daemon_package: PackageDefinition::new("systemd", PackageManager::Pacman),
            }
        } else {
            SystemService {
                service_name: "chrony",
                daemon_package: PackageDefinition::new("chrony", PackageManager::Pacman),
            }
        },
    ]
}

fn display_manager_service(dm: crate::arch::config::DisplayManager) -> Option<SystemService> {
    use crate::arch::config::DisplayManager;
    match dm {
        DisplayManager::Gdm => Some(SystemService {
            service_name: "gdm",
            daemon_package: PackageDefinition::new("gdm", PackageManager::Pacman),
        }),
        DisplayManager::Lightdm => Some(SystemService {
            service_name: "lightdm",
            daemon_package: PackageDefinition::new("lightdm", PackageManager::Pacman),
        }),
        DisplayManager::None => None,
    }
}

fn vm_guest_service(linux_variant: ArchLinuxVariant, vm: Option<&str>) -> Option<SystemService> {
    match vm {
        Some("kvm" | "qemu" | "bochs") => Some(SystemService {
            service_name: "qemu-guest-agent",
            daemon_package: PackageDefinition::new("qemu-guest-agent", PackageManager::Pacman),
        }),
        Some("vmware") if linux_variant.is_systemd() => Some(SystemService {
            service_name: "vmtoolsd",
            daemon_package: PackageDefinition::new("open-vm-tools", PackageManager::Pacman),
        }),
        Some("oracle") if linux_variant.is_systemd() => Some(SystemService {
            service_name: "vboxservice",
            daemon_package: PackageDefinition::new(
                "virtualbox-guest-utils",
                PackageManager::Pacman,
            ),
        }),
        _ => None,
    }
}

/// Services and matching package integrations resolved together.
#[derive(Debug, Clone)]
pub struct ServiceConfiguration {
    required: Vec<SystemService>,
    optional: Vec<SystemService>,
    display_manager: Option<SystemService>,
    init_packages: Vec<String>,
    user_ssh_agent: bool,
}

impl ServiceConfiguration {
    pub(super) fn resolve(
        variant: ArchLinuxVariant,
        desktop: DesktopEnvironment,
        dm: DisplayManager,
        minimal: bool,
        vm: Option<&str>,
    ) -> Self {
        let required = required_system_services(variant).to_vec();
        let optional: Vec<_> = vm_guest_service(variant, vm).into_iter().collect();
        let display_manager = (!minimal && desktop.requires_display_manager())
            .then(|| display_manager_service(dm))
            .flatten();
        let init_packages = required
            .iter()
            .chain(&optional)
            .chain(display_manager.iter())
            .filter_map(|service| variant.init_service_package(service.daemon_package.package_name))
            .collect();
        Self {
            required,
            optional,
            display_manager,
            init_packages,
            user_ssh_agent: variant.is_systemd(),
        }
    }
    pub fn daemon_packages(&self) -> impl Iterator<Item = &PackageDefinition> {
        self.required
            .iter()
            .chain(&self.optional)
            .chain(self.display_manager.iter())
            .map(|service| &service.daemon_package)
    }
    pub fn required(&self) -> &[SystemService] {
        &self.required
    }
    pub fn optional(&self) -> &[SystemService] {
        &self.optional
    }
    pub fn display_manager(&self) -> Option<SystemService> {
        self.display_manager
    }
    pub fn init_packages(&self) -> &[String] {
        &self.init_packages
    }
    pub fn user_ssh_agent(&self) -> bool {
        self.user_ssh_agent
    }
}
