//! Which distribution and init system the installer targets.
//!
//! Detection reads files below a root rather than asking PID 1: `artix-chroot`
//! runs the chroot under `unshare --fork --pid`, so PID 1 inside the chroot is
//! the namespace's shell, not the target init.
//!
//! A variant is a supported base distribution/init combination. CPU
//! architecture and the instantOS/minimal installation profile are separate.

use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::common::distro::OperatingSystem;

/// Test/e2e override for [`ArchLinuxVariant::detect`]: `arch`, `artix-openrc`,
/// `artix-runit`, `artix-s6` or `artix-dinit`.
pub const ARCH_LINUX_VARIANT_ENV: &str = "INS_ARCH_LINUX_VARIANT";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InitSystem {
    Systemd,
    OpenRC,
    Runit,
    S6,
    Dinit,
}

impl InitSystem {
    /// Detect the configured init executable below `root`.
    /// Installed service tools do not identify the init selected for boot.
    pub fn detect_from_root(root: &Path) -> Result<Self> {
        let executable = resolve_init_executable(root).with_context(|| {
            format!("could not detect the init system below {}", root.display())
        })?;
        match executable.file_name().and_then(|name| name.to_str()) {
            Some("systemd") => Ok(Self::Systemd),
            Some("openrc-init") => Ok(Self::OpenRC),
            Some("runit-init") => Ok(Self::Runit),
            Some("s6-init") => Ok(Self::S6),
            Some("dinit-init" | "dinit") => Ok(Self::Dinit),
            _ => bail!(
                "unsupported init executable {} below {}; expected systemd, openrc-init, runit-init, s6-init or dinit-init",
                executable.display(),
                root.display()
            ),
        }
    }
}

/// Resolve `/sbin/init` as the inspected system would. Host canonicalization
/// would follow absolute symlinks into the host instead of the target root.
fn resolve_init_executable(root: &Path) -> Result<PathBuf> {
    let mut resolved = PathBuf::new();
    let mut pending = PathBuf::from("/sbin/init");
    let mut symlinks = 0;

    while let Some(component) = pending.components().next() {
        let rest = pending.components().skip(1).collect::<PathBuf>();
        match component {
            Component::RootDir => resolved.clear(),
            Component::CurDir => {}
            Component::ParentDir => {
                resolved.pop();
            }
            Component::Normal(name) => {
                resolved.push(name);
                let path = root.join(&resolved);
                let metadata = std::fs::symlink_metadata(&path)
                    .with_context(|| format!("could not inspect {}", path.display()))?;
                if metadata.file_type().is_symlink() {
                    symlinks += 1;
                    anyhow::ensure!(symlinks <= 40, "too many symlinks resolving /sbin/init");
                    let target = std::fs::read_link(&path)
                        .with_context(|| format!("could not read {}", path.display()))?;
                    resolved.pop();
                    pending = target.join(rest);
                    continue;
                }
            }
            Component::Prefix(_) => bail!("unsupported path prefix in /sbin/init"),
        }
        pending = rest;
    }

    anyhow::ensure!(
        root.join(&resolved).is_file(),
        "/sbin/init does not resolve to a regular file"
    );
    Ok(resolved)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArtixInitSystem {
    OpenRC,
    Runit,
    S6,
    Dinit,
}

impl ArtixInitSystem {
    fn init_system(self) -> InitSystem {
        match self {
            Self::OpenRC => InitSystem::OpenRC,
            Self::Runit => InitSystem::Runit,
            Self::S6 => InitSystem::S6,
            Self::Dinit => InitSystem::Dinit,
        }
    }

    /// Package-name suffix for this init, e.g. `networkmanager-openrc`.
    fn package_suffix(self) -> &'static str {
        match self {
            Self::OpenRC => "openrc",
            Self::Runit => "runit",
            Self::S6 => "s6",
            Self::Dinit => "dinit",
        }
    }
}

impl TryFrom<InitSystem> for ArtixInitSystem {
    type Error = anyhow::Error;

