//! Where the installer is running, and what that permits.
//!
//! [`HostProfile`] distinguishes a disposable live ISO from a running system,
//! whose configuration must remain untouched. [`TargetRelation`] prevents the
//! installer from repartitioning its own host disk. Both come from the machine,
//! not wizard answers; [`HOST_ENV`] overrides detection in tests.

use anyhow::{Context, Result, bail};

use crate::arch::engine::{DevicePath, DiskPath};
use crate::common::distro::OperatingSystem;

/// Test/e2e override for [`HostProfile::detect`]: `liveiso`, `running` or
/// `foreign` (`arch`/`instantos` spell the running variants explicitly).
/// Unset, the profile is detected from the machine.
pub const HOST_ENV: &str = "INS_HOST_ENV";

/// What the installer is running on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostProfile {
    /// The instantOS live ISO: `/etc` is the archiso cowspace (RAM).
    LiveIso,
    /// A running Arch-family system that is not instantOS.
    RunningArch,
    /// A running instantOS installation.
    RunningInstantOs,
    /// Any other distribution. Not supported for installation yet.
    ForeignDistro,
}

impl HostProfile {
    /// The profile of this machine, honouring the [`HOST_ENV`] override.
    pub fn detect() -> Result<Self> {
        match std::env::var(HOST_ENV) {
            Ok(value) => Self::from_override(&value),
            Err(std::env::VarError::NotPresent) => Ok(Self::detect_uncached()),
            Err(error) => Err(error).with_context(|| format!("{HOST_ENV} is not valid UTF-8")),
        }
    }

    fn from_override(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "" => Ok(Self::detect_uncached()),
            "liveiso" => Ok(Self::LiveIso),
            "running" => Ok(Self::RunningArch),
            "instantos" => Ok(Self::RunningInstantOs),
            "foreign" => Ok(Self::ForeignDistro),
            other => {
                bail!("{HOST_ENV}={other:?} is not one of liveiso, running, instantos, foreign")
            }
        }
    }

    /// Live-ISO detection takes precedence over the distribution: the ISO
    /// carries an Arch `os-release`, so asking the distribution first would
    /// classify a live session as a running Arch system.
    fn detect_uncached() -> Self {
        if crate::common::distro::is_live_iso() {
            return Self::LiveIso;
        }
        match OperatingSystem::detect() {
            OperatingSystem::InstantOS => Self::RunningInstantOs,
            os if os.in_family(&OperatingSystem::Arch) => Self::RunningArch,
            _ => Self::ForeignDistro,
        }
    }

    /// Whether the host's own configuration may be rewritten. Only a live
    /// ISO qualifies; a running system's `/etc` must remain untouched.
    pub fn etc_is_ephemeral(self) -> bool {
        matches!(self, Self::LiveIso)
    }

    /// Whether this host is supported as an install source today.
    pub fn supports_installation(self) -> bool {
        matches!(
            self,
            Self::LiveIso | Self::RunningArch | Self::RunningInstantOs
        )
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::LiveIso => "the instantOS live ISO",
            Self::RunningArch => "a running Arch Linux system",
            Self::RunningInstantOs => "a running instantOS system",
            Self::ForeignDistro => "this distribution",
        }
    }
}

/// The installer's environment, resolved once and carried by the install plan
/// (see `CONTEXT.md`: an install-plan input, never wizard state).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstallEnvironment {
    host: HostProfile,
    /// The running disk standing in the way of this target, if any. `None` is
    /// the ordinary case: a spare disk, or a live ISO where nothing is running
    /// from a disk at all.
    target: Option<TargetRelation>,
}

impl InstallEnvironment {
    pub fn new(host: HostProfile, target: Option<TargetRelation>) -> Self {
        Self { host, target }
    }

