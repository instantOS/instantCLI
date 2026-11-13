//! Configure services for the next boot using the target init's supported tools.
//! s6 uses s6-frontend, with one commit/install after the entire batch. In a
//! fresh target chroot, `--init` installs the database without contacting init.

use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result};

use super::CommandRunner;
use crate::arch::arch_linux_variant::{ArchLinuxVariant, ArtixInitSystem};

/// A daemon and its separately packaged Artix init integration.
#[derive(Clone, Copy)]
pub struct SystemService {
    pub service_name: &'static str,
    daemon_package: &'static str,
}

pub fn required_system_services(linux_variant: ArchLinuxVariant) -> [SystemService; 3] {
    [
        SystemService {
            service_name: "NetworkManager",
            daemon_package: "networkmanager",
        },
        SystemService {
            service_name: "sshd",
            daemon_package: "openssh",
        },
        if linux_variant.is_systemd() {
            SystemService {
                service_name: "systemd-timesyncd",
                daemon_package: "systemd",
            }
        } else {
            SystemService {
                service_name: "chrony",
                daemon_package: "chrony",
            }
        },
    ]
}

pub fn display_manager_service(dm: crate::arch::config::DisplayManager) -> Option<SystemService> {
    use crate::arch::config::DisplayManager;
    match dm {
        DisplayManager::Gdm => Some(SystemService {
            service_name: "gdm",
            daemon_package: "gdm",
        }),
        DisplayManager::Lightdm => Some(SystemService {
            service_name: "lightdm",
            daemon_package: "lightdm",
        }),
        DisplayManager::None => None,
    }
}

pub fn vm_guest_service(
    linux_variant: ArchLinuxVariant,
    vm: Option<&str>,
) -> Option<SystemService> {
    match vm {
        Some("kvm" | "qemu" | "bochs") => Some(SystemService {
            service_name: "qemu-guest-agent",
            daemon_package: "qemu-guest-agent",
        }),
        Some("vmware") if linux_variant.is_systemd() => Some(SystemService {
            service_name: "vmtoolsd",
            daemon_package: "open-vm-tools",
        }),
        Some("oracle") if linux_variant.is_systemd() => Some(SystemService {
            service_name: "vboxservice",
            daemon_package: "virtualbox-guest-utils",
        }),
        _ => None,
    }
}

/// Shared by fresh installation planning and conversion of an existing system.
pub fn artix_init_service_packages(
    linux_variant: ArchLinuxVariant,
    desktop: crate::arch::config::DesktopEnvironment,
    dm: crate::arch::config::DisplayManager,
    minimal: bool,
    vm: Option<&str>,
) -> Vec<String> {
    if !linux_variant.is_artix() {
        return Vec::new();
    }
    let mut services = required_system_services(linux_variant).to_vec();
    if !minimal && desktop.requires_display_manager() {
        services.extend(display_manager_service(dm));
    }
    services.extend(vm_guest_service(linux_variant, vm));
    services
        .into_iter()
        .filter_map(|service| linux_variant.init_service_package(service.daemon_package))
        .collect()
}

