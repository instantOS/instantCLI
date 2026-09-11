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
}