    /// Resolve the environment for an install targeting `disk`.
    ///
    /// Inside the chroot the "running system" *is* the target, so the conflict
    /// is reported as [`TargetRelation::RootDevice`]. No chroot-side step consults
    /// it — partitioning, mirroring and the identity probe all run on the host
    /// — and reporting the truth keeps the value meaningful if that changes.
    pub fn detect(disk: &DiskPath) -> Self {
        let host = HostProfile::detect().unwrap_or_else(|error| {
            eprintln!("Warning: could not classify the host environment: {error:#}");
            HostProfile::detect_uncached()
        });
        let target = if crate::arch::execution::is_chroot() {
            Some(TargetRelation::RootDevice)
        } else {
            running_disk_conflict(
                disk.as_str(),
                crate::arch::disks::root_device().as_ref(),
                crate::arch::disks::boot_disk().as_ref(),
            )
        };
        Self::new(host, target)
    }

    pub fn host(&self) -> HostProfile {
        self.host
    }

    /// The running disk blocking this target, if any.
    ///
    /// When this is `Some`, the target may be neither repartitioned (that
    /// destroys the filesystem the installer is executing from — that needs a
    /// live medium, not a wizard) nor probed for an existing installation.
    pub fn target(&self) -> Option<TargetRelation> {
        self.target
    }

    /// Whether partitions under the target disk may be probed for an existing
    /// installation.
    ///
    /// The probe mounts every Linux filesystem under the target disk and reads
    /// `/etc/instant/installation.toml` from it. If the target disk is the
    /// running disk, that reads the *running* system, which would report
    /// `AlreadyInstalled` for a brand-new install.
    pub fn may_probe_for_existing_install(&self) -> bool {
        self.target.is_none()
    }

    pub fn target_label(&self) -> &'static str {
        self.target.map_or(
            "a different disk from the running system",
            TargetRelation::label,
        )
    }
}

/// How the install target relates to the running system.
///
/// A classification, not a reason for refusal: both variants are refused by the
/// same check and differ only in the wording the user sees.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetRelation {
    /// The target is the device the running system is executing from.
    RootDevice,
    /// The target is the disk the running system booted from.
    BootDisk,
}

impl TargetRelation {
    pub fn label(self) -> &'static str {
        match self {
            Self::RootDevice => "the running root filesystem device",
            Self::BootDisk => "the disk this system booted from",
        }
    }
}

/// The running device `candidate` names, if any.
///
/// The candidate is a plain path because the caller's own type says nothing
/// useful here: the wizard has a validated [`DiskPath`], the execution layer a
/// string it has not classified, and the comparison is textual either way.
pub fn running_disk_conflict(
    candidate: &str,
    root_device: Option<&DevicePath>,
    boot_disk: Option<&DevicePath>,
) -> Option<TargetRelation> {
    if root_device.is_some_and(|root| root.is(candidate)) {
        return Some(TargetRelation::RootDevice);
    }
    if boot_disk.is_some_and(|boot| boot.is(candidate)) {
        return Some(TargetRelation::BootDisk);
    }
    None
}

