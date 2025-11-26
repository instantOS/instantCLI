use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::net::TcpStream;
use std::time::Duration;
use crate::common::paths;
use crate::common::compositor::CompositorType;
use crate::assist::{self, AssistCommands};
use crate::dot::commands::DotCommands;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutostartConfig {
    #[serde(default)]
    pub disabled: bool,
}

impl Default for AutostartConfig {
    fn default() -> Self {
        Self { disabled: false }
    }
}

pub fn load_config() -> Result<AutostartConfig> {
    let config_dir = paths::instant_config_dir()?;
    let config_path = config_dir.join("autostart.toml");

    if !config_path.exists() {
        return Ok(AutostartConfig::default());
    }

    let content = fs::read_to_string(&config_path)
        .context("Failed to read autostart config")?;
    
    toml::from_str(&content).context("Failed to parse autostart config")
}

fn is_already_running() -> bool {
    // Simple lock file mechanism
    let lock_path = std::env::temp_dir().join("instant_autostart.lock");
    
    // If lock file exists, check if process is still running
    if lock_path.exists() {
        if let Ok(pid_str) = fs::read_to_string(&lock_path) {
            if let Ok(pid) = pid_str.trim().parse::<i32>() {
                // Check if process exists by checking /proc/<pid>
                let proc_path = PathBuf::from(format!("/proc/{}", pid));
                if proc_path.exists() {
                    return true;
                }
            }
        }
    }

    // Write current PID to lock file
    let _ = fs::write(&lock_path, std::process::id().to_string());
    false
}

fn check_internet() -> bool {
    // Try to connect to Google DNS
    TcpStream::connect_timeout(
        &"8.8.8.8:53".parse().unwrap(),
        Duration::from_secs(2)
    ).is_ok()
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

    if let CompositorType::Sway = compositor {
        if debug {
            println!("Running assist setup for Sway");
        }
        assist::dispatch_assist_command(debug, Some(AssistCommands::Setup))?;
    }

    if check_internet() {
        if debug {
            println!("Internet connection detected, running dot update");
        }
        crate::dot::commands::handle_dot_command(&DotCommands::Update, None, debug)?;
    } else if debug {
        println!("No internet connection detected");
    }

    Ok(())
}
