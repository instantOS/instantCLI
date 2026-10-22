use super::EmulatorPlatform;

/// Emulator identity and supported launch inputs. Discovery, parsing, and the
/// interactive builder all use this definition; installation availability is a
/// separate concern from the presence of save data.
impl EmulatorPlatform {
    pub const ALL: [Self; 6] = [
        Self::Eden,
        Self::Dolphin,
        Self::Pcsx2,
        Self::Azahar,
        Self::Mgba,
        Self::DuckStation,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::Dolphin => "dolphin",
            Self::Eden => "eden",
            Self::Azahar => "azahar",
            Self::Mgba => "mgba",
            Self::Pcsx2 => "pcsx2",
            Self::DuckStation => "duckstation",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Dolphin => "Dolphin",
            Self::Eden => "Eden",
            Self::Azahar => "Azahar",
            Self::Mgba => "mGBA",
            Self::Pcsx2 => "PCSX2",
            Self::DuckStation => "DuckStation",
        }
    }

    pub fn system_name(self) -> &'static str {
        match self {
            Self::Dolphin => "GameCube / Wii",
            Self::Eden => "Nintendo Switch",
            Self::Azahar => "Nintendo 3DS",
            Self::Mgba => "Game Boy / Color / Advance",
            Self::Pcsx2 => "PlayStation 2",
            Self::DuckStation => "PlayStation 1",
        }
    }

    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::Dolphin => &["iso", "wbfs", "gcm", "ciso", "gcz", "wad", "dol", "elf"],
            Self::Eden => &["nsp", "xci", "nca"],
            Self::Azahar => &["3ds", "3dsx", "cia", "app", "elf", "axf", "cci", "cxi"],
            Self::Mgba => &["gba", "gb", "gbc", "sgb", "zip", "7z"],
            Self::Pcsx2 => &["iso", "bin", "chd", "cso", "gz", "elf", "irx"],
            Self::DuckStation => &[
                "bin", "cue", "iso", "img", "chd", "pbp", "ecm", "mds", "psf", "minipsf", "m3u",
            ],
        }
    }

    pub fn flatpak_id(self) -> Option<&'static str> {
        match self {
            Self::Dolphin => Some("org.DolphinEmu.dolphin-emu"),
            Self::Azahar => Some("org.azahar_emu.Azahar"),
            Self::Pcsx2 => Some("net.pcsx2.PCSX2"),
            Self::DuckStation => Some("org.duckstation.DuckStation"),
            Self::Eden | Self::Mgba => None,
        }
    }

    pub fn native_command(self) -> Option<&'static str> {
        match self {
            Self::Dolphin => Some("dolphin-emu"),
            Self::Azahar => Some("azahar"),
            Self::Mgba => Some("mgba-qt"),
            Self::Pcsx2 => Some("pcsx2-qt"),
            Self::DuckStation => Some("duckstation-qt"),
            Self::Eden => None,
        }
    }

    pub fn matches_appimage(self, path: &std::path::Path) -> bool {
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| {
                let name = name.to_ascii_lowercase();
                name.ends_with(".appimage") && name.contains(self.id())
            })
    }

    pub fn supports_batch_mode(self) -> bool {
        matches!(self, Self::Dolphin | Self::Pcsx2 | Self::DuckStation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::launch_command::{
        EmulatorLaunchCommand, EmulatorLauncher, EmulatorOptions, LaunchCommand, LaunchCommandKind,
    };

    #[test]
    fn all_supported_launchers_round_trip_with_options_and_quoted_paths() {
        for platform in EmulatorPlatform::ALL {
            let mut launchers = vec![EmulatorLauncher::AppImage {
                path: format!("/emulators/{}/{}'s.AppImage", platform.id(), platform.id()).into(),
            }];
            if let Some(command) = platform.native_command() {
                launchers.push(EmulatorLauncher::Native { command });
            }
            if let Some(app_id) = platform.flatpak_id() {
                launchers.push(EmulatorLauncher::Flatpak { app_id });
            }
            for launcher in launchers {
                for fullscreen in [false, true] {
                    for batch_mode in [false, true] {
                        if batch_mode && !platform.supports_batch_mode() {
                            continue;
                        }
                        let command = LaunchCommand {
                            wrappers: Default::default(),
                            kind: LaunchCommandKind::Emulator(EmulatorLaunchCommand {
                                platform,
                                launcher: launcher.clone(),
                                game: "/games/Player's game.rom".into(),
                                options: EmulatorOptions {
                                    fullscreen,
                                    batch_mode,
                                },
                            }),
                        };
                        let shell = command.to_shell_command();
                        assert_eq!(shell.parse::<LaunchCommand>().unwrap(), command, "{shell}");
                    }
                }
            }
        }
    }

    #[test]
    fn executable_paths_remain_one_shell_argument() {
        for path in [
            "/games/Player's game",
            "/games/a; echo unsafe",
            "/games/$(touch unwanted)",
            "/games/ordinary",
        ] {
            let command = LaunchCommand::executable(std::path::Path::new(path));
            assert_eq!(
                shell_words::split(&command.to_shell_command()).unwrap(),
                vec![path]
            );
            let serialized = serde_json::to_string(&command).unwrap();
            assert_eq!(
                serde_json::from_str::<LaunchCommand>(&serialized).unwrap(),
                command
            );
        }
    }
}
