use std::path::PathBuf;

use super::types::EmulatorPlatform;

pub fn matches_flatpak_app<'a>(tokens: &'a [String], app_id: &str) -> Option<&'a [String]> {
    match tokens {
        [flatpak, run, token_app_id, tail @ ..]
            if flatpak == "flatpak" && run == "run" && token_app_id == app_id =>
        {
            Some(tail)
        }
        _ => None,
    }
}

pub fn matches_appimage(path: &str, platform: EmulatorPlatform) -> bool {
    let names = match platform {
        EmulatorPlatform::Dolphin => &["dolphin"][..],
        EmulatorPlatform::Eden => &["eden"][..],
        EmulatorPlatform::Azahar => &["azahar"][..],
        EmulatorPlatform::Mgba => &["mgba"][..],
        EmulatorPlatform::Pcsx2 => &["pcsx2"][..],
        EmulatorPlatform::DuckStation => &["duckstation"][..],
    };

    file_name_lower(path).is_some_and(|name| {
        name.ends_with(".appimage") && names.iter().any(|needle| name.contains(needle))
    })
}

fn file_name_lower(path: &str) -> Option<String> {
    PathBuf::from(path)
        .file_name()
        .and_then(|value| value.to_str())
        .map(|value| value.to_lowercase())
}
