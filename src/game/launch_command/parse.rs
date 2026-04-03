use std::collections::BTreeMap;
use std::path::PathBuf;

use super::detect::{matches_appimage, matches_flatpak_app};
use super::render::shell_escape;
use super::types::{
    EmulatorLaunchCommand, EmulatorLauncher, EmulatorOptions, EmulatorPlatform, GamescopeOptions,
    LaunchCommand, LaunchCommandKind, LaunchWrappers, ProtonSelection, SteamLaunchCommand,
    WineLaunchCommand, WineRunner,
};

pub fn parse_launch_command(input: &str, tokens: Vec<String>) -> LaunchCommand {
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

    if let Some(kind) = parse_steam_command(remaining) {
        return LaunchCommand { wrappers, kind };
    }

    if let Some(kind) = parse_wine_command(remaining, &env) {
        return LaunchCommand { wrappers, kind };
    }

    if let Some(kind) = parse_emulator_command(remaining) {
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

fn parse_emulator_command(tokens: &[String]) -> Option<LaunchCommandKind> {
    if let Some(tail) = matches_flatpak_app(tokens, "org.DolphinEmu.dolphin-emu") {
        return parse_dolphin(
            EmulatorLauncher::Flatpak {
                app_id: "org.DolphinEmu.dolphin-emu",
            },
            tail,
        );
    }
    if let Some(tail) = matches_flatpak_app(tokens, "org.azahar_emu.Azahar") {
        return parse_azahar(
            EmulatorLauncher::Flatpak {
                app_id: "org.azahar_emu.Azahar",
            },
            tail,
        );
    }
    if let Some(tail) = matches_flatpak_app(tokens, "net.pcsx2.PCSX2") {
        return parse_pcsx2(
            EmulatorLauncher::Flatpak {
                app_id: "net.pcsx2.PCSX2",
            },
            tail,
        );
    }
    if let Some(tail) = matches_flatpak_app(tokens, "org.duckstation.DuckStation") {
        return parse_duckstation(
            EmulatorLauncher::Flatpak {
                app_id: "org.duckstation.DuckStation",
            },
            tail,
        );
    }

    let launcher = match tokens.first().map(String::as_str) {
        Some("mgba-qt") => Some((
            EmulatorPlatform::Mgba,
            EmulatorLauncher::Native { command: "mgba-qt" },
            &tokens[1..],
        )),
        Some("azahar") => Some((
            EmulatorPlatform::Azahar,
            EmulatorLauncher::Native { command: "azahar" },
            &tokens[1..],
        )),
        Some("pcsx2-qt") => Some((
            EmulatorPlatform::Pcsx2,
            EmulatorLauncher::Native {
                command: "pcsx2-qt",
            },
            &tokens[1..],
        )),
        Some("duckstation-qt") => Some((
            EmulatorPlatform::DuckStation,
            EmulatorLauncher::Native {
                command: "duckstation-qt",
            },
            &tokens[1..],
        )),
        Some(path) if matches_appimage(path, EmulatorPlatform::Eden) => Some((
            EmulatorPlatform::Eden,
            EmulatorLauncher::AppImage {
                path: PathBuf::from(path),
            },
            &tokens[1..],
        )),
        Some(path) if matches_appimage(path, EmulatorPlatform::Pcsx2) => Some((
            EmulatorPlatform::Pcsx2,
            EmulatorLauncher::AppImage {
                path: PathBuf::from(path),
            },
            &tokens[1..],
        )),
        Some(path) if matches_appimage(path, EmulatorPlatform::DuckStation) => Some((
            EmulatorPlatform::DuckStation,
            EmulatorLauncher::AppImage {
                path: PathBuf::from(path),
            },
            &tokens[1..],
        )),
        _ => None,
    }?;

    match launcher.0 {
        EmulatorPlatform::Dolphin => unreachable!(),
        EmulatorPlatform::Eden => parse_eden(launcher.1, launcher.2),
        EmulatorPlatform::Azahar => parse_azahar(launcher.1, launcher.2),
        EmulatorPlatform::Mgba => parse_mgba(launcher.1, launcher.2),
        EmulatorPlatform::Pcsx2 => parse_pcsx2(launcher.1, launcher.2),
        EmulatorPlatform::DuckStation => parse_duckstation(launcher.1, launcher.2),
    }
}

fn parse_dolphin(launcher: EmulatorLauncher, tail: &[String]) -> Option<LaunchCommandKind> {
    let mut batch_mode = false;
    let mut fullscreen = false;
    let mut game = None;
    let mut idx = 0;
    while idx < tail.len() {
        match tail[idx].as_str() {
            "-b" => {
                batch_mode = true;
                idx += 1;
            }
            "-f" => {
                fullscreen = true;
                idx += 1;
            }
            "-e" => {
                game = Some(PathBuf::from(tail.get(idx + 1)?.clone()));
                idx += 2;
            }
            _ => return None,
        }
    }
    Some(LaunchCommandKind::Emulator(EmulatorLaunchCommand {
        platform: EmulatorPlatform::Dolphin,
        launcher,
        game: game?,
        options: EmulatorOptions {
            fullscreen,
            batch_mode,
        },
    }))
}

fn parse_eden(launcher: EmulatorLauncher, tail: &[String]) -> Option<LaunchCommandKind> {
    let mut fullscreen = false;
    let mut game = None;
    let mut idx = 0;
    while idx < tail.len() {
        match tail[idx].as_str() {
            "-f" | "--fullscreen" => {
                fullscreen = true;
                idx += 1;
            }
            "-g" | "--game" => {
                game = Some(PathBuf::from(tail.get(idx + 1)?.clone()));
                idx += 2;
            }
            _ => return None,
        }
    }
    Some(LaunchCommandKind::Emulator(EmulatorLaunchCommand {
        platform: EmulatorPlatform::Eden,
        launcher,
        game: game?,
        options: EmulatorOptions {
            fullscreen,
            batch_mode: false,
        },
    }))
}

fn parse_azahar(launcher: EmulatorLauncher, tail: &[String]) -> Option<LaunchCommandKind> {
    parse_separator_platform(EmulatorPlatform::Azahar, launcher, tail, "-f")
}

fn parse_pcsx2(launcher: EmulatorLauncher, tail: &[String]) -> Option<LaunchCommandKind> {
    parse_separator_platform(EmulatorPlatform::Pcsx2, launcher, tail, "-fullscreen")
}

fn parse_duckstation(launcher: EmulatorLauncher, tail: &[String]) -> Option<LaunchCommandKind> {
    parse_separator_platform(EmulatorPlatform::DuckStation, launcher, tail, "-fullscreen")
}

fn parse_separator_platform(
    platform: EmulatorPlatform,
    launcher: EmulatorLauncher,
    tail: &[String],
    fullscreen_flag: &str,
) -> Option<LaunchCommandKind> {
    let mut batch_mode = false;
    let mut fullscreen = false;
    let mut game = None;
    let mut idx = 0;
    while idx < tail.len() {
        match tail[idx].as_str() {
            "-batch" => {
                batch_mode = true;
                idx += 1;
            }
            flag if flag == fullscreen_flag => {
                fullscreen = true;
                idx += 1;
            }
            "--" => {
                game = Some(PathBuf::from(tail.get(idx + 1)?.clone()));
                idx += 2;
                if idx != tail.len() {
                    return None;
                }
            }
            _ => return None,
        }
    }
    Some(LaunchCommandKind::Emulator(EmulatorLaunchCommand {
        platform,
        launcher,
        game: game?,
        options: EmulatorOptions {
            fullscreen,
            batch_mode,
        },
    }))
}

fn parse_mgba(launcher: EmulatorLauncher, tail: &[String]) -> Option<LaunchCommandKind> {
    let mut fullscreen = false;
    let mut game = None;
    let mut idx = 0;
    while idx < tail.len() {
        match tail[idx].as_str() {
            "-f" => {
                fullscreen = true;
                idx += 1;
            }
            value if game.is_none() => {
                game = Some(PathBuf::from(value));
                idx += 1;
                if idx != tail.len() {
                    return None;
                }
            }
            _ => return None,
        }
    }
    Some(LaunchCommandKind::Emulator(EmulatorLaunchCommand {
        platform: EmulatorPlatform::Mgba,
        launcher,
        game: game?,
        options: EmulatorOptions {
            fullscreen,
            batch_mode: false,
        },
    }))
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

fn join_shell_words(tokens: &[String]) -> String {
    tokens
        .iter()
        .map(|token| shell_escape(token))
        .collect::<Vec<_>>()
        .join(" ")
}