/// The user-facing explanation for a refused target.
///
/// The running disk is never a valid target — partitioning it would destroy the
/// filesystem the installer is running from. The message therefore names the
/// way out: a different disk works from the running system, while the running
/// disk itself needs the live ISO.
pub fn running_disk_message(disk: &str, conflict: TargetRelation) -> String {
    format!(
        "Cannot install onto {disk}: that is {}.\n\
         \n\
         The installer is running from that disk, so partitioning it would destroy the\n\
         system this installation is running from. Booting the instantOS live ISO is the\n\
         only way to install onto it.\n\
         \n\
         Already running from a live ISO? Pick a different disk. Running from an\n\
         installed system? Pick a *different* disk — installing to a spare drive works\n\
         directly from the running system.",
        conflict.label()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn disk(value: &str) -> DiskPath {
        DiskPath::parse(value).expect("test device path")
    }

    fn device(value: &str) -> DevicePath {
        DevicePath::parse(value).expect("test device path")
    }

    #[test]
    fn host_profile_override_parses_every_documented_value() {
        assert_eq!(
            HostProfile::from_override("liveiso").unwrap(),
            HostProfile::LiveIso
        );
        assert_eq!(
            HostProfile::from_override(" LiveISO ").unwrap(),
            HostProfile::LiveIso
        );
        assert_eq!(
            HostProfile::from_override("running").unwrap(),
            HostProfile::RunningArch
        );
        assert_eq!(
            HostProfile::from_override("instantos").unwrap(),
            HostProfile::RunningInstantOs
        );
        assert_eq!(
            HostProfile::from_override("foreign").unwrap(),
            HostProfile::ForeignDistro
        );
        assert!(HostProfile::from_override("debian").is_err());
    }

    #[test]
    fn only_a_live_iso_hosts_a_throwaway_etc() {
        assert!(HostProfile::LiveIso.etc_is_ephemeral());
        assert!(!HostProfile::RunningArch.etc_is_ephemeral());
        assert!(!HostProfile::RunningInstantOs.etc_is_ephemeral());
        assert!(!HostProfile::ForeignDistro.etc_is_ephemeral());
    }

    #[test]
    fn a_running_arch_host_supports_installation_but_not_reconfiguration() {
        assert!(HostProfile::RunningArch.supports_installation());
        assert!(!HostProfile::ForeignDistro.supports_installation());
    }

    #[test]
    fn only_a_spare_disk_may_be_probed() {
        let spare = InstallEnvironment::new(HostProfile::RunningArch, None);
        assert!(spare.may_probe_for_existing_install());
        for conflict in [TargetRelation::RootDevice, TargetRelation::BootDisk] {
            let running = InstallEnvironment::new(HostProfile::RunningArch, Some(conflict));
            assert!(running.target().is_some());
            assert!(!running.may_probe_for_existing_install());
        }
    }

    #[test]
    fn running_disk_conflict_names_which_running_device_matched() {
        // Root on a whole disk: the root device is itself a legal disk target,
        // so it can match and is named first.
        assert_eq!(
            running_disk_conflict(
                disk("/dev/nvme0n1").as_str(),
                Some(&device("/dev/nvme0n1")),
                Some(&device("/dev/nvme0n1"))
            ),
            Some(TargetRelation::RootDevice)
        );
        // Root on a partition: no disk target can equal it, so the boot disk is
        // what catches the conflict. This is the case a partitioned host hits.
        assert_eq!(
            running_disk_conflict(
                disk("/dev/nvme0n1").as_str(),
                Some(&device("/dev/nvme0n1p2")),
                Some(&device("/dev/nvme0n1"))
            ),
            Some(TargetRelation::BootDisk)
        );
        assert_eq!(
            running_disk_conflict(
                disk("/dev/sdb").as_str(),
                Some(&device("/dev/nvme0n1p2")),
                Some(&device("/dev/nvme0n1"))
            ),
            None
        );
        // A container or namespace with no findable root must not fall into a
        // guarded state, or the installer would refuse every disk.
        assert_eq!(
            running_disk_conflict(disk("/dev/sdb").as_str(), None, None),
            None
        );
    }

    #[test]
    fn the_refusal_message_names_the_way_out() {
        let message = running_disk_message("/dev/nvme0n1", TargetRelation::BootDisk);
        assert!(message.contains("/dev/nvme0n1"));
        assert!(message.contains("live ISO"));
        assert!(
            message.contains("different disk"),
            "must say what is possible instead of only what is not: {message}"
        );
    }

    #[test]
    fn environment_combines_a_profile_and_a_conflict() {
        let environment =
            InstallEnvironment::new(HostProfile::RunningArch, Some(TargetRelation::BootDisk));
        assert_eq!(environment.host(), HostProfile::RunningArch);
        assert_eq!(environment.target(), Some(TargetRelation::BootDisk));
        assert_eq!(environment.target_label(), TargetRelation::BootDisk.label());
    }
}
