use std::path::PathBuf;

use anyhow::{Context, Result, anyhow};

/// Desktop entry structure
#[derive(Debug, Clone)]
pub struct DesktopEntry {
    pub name: String,
    pub exec: String,
    pub icon: String,
    pub comment: String,
    pub terminal: bool,
    pub categories: Vec<String>,
}

impl DesktopEntry {
    fn new(name: &str, exec: &str, icon: &str, comment: &str, terminal: bool) -> Self {
        Self {
            name: name.to_string(),
            exec: exec.to_string(),
            icon: icon.to_string(),
            comment: comment.to_string(),
            terminal,
            categories: vec!["Game".to_string()],
        }
    }

    fn to_desktop_file_content(&self) -> String {
        let mut content = String::new();
        content.push_str("[Desktop Entry]\n");
        content.push_str("Type=Application\n");
        content.push_str(&format!("Name={}\n", self.name));
        content.push_str(&format!("Exec={}\n", self.exec));
        content.push_str(&format!("Icon={}\n", self.icon));
        content.push_str(&format!("Comment={}\n", self.comment));
        content.push_str(&format!("Terminal={}\n", if self.terminal { "true" } else { "false" }));
        content.push_str(&format!("Categories={};\n", self.categories.join(";")));
        content
    }
}

/// Get the desktop directory path
fn get_desktop_dir() -> Result<PathBuf> {
    // First try XDG_DESKTOP_DIR
    if let Ok(desktop_dir) = std::env::var("XDG_DESKTOP_DIR") {
        let path = PathBuf::from(desktop_dir);
        if path.is_dir() {
            return Ok(path);
        }
    }

    // Fallback to home directory + Desktop
    let home = dirs::home_dir().ok_or_else(|| anyhow!("Cannot determine home directory"))?;
    let desktop = home.join("Desktop");

    if desktop.is_dir() {
        return Ok(desktop);
    }

    // Some systems use lowercase 'desktop'
    let desktop_lower = home.join("desktop");
    if desktop_lower.is_dir() {
        return Ok(desktop_lower);
    }

    // If neither exists, create Desktop
    std::fs::create_dir_all(&desktop)?;
    Ok(desktop)
}

/// Get the applications directory for user desktop entries
fn get_applications_dir() -> Result<PathBuf> {
    // Try XDG_DATA_HOME first
    let data_home = if let Ok(data_home) = std::env::var("XDG_DATA_HOME") {
        PathBuf::from(data_home)
    } else {
        let home = dirs::home_dir().ok_or_else(|| anyhow!("Cannot determine home directory"))?;
        home.join(".local/share")
    };

    let applications = data_home.join("applications");
    std::fs::create_dir_all(&applications)?;
    Ok(applications)
}

/// Generate a safe filename from a game name
fn sanitize_filename(name: &str) -> String {
    name.to_lowercase()
        .replace(|c: char| !c.is_alphanumeric() && c != '-' && c != '_', "-")
        .replace("--", "-")
        .trim_matches('-')
        .to_string()
}

/// Check if a desktop entry file exists for a game
fn get_desktop_file_path(name: &str, in_desktop_dir: bool) -> Result<Option<PathBuf>> {
    let filename = format!("ins-game-{}.desktop", sanitize_filename(name));

    if in_desktop_dir {
        let desktop = get_desktop_dir()?;
        let path = desktop.join(&filename);
        if path.exists() {
            return Ok(Some(path));
        }
    } else {
        let applications = get_applications_dir()?;
        let path = applications.join(&filename);
        if path.exists() {
            return Ok(Some(path));
        }
    }

    Ok(None)
}

/// Check if a game has a desktop shortcut
pub fn is_game_on_desktop(name: &str) -> Result<bool> {
    // Check both desktop and applications directories
    Ok(get_desktop_file_path(name, true)?.is_some()
        || get_desktop_file_path(name, false)?.is_some())
}

/// Get the path to an existing desktop entry file for a game
pub fn get_game_desktop_path(name: &str) -> Result<Option<PathBuf>> {
    // Prefer desktop dir, fall back to applications dir
    if let Some(path) = get_desktop_file_path(name, true)? {
        return Ok(Some(path));
    }
    get_desktop_file_path(name, false)
}

/// Detect if running as an AppImage binary
fn is_appimage() -> bool {
    // Check APPIMAGE environment variable (set by AppImage runtime)
    if std::env::var("APPIMAGE").is_ok() {
        return true;
    }
    // Fallback: check if current exe path ends with .AppImage
    if let Ok(current_exe) = std::env::current_exe()
        && let Some(path_str) = current_exe.to_str()
    {
        return path_str.to_lowercase().ends_with(".appimage");
    }
    false
}

/// Detect if running on SteamOS
fn is_steamos() -> Result<bool> {
    if let Ok(os_release) = std::fs::read_to_string("/etc/os-release")
        && (os_release.contains("steamdeck") || os_release.contains("SteamOS"))
    {
        return Ok(true);
    }
    Ok(std::env::var("STEAM_DECK").is_ok())
}

/// Build exec command for desktop entries
/// On SteamOS in gaming mode, AppImages need --appimage-extract-and-run
fn build_exec_command(game_name: &str) -> String {
    let ins_bin = std::env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "ins".to_string());

    if is_appimage() {
        if is_steamos().unwrap_or(false) {
            // SteamOS gaming mode doesn't have FUSE
            format!(
                "{} --appimage-extract-and-run game launch \"{}\"",
                ins_bin, game_name
            )
        } else {
            format!("{} game launch \"{}\"", ins_bin, game_name)
        }
    } else {
        format!("{} game launch \"{}\"", ins_bin, game_name)
    }
}