    fn try_from(init: InitSystem) -> Result<Self> {
        match init {
            InitSystem::OpenRC => Ok(Self::OpenRC),
            InitSystem::Runit => Ok(Self::Runit),
            InitSystem::S6 => Ok(Self::S6),
            InitSystem::Dinit => Ok(Self::Dinit),
            InitSystem::Systemd => bail!("systemd is not an Artix init system"),
        }
    }
}

/// Supported base distribution and init system, independent of CPU architecture
/// and the full instantOS/minimal installation profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArchLinuxVariant {
    Arch,
    Artix(ArtixInitSystem),
}

impl ArchLinuxVariant {
    /// The Arch Linux variant of this machine, honouring the [`ARCH_LINUX_VARIANT_ENV`] override.
    pub fn detect() -> Result<Self> {
        match std::env::var(ARCH_LINUX_VARIANT_ENV) {
            Ok(value) => match Self::parse_override(&value)? {
                Some(linux_variant) => Ok(linux_variant),
                None => Self::detect_from_root(Path::new("/")),
            },
            Err(std::env::VarError::NotPresent) => Self::detect_from_root(Path::new("/")),
            Err(error) => {
                Err(error).with_context(|| format!("{ARCH_LINUX_VARIANT_ENV} is not valid UTF-8"))
            }
        }
    }

    /// Detect the Arch Linux variant below `root` without consulting the running kernel.
    pub fn detect_from_root(root: &Path) -> Result<Self> {
        let os = OperatingSystem::detect_from_root(root);
        let init = InitSystem::detect_from_root(root).with_context(|| {
            format!(
                "cannot identify the Arch Linux variant at {}",
                root.display()
            )
        })?;

        match (os, init) {
            (OperatingSystem::Arch | OperatingSystem::InstantOS, InitSystem::Systemd) => {
                Ok(Self::Arch)
            }
            (OperatingSystem::Artix, InitSystem::Systemd) => {
                bail!("Artix Linux with systemd is not supported")
            }
            (OperatingSystem::Artix | OperatingSystem::InstantOS, init) => {
                Ok(Self::Artix(ArtixInitSystem::try_from(init)?))
            }
            (OperatingSystem::Arch, init) => bail!(
                "Arch Linux with {} is not supported; only systemd is supported on Arch",
                init.label()
            ),
            (os, init) => bail!(
                "{os} with {} is not supported; the installer supports Arch Linux and Artix Linux",
                init.label()
            ),
        }
    }

    /// Reject a mounted target whose distribution/init differs from the plan.
    /// Read the target files directly rather than honoring the host override.
    pub fn verify_root(self, root: &Path) -> Result<()> {
        let target = Self::detect_from_root(root)?;
        anyhow::ensure!(
            target == self,
            "Target variant {} differs from installation variant {}; refusing chroot re-entry",
            target.id(),
            self.id()
        );
        Ok(())
    }

    /// Parse a [`ARCH_LINUX_VARIANT_ENV`] value. `None` means the value was empty and
    /// detection should fall back to the machine.
    fn parse_override(value: &str) -> Result<Option<Self>> {
        match value.trim().to_ascii_lowercase().as_str() {
            "" => Ok(None),
            "arch" => Ok(Some(Self::Arch)),
            "artix-openrc" => Ok(Some(Self::Artix(ArtixInitSystem::OpenRC))),
            "artix-runit" => Ok(Some(Self::Artix(ArtixInitSystem::Runit))),
            "artix-s6" => Ok(Some(Self::Artix(ArtixInitSystem::S6))),
            "artix-dinit" => Ok(Some(Self::Artix(ArtixInitSystem::Dinit))),
            other => bail!(
                "{ARCH_LINUX_VARIANT_ENV}={other:?} is not one of arch, artix-openrc, artix-runit, artix-s6, artix-dinit"
            ),
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::Arch => "arch",
            Self::Artix(ArtixInitSystem::OpenRC) => "artix-openrc",
            Self::Artix(ArtixInitSystem::Runit) => "artix-runit",
            Self::Artix(ArtixInitSystem::S6) => "artix-s6",
            Self::Artix(ArtixInitSystem::Dinit) => "artix-dinit",
        }
    }

