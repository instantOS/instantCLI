#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::super::*;

    #[test]
    fn serializes_eden_with_wrappers() {
        let command = LaunchCommand {
            wrappers: LaunchWrappers {
                gamemode: true,
                gamescope: Some(GamescopeOptions {
                    options: vec!["-f".to_string(), "-h".to_string(), "1080".to_string()],
                }),
            },
            kind: LaunchCommandKind::Emulator(EmulatorLaunchCommand {
                platform: EmulatorPlatform::Eden,
                launcher: EmulatorLauncher::AppImage {
                    path: PathBuf::from("~/AppImages/eden.AppImage"),
                },
                game: PathBuf::from("/roms/Zelda.xci"),
                options: EmulatorOptions {
                    fullscreen: true,
                    batch_mode: false,
                },
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
            "gamemoderun gamescope -W 1280 -- '~/Applications/eden.AppImage' -f -g '/games/Test.xci'",
        )
        .unwrap();

        assert!(command.wrappers.gamemode);
        assert_eq!(
            command.wrappers.gamescope,
            Some(GamescopeOptions {
                options: vec!["-W".to_string(), "1280".to_string()]
            })
        );
        assert!(matches!(
            command.kind,
            LaunchCommandKind::Emulator(EmulatorLaunchCommand {
                platform: EmulatorPlatform::Eden,
                ..
            })
        ));
    }

    #[test]
    fn deserializes_known_dolphin_command() {
        let command = LaunchCommand::from_str(
            "flatpak run org.DolphinEmu.dolphin-emu -b -f -e '/games/Test.iso'",
        )
        .unwrap();
        assert!(matches!(
            command.kind,
            LaunchCommandKind::Emulator(EmulatorLaunchCommand {
                platform: EmulatorPlatform::Dolphin,
                ..
            })
        ));
    }

    #[test]
    fn deserializes_known_pcsx2_appimage_command() {
        let command = LaunchCommand::from_str(
            "'~/Applications/pcsx2-qt.AppImage' -batch -fullscreen -- '/games/Test.iso'",
        )
        .unwrap();
        assert!(matches!(
            command.kind,
            LaunchCommandKind::Emulator(EmulatorLaunchCommand {
                platform: EmulatorPlatform::Pcsx2,
                ..
            })
        ));
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
