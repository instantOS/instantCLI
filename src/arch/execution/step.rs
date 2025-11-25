use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, clap::ValueEnum, Clone, Copy, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum InstallStep {
    /// Prepare disk (partition, format, mount)
    Disk,
    /// Install base system (pacstrap)
    Base,
    /// Generate fstab
    Fstab,
    /// Configure system (timezone, locale, hostname, users)
    Config,
    /// Install bootloader
    Bootloader,
    /// Post-installation setup
    Post,
}

impl InstallStep {
    pub fn requires_chroot(&self) -> bool {
        match self {
            InstallStep::Disk => false,
            InstallStep::Base => false,
            InstallStep::Fstab => false,
            InstallStep::Config => true,
            InstallStep::Bootloader => true,
            InstallStep::Post => true,
        }
    }

    pub fn dependencies(&self) -> Vec<InstallStep> {
        match self {
            InstallStep::Disk => vec![],
            InstallStep::Base => vec![InstallStep::Disk],
            InstallStep::Fstab => vec![InstallStep::Base],
            InstallStep::Config => vec![InstallStep::Base],
            InstallStep::Bootloader => vec![InstallStep::Base],
            InstallStep::Post => vec![InstallStep::Bootloader],
        }
    }
}
