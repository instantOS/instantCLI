mod detect;
mod parse;
mod render;
mod tests;

use std::fmt::{self, Display};
use std::path::PathBuf;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

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

impl LaunchCommand {
    pub fn manual(command: impl Into<String>) -> Self {
        Self {
            wrappers: LaunchWrappers::default(),
            kind: LaunchCommandKind::Manual {
                command: command.into(),
            },
        }
    }

    pub fn from_shell_or_manual(command: impl Into<String>) -> Self {
        let command = command.into();
        Self::from_str(&command).unwrap_or_else(|_| Self::manual(command))
    }

    pub fn to_shell_command(&self) -> String {
        render::render_to_shell_command(self)
    }
}

impl Display for LaunchCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_shell_command())
    }
}

impl Serialize for LaunchCommand {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_shell_command())
    }
}

impl<'de> Deserialize<'de> for LaunchCommand {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Ok(Self::from_shell_or_manual(raw))
    }
}

impl FromStr for LaunchCommand {
    type Err = shell_words::ParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let tokens = shell_words::split(input)?;
        Ok(parse::parse_launch_command(input, tokens))
    }
}