/// Query the init's configured boot state, including inside a target chroot.
pub fn service_enabled(linux_variant: ArchLinuxVariant, service: &str) -> Result<bool> {
    match linux_variant {
        ArchLinuxVariant::Arch => Ok(Command::new("systemctl")
            .args(["is-enabled", service])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .context("Could not query systemd services")?
            .success()),
        ArchLinuxVariant::Artix(ArtixInitSystem::S6) => {
            let service = format!("{service}-srv");
            let output = Command::new("s6")
                .args(["--color=no", "set", "status"])
                .output()
                .context("Could not query s6-frontend service state")?;
            anyhow::ensure!(
                output.status.success(),
                "Could not query s6-frontend service state: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
            Ok(s6_service_enabled(
                &String::from_utf8_lossy(&output.stdout),
                &service,
            ))
        }
        ArchLinuxVariant::Artix(ArtixInitSystem::OpenRC) => Ok(Path::new("/etc/runlevels")
            .join("default")
            .join(service)
            .exists()),
        ArchLinuxVariant::Artix(ArtixInitSystem::Runit) => Ok(Path::new("/etc/runit/runsvdir")
            .join("default")
            .join(service)
            .exists()),
        ArchLinuxVariant::Artix(ArtixInitSystem::Dinit) => {
            Ok(Path::new("/etc/dinit.d/boot.d").join(service).exists())
        }
    }
}

fn s6_service_enabled(output: &str, service: &str) -> bool {
    output.lines().any(|line| {
        matches!(line.trim().split_once('/'), Some((name, "active" | "always")) if name == service)
    })
}

/// Enable required and optional services, then persist the batch once.
/// Enabling is idempotent; always finishing also repairs an interrupted s6
/// commit/install even when the working set already marks a service enabled.
pub fn enable_services(
    linux_variant: ArchLinuxVariant,
    required: &[&str],
    optional: &[&str],
    executor: &dyn CommandRunner,
) -> Result<()> {
    enable_services_with(
        linux_variant,
        required,
        optional,
        executor,
        super::is_chroot(),
    )
}

fn enable_services_with(
    linux_variant: ArchLinuxVariant,
    required: &[&str],
    optional: &[&str],
    executor: &dyn CommandRunner,
    in_chroot: bool,
) -> Result<()> {
    for service in required {
        stage_service(linux_variant, service, executor)?;
    }
    for service in optional {
        if let Err(error) = stage_service(linux_variant, service, executor) {
            println!("Warning: Could not enable optional service {service}: {error:#}");
        }
    }
    if matches!(linux_variant, ArchLinuxVariant::Artix(ArtixInitSystem::S6)) {
        executor.run(Command::new("s6").args(["set", "commit"]))?;
        let mut install = Command::new("s6");
        install.args(["live", "install"]);
        if in_chroot {
            install.arg("--init");
        }
        executor.run(&mut install)?;
    }
    Ok(())
}

fn stage_service(
    linux_variant: ArchLinuxVariant,
    service: &str,
    executor: &dyn CommandRunner,
) -> Result<()> {
    match linux_variant {
        ArchLinuxVariant::Arch => executor.run(Command::new("systemctl").args(["enable", service])),
        ArchLinuxVariant::Artix(ArtixInitSystem::OpenRC) => {
            executor.run(Command::new("rc-update").args(["add", service, "default"]))
        }
        ArchLinuxVariant::Artix(ArtixInitSystem::Runit) => executor.run(Command::new("ln").args([
            "-sfn",
            &format!("/etc/runit/sv/{service}"),
            &format!("/etc/runit/runsvdir/default/{service}"),
        ])),
        ArchLinuxVariant::Artix(ArtixInitSystem::S6) => {
            // Artix packages separate the daemon (-srv) and logging (-log)
            // services. Enable the daemon; its dependencies pull in logging.
            let service = format!("{service}-srv");
            executor.run(Command::new("s6").args([
                "set",
                "enable",
                "--pull-dependencies",
                &service,
            ]))
        }
        ArchLinuxVariant::Artix(ArtixInitSystem::Dinit) => {
            executor.run(Command::new("dinitctl").args(["--offline", "enable", service]))
        }
    }
    .with_context(|| format!("Could not enable {service} on {}", linux_variant.label()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arch::execution::mock::MockRunner;

    #[test]
    fn networkmanager_is_selected_for_every_supported_variant() {
        use crate::arch::config::{DesktopEnvironment, DisplayManager};
        for (variant, suffix) in [
            (ArchLinuxVariant::Arch, None),
            (
                ArchLinuxVariant::Artix(ArtixInitSystem::OpenRC),
                Some("openrc"),
            ),
            (
                ArchLinuxVariant::Artix(ArtixInitSystem::Runit),
                Some("runit"),
            ),
            (ArchLinuxVariant::Artix(ArtixInitSystem::S6), Some("s6")),
            (
                ArchLinuxVariant::Artix(ArtixInitSystem::Dinit),
                Some("dinit"),
            ),
        ] {
            let services = required_system_services(variant);
            assert!(
                services
                    .iter()
                    .any(|service| service.service_name == "NetworkManager"
                        && service.daemon_package == "networkmanager")
            );
            let packages = artix_init_service_packages(
                variant,
                DesktopEnvironment::Tty,
                DisplayManager::None,
                true,
                None,
            );
            if let Some(suffix) = suffix {
                for daemon in ["networkmanager", "openssh", "chrony"] {
                    assert!(packages.contains(&format!("{daemon}-{suffix}")));
                }
            } else {
                assert!(packages.is_empty());
            }
        }
    }

    #[test]
    fn s6_installs_one_database_offline_in_a_chroot() {
        let runner = MockRunner::new();
        enable_services_with(
            ArchLinuxVariant::Artix(ArtixInitSystem::S6),
            &["NetworkManager", "sshd"],
            &["qemu-guest-agent"],
            &runner,
            true,
        )
        .unwrap();
        assert_eq!(
            runner.command_log(),
            vec![
                "s6 set enable --pull-dependencies NetworkManager-srv",
                "s6 set enable --pull-dependencies sshd-srv",
                "s6 set enable --pull-dependencies qemu-guest-agent-srv",
                "s6 set commit",
                "s6 live install --init",
            ]
        );
    }

    #[test]
    fn s6_updates_the_live_database_on_an_existing_system() {
        let runner = MockRunner::new();
        enable_services_with(
            ArchLinuxVariant::Artix(ArtixInitSystem::S6),
            &["sshd"],
            &[],
            &runner,
            false,
        )
        .unwrap();
        assert_eq!(runner.command_log().last().unwrap(), "s6 live install");
    }

    #[test]
    fn s6_enabled_state_distinguishes_active_essential_and_disabled_services() {
        assert!(s6_service_enabled("gdm/active\n", "gdm"));
        assert!(s6_service_enabled("gdm/always\n", "gdm"));
        assert!(!s6_service_enabled("gdm/usable\n", "gdm"));
        assert!(!s6_service_enabled("gdm/masked\n", "gdm"));
        assert!(!s6_service_enabled("lightdm/active\n", "gdm"));
        assert!(!s6_service_enabled("", "gdm"));
    }

    struct RejectService {
        runner: MockRunner,
        service: &'static str,
    }

    impl CommandRunner for RejectService {
        fn dry_run(&self) -> bool {
            false
        }
        fn run(&self, command: &mut Command) -> Result<()> {
            self.runner.run(command)?;
            anyhow::ensure!(
                !command.get_args().any(|arg| arg == self.service),
                "service unavailable"
            );
            Ok(())
        }
        fn run_with_input(&self, command: &mut Command, input: &str) -> Result<()> {
            self.runner.run_with_input(command, input)
        }
        fn run_with_output(&self, command: &mut Command) -> Result<Option<std::process::Output>> {
            self.runner.run_with_output(command)
        }
        fn log(&self, message: &str) {
            self.runner.log(message);
        }
    }

    #[test]
    fn required_service_failure_prevents_s6_commit() {
        let runner = RejectService {
            runner: MockRunner::new(),
            service: "sshd-srv",
        };
        assert!(
            enable_services_with(
                ArchLinuxVariant::Artix(ArtixInitSystem::S6),
                &["sshd"],
                &[],
                &runner,
                true
            )
            .is_err()
        );
        assert!(
            !runner
                .runner
                .command_log()
                .iter()
                .any(|command| command == "s6 set commit")
        );
    }

    #[test]
    fn optional_service_failure_still_persists_required_services() {
        let runner = RejectService {
            runner: MockRunner::new(),
            service: "qemu-guest-agent-srv",
        };
        enable_services_with(
            ArchLinuxVariant::Artix(ArtixInitSystem::S6),
            &["sshd"],
            &["qemu-guest-agent"],
            &runner,
            true,
        )
        .unwrap();
        assert_eq!(
            runner.runner.command_log().last().unwrap(),
            "s6 live install --init"
        );
    }

    #[test]
    fn other_inits_use_their_native_idempotent_enable_operation() {
        for (linux_variant, expected) in [
            (ArchLinuxVariant::Arch, "systemctl enable NetworkManager"),
            (
                ArchLinuxVariant::Artix(ArtixInitSystem::OpenRC),
                "rc-update add NetworkManager default",
            ),
            (
                ArchLinuxVariant::Artix(ArtixInitSystem::Runit),
                "ln -sfn /etc/runit/sv/NetworkManager /etc/runit/runsvdir/default/NetworkManager",
            ),
            (
                ArchLinuxVariant::Artix(ArtixInitSystem::Dinit),
                "dinitctl --offline enable NetworkManager",
            ),
        ] {
            let runner = MockRunner::new();
            enable_services_with(linux_variant, &["NetworkManager"], &[], &runner, true).unwrap();
            assert_eq!(runner.command_log(), vec![expected]);
        }
    }
}
