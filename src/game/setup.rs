use anyhow::{Context, Result};
use std::collections::{HashMap, HashSet};

use crate::dot::path_serde::TildePath;
use crate::fzf_wrapper::{ConfirmResult, FzfSelectable, FzfWrapper};
use crate::game::config::{GameInstallation, InstallationsConfig, InstantGameConfig};
use crate::game::games::validation::validate_game_manager_initialized;
use crate::game::restic::cache;
use crate::menu::protocol;

/// Set up games that have been added but don't have installations configured on this device
pub fn setup_uninstalled_games() -> Result<()> {
    // Load configurations
    let game_config = InstantGameConfig::load().context("Failed to load game configuration")?;
    let mut installations =
        InstallationsConfig::load().context("Failed to load installations configuration")?;

    // Check if game manager is initialized
    if !validate_game_manager_initialized()? {
        return Ok(());
    }

    // Find games without installations
    let uninstalled_games = find_uninstalled_games(&game_config, &installations)?;

    if uninstalled_games.is_empty() {
        println!("✅ All games are already configured for this device!");
        return Ok(());
    }

    println!(
        "Found {} game(s) that need to be set up on this device:\n",
        uninstalled_games.len()
    );

    for game_name in &uninstalled_games {
        println!("  • {game_name}");
    }

    println!();

    // Process each uninstalled game
    for game_name in uninstalled_games {
        if let Err(e) = setup_single_game(&game_name, &game_config, &mut installations) {
            eprintln!("❌ Failed to set up game '{}': {}", game_name, e);
            
            // Ask if user wants to continue with other games
            match FzfWrapper::confirm(&format!(
                "Would you like to continue setting up the remaining games?"
            ))
            .map_err(|e| anyhow::anyhow!("Failed to get confirmation: {}", e))?
            {
                ConfirmResult::Yes => continue,
                ConfirmResult::No | ConfirmResult::Cancelled => {
                    println!("Setup cancelled by user.");
                    break;
                }
            }
        }
    }

    Ok(())
}

/// Find games that are in the config but don't have installations
fn find_uninstalled_games(
    game_config: &InstantGameConfig,
    installations: &InstallationsConfig,
) -> Result<Vec<String>> {
    let installed_games: HashSet<_> = installations
        .installations
        .iter()
        .map(|inst| &inst.game_name.0)
        .collect();

    let uninstalled_games: Vec<String> = game_config
        .games
        .iter()
        .filter(|game| !installed_games.contains(&game.name.0))
        .map(|game| game.name.0.clone())
        .collect();

    Ok(uninstalled_games)
}

/// Set up a single game by collecting paths from snapshots and letting user choose
fn setup_single_game(
    game_name: &str,
    game_config: &InstantGameConfig,
    installations: &mut InstallationsConfig,
) -> Result<()> {
    println!("🎮 Setting up game: {game_name}");

    // Get all snapshots for this game to extract paths
    let snapshots = cache::get_snapshots_for_game(game_name, game_config)
        .context("Failed to get snapshots for game")?;

    if snapshots.is_empty() {
        println!("❌ No snapshots found for game '{game_name}'.");
        println!("This game has no backups yet. You'll need to add an installation manually using 'instant game add'.");
        return Ok(());
    }

    // Extract unique paths from all snapshots
    let unique_paths = extract_unique_paths_from_snapshots(&snapshots)?;

    if unique_paths.is_empty() {
        println!("❌ No save paths found in snapshots for game '{game_name}'.");
        println!("This is unusual. You may need to set up the installation manually.");
        return Ok(());
    }

    println!(
        "\nFound {} unique save path(s) from different devices/snapshots:",
        unique_paths.len()
    );

    // Let user choose from available paths or enter a custom one
    let chosen_path = choose_installation_path(game_name, &unique_paths)?;

    if let Some(path_str) = chosen_path {
        // Convert to TildePath
        let save_path = TildePath::from_str(&path_str)
            .map_err(|e| anyhow::anyhow!("Invalid save path: {}", e))?;

        // Create installation
        let installation = GameInstallation::new(game_name, save_path.clone());
        installations.installations.push(installation);
        installations.save()?;

        println!("✅ Game '{game_name}' set up successfully with save path: {path_str}");
        
        // Ask if user wants to create the directory if it doesn't exist
        if !save_path.as_path().exists() {
            match FzfWrapper::confirm(&format!(
                "Save path '{path_str}' does not exist. Would you like to create it?"
            ))
            .map_err(|e| anyhow::anyhow!("Failed to get confirmation: {}", e))?
            {
                ConfirmResult::Yes => {
                    std::fs::create_dir_all(save_path.as_path())
                        .context("Failed to create save directory")?;
                    println!("✅ Created save directory: {path_str}");
                }
                ConfirmResult::No | ConfirmResult::Cancelled => {
                    println!("Directory not created. You can create it later when needed.");
                }
            }
        }
    } else {
        println!("Setup cancelled for game '{game_name}'.");
    }

    println!();
    Ok(())
}