    pub fn is_arch(self) -> bool {
        matches!(self, Self::Arch)
    }

    pub fn is_systemd(self) -> bool {
        self.init_system() == InitSystem::Systemd
    }

    pub fn is_artix(self) -> bool {
        matches!(self, Self::Artix(_))
    }

    pub fn init_system(self) -> InitSystem {
        match self {
            Self::Arch => InitSystem::Systemd,
            Self::Artix(init) => init.init_system(),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Arch => "Arch Linux",
            Self::Artix(ArtixInitSystem::OpenRC) => "Artix Linux (OpenRC)",
            Self::Artix(ArtixInitSystem::Runit) => "Artix Linux (runit)",
            Self::Artix(ArtixInitSystem::S6) => "Artix Linux (s6)",
            Self::Artix(ArtixInitSystem::Dinit) => "Artix Linux (dinit)",
        }
    }

    pub fn bootstrap_command(self) -> &'static str {
        match self {
            Self::Arch => "pacstrap",
            Self::Artix(_) => "basestrap",
        }
    }

    pub fn chroot_command(self) -> &'static str {
        match self {
            Self::Arch => "arch-chroot",
            Self::Artix(_) => "artix-chroot",
        }
    }

    pub fn fstab_command(self) -> &'static str {
        match self {
            Self::Arch => "genfstab",
            Self::Artix(_) => "fstabgen",
        }
    }

    pub fn keyring_package(self) -> &'static str {
        match self {
            Self::Arch => "archlinux-keyring",
            Self::Artix(_) => "artix-keyring",
        }
    }

    pub fn keyring_name(self) -> &'static str {
        match self {
            Self::Arch => "archlinux",
            Self::Artix(_) => "artix",
        }
    }

    pub fn native_repositories(self) -> &'static [&'static str] {
        match self {
            Self::Arch => &["core", "extra", "multilib"],
            Self::Artix(_) => &["system", "world", "galaxy", "lib32"],
        }
    }

    pub fn additional_arch_repositories(self) -> &'static [&'static str] {
        match self {
            Self::Arch => &[],
            Self::Artix(_) => &["extra", "multilib"],
        }
    }

    pub fn bootstrap_init_packages(self) -> &'static [&'static str] {
        match self {
            Self::Arch => &[],
            Self::Artix(ArtixInitSystem::OpenRC) => &["openrc", "elogind-openrc"],
            Self::Artix(ArtixInitSystem::Runit) => &["runit", "elogind-runit"],
            Self::Artix(ArtixInitSystem::S6) => &["s6-base", "elogind-s6"],
            Self::Artix(ArtixInitSystem::Dinit) => &["dinit", "elogind-dinit"],
        }
    }

    pub fn init_service_package(self, base: &str) -> Option<String> {
        match self {
            Self::Arch => None,
            Self::Artix(init) => Some(format!("{base}-{}", init.package_suffix())),
        }
    }

    /// Installer policy: Artix's Plymouth configuration is not implemented.
    pub fn supports_plymouth_configuration(self) -> bool {
        self.is_arch()
    }

    /// Installer policy: the bundled package snapshots cover Arch only.
    pub fn supports_offline_installation(self) -> bool {
        self.is_arch()
    }

    pub fn uses_systemd_initramfs(self) -> bool {
        self.is_systemd()
    }

    pub fn encryption_kernel_param(self, uuid: &str) -> String {
        match self {
            Self::Arch => format!(
                "rd.luks.name={uuid}=cryptlvm root=/dev/mapper/instantOS-root resume=/dev/mapper/instantOS-swap"
            ),
            Self::Artix(_) => format!(
                "cryptdevice=UUID={uuid}:cryptlvm root=/dev/mapper/instantOS-root resume=/dev/mapper/instantOS-swap"
            ),
        }
    }
}

