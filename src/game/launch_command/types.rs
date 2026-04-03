use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LaunchWrappers {
    pub gamemode: bool,
    pub gamescope: Option<GamescopeOptions>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GamescopeOptions {
    pub options: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchCommand {
    pub wrappers: LaunchWrappers,
    pub kind: LaunchCommandKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchCommandKind {
    Manual { command: String },
    Steam(SteamLaunchCommand),
    Wine(WineLaunchCommand),
    Emulator(EmulatorLaunchCommand),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SteamLaunchCommand {
    pub app_id: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WineLaunchCommand {
    pub runner: WineRunner,
    pub prefix: Option<PathBuf>,
    pub proton: ProtonSelection,
    pub executable: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WineRunner {
    #[default]
    UmuRun,
    Wine,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ProtonSelection {
    GeProtonLatest,
    #[default]
    UmuProtonLatest,
    Custom(PathBuf),
}

impl ProtonSelection {
    pub fn from_env_value(value: &str) -> Self {
        match value {
            "" => Self::UmuProtonLatest,
            "GE-Proton" => Self::GeProtonLatest,
            other => Self::Custom(PathBuf::from(other)),
        }
    }

    pub fn to_env_value(&self) -> Option<String> {
        match self {
            ProtonSelection::UmuProtonLatest => None,
            ProtonSelection::GeProtonLatest => Some("GE-Proton".to_string()),
            ProtonSelection::Custom(path) => Some(path.to_string_lossy().into_owned()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmulatorLaunchCommand {
    pub platform: EmulatorPlatform,
    pub launcher: EmulatorLauncher,
    pub game: PathBuf,
    pub options: EmulatorOptions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmulatorPlatform {
    Dolphin,
    Eden,
    Azahar,
    Mgba,
    Pcsx2,
    DuckStation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmulatorLauncher {
    Flatpak { app_id: &'static str },
    Native { command: &'static str },
    AppImage { path: PathBuf },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EmulatorOptions {
    pub fullscreen: bool,
    pub batch_mode: bool,
}
