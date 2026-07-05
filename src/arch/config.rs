use crate::arch::engine::{InstallContext, QuestionId};

/// Subvolume layout created for btrfs installations.
pub const BTRFS_ROOT_SUBVOLUME: &str = "@";
pub const BTRFS_HOME_SUBVOLUME: &str = "@home";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopEnvironment {
    Sway,
    Niri,
    InstantWM,
    Hyprland,
    Tty,
}

impl DesktopEnvironment {
    pub const DEFAULT: Self = Self::Sway;

    pub fn from_answer(answer: &str) -> Self {
        match answer {
            "sway" => Self::Sway,
            "niri" => Self::Niri,
            "instantwm" => Self::InstantWM,
            "hyprland" => Self::Hyprland,
            "none/tty" => Self::Tty,
            _ => Self::DEFAULT,
        }
    }

    pub fn from_context(context: &InstallContext) -> Self {
        context
            .get_answer(&QuestionId::DesktopEnvironment)
            .map(|answer| Self::from_answer(answer))
            .unwrap_or(Self::DEFAULT)
    }

    pub fn answer_value(&self) -> &'static str {
        match self {
            Self::Sway => "sway",
            Self::Niri => "niri",
            Self::InstantWM => "instantwm",
            Self::Hyprland => "hyprland",
            Self::Tty => "none/tty",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Sway => "Sway",
            Self::Niri => "niri",
            Self::InstantWM => "instantWM",
            Self::Hyprland => "Hyprland",
            Self::Tty => "None / TTY",
        }
    }

    pub fn session_name(&self) -> Option<&'static str> {
        match self {
            Self::Sway => Some("sway"),
            Self::Niri => Some("niri"),
            Self::InstantWM => Some("instantwm"),
            Self::Hyprland => Some("hyprland"),
            Self::Tty => None,
        }
    }

    pub fn package_names(&self) -> &'static [&'static str] {
        match self {
            Self::Sway => &["sway"],
            Self::Niri => &["niri"],
            Self::InstantWM => &[],
            Self::Hyprland => &["hyprland"],
            Self::Tty => &[],
        }
    }

    pub fn requires_display_manager(&self) -> bool {
        !matches!(self, Self::Tty)
    }
}

/// Root filesystem choice for the installation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootFilesystem {
    Btrfs,
    Ext4,
}

impl RootFilesystem {
    /// btrfs is the recommended default (snapshots, compression).
    pub const DEFAULT: Self = Self::Btrfs;

    pub fn from_answer(answer: &str) -> Self {
        match answer {
            "ext4" => Self::Ext4,
            "btrfs" => Self::Btrfs,
            _ => Self::DEFAULT,
        }
    }

    pub fn from_context(context: &InstallContext) -> Self {
        context
            .get_answer(&QuestionId::RootFilesystem)
            .map(|answer| Self::from_answer(answer))
            .unwrap_or(Self::DEFAULT)
    }

    pub fn answer_value(&self) -> &'static str {
        match self {
            Self::Btrfs => "btrfs",
            Self::Ext4 => "ext4",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Btrfs => "btrfs",
            Self::Ext4 => "ext4",
        }
    }

    pub fn is_btrfs(&self) -> bool {
        matches!(self, Self::Btrfs)
    }
}

/// Compression algorithm for btrfs root filesystems.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BtrfsCompression {
    None,
    Zstd,
    Lzo,
    Zlib,
}

impl BtrfsCompression {
    /// zstd offers the best balance of speed and ratio and is the default.
    pub const DEFAULT: Self = Self::Zstd;

    pub fn from_answer(answer: &str) -> Self {
        match answer {
            "none" => Self::None,
            "zstd" => Self::Zstd,
            "lzo" => Self::Lzo,
            "zlib" => Self::Zlib,
            _ => Self::DEFAULT,
        }
    }

    pub fn from_context(context: &InstallContext) -> Self {
        context
            .get_answer(&QuestionId::BtrfsCompression)
            .map(|answer| Self::from_answer(answer))
            .unwrap_or(Self::DEFAULT)
    }

    pub fn answer_value(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Zstd => "zstd",
            Self::Lzo => "lzo",
            Self::Zlib => "zlib",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::None => "None (no compression)",
            Self::Zstd => "zstd (recommended)",
            Self::Lzo => "lzo (fastest)",
            Self::Zlib => "zlib (highest ratio)",
        }
    }

    /// The `compress=` mount option value, or `None` when compression is disabled.
    pub fn mount_option(&self) -> Option<&'static str> {
        match self {
            Self::None => None,
            Self::Zstd => Some("compress=zstd"),
            Self::Lzo => Some("compress=lzo"),
            Self::Zlib => Some("compress=zlib"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::DesktopEnvironment;
    use super::{BtrfsCompression, RootFilesystem};

    #[test]
    fn parses_desktop_environment_answers() {
        assert_eq!(
            DesktopEnvironment::from_answer("instantwm"),
            DesktopEnvironment::InstantWM
        );
        assert_eq!(
            DesktopEnvironment::from_answer("none/tty"),
            DesktopEnvironment::Tty
        );
        assert_eq!(
            DesktopEnvironment::from_answer("unknown"),
            DesktopEnvironment::Sway
        );
    }

    #[test]
    fn root_filesystem_defaults_to_btrfs() {
        assert_eq!(RootFilesystem::from_answer("ext4"), RootFilesystem::Ext4);
        assert_eq!(RootFilesystem::from_answer("btrfs"), RootFilesystem::Btrfs);
        assert_eq!(
            RootFilesystem::from_answer("unknown"),
            RootFilesystem::Btrfs
        );
        assert_eq!(RootFilesystem::DEFAULT, RootFilesystem::Btrfs);
    }

    #[test]
    fn btrfs_compression_mount_options() {
        assert_eq!(
            BtrfsCompression::from_answer("unknown"),
            BtrfsCompression::Zstd
        );
        assert_eq!(BtrfsCompression::None.mount_option(), None);
        assert_eq!(BtrfsCompression::Zstd.mount_option(), Some("compress=zstd"));
        assert_eq!(BtrfsCompression::Lzo.mount_option(), Some("compress=lzo"));
        assert_eq!(BtrfsCompression::Zlib.mount_option(), Some("compress=zlib"));
    }
}
