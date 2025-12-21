use crate::assist::{self, AssistCommands};
use crate::common::paths;
use crate::common::systemd;
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

    if debug {
        println!("Applying settings");
    }
    if let Err(e) = crate::settings::commands::dispatch_settings_command(
        debug,
        false,
        Some(crate::settings::commands::SettingsCommands::Apply),
        None,
    ) && debug
    {
        eprintln!("Failed to apply settings: {}", e);
    }

    if debug {
        println!("Running assist setup");
    }
    if let Err(e) =
        assist::dispatch_assist_command(debug, false, Some(AssistCommands::Setup { wm: None }))
        && debug
    {
        eprintln!("Assist setup failed: {}", e);
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
    if let Err(e) = crate::wallpaper::commands::apply_configured_wallpaper().await
        && debug
    {
        eprintln!("Failed to apply wallpaper: {}", e);
    }

    // Start polkit agent if needed
    ensure_polkit_agent(debug).await;

    // Launch welcome app if enabled
    if should_show_welcome() {
        if debug {
            println!("Launching welcome app");
        }
        if let Err(e) = crate::welcome::commands::handle_welcome_command(&None, true, debug)
            && debug
        {
            eprintln!("Failed to launch welcome app: {}", e);
        }
    } else if debug {
        println!("Welcome app autostart is disabled");
    }

    Ok(())
}

async fn ensure_polkit_agent(debug: bool) {
    use crate::common::display_server::DisplayServer;

    // Skip if not a desktop session
    if !DisplayServer::detect().is_desktop_session() {
        if debug {
            println!("Not running in a desktop session, skipping polkit agent setup");
        }
        return;
    }

    // Check if polkit agent is already running
    if crate::doctor::checks::security::is_polkit_agent_running().await {
        if debug {
            println!("Polkit authentication agent is already running");
        }
        return;
    }

    if debug {
        println!("No polkit authentication agent found, attempting to start one");
    }

    // Try to start hyprpolkitagent via systemd user service if available
    let systemd_manager = systemd::SystemdManager::user();
    if systemd_manager.service_exists("hyprpolkitagent.service") {
        if debug {
            println!("hyprpolkitagent.service found, attempting to start systemd service");
        }

        match systemd_manager.start("hyprpolkitagent.service") {
            Ok(()) => {
                if debug {
                    println!("Successfully started hyprpolkitagent service");
                }
                return;
            }
            Err(e) => {
                if debug {
                    eprintln!("Failed to start hyprpolkitagent service: {}", e);
                }
            }
        }
    }

    // Fallback: try to start hyprpolkitagent via PATH if available
    if which::which("hyprpolkitagent").is_ok() {
        if debug {
            println!("hyprpolkitagent found in PATH, attempting to start in background");
        }

        match std::process::Command::new("hyprpolkitagent").spawn() {
            Ok(child) => {
                std::mem::forget(child);
                if debug {
                    println!("Successfully started hyprpolkitagent in background");
                }
                return;
            }
            Err(e) => {
                if debug {
                    eprintln!("Failed to start hyprpolkitagent: {}", e);
                }
            }
        }
    }

    // Fallback: try to start lxpolkit in background if installed
    if which::which("lxpolkit").is_ok() {
        if debug {
            println!("hyprpolkitagent not available, trying lxpolkit as fallback");
        }

        match std::process::Command::new("lxpolkit").spawn() {
            Ok(child) => {
                std::mem::forget(child);
                if debug {
                    println!("Successfully started lxpolkit in background");
                }
                return;
            }
            Err(e) => {
                if debug {
                    eprintln!("Failed to start lxpolkit: {}", e);
                }
            }
        }
    }

    if debug {
        println!(
            "No polkit agent could be started. Neither hyprpolkitagent nor lxpolkit are available."
        );
    }
}

fn should_show_welcome() -> bool {
    use crate::settings::store::{BoolSettingKey, SettingsStore};

    // Try to load settings and check if welcome autostart is enabled
    match SettingsStore::load() {
        Ok(store) => {
            let key = BoolSettingKey::new("system.welcome_autostart", true);
            store.bool(key)
        }
        Err(_) => {
            // If we can't load settings, default to true (show welcome on first boot)
            true
        }
    }
}
