use serde::{Deserialize, Serialize};

/// Identifies a configuration wizard step or stored answer.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    clap::ValueEnum,
)]
pub enum StepId {
    Hostname,
    Username,
    Password,
    Keymap,
    Disk,
    PrepareDisk,
    MirrorRegion,
    Timezone,
    Locale,
    Kernel,
    DesktopEnvironment,
    RootFilesystem,
    BtrfsCompression,
    DisplayManager,
    UseEncryption,
    EncryptionPassword,
    UsePlymouth,
    Autologin,
    UseXorg,
    LogUpload,
    ConfirmInstall,
    VirtualBoxWarning,
    WeakPasswordWarning,
    LowRamWarning,
    DualBootEspWarning,
    MinimalMode,
    PartitioningMethod,
    RunCfdisk,
    RootPartition,
    SwapPartition,
    BootPartition,
    HomePartition,
    DualBootPartition,
    DualBootSize,
    DualBootInstructions,
}

/// Kernel variants the wizard can install.
///
/// The stored answer is the pacman package name ([`Kernel::label`]); parse it
/// back with [`Kernel::from_answer`] at every consumer instead of matching raw
/// strings. The match in [`crate::arch::engine::system_info`] is exhaustive so
/// a new variant must pick its GPU driver packages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kernel {
    Linux,
    Lts,
    Zen,
}

impl Kernel {
    pub const ALL: [Kernel; 3] = [Kernel::Linux, Kernel::Lts, Kernel::Zen];

    /// The pacman package name; also the persisted wizard answer.
    pub fn label(&self) -> &'static str {
        match self {
            Kernel::Linux => "linux",
            Kernel::Lts => "linux-lts",
            Kernel::Zen => "linux-zen",
        }
    }

    /// Parse a stored answer, rejecting unknown values instead of guessing.
    pub fn from_answer(answer: &str) -> Option<Kernel> {
        Self::ALL.iter().copied().find(|k| k.label() == answer)
    }
}

/// How the target disk will be partitioned.
///
/// This is the typed form of the `PartitioningMethod` answer; parse it with
/// [`PartitioningMethod::from_answer`] (via
/// [`InstallContext::partitioning_method`]) instead of substring-matching the
/// raw label at each consumer — the labels are display strings and must not
/// double as dispatch keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PartitioningMethod {
    Automatic,
    DualBoot,
    Manual,
}

impl PartitioningMethod {
    /// Stable value persisted by the wizard. This is deliberately independent
    /// of the human-facing menu label.
    pub fn answer_value(self) -> &'static str {
        match self {
            Self::Automatic => "automatic",
            Self::DualBoot => "dual_boot",
            Self::Manual => "manual",
        }
    }

    pub fn from_answer(answer: &str) -> Option<Self> {
        match answer {
            "automatic" => Some(Self::Automatic),
            "dual_boot" => Some(Self::DualBoot),
            "manual" => Some(Self::Manual),
            _ => None,
        }
    }
}

impl std::fmt::Display for PartitioningMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PartitioningMethod::Automatic => write!(f, "automatic"),
            PartitioningMethod::DualBoot => write!(f, "dual-boot"),
            PartitioningMethod::Manual => write!(f, "manual"),
        }
    }
}

/// Privacy classification for an answer when building a support report.
///
/// This is deliberately exhaustive: adding a new installer answer requires a
/// conscious decision about whether it is safe to share. `SystemDetail`
/// answers are only included when the user explicitly opts into hardware and
/// system details; personal and secret answers are never included.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnswerPrivacy {
    Anonymous,
    SystemDetail,
    Personal,
    Secret,
}

impl StepId {
    pub fn answer_privacy(self) -> AnswerPrivacy {
        match self {
            Self::Hostname | Self::Username => AnswerPrivacy::Personal,
            Self::Password | Self::EncryptionPassword => AnswerPrivacy::Secret,
            Self::Disk
            | Self::Keymap
            | Self::MirrorRegion
            | Self::Timezone
            | Self::Locale
            | Self::RootPartition
            | Self::SwapPartition
            | Self::BootPartition
            | Self::HomePartition
            | Self::DualBootPartition
            | Self::DualBootSize => AnswerPrivacy::SystemDetail,
            Self::PrepareDisk
            | Self::Kernel
            | Self::DesktopEnvironment
            | Self::RootFilesystem
            | Self::BtrfsCompression
            | Self::DisplayManager
            | Self::UseEncryption
            | Self::UsePlymouth
            | Self::Autologin
            | Self::UseXorg
            | Self::LogUpload
            | Self::ConfirmInstall
            | Self::VirtualBoxWarning
            | Self::WeakPasswordWarning
            | Self::LowRamWarning
            | Self::DualBootEspWarning
            | Self::MinimalMode
            | Self::PartitioningMethod
            | Self::RunCfdisk
            | Self::DualBootInstructions => AnswerPrivacy::Anonymous,
        }
    }
}

#[cfg(test)]
mod privacy_tests {
    use super::{AnswerPrivacy, StepId};

    #[test]
    fn identity_and_password_answers_are_never_shareable() {
        assert_eq!(StepId::Hostname.answer_privacy(), AnswerPrivacy::Personal);
        assert_eq!(StepId::Username.answer_privacy(), AnswerPrivacy::Personal);
        assert_eq!(StepId::Password.answer_privacy(), AnswerPrivacy::Secret);
        assert_eq!(
            StepId::EncryptionPassword.answer_privacy(),
            AnswerPrivacy::Secret
        );
    }

    #[test]
    fn reproduction_choices_are_marked_anonymous() {
        assert_eq!(
            StepId::PartitioningMethod.answer_privacy(),
            AnswerPrivacy::Anonymous
        );
        assert_eq!(StepId::Kernel.answer_privacy(), AnswerPrivacy::Anonymous);
        assert_eq!(
            StepId::DesktopEnvironment.answer_privacy(),
            AnswerPrivacy::Anonymous
        );
    }
}

#[allow(clippy::upper_case_acronyms)]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub enum BootMode {
    UEFI64,
    UEFI32,
    #[default]
    BIOS,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum GpuKind {
    Nvidia,
    Amd,
    Intel,
    Other(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct SystemInfo {
    pub boot_mode: BootMode,
    pub has_amd_cpu: bool,
    pub has_intel_cpu: bool,
    pub gpus: Vec<GpuKind>,
    pub vm_type: Option<String>,
    pub internet_connected: bool,
    pub architecture: String,
    pub distro: String,
    pub total_ram_gb: Option<u64>,
    /// PCI/USB vendor IDs (lowercase hex, no `0x` prefix) of the network
    /// interfaces. Used to pick the matching `linux-firmware-*` split
    /// packages. Optional in imported configurations for compatibility with
    /// config files written before this field existed.
    #[serde(default)]
    pub network_vendor_ids: Vec<String>,
}
