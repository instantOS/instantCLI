use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::game::launch_command::{
    EmulatorLauncher, EmulatorPlatform, LaunchCommand, LaunchCommandKind, SteamLaunchCommand,
};

/// The configuration namespace that owns the discovered saves. Native binaries
/// and AppImages share a namespace; Flatpak uses its own isolated directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmulatorSaveSource {
    Native,
    Flatpak,
}

impl EmulatorSaveSource {
    pub fn matches(self, launcher: &EmulatorLauncher) -> bool {
        matches!(
            (self, launcher),
            (Self::Flatpak, EmulatorLauncher::Flatpak { .. })
                | (
                    Self::Native,
                    EmulatorLauncher::Native { .. } | EmulatorLauncher::AppImage { .. }
                )
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DiscoveryLaunchHint {
    Emulator {
        platform: EmulatorPlatform,
        source: EmulatorSaveSource,
        game: Option<PathBuf>,
    },
    Steam {
        app_id: u32,
    },
    Wine {
        prefix: PathBuf,
    },
}

impl DiscoveryLaunchHint {
    /// Only prefill a complete command when its installation uses the same save
    /// namespace. Incomplete hints remain available to the interactive builder.
    pub fn prefilled_command(&self) -> anyhow::Result<Option<LaunchCommand>> {
        let launchers = match self {
            Self::Emulator {
                platform,
                game: Some(_),
                ..
            } => super::super::emulator::detected_launchers(*platform)?,
            _ => Vec::new(),
        };
        Ok(self.command_from_launchers(&launchers))
    }

    fn command_from_launchers(&self, launchers: &[EmulatorLauncher]) -> Option<LaunchCommand> {
        match self {
            Self::Steam { app_id } => Some(LaunchCommand {
                wrappers: Default::default(),
                kind: LaunchCommandKind::Steam(SteamLaunchCommand { app_id: *app_id }),
            }),
            Self::Emulator {
                platform,
                source,
                game: Some(game),
            } => launchers
                .iter()
                .find(|launcher| source.matches(launcher))
                .map(|launcher| {
                    super::super::emulator::launch_command(
                        *platform,
                        launcher.clone(),
                        game,
                        Default::default(),
                    )
                }),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefill_uses_the_save_namespace_and_never_falls_back_to_another() {
        let native = EmulatorLauncher::Native { command: "azahar" };
        let flatpak = EmulatorLauncher::Flatpak {
            app_id: "org.azahar_emu.Azahar",
        };
        for (source, expected, other) in [
            (EmulatorSaveSource::Native, native.clone(), flatpak.clone()),
            (EmulatorSaveSource::Flatpak, flatpak.clone(), native.clone()),
        ] {
            let hint = DiscoveryLaunchHint::Emulator {
                platform: EmulatorPlatform::Azahar,
                source,
                game: Some("/games/game.3ds".into()),
            };
            assert!(
                hint.command_from_launchers(std::slice::from_ref(&other))
                    .is_none()
            );
            let command = hint
                .command_from_launchers(&[other, expected.clone()])
                .unwrap();
            let LaunchCommandKind::Emulator(emulator) = command.kind else {
                panic!("expected emulator command");
            };
            assert_eq!(emulator.launcher, expected);
            assert_eq!(emulator.game, PathBuf::from("/games/game.3ds"));
        }
    }

    #[test]
    fn incomplete_discovery_retains_hint_without_inventing_a_command() {
        for hint in [
            DiscoveryLaunchHint::Wine {
                prefix: "/prefix".into(),
            },
            DiscoveryLaunchHint::Emulator {
                platform: EmulatorPlatform::Pcsx2,
                source: EmulatorSaveSource::Native,
                game: None,
            },
        ] {
            assert!(hint.prefilled_command().unwrap().is_none());
            let serialized = serde_json::to_string(&hint).unwrap();
            assert_eq!(
                serde_json::from_str::<DiscoveryLaunchHint>(&serialized).unwrap(),
                hint
            );
        }
    }

    #[test]
    fn steam_prefill_keeps_explicit_app_id() {
        let hint = DiscoveryLaunchHint::Steam { app_id: 12345 };
        assert_eq!(
            hint.prefilled_command().unwrap().unwrap().to_string(),
            "steam steam://rungameid/12345"
        );
    }
}
