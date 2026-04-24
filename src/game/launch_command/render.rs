use super::types::{
    EmulatorLaunchCommand, EmulatorLauncher, EmulatorPlatform, LaunchCommand, LaunchCommandKind,
    WineRunner,
};

pub(super) fn render_to_shell_command(command: &LaunchCommand) -> String {
    let env_assignments = env_assignments(command);
    let raw_command = raw_shell_command(command);

    if command.wrappers == Default::default() {
        return match &command.kind {
            LaunchCommandKind::Manual { command } => command.clone(),
            _ => {
                let mut parts = env_assignments;
                parts.extend(raw_command);
                parts.join(" ")
            }
        };
    }

    let mut parts = env_assignments;
    if command.wrappers.gamemode {
        parts.push("gamemoderun".to_string());
    }
    if let Some(gamescope) = &command.wrappers.gamescope {
        parts.push("gamescope".to_string());
        parts.extend(gamescope.options.iter().map(|option| shell_escape(option)));
        parts.push("--".to_string());
    }

    match &command.kind {
        LaunchCommandKind::Manual { command } => {
            parts.push("bash".to_string());
            parts.push("-lc".to_string());
            parts.push(shell_escape(command));
        }
        _ => parts.extend(raw_command),
    }

    parts.join(" ")
}

fn env_assignments(command: &LaunchCommand) -> Vec<String> {
    let mut envs = Vec::new();
    if let LaunchCommandKind::Wine(wine) = &command.kind {
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

fn raw_shell_command(command: &LaunchCommand) -> Vec<String> {
    match &command.kind {
        LaunchCommandKind::Manual { command } => vec![command.clone()],
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
        LaunchCommandKind::Emulator(emulator) => render_emulator(emulator),
    }
}

fn render_emulator(emulator: &EmulatorLaunchCommand) -> Vec<String> {
    let mut parts = match &emulator.launcher {
        EmulatorLauncher::Flatpak { app_id } => {
            vec![
                "flatpak".to_string(),
                "run".to_string(),
                (*app_id).to_string(),
            ]
        }
        EmulatorLauncher::Native { command } => vec![(*command).to_string()],
        EmulatorLauncher::AppImage { path } => vec![shell_escape(&path.to_string_lossy())],
    };

    match emulator.platform {
        EmulatorPlatform::Dolphin => {
            if emulator.options.batch_mode {
                parts.push("-b".to_string());
            }
            if emulator.options.fullscreen {
                parts.push("-f".to_string());
            }
            parts.push("-e".to_string());
            parts.push(shell_escape(&emulator.game.to_string_lossy()));
        }
        EmulatorPlatform::Eden => {
            if emulator.options.fullscreen {
                parts.push("-f".to_string());
            }
            parts.push("-g".to_string());
            parts.push(shell_escape(&emulator.game.to_string_lossy()));
        }
        EmulatorPlatform::Azahar => {
            if emulator.options.fullscreen {
                parts.push("-f".to_string());
            }
            parts.push("--".to_string());
            parts.push(shell_escape(&emulator.game.to_string_lossy()));
        }
        EmulatorPlatform::Mgba => {
            if emulator.options.fullscreen {
                parts.push("-f".to_string());
            }
            parts.push(shell_escape(&emulator.game.to_string_lossy()));
        }
        EmulatorPlatform::Pcsx2 | EmulatorPlatform::DuckStation => {
            if emulator.options.batch_mode {
                parts.push("-batch".to_string());
            }
            if emulator.options.fullscreen {
                parts.push("-fullscreen".to_string());
            }
            parts.push("--".to_string());
            parts.push(shell_escape(&emulator.game.to_string_lossy()));
        }
    }

    parts
}

fn env_assignment(key: &str, value: &str) -> String {
    format!("{key}={}", shell_escape(value))
}

pub fn shell_escape(value: &str) -> String {
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