/// Add a game to the desktop as a .desktop entry
pub fn add_game_to_desktop(
    name: &str,
    _launch_command: &str,
) -> Result<(bool, Option<PathBuf>)> {
    // Check if already exists
    if is_game_on_desktop(name)? {
        return Ok((false, get_game_desktop_path(name)?));
    }

    let exec = build_exec_command(name);
    let icon = "applications-games";
    let comment = format!("Launch {} with automatic save sync", name);

    let entry = DesktopEntry::new(name, &exec, icon, &comment, false);
    let content = entry.to_desktop_file_content();
    let filename = format!("ins-game-{}.desktop", sanitize_filename(name));

    // Try to write to Desktop first, fall back to applications dir
    let (path, location) = match get_desktop_dir() {
        Ok(desktop) => {
            let path = desktop.join(&filename);
            match std::fs::write(&path, &content) {
                Ok(_) => (path, "Desktop"),
                Err(_) => {
                    let apps = get_applications_dir()?;
                    let path = apps.join(&filename);
                    std::fs::write(&path, &content)?;
                    (path, "applications directory")
                }
            }
        }
        Err(_) => {
            let apps = get_applications_dir()?;
            let path = apps.join(&filename);
            std::fs::write(&path, &content)?;
            (path, "applications directory")
        }
    };

    // Make the file executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = std::fs::metadata(&path)?;
        let mut permissions = metadata.permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&path, permissions)?;
    }

    eprintln!("Created desktop shortcut at: {}", path.display());
    eprintln!("Location: {}", location);

    Ok((true, Some(path)))
}

/// Remove a game from the desktop
pub fn remove_game_from_desktop(name: &str) -> Result<bool> {
    if let Some(path) = get_game_desktop_path(name)? {
        std::fs::remove_file(&path)
            .with_context(|| format!("Failed to remove desktop file: {}", path.display()))?;
        eprintln!("Removed desktop shortcut: {}", path.display());
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Add the game menu to the desktop
pub fn add_menu_to_desktop() -> Result<(bool, Option<PathBuf>)> {
    use crate::common::terminal::detect_terminal;

    let menu_name = "ins game menu";

    // Check if already exists
    if is_game_on_desktop(menu_name)? {
        return Ok((false, get_game_desktop_path(menu_name)?));
    }

    let ins_bin = std::env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "ins".to_string());

    let terminal = detect_terminal();
    let terminal_path = which::which(&terminal).context("Failed to find terminal emulator")?;
    let terminal_str = terminal_path.to_string_lossy().to_string();

    // Build exec command
    let exec = if is_appimage() && is_steamos().unwrap_or(false) {
        format!(
            "{} -- \"{}\" --appimage-extract-and-run game menu",
            terminal_str, ins_bin
        )
    } else {
        format!("{} -- \"{}\" game menu", terminal_str, ins_bin)
    };

    let icon = "utilities-terminal";
    let comment = "Launch the ins game menu";

    let entry = DesktopEntry::new(menu_name, &exec, icon, comment, false);
    let content = entry.to_desktop_file_content();
    let filename = format!("ins-game-{}.desktop", sanitize_filename(menu_name));

    // Try to write to Desktop first, fall back to applications dir
    let (path, location) = match get_desktop_dir() {
        Ok(desktop) => {
            let path = desktop.join(&filename);
            match std::fs::write(&path, &content) {
                Ok(_) => (path, "Desktop"),
                Err(_) => {
                    let apps = get_applications_dir()?;
                    let path = apps.join(&filename);
                    std::fs::write(&path, &content)?;
                    (path, "applications directory")
                }
            }
        }
        Err(_) => {
            let apps = get_applications_dir()?;
            let path = apps.join(&filename);
            std::fs::write(&path, &content)?;
            (path, "applications directory")
        }
    };

    // Make the file executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = std::fs::metadata(&path)?;
        let mut permissions = metadata.permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&path, permissions)?;
    }

    eprintln!("Created desktop shortcut at: {}", path.display());
    eprintln!("Location: {}", location);

    Ok((true, Some(path)))
}

/// Remove the game menu from the desktop
pub fn remove_menu_from_desktop() -> Result<bool> {
    let menu_name = "ins game menu";
    remove_game_from_desktop(menu_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_desktop_entry_formatting() {
        let entry = DesktopEntry::new(
            "Test Game",
            "ins game launch \"Test Game\"",
            "applications-games",
            "Launch Test Game",
            false,
        );

        let content = entry.to_desktop_file_content();
        assert!(content.contains("[Desktop Entry]"));
        assert!(content.contains("Type=Application"));
        assert!(content.contains("Name=Test Game"));
        assert!(content.contains("Exec=ins game launch"));
        assert!(content.contains("Icon=applications-games"));
        assert!(content.contains("Comment=Launch Test Game"));
        assert!(content.contains("Terminal=false"));
        assert!(content.contains("Categories=Game;"));
    }

    #[test]
    fn test_sanitize_filename() {
        assert_eq!(sanitize_filename("Test Game"), "test-game");
        assert_eq!(sanitize_filename("Super Mario 64"), "super-mario-64");
        assert_eq!(sanitize_filename("Game\\Name"), "game-name");
        assert_eq!(sanitize_filename("--Game--"), "game");
    }
}