impl InitSystem {
    pub fn label(self) -> &'static str {
        match self {
            Self::Systemd => "systemd",
            Self::OpenRC => "OpenRC",
            Self::Runit => "runit",
            Self::S6 => "s6",
            Self::Dinit => "dinit",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    const ARCH_OS_RELEASE: &str = "NAME=\"Arch Linux\"\nID=arch\n";
    const ARTIX_OS_RELEASE: &str = "NAME=\"Artix Linux\"\nID=artix\nID_LIKE=arch\n";
    const INSTANTOS_OS_RELEASE: &str = "NAME=\"instantOS\"\nID=instantos\nID_LIKE=arch\n";
    const UBUNTU_OS_RELEASE: &str = "NAME=\"Ubuntu\"\nID=ubuntu\nID_LIKE=debian\n";

    fn fixture(os_release: &str, init: Option<&str>) -> TempDir {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("etc")).unwrap();
        fs::create_dir_all(root.path().join("usr/bin")).unwrap();
        fs::write(root.path().join("etc/os-release"), os_release).unwrap();
        std::os::unix::fs::symlink("usr/bin", root.path().join("sbin")).unwrap();
        if let Some(init) = init {
            let path = root.path().join(init);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "init executable").unwrap();
            std::os::unix::fs::symlink(format!("/{init}"), root.path().join("usr/bin/init"))
                .unwrap();
        }
        root
    }

    #[test]
    fn artix_detects_every_supported_init() {
        for (init, executable) in [
            (ArtixInitSystem::OpenRC, "usr/bin/openrc-init"),
            (ArtixInitSystem::Runit, "usr/bin/runit-init"),
            (ArtixInitSystem::S6, "usr/bin/s6-init"),
            (ArtixInitSystem::Dinit, "usr/bin/dinit-init"),
        ] {
            let root = fixture(ARTIX_OS_RELEASE, Some(executable));
            let variant = ArchLinuxVariant::detect_from_root(root.path()).unwrap();
            assert_eq!(variant, ArchLinuxVariant::Artix(init));
            assert!(variant.is_artix());
            assert!(!variant.is_systemd());
            assert_eq!(variant.init_system(), init.init_system());
        }
    }

    #[test]
    fn chroot_reentry_rejects_a_different_distribution_or_init() {
        let arch = fixture(ARCH_OS_RELEASE, Some("usr/lib/systemd/systemd"));
        let openrc = fixture(ARTIX_OS_RELEASE, Some("usr/bin/openrc-init"));
        let dinit = fixture(ARTIX_OS_RELEASE, Some("usr/bin/dinit-init"));
        let expected = ArchLinuxVariant::Artix(ArtixInitSystem::OpenRC);
        assert!(expected.verify_root(openrc.path()).is_ok());
        assert!(expected.verify_root(arch.path()).is_err());
        assert!(expected.verify_root(dinit.path()).is_err());
    }

    #[test]
    fn arch_systemd_detects_arch() {
        let root = fixture(ARCH_OS_RELEASE, Some("usr/lib/systemd/systemd"));
        let variant = ArchLinuxVariant::detect_from_root(root.path()).unwrap();
        assert_eq!(variant, ArchLinuxVariant::Arch);
        assert!(variant.is_systemd());
        assert!(!variant.is_artix());
        assert_eq!(variant.init_system().label(), "systemd");
        assert_eq!(variant.label(), "Arch Linux");
    }

    #[test]
    fn instantos_follows_its_init() {
        for (executable, variant) in [
            ("usr/lib/systemd/systemd", ArchLinuxVariant::Arch),
            (
                "usr/bin/openrc-init",
                ArchLinuxVariant::Artix(ArtixInitSystem::OpenRC),
            ),
        ] {
            let root = fixture(INSTANTOS_OS_RELEASE, Some(executable));
            assert_eq!(
                ArchLinuxVariant::detect_from_root(root.path()).unwrap(),
                variant
            );
        }
    }

    #[test]
    fn a_foreign_distro_is_refused_with_either_init() {
        for executable in ["usr/lib/systemd/systemd", "usr/bin/openrc-init"] {
            let root = fixture(UBUNTU_OS_RELEASE, Some(executable));
            let error = ArchLinuxVariant::detect_from_root(root.path()).unwrap_err();
            assert!(error.to_string().contains("Ubuntu"), "{error}");
        }
    }

    #[test]
    fn unsupported_distribution_init_combinations_are_refused() {
        for (os, init) in [
            (ARCH_OS_RELEASE, "usr/bin/openrc-init"),
            (ARTIX_OS_RELEASE, "usr/lib/systemd/systemd"),
        ] {
            let root = fixture(os, Some(init));
            assert!(ArchLinuxVariant::detect_from_root(root.path()).is_err());
        }
    }

    #[test]
    fn installed_utilities_do_not_override_the_configured_init() {
        let root = fixture(ARTIX_OS_RELEASE, Some("usr/bin/dinit-init"));
        for path in [
            "usr/bin/s6-svscan",
            "usr/bin/runit",
            "usr/bin/rc-update",
            "usr/lib/systemd/systemd",
        ] {
            let path = root.path().join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "unused utility").unwrap();
        }
        assert_eq!(
            ArchLinuxVariant::detect_from_root(root.path()).unwrap(),
            ArchLinuxVariant::Artix(ArtixInitSystem::Dinit)
        );
    }

    #[test]
    fn init_symlinks_are_resolved_inside_the_inspected_root() {
        let root = fixture(ARTIX_OS_RELEASE, Some("usr/bin/dinit"));
        let init = root.path().join("usr/bin/init");
        fs::remove_file(&init).unwrap();
        std::os::unix::fs::symlink("dinit-init", &init).unwrap();
        std::os::unix::fs::symlink(
            "../../usr/bin/dinit",
            root.path().join("usr/bin/dinit-init"),
        )
        .unwrap();
        assert_eq!(
            InitSystem::detect_from_root(root.path()).unwrap(),
            InitSystem::Dinit
        );
        fs::remove_file(root.path().join("sbin")).unwrap();
        std::os::unix::fs::symlink("/usr/bin", root.path().join("sbin")).unwrap();
        assert_eq!(
            InitSystem::detect_from_root(root.path()).unwrap(),
            InitSystem::Dinit
        );
    }

    #[test]
    fn missing_unknown_and_looping_init_targets_are_refused() {
        let root = fixture(ARTIX_OS_RELEASE, None);
        assert!(InitSystem::detect_from_root(root.path()).is_err());
        fs::write(root.path().join("usr/bin/init"), "unknown init").unwrap();
        assert!(InitSystem::detect_from_root(root.path()).is_err());
        fs::remove_file(root.path().join("usr/bin/init")).unwrap();
        std::os::unix::fs::symlink("missing", root.path().join("usr/bin/init")).unwrap();
        assert!(InitSystem::detect_from_root(root.path()).is_err());
        fs::remove_file(root.path().join("usr/bin/init")).unwrap();
        std::os::unix::fs::symlink("init", root.path().join("usr/bin/init")).unwrap();
        let error = InitSystem::detect_from_root(root.path()).unwrap_err();
        assert!(format!("{error:#}").contains("too many symlinks"));
    }

    #[test]
    fn variant_override_parses_every_documented_value() {
        assert_eq!(
            ArchLinuxVariant::parse_override("arch").unwrap(),
            Some(ArchLinuxVariant::Arch)
        );
        assert_eq!(
            ArchLinuxVariant::parse_override(" Artix-OpenRC ").unwrap(),
            Some(ArchLinuxVariant::Artix(ArtixInitSystem::OpenRC))
        );
        assert_eq!(
            ArchLinuxVariant::parse_override("artix-runit").unwrap(),
            Some(ArchLinuxVariant::Artix(ArtixInitSystem::Runit))
        );
        assert_eq!(
            ArchLinuxVariant::parse_override("artix-s6").unwrap(),
            Some(ArchLinuxVariant::Artix(ArtixInitSystem::S6))
        );
        assert_eq!(
            ArchLinuxVariant::parse_override("artix-dinit").unwrap(),
            Some(ArchLinuxVariant::Artix(ArtixInitSystem::Dinit))
        );
        assert_eq!(ArchLinuxVariant::parse_override("").unwrap(), None);
        assert!(ArchLinuxVariant::parse_override("systemd").is_err());
    }

    #[test]
    fn artix_and_arch_use_different_encryption_parameters() {
        let uuid = "1234-abcd";
        assert_eq!(
            ArchLinuxVariant::Arch.encryption_kernel_param(uuid),
            "rd.luks.name=1234-abcd=cryptlvm root=/dev/mapper/instantOS-root resume=/dev/mapper/instantOS-swap"
        );
        assert_eq!(
            ArchLinuxVariant::Artix(ArtixInitSystem::OpenRC).encryption_kernel_param(uuid),
            "cryptdevice=UUID=1234-abcd:cryptlvm root=/dev/mapper/instantOS-root resume=/dev/mapper/instantOS-swap"
        );
    }

    #[test]
    fn package_and_repository_values_match_the_variant() {
        assert_eq!(
            ArchLinuxVariant::Arch.native_repositories(),
            &["core", "extra", "multilib"]
        );
        assert!(
            ArchLinuxVariant::Arch
                .additional_arch_repositories()
                .is_empty()
        );
        assert!(ArchLinuxVariant::Arch.bootstrap_init_packages().is_empty());
        assert_eq!(
            ArchLinuxVariant::Arch.init_service_package("networkmanager"),
            None
        );
        assert_eq!(
            ArchLinuxVariant::Arch.keyring_package(),
            "archlinux-keyring"
        );
        assert_eq!(ArchLinuxVariant::Arch.keyring_name(), "archlinux");
        assert_eq!(ArchLinuxVariant::Arch.bootstrap_command(), "pacstrap");
        assert_eq!(ArchLinuxVariant::Arch.chroot_command(), "arch-chroot");
        assert_eq!(ArchLinuxVariant::Arch.fstab_command(), "genfstab");
        assert!(ArchLinuxVariant::Arch.supports_plymouth_configuration());
        assert!(ArchLinuxVariant::Arch.supports_offline_installation());
        assert!(ArchLinuxVariant::Arch.uses_systemd_initramfs());

        let artix = ArchLinuxVariant::Artix(ArtixInitSystem::S6);
        assert_eq!(
            artix.native_repositories(),
            &["system", "world", "galaxy", "lib32"]
        );
        assert_eq!(artix.additional_arch_repositories(), &["extra", "multilib"]);
        assert_eq!(artix.bootstrap_init_packages(), &["s6-base", "elogind-s6"]);
        assert_eq!(
            artix.init_service_package("networkmanager"),
            Some("networkmanager-s6".to_owned())
        );
        assert_eq!(artix.keyring_package(), "artix-keyring");
        assert_eq!(artix.keyring_name(), "artix");
        assert_eq!(artix.bootstrap_command(), "basestrap");
        assert_eq!(artix.chroot_command(), "artix-chroot");
        assert_eq!(artix.fstab_command(), "fstabgen");
        assert!(!artix.supports_plymouth_configuration());
        assert!(!artix.supports_offline_installation());
        assert!(!artix.uses_systemd_initramfs());
        assert_eq!(artix.label(), "Artix Linux (s6)");
    }

    #[test]
    fn every_artix_init_has_its_own_packages_and_service_suffix() {
        for (init, suffix, init_package, package) in [
            (
                ArtixInitSystem::OpenRC,
                "openrc",
                "openrc",
                "elogind-openrc",
            ),
            (ArtixInitSystem::Runit, "runit", "runit", "elogind-runit"),
            (ArtixInitSystem::S6, "s6", "s6-base", "elogind-s6"),
            (ArtixInitSystem::Dinit, "dinit", "dinit", "elogind-dinit"),
        ] {
            let linux_variant = ArchLinuxVariant::Artix(init);
            assert_eq!(
                linux_variant.bootstrap_init_packages(),
                &[init_package, package]
            );
            assert_eq!(
                linux_variant.init_service_package("networkmanager"),
                Some(format!("networkmanager-{suffix}"))
            );
        }
    }
}
