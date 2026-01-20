use serde::{Deserialize, Serialize};

/// Represents a unique identifier for a question
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, clap::ValueEnum)]
pub enum QuestionId {
    Hostname,
    Username,
    Password,
    Keymap,
    Disk,
    MirrorRegion,
    Timezone,
    Locale,
    Kernel,
    UseEncryption,
    EncryptionPassword,
    UsePlymouth,
    Autologin,
    LogUpload,
    ConfirmInstall,
    VirtualBoxWarning,
    WeakPasswordWarning,
    LowRamWarning,
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
