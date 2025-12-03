use crate::assist::{self, AssistCommands};
use crate::common::compositor::CompositorType;
use crate::common::paths;
use crate::dot::commands::DotCommands;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AutostartConfig {
    #[serde(default)]
    pub disabled: bool,
}

pub fn load_config() -> Result<AutostartConfig> {
    let config_dir = paths::instant_config_dir()?;
    let config_path = config_dir.join("autostart.toml");

    if !config_path.exists() {
        return Ok(AutostartConfig::default());
    }

    let content = fs::read_to_string(&config_path).context("Failed to read autostart config")?;

    toml::from_str(&content).context("Failed to parse autostart config")
}

fn is_already_running() -> bool {
    // Simple lock file mechanism
    let lock_path = std::env::temp_dir().join("instant_autostart.lock");

    // If lock file exists, check if process is still running
    if lock_path.exists()
        && let Ok(pid_str) = fs::read_to_string(&lock_path)
        && let Ok(pid) = pid_str.trim().parse::<i32>()
    {
        // Check if process exists by checking /proc/<pid>
        let proc_path = PathBuf::from(format!("/proc/{}", pid));
        if proc_path.exists() {
            return true;
        }
    }

    // Write current PID to lock file
    let _ = fs::write(&lock_path, std::process::id().to_string());
    false
}

pub async fn run(debug: bool) -> Result<()> {
    let config = load_config()?;

    if config.disabled {
        if debug {
            println!("Autostart is disabled in config");
        }
        return Ok(());
    }

    if is_already_running() {
        if debug {
            println!("Autostart is already running");
        }
        return Ok(());
    }

    let compositor = CompositorType::detect();
    if debug {
        println!("Detected compositor: {:?}", compositor);
    }

    if which::which("nvidia-settings").is_ok() {
        if debug {
            println!("Found nvidia-settings, loading settings");
        }
        if let Err(e) = std::process::Command::new("nvidia-settings")
            .arg("-l")
            .status()
            && debug
        {
            eprintln!("Failed to run nvidia-settings: {}", e);
        }
    }

    if let CompositorType::Sway = compositor {
        if debug {
            println!("Running assist setup for Sway");
        }
        assist::dispatch_assist_command(debug, Some(AssistCommands::Setup))?;
    }

    if crate::common::network::check_internet() {
        if debug {
            println!("Internet connection detected, running dot update");
        }
        crate::dot::commands::handle_dot_command(
            &DotCommands::Update { no_apply: false },
            None,
            debug,
        )?;
    } else if debug {
        println!("No internet connection detected");
    }

    // Apply wallpaper
    if debug {
        println!("Applying wallpaper");
    }
    if let Err(e) = crate::wallpaper::commands::apply_configured_wallpaper().await {
        if debug {
            eprintln!("Failed to apply wallpaper: {}", e);
        }
    }

    Ok(())
}
