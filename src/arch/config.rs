use serde::{Deserialize, Serialize};

use crate::arch::engine::{InstallContext, StepId};

/// Subvolume layout created for btrfs installations.
pub const BTRFS_ROOT_SUBVOLUME: &str = "@";
pub const BTRFS_HOME_SUBVOLUME: &str = "@home";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopEnvironment {
    Sway,
    Niri,
    InstantWM,
    Hyprland,
    Tty,
}

impl DesktopEnvironment {
    pub const DEFAULT: Self = Self::InstantWM;
    pub const ALL: [Self; 5] = [
        Self::Sway,
        Self::Niri,
        Self::InstantWM,
        Self::Hyprland,
        Self::Tty,
    ];

    /// Strict parse of the `answer_value` vocabulary; `None` on anything else.
    pub fn try_from_answer(answer: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|value| value.answer_value() == answer)
    }

    pub fn selected_or_default(context: &InstallContext) -> Self {
        context
            .get_answer(&StepId::DesktopEnvironment)
            .and_then(|answer| Self::try_from_answer(answer))
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

    /// Session desktop ID used by GDM. instantWM has a separate native
    /// Wayland entry; the generic session name remains its X11/LightDM ID.
    pub fn gdm_session_name(&self) -> Option<&'static str> {
        match self {
            Self::InstantWM => Some("instantwm-wayland"),
            _ => self.session_name(),
        }
    }

    pub fn package_names(&self) -> &'static [&'static str] {
        match self {
            Self::Sway => &["sway", "swayidle", "swaylock"],
            Self::Niri => &["niri"],
            Self::InstantWM => &[],
            Self::Hyprland => &["hyprland", "hypridle", "hyprlock"],
            Self::Tty => &[],
        }
    }

    /// Font packages required by this desktop environment's configuration.
    ///
    /// These ensure that fonts referenced in the DE's config templates
    /// (bars, window titles, terminal emulators, etc.) are available.
    pub fn font_packages(&self) -> &'static [&'static str] {
        match self {
            Self::Tty => &[],
            Self::Sway | Self::Niri | Self::InstantWM | Self::Hyprland => {
                &["ttf-jetbrains-mono-nerd", "inter-font"]
            }
        }
    }

    pub fn requires_display_manager(&self) -> bool {
        !matches!(self, Self::Tty)
    }
}

/// Root filesystem choice for the installation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RootFilesystem {
    Btrfs,
    Ext4,
}

impl RootFilesystem {
    /// btrfs is the recommended default (snapshots, compression).
    pub const DEFAULT: Self = Self::Btrfs;
    pub const ALL: [Self; 2] = [Self::Btrfs, Self::Ext4];

    /// Strict parse of the `answer_value` vocabulary; `None` on anything else.
    pub fn try_from_answer(answer: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|value| value.answer_value() == answer)
    }

    pub fn selected_or_default(context: &InstallContext) -> Self {
        context
            .get_answer(&StepId::RootFilesystem)
            .and_then(|answer| Self::try_from_answer(answer))
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BtrfsCompression {
    None,
    Zstd,
    Lzo,
    Zlib,
}

impl BtrfsCompression {
    /// zstd offers the best balance of speed and ratio and is the default.
    pub const DEFAULT: Self = Self::Zstd;
    pub const ALL: [Self; 4] = [Self::None, Self::Zstd, Self::Lzo, Self::Zlib];

    /// Strict parse of the `answer_value` vocabulary; `None` on anything else.
    pub fn try_from_answer(answer: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|value| value.answer_value() == answer)
    }

    pub fn selected_or_default(context: &InstallContext) -> Self {
        context
            .get_answer(&StepId::BtrfsCompression)
            .and_then(|answer| Self::try_from_answer(answer))
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

/// Display manager choice for the installation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplayManager {
    Gdm,
    Lightdm,
    None,
}

impl DisplayManager {
    /// gdm is the default.
    pub const DEFAULT: Self = Self::Gdm;
    pub const ALL: [Self; 3] = [Self::Gdm, Self::Lightdm, Self::None];

    /// Strict parse of the `answer_value` vocabulary; `None` on anything else.
    pub fn try_from_answer(answer: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|value| value.answer_value() == answer)
    }

    pub fn selected_or_default(context: &InstallContext) -> Self {
        context
            .get_answer(&StepId::DisplayManager)
            .and_then(|answer| Self::try_from_answer(answer))
            .unwrap_or(Self::DEFAULT)
    }

    pub fn answer_value(&self) -> &'static str {
        match self {
            Self::Gdm => "gdm",
            Self::Lightdm => "lightdm",
            Self::None => "none",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Gdm => "gdm (default)",
            Self::Lightdm => "lightdm",
            Self::None => "none",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::DesktopEnvironment;
    use super::{BtrfsCompression, DisplayManager, RootFilesystem};

    #[test]
    fn parses_display_manager_answers_strictly() {
        assert_eq!(
            DisplayManager::try_from_answer("gdm"),
            Some(DisplayManager::Gdm)
        );
        assert_eq!(
            DisplayManager::try_from_answer("lightdm"),
            Some(DisplayManager::Lightdm)
        );
        assert_eq!(
            DisplayManager::try_from_answer("none"),
            Some(DisplayManager::None)
        );
        assert_eq!(DisplayManager::DEFAULT, DisplayManager::Gdm);
        assert_eq!(DisplayManager::try_from_answer("unknown"), None);
    }

    #[test]
    fn parses_desktop_environment_answers() {
        assert_eq!(
            DesktopEnvironment::try_from_answer("instantwm"),
            Some(DesktopEnvironment::InstantWM)
        );
        assert_eq!(
            DesktopEnvironment::try_from_answer("none/tty"),
            Some(DesktopEnvironment::Tty)
        );
        assert_eq!(DesktopEnvironment::try_from_answer("unknown"), None);
        assert_eq!(DesktopEnvironment::try_from_answer("hyprLand"), None);
        assert_eq!(
            DesktopEnvironment::try_from_answer("hyprland"),
            Some(DesktopEnvironment::Hyprland)
        );
    }

    #[test]
    fn instantwm_uses_wayland_session_with_gdm() {
        assert_eq!(
            DesktopEnvironment::InstantWM.session_name(),
            Some("instantwm")
        );
        assert_eq!(
            DesktopEnvironment::InstantWM.gdm_session_name(),
            Some("instantwm-wayland")
        );
    }

    #[test]
    fn parses_root_filesystem_answers_strictly() {
        assert_eq!(
            RootFilesystem::try_from_answer("ext4"),
            Some(RootFilesystem::Ext4)
        );
        assert_eq!(
            RootFilesystem::try_from_answer("btrfs"),
            Some(RootFilesystem::Btrfs)
        );
        assert_eq!(RootFilesystem::DEFAULT, RootFilesystem::Btrfs);
        assert_eq!(RootFilesystem::try_from_answer("unknown"), None);
    }

    #[test]
    fn btrfs_compression_mount_options() {
        assert_eq!(BtrfsCompression::try_from_answer("unknown"), None);
        assert_eq!(BtrfsCompression::None.mount_option(), None);
        assert_eq!(BtrfsCompression::Zstd.mount_option(), Some("compress=zstd"));
        assert_eq!(BtrfsCompression::Lzo.mount_option(), Some("compress=lzo"));
        assert_eq!(BtrfsCompression::Zlib.mount_option(), Some("compress=zlib"));
    }
}
