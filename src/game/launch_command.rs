use std::collections::BTreeMap;
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
    Eden(EdenLaunchCommand),
    Steam(SteamLaunchCommand),
    Wine(WineLaunchCommand),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdenLaunchCommand {
    pub appimage: PathBuf,
    pub game: PathBuf,
    pub fullscreen: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WineLaunchCommand {
    pub runner: WineRunner,
    pub prefix: Option<PathBuf>,
    pub proton: ProtonSelection,
    pub executable: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SteamLaunchCommand {
    pub app_id: u32,
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
        let env_assignments = self.env_assignments();
        let raw_command = self.raw_shell_command();

        if self.wrappers == LaunchWrappers::default() {
            return match &self.kind {
                LaunchCommandKind::Manual { command } => command.clone(),
                _ => {
                    let mut parts = env_assignments;
                    parts.extend(raw_command);
                    parts.join(" ")
                }
            };
        }

        let mut parts = env_assignments;
        if self.wrappers.gamemode {
            parts.push("gamemoderun".to_string());
        }
        if let Some(gamescope) = &self.wrappers.gamescope {
            parts.push("gamescope".to_string());
            parts.extend(gamescope.options.iter().map(|option| shell_escape(option)));
            parts.push("--".to_string());
        }

        match &self.kind {
            LaunchCommandKind::Manual { command } => {
                parts.push("bash".to_string());
                parts.push("-lc".to_string());
                parts.push(shell_escape(command));
            }
            _ => parts.extend(raw_command),
        }

        parts.join(" ")
    }

    fn env_assignments(&self) -> Vec<String> {
        let mut envs = Vec::new();
        if let LaunchCommandKind::Wine(wine) = &self.kind {
            if let Some(prefix) = &wine.prefix {
                envs.push(env_assignment("WINEPREFIX", &prefix.to_string_lossy()));
            }

            if matches!(wine.runner, WineRunner::UmuRun)
                && let Some(proton) = wine.proton.to_env_value()
            {
                envs.push(env_assignment("PROTONPATH", &proton));
            }
        }
        envs
    }

    fn raw_shell_command(&self) -> Vec<String> {
        match &self.kind {
            LaunchCommandKind::Manual { command } => vec![command.clone()],
            LaunchCommandKind::Eden(eden) => {
                let mut parts = vec![shell_escape(&eden.appimage.to_string_lossy())];
                if eden.fullscreen {
                    parts.push("-f".to_string());
                }
                parts.push("-g".to_string());
                parts.push(shell_escape(&eden.game.to_string_lossy()));
                parts
            }
            LaunchCommandKind::Steam(steam) => {
                vec![
                    "steam".to_string(),
                    format!("steam://rungameid/{}", steam.app_id),
                ]
            }
            LaunchCommandKind::Wine(wine) => {
                let mut parts = vec![match wine.runner {
                    WineRunner::UmuRun => "umu-run".to_string(),
                    WineRunner::Wine => "wine".to_string(),
                }];
                parts.push(shell_escape(&wine.executable.to_string_lossy()));
                parts
            }
        }
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
        Ok(parse_launch_command_tokens(input, tokens))
    }
}

fn parse_launch_command_tokens(input: &str, tokens: Vec<String>) -> LaunchCommand {
    let mut remaining = tokens.as_slice();
    let mut env = BTreeMap::new();

    while let Some((key, value)) = parse_env_assignment(remaining.first()) {
        env.insert(key.to_string(), value.to_string());
        remaining = &remaining[1..];
    }

    let mut wrappers = LaunchWrappers::default();

    loop {
        match remaining.first().map(String::as_str) {
            Some("gamemoderun") if !wrappers.gamemode => {
                wrappers.gamemode = true;
                remaining = &remaining[1..];
            }
            Some("gamescope") if wrappers.gamescope.is_none() => {
                let Some(separator_idx) = remaining.iter().position(|token| token == "--") else {
                    return LaunchCommand::manual(input);
                };
                wrappers.gamescope = Some(GamescopeOptions {
                    options: remaining[1..separator_idx].to_vec(),
                });
                remaining = &remaining[separator_idx + 1..];
            }
            _ => break,
        }
    }

    if let Some(command) = parse_manual_bash_command(remaining) {
        return LaunchCommand {
            wrappers,
            kind: LaunchCommandKind::Manual { command },
        };
    }

    if let Some(kind) = parse_eden_command(remaining) {
        return LaunchCommand { wrappers, kind };
    }

    if let Some(kind) = parse_steam_command(remaining) {
        return LaunchCommand { wrappers, kind };
    }

    if let Some(kind) = parse_wine_command(remaining, &env) {
        return LaunchCommand { wrappers, kind };
    }

    if wrappers != LaunchWrappers::default() {
        return LaunchCommand {
            wrappers,
            kind: LaunchCommandKind::Manual {
                command: join_shell_words(remaining),
            },
        };
    }

    LaunchCommand::manual(input)
}

fn parse_manual_bash_command(tokens: &[String]) -> Option<String> {
    if tokens.len() == 3 && tokens[0] == "bash" && tokens[1] == "-lc" {
        return Some(tokens[2].clone());
    }
    None
}

fn parse_eden_command(tokens: &[String]) -> Option<LaunchCommandKind> {
    let executable = tokens.first()?;
    if !is_eden_appimage(executable) {
        return None;
    }

    let mut fullscreen = false;
    let mut game = None;
    let mut idx = 1;
    while idx < tokens.len() {
        match tokens[idx].as_str() {
            "-f" | "--fullscreen" => {
                fullscreen = true;
                idx += 1;
            }
            "-g" | "--game" => {
                let value = tokens.get(idx + 1)?.clone();
                game = Some(PathBuf::from(value));
                idx += 2;
            }
            _ => return None,
        }
    }

    Some(LaunchCommandKind::Eden(EdenLaunchCommand {
        appimage: PathBuf::from(executable),
        game: game?,
        fullscreen,
    }))
}

fn parse_wine_command(
    tokens: &[String],
    env: &BTreeMap<String, String>,
) -> Option<LaunchCommandKind> {
    let command = tokens.first()?;
    let executable = tokens.get(1)?;
    if tokens.len() != 2 {
        return None;
    }

    let runner = match command.as_str() {
        "umu-run" => WineRunner::UmuRun,
        "wine" => WineRunner::Wine,
        _ => return None,
    };

    if matches!(runner, WineRunner::Wine) && env.contains_key("PROTONPATH") {
        return None;
    }

    Some(LaunchCommandKind::Wine(WineLaunchCommand {
        runner,
        prefix: env.get("WINEPREFIX").map(PathBuf::from),
        proton: match runner {
            WineRunner::UmuRun => env
                .get("PROTONPATH")
                .map(|value| ProtonSelection::from_env_value(value))
                .unwrap_or_default(),
            WineRunner::Wine => ProtonSelection::UmuProtonLatest,
        },
        executable: PathBuf::from(executable),
    }))
}

fn parse_steam_command(tokens: &[String]) -> Option<LaunchCommandKind> {
    match tokens {
        [steam, uri] if steam == "steam" => {
            let app_id = uri.strip_prefix("steam://rungameid/")?.parse().ok()?;
            Some(LaunchCommandKind::Steam(SteamLaunchCommand { app_id }))
        }
        [steam, applaunch, app_id] if steam == "steam" && applaunch == "-applaunch" => {
            Some(LaunchCommandKind::Steam(SteamLaunchCommand {
                app_id: app_id.parse().ok()?,
            }))
        }
        _ => None,
    }
}

fn parse_env_assignment(token: Option<&String>) -> Option<(&str, &str)> {
    let token = token?;
    let eq_idx = token.find('=')?;
    let key = &token[..eq_idx];
    if key.is_empty()
        || !key
            .chars()
            .all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
    {
        return None;
    }
    Some((key, &token[eq_idx + 1..]))
}

fn is_eden_appimage(path: &str) -> bool {
    let file_name = PathBuf::from(path)
        .file_name()
        .and_then(|value| value.to_str())
        .map(|value| value.to_lowercase());

    file_name.is_some_and(|name| name.ends_with(".appimage") && name.contains("eden"))
}

fn join_shell_words(tokens: &[String]) -> String {
    tokens
        .iter()
        .map(|token| shell_escape(token))
        .collect::<Vec<_>>()
        .join(" ")
}

fn env_assignment(key: &str, value: &str) -> String {
    format!("{key}={}", shell_escape(value))
}

fn shell_escape(value: &str) -> String {
    if !value.is_empty()
        && value.chars().all(|ch| {
            ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '_' | '-' | ':' | '+' | '=')
        })
    {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\"'\"'"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_eden_with_wrappers() {
        let command = LaunchCommand {
            wrappers: LaunchWrappers {
                gamemode: true,
                gamescope: Some(GamescopeOptions {
                    options: vec!["-f".to_string(), "-h".to_string(), "1080".to_string()],
                }),
            },
            kind: LaunchCommandKind::Eden(EdenLaunchCommand {
                appimage: PathBuf::from("~/AppImages/eden.AppImage"),
                game: PathBuf::from("/roms/Zelda.xci"),
                fullscreen: true,
            }),
        };

        assert_eq!(
            command.to_string(),
            "gamemoderun gamescope -f -h 1080 -- '~/AppImages/eden.AppImage' -f -g /roms/Zelda.xci"
        );
    }

    #[test]
    fn deserializes_known_eden_command() {
        let command = LaunchCommand::from_str(
            "gamemoderun gamescope -W 1280 -- '/tmp/Eden.AppImage' -f -g '/games/Test.xci'",
        )
        .unwrap();

        assert!(command.wrappers.gamemode);
        assert_eq!(
            command.wrappers.gamescope,
            Some(GamescopeOptions {
                options: vec!["-W".to_string(), "1280".to_string()]
            })
        );
        assert!(matches!(command.kind, LaunchCommandKind::Eden(_)));
    }

    #[test]
    fn deserializes_known_umu_command() {
        let command = LaunchCommand::from_str(
            "WINEPREFIX='/prefix dir' PROTONPATH=GE-Proton umu-run '/games/Test.exe'",
        )
        .unwrap();

        assert_eq!(
            command.kind,
            LaunchCommandKind::Wine(WineLaunchCommand {
                runner: WineRunner::UmuRun,
                prefix: Some(PathBuf::from("/prefix dir")),
                proton: ProtonSelection::GeProtonLatest,
                executable: PathBuf::from("/games/Test.exe"),
            })
        );
    }

    #[test]
    fn deserializes_known_steam_command() {
        let command = LaunchCommand::from_str("steam steam://rungameid/12345").unwrap();

        assert_eq!(
            command.kind,
            LaunchCommandKind::Steam(SteamLaunchCommand { app_id: 12345 })
        );
    }

    #[test]
    fn falls_back_to_manual_for_unknown_eden_options() {
        let command = LaunchCommand::from_shell_or_manual(
            "'/tmp/Eden.AppImage' --profile foo -g '/games/Test.xci'",
        );

        assert!(matches!(command.kind, LaunchCommandKind::Manual { .. }));
    }
}
