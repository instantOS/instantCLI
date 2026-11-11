use std::path::PathBuf;

pub(super) fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~"))
        .join("instantwm")
        .join("config.toml")
}

pub(super) fn hyprland_config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~"))
        .join("hypr")
        .join("hyprland.lua")
}