/// Extract unique paths from all snapshots, grouped by frequency for better presentation
fn extract_unique_paths_from_snapshots(
    snapshots: &[crate::restic::wrapper::Snapshot],
) -> Result<Vec<PathInfo>> {
    let mut path_frequency: HashMap<String, PathInfo> = HashMap::new();

    for snapshot in snapshots {
        for path in &snapshot.paths {
            let entry = path_frequency.entry(path.clone()).or_insert(PathInfo {
                path: path.clone(),
                frequency: 0,
                devices: HashSet::new(),
                first_seen: snapshot.time.clone(),
                last_seen: snapshot.time.clone(),
            });

            entry.frequency += 1;
            entry.devices.insert(snapshot.hostname.clone());
            
            // Update first/last seen (simple string comparison should work for ISO dates)
            if snapshot.time < entry.first_seen {
                entry.first_seen = snapshot.time.clone();
            }
            if snapshot.time > entry.last_seen {
                entry.last_seen = snapshot.time.clone();
            }
        }
    }

    let mut paths: Vec<PathInfo> = path_frequency.into_values().collect();
    
    // Sort by frequency (most common first) and then by number of devices
    paths.sort_by(|a, b| {
        b.frequency
            .cmp(&a.frequency)
            .then(b.devices.len().cmp(&a.devices.len()))
    });

    Ok(paths)
}

/// Information about a path found in snapshots
#[derive(Debug, Clone)]
struct PathInfo {
    path: String,
    frequency: usize,
    devices: HashSet<String>,
    first_seen: String,
    last_seen: String,
}

impl FzfSelectable for PathInfo {
    fn fzf_display_text(&self) -> String {
        let devices_str = if self.devices.len() == 1 {
            format!("1 device: {}", self.devices.iter().next().unwrap())
        } else {
            format!("{} devices: {}", self.devices.len(), 
                    self.devices.iter().cloned().collect::<Vec<_>>().join(", "))
        };
        
        format!("{} (used {} times on {})", self.path, self.frequency, devices_str)
    }

    fn fzf_key(&self) -> String {
        self.path.clone()
    }

    fn fzf_preview(&self) -> protocol::FzfPreview {
        let mut preview = String::new();
        preview.push_str(&format!("📁 SAVE PATH DETAILS\n\n"));
        preview.push_str(&format!("Path:           {}\n", self.path));
        preview.push_str(&format!("Usage Count:    {} snapshots\n", self.frequency));
        preview.push_str(&format!("Device Count:   {} unique devices\n", self.devices.len()));
        
        preview.push_str(&format!("\n🖥️  DEVICES USING THIS PATH:\n"));
        for device in &self.devices {
            preview.push_str(&format!("  • {}\n", device));
        }
        
        // Format dates
        if let (Ok(first), Ok(last)) = (
            chrono::DateTime::parse_from_rfc3339(&self.first_seen),
            chrono::DateTime::parse_from_rfc3339(&self.last_seen),
        ) {
            let first_str = first.format("%Y-%m-%d %H:%M:%S").to_string();
            let last_str = last.format("%Y-%m-%d %H:%M:%S").to_string();
            
            preview.push_str(&format!("\n📅 USAGE TIMELINE:\n"));
            preview.push_str(&format!("First Seen:     {}\n", first_str));
            preview.push_str(&format!("Last Seen:      {}\n", last_str));
        }
        
        protocol::FzfPreview::Text(preview)
    }
}

/// Simple wrapper for string options in fzf
#[derive(Debug, Clone)]
struct StringOption {
    text: String,
    value: String,
}

impl StringOption {
    fn new(text: String, value: String) -> Self {
        Self { text, value }
    }
}

impl FzfSelectable for StringOption {
    fn fzf_display_text(&self) -> String {
        self.text.clone()
    }

    fn fzf_key(&self) -> String {
        self.value.clone()
    }
}

/// Let user choose from available paths or enter a custom one
fn choose_installation_path(
    game_name: &str,
    paths: &[PathInfo],
) -> Result<Option<String>> {
    println!("\nChoose how to set up the save path for '{}':", game_name);
    
    // Create options including the paths and a custom option
    let mut options = vec![StringOption::new(
        "[Enter custom path]".to_string(), 
        "CUSTOM".to_string()
    )];
    
    // Add existing paths
    for path_info in paths {
        options.push(StringOption::new(
            path_info.fzf_display_text(),
            path_info.path.clone(),
        ));
    }
    
    let selected = FzfWrapper::select_one(options)
        .map_err(|e| anyhow::anyhow!("Failed to select path option: {}", e))?;
    
    match selected {
        Some(selection) => {
            if selection.value == "CUSTOM" {
                // User wants to enter a custom path
                let custom_path = FzfWrapper::input(&format!(
                    "Enter custom save path for '{}' (e.g., ~/.local/share/{}/saves):",
                    game_name, game_name.to_lowercase().replace(' ', "-")
                ))
                .map_err(|e| anyhow::anyhow!("Failed to get custom path input: {}", e))?
                .trim()
                .to_string();
                
                if custom_path.is_empty() {
                    println!("Empty path provided. Setup cancelled.");
                    return Ok(None);
                }
                
                Ok(Some(custom_path))
            } else {
                // User selected one of the existing paths
                Ok(Some(selection.value))
            }
        }
        None => {
            println!("No path selected. Setup cancelled.");
            Ok(None)
        }
    }
}