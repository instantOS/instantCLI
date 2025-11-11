use crate::common::requirements::RequiredPackage;
use crate::ui::prelude::NerdFont;

/// Required package for playerctl (media player control)
pub static PLAYERCTL_PACKAGE: RequiredPackage = RequiredPackage {
    name: "playerctl",
    arch_package_name: Some("playerctl"),
    ubuntu_package_name: Some("playerctl"),
    tests: &[crate::common::requirements::InstallTest::WhichSucceeds("playerctl")],
};

/// Required package for qrencode (QR code generation)
pub static QRENCODE_PACKAGE: RequiredPackage = RequiredPackage {
    name: "qrencode",
    arch_package_name: Some("qrencode"),
    ubuntu_package_name: Some("qrencode"),
    tests: &[crate::common::requirements::InstallTest::WhichSucceeds("qrencode")],
};

/// Required package for flameshot (screenshot and annotation tool)
pub static FLAMESHOT_PACKAGE: RequiredPackage = RequiredPackage {
    name: "flameshot",
    arch_package_name: Some("flameshot"),
    ubuntu_package_name: Some("flameshot"),
    tests: &[crate::common::requirements::InstallTest::WhichSucceeds("flameshot")],
};

/// Required packages for screenshot to clipboard (both Wayland and X11)
pub static SCREENSHOT_CLIPBOARD_PACKAGES: &[RequiredPackage] = &[
    // Wayland tools
    RequiredPackage {
        name: "slurp",
        arch_package_name: Some("slurp"),
        ubuntu_package_name: Some("slurp"),
        tests: &[crate::common::requirements::InstallTest::WhichSucceeds("slurp")],
    },
    RequiredPackage {
        name: "grim",
        arch_package_name: Some("grim"),
        ubuntu_package_name: Some("grim"),
        tests: &[crate::common::requirements::InstallTest::WhichSucceeds("grim")],
    },
    RequiredPackage {
        name: "wl-clipboard",
        arch_package_name: Some("wl-clipboard"),
        ubuntu_package_name: Some("wl-clipboard"),
        tests: &[crate::common::requirements::InstallTest::WhichSucceeds("wl-copy")],
    },
    // X11 tools
    RequiredPackage {
        name: "slop",
        arch_package_name: Some("slop"),
        ubuntu_package_name: Some("slop"),
        tests: &[crate::common::requirements::InstallTest::WhichSucceeds("slop")],
    },
    RequiredPackage {
        name: "imagemagick",
        arch_package_name: Some("imagemagick"),
        ubuntu_package_name: Some("imagemagick"),
        tests: &[crate::common::requirements::InstallTest::WhichSucceeds("import")],
    },
    RequiredPackage {
        name: "xclip",
        arch_package_name: Some("xclip"),
        ubuntu_package_name: Some("xclip"),
        tests: &[crate::common::requirements::InstallTest::WhichSucceeds("xclip")],
    },
];

/// A tree structure for organizing assists
/// 
/// This type-safe design ensures that:
/// - Actions (leaves) have execution logic
/// - Groups (branches) have children but no execution logic
/// - Invalid states (e.g., a leaf with children) are unrepresentable
#[derive(Debug, Clone)]
pub enum AssistEntry {
    /// A leaf action that can be executed
    Action(AssistAction),
    /// A group containing other assists
    Group(AssistGroup),
}

/// An executable assist action (leaf node in the assist tree)
#[derive(Debug, Clone)]
pub struct AssistAction {
    /// Single character key to trigger this action
    pub key: char,
    /// Display name shown in menus
    pub title: &'static str,
    /// Brief description of what the action does
    pub description: &'static str,
    /// Icon displayed in menus
    pub icon: NerdFont,
    /// System packages required for this action to work
    pub requirements: &'static [RequiredPackage],
    /// Function to execute when this action is selected
    pub execute: fn() -> anyhow::Result<()>,
}

/// A group of related assists (branch node in the assist tree)
#[derive(Debug, Clone)]
pub struct AssistGroup {
    /// Single character key to enter this group
    pub key: char,
    /// Display name shown in menus
    pub title: &'static str,
    /// Brief description of what this group contains
    pub description: &'static str,
    /// Icon displayed in menus
    pub icon: NerdFont,
    /// Child actions and subgroups
    pub children: &'static [AssistEntry],
}

/// The main assist registry defining all available assists
/// 
/// # Structure
/// 
/// - **Actions**: Leaf nodes that execute commands when selected
/// - **Groups**: Branch nodes that contain child actions/groups
/// 
/// # Example
/// 
/// ```ignore
/// AssistEntry::Action(AssistAction {
///     key: 'c',
///     title: "Caffeine",
///     description: "Keep system awake",
///     icon: NerdFont::Lightbulb,
///     requirements: &[],
///     execute: assists::caffeine,
/// })
/// ```
/// 
/// For grouped actions:
/// 
/// ```ignore
/// AssistEntry::Group(AssistGroup {
///     key: 'v',
///     title: "Media Navigation",
///     description: "Control media playback",
///     icon: NerdFont::Music,
///     children: &[
///         AssistEntry::Action(AssistAction { ... }),
///         AssistEntry::Action(AssistAction { ... }),
///     ],
/// })
/// ```
pub const ASSISTS: &[AssistEntry] = &[
    AssistEntry::Action(AssistAction {
        key: 'c',
        title: "Caffeine",
        description: "Keep system awake",
        icon: NerdFont::Lightbulb,
        requirements: &[],
        execute: assists::caffeine,
    }),
    AssistEntry::Action(AssistAction {
        key: 'a',
        title: "Volume",
        description: "Adjust audio volume",
        icon: NerdFont::VolumeUp,
        requirements: &[],
        execute: assists::volume,
    }),
    AssistEntry::Action(AssistAction {
        key: 'm',
        title: "Music",
        description: "Play/pause music",
        icon: NerdFont::Music,
        requirements: &[PLAYERCTL_PACKAGE],
        execute: assists::music,
    }),
    AssistEntry::Action(AssistAction {
        key: 'e',
        title: "QR Encode Clipboard",
        description: "Generate QR code from clipboard",
        icon: NerdFont::Square,
        requirements: &[QRENCODE_PACKAGE],
        execute: assists::qr_encode_clipboard,
    }),
    AssistEntry::Group(AssistGroup {
        key: 's',
        title: "Screenshot",
        description: "Screenshot and annotation tools",
        icon: NerdFont::Image,
        children: &[
            AssistEntry::Action(AssistAction {
                key: 'a',
                title: "Screenshot & Annotate",
                description: "Take screenshot with flameshot",
                icon: NerdFont::Edit,
                requirements: &[FLAMESHOT_PACKAGE],
                execute: assists::screenshot_annotate,
            }),
            AssistEntry::Action(AssistAction {
                key: 'c',
                title: "Screenshot to Clipboard",
                description: "Capture area to clipboard",
                icon: NerdFont::Clipboard,
                requirements: SCREENSHOT_CLIPBOARD_PACKAGES,
                execute: assists::screenshot_to_clipboard,
            }),
        ],
    }),
    AssistEntry::Group(AssistGroup {
        key: 'v',
        title: "Media Navigation",
        description: "Control media playback tracks",
        icon: NerdFont::Music,
        children: &[
            AssistEntry::Action(AssistAction {
                key: 'n',
                title: "Previous Track",
                description: "Go to previous track",
                icon: NerdFont::ChevronLeft,
                requirements: &[PLAYERCTL_PACKAGE],
                execute: assists::previous_track,
            }),
            AssistEntry::Action(AssistAction {
                key: 'p',
                title: "Next Track",
                description: "Go to next track",
                icon: NerdFont::ChevronRight,
                requirements: &[PLAYERCTL_PACKAGE],
                execute: assists::next_track,
            }),
        ],
    }),
];

mod assists {
    use anyhow::Result;
    use anyhow::Context;
    use std::process::Command;
    use super::super::utils;

    /// Toggle caffeine mode - keeps system awake
    pub fn caffeine() -> Result<()> {
        use crate::common::display_server::DisplayServer;

        match DisplayServer::detect() {
            DisplayServer::Wayland => {
                let command = "echo 'Caffeine running - press Ctrl+C to quit' && systemd-inhibit --what=idle --who=Caffeine --why=Caffeine --mode=block sleep inf";
                utils::launch_in_terminal(command)?;
                Ok(())
            }
            DisplayServer::X11 => {
                anyhow::bail!("X11 support is work in progress. Caffeine currently only supports Wayland.");
            }
            DisplayServer::Unknown => {
                anyhow::bail!("Unknown display server. Caffeine currently only supports Wayland.");
            }
        }
    }

    /// Volume slider control
    pub fn volume() -> Result<()> {
        utils::menu_command(&["slide", "--preset", "audio", "--gui"])
    }

    /// Music playback control using playerctl
    pub fn music() -> Result<()> {
        Command::new("playerctl")
            .arg("play-pause")
            .spawn()
            .context("Failed to control playback with playerctl")?;
        Ok(())
    }

    /// Go to previous track using playerctl
    pub fn previous_track() -> Result<()> {
        Command::new("playerctl")
            .arg("previous")
            .spawn()
            .context("Failed to go to previous track with playerctl")?;
        Ok(())
    }

    /// Go to next track using playerctl
    pub fn next_track() -> Result<()> {
        Command::new("playerctl")
            .arg("next")
            .spawn()
            .context("Failed to go to next track with playerctl")?;
        Ok(())
    }

    /// Generate QR code from clipboard contents
    pub fn qr_encode_clipboard() -> Result<()> {
        use std::io::Write;
        use crate::common::display_server::DisplayServer;

        let display_server = DisplayServer::detect();

        // Get clipboard contents using appropriate command
        let (clipboard_cmd, clipboard_args) = display_server.get_clipboard_command();
        let clipboard_content = Command::new(clipboard_cmd)
            .args(clipboard_args)
            .output()
            .with_context(|| format!("Failed to get clipboard with {}", clipboard_cmd))?
            .stdout;
        
        let clipboard_text = String::from_utf8_lossy(&clipboard_content);
        
        if clipboard_text.trim().is_empty() {
            anyhow::bail!("Clipboard is empty");
        }
        
        // Create a temporary file with the clipboard content
        let temp_content = std::env::temp_dir().join(format!("qr_content_{}.txt", std::process::id()));
        std::fs::write(&temp_content, clipboard_text.as_bytes())
            .context("Failed to write clipboard content to temp file")?;
        
        // Create a temporary script to display QR code
        let temp_script = std::env::temp_dir().join(format!("qr_display_{}.sh", std::process::id()));
        let mut script = std::fs::File::create(&temp_script)
            .context("Failed to create temporary script")?;
        
        writeln!(script, "#!/bin/bash")?;
        writeln!(script, "echo 'QR Code for clipboard contents:'")?;
        writeln!(script, "echo")?;
        writeln!(script, "cat '{}' | qrencode -t ansiutf8", temp_content.display())?;
        writeln!(script, "echo")?;
        writeln!(script, "echo 'Press any key to close...'")?;
        writeln!(script, "read -n 1")?;
        writeln!(script, "rm -f '{}' '{}'", temp_content.display(), temp_script.display())?;
        
        drop(script);
        
        // Make script executable
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&temp_script)?.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&temp_script, perms)?;
        }
        
        // Launch in terminal
        let terminal = crate::common::terminal::detect_terminal();
        
        Command::new(terminal)
            .arg("-e")
            .arg(temp_script.as_os_str())
            .spawn()
            .context("Failed to launch terminal with QR code")?;
        
        Ok(())
    }

    /// Take screenshot and annotate it using flameshot
    pub fn screenshot_annotate() -> Result<()> {
        use crate::common::display_server::DisplayServer;
        
        let display_server = DisplayServer::detect();
        
        if display_server.is_wayland() {
            // Check if flameshot is already running
            let flameshot_running = Command::new("pgrep")
                .arg("flameshot")
                .output()
                .map(|o| !o.stdout.is_empty())
                .unwrap_or(false);
            
            if !flameshot_running {
                // Start flameshot daemon in background with Wayland environment
                Command::new("flameshot")
                    .env("SDL_VIDEODRIVER", "wayland")
                    .env("_JAVA_AWT_WM_NONREPARENTING", "1")
                    .env("QT_QPA_PLATFORM", "wayland")
                    .env("XDG_CURRENT_DESKTOP", "sway")
                    .env("XDG_SESSION_DESKTOP", "sway")
                    .spawn()
                    .context("Failed to start flameshot daemon")?;
                
                // Give it time to initialize
                std::thread::sleep(std::time::Duration::from_secs(2));
            }
            
            // Launch flameshot GUI with Wayland environment
            Command::new("flameshot")
                .arg("gui")
                .env("SDL_VIDEODRIVER", "wayland")
                .env("_JAVA_AWT_WM_NONREPARENTING", "1")
                .env("QT_QPA_PLATFORM", "wayland")
                .env("XDG_CURRENT_DESKTOP", "sway")
                .env("XDG_SESSION_DESKTOP", "sway")
                .spawn()
                .context("Failed to launch flameshot gui")?;
        } else {
            // X11 - small delay seems to be needed
            std::thread::sleep(std::time::Duration::from_millis(100));
            
            // Launch flameshot GUI
            Command::new("flameshot")
                .arg("gui")
                .spawn()
                .context("Failed to launch flameshot gui")?;
        }
        
        Ok(())
    }

    /// Take a screenshot of selected area to clipboard
    pub fn screenshot_to_clipboard() -> Result<()> {
        use crate::common::display_server::DisplayServer;
        use std::io::Write;
        
        let display_server = DisplayServer::detect();
        
        if display_server.is_wayland() {
            // Get selected area using slurp
            let slurp_output = Command::new("slurp")
                .output()
                .context("Failed to run slurp for area selection")?;
            
            if !slurp_output.status.success() {
                // User cancelled selection
                return Ok(());
            }
            
            let geometry = String::from_utf8_lossy(&slurp_output.stdout).trim().to_string();
            
            if geometry.is_empty() {
                return Ok(());
            }
            
            // Capture screenshot with grim and pipe to wl-copy
            let grim_output = Command::new("grim")
                .args(["-g", &geometry, "-"])
                .output()
                .context("Failed to capture screenshot with grim")?;
            
            if !grim_output.status.success() {
                anyhow::bail!("Failed to capture screenshot");
            }
            
            // Copy to clipboard
            let mut wl_copy = Command::new("wl-copy")
                .stdin(std::process::Stdio::piped())
                .spawn()
                .context("Failed to start wl-copy")?;
            
            if let Some(mut stdin) = wl_copy.stdin.take() {
                stdin.write_all(&grim_output.stdout)
                    .context("Failed to write screenshot to wl-copy")?;
            }
            
            wl_copy.wait().context("Failed to wait for wl-copy")?;
            
        } else if display_server.is_x11() {
            // Get selected area using slop
            let slop_output = Command::new("slop")
                .arg("-f")
                .arg("%g")
                .output()
                .context("Failed to run slop for area selection")?;
            
            if !slop_output.status.success() {
                // User cancelled selection
                return Ok(());
            }
            
            let geometry = String::from_utf8_lossy(&slop_output.stdout).trim().to_string();
            
            if geometry.is_empty() {
                return Ok(());
            }
            
            // Capture screenshot with import (imagemagick) and pipe to xclip
            let import_output = Command::new("import")
                .args(["-window", "root", "-crop", &geometry, "png:-"])
                .output()
                .context("Failed to capture screenshot with import")?;
            
            if !import_output.status.success() {
                anyhow::bail!("Failed to capture screenshot");
            }
            
            // Copy to clipboard
            let mut xclip = Command::new("xclip")
                .args(["-selection", "clipboard", "-t", "image/png"])
                .stdin(std::process::Stdio::piped())
                .spawn()
                .context("Failed to start xclip")?;
            
            if let Some(mut stdin) = xclip.stdin.take() {
                stdin.write_all(&import_output.stdout)
                    .context("Failed to write screenshot to xclip")?;
            }
            
            xclip.wait().context("Failed to wait for xclip")?;
            
        } else {
            anyhow::bail!("Unknown display server - cannot take screenshot");
        }
        
        Ok(())
    }
}

impl AssistEntry {
    /// Get the key for this entry
    pub fn key(&self) -> char {
        match self {
            AssistEntry::Action(action) => action.key,
            AssistEntry::Group(group) => group.key,
        }
    }

    /// Get the title for this entry
    pub fn title(&self) -> &'static str {
        match self {
            AssistEntry::Action(action) => action.title,
            AssistEntry::Group(group) => group.title,
        }
    }

    /// Get the icon for this entry
    pub fn icon(&self) -> NerdFont {
        match self {
            AssistEntry::Action(action) => action.icon,
            AssistEntry::Group(group) => group.icon,
        }
    }
}

/// Find an assist action by its key sequence (e.g., "c" or "vn")
pub fn find_action(key_sequence: &str) -> Option<&'static AssistAction> {
    if key_sequence.is_empty() {
        return None;
    }
    
    let mut chars = key_sequence.chars();
    let first_key = chars.next()?;
    
    // Find the entry with the first key
    let entry = ASSISTS.iter().find(|entry| entry.key() == first_key)?;
    
    match entry {
        AssistEntry::Action(action) => {
            // Single key action
            if chars.next().is_none() {
                Some(action)
            } else {
                None
            }
        }
        AssistEntry::Group(group) => {
            // Multi-key sequence - search in children
            let second_key = chars.next()?;
            if chars.next().is_some() {
                // We only support 2-level depth for now
                return None;
            }
            
            group.children.iter().find_map(|child| match child {
                AssistEntry::Action(action) if action.key == second_key => Some(action),
                _ => None,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_single_key_action() {
        let action = find_action("c");
        assert!(action.is_some());
        assert_eq!(action.unwrap().title, "Caffeine");
    }

    #[test]
    fn test_find_grouped_action() {
        let action = find_action("vn");
        assert!(action.is_some());
        assert_eq!(action.unwrap().title, "Previous Track");
        
        let action = find_action("vp");
        assert!(action.is_some());
        assert_eq!(action.unwrap().title, "Next Track");
    }

    #[test]
    fn test_find_nonexistent_action() {
        assert!(find_action("z").is_none());
        assert!(find_action("vz").is_none());
        assert!(find_action("").is_none());
    }

    #[test]
    fn test_group_key_not_an_action() {
        // "v" is a group, not an action
        assert!(find_action("v").is_none());
    }

    #[test]
    fn test_find_screenshot_action() {
        let action = find_action("sa");
        assert!(action.is_some());
        assert_eq!(action.unwrap().title, "Screenshot & Annotate");
    }

    #[test]
    fn test_screenshot_group_key_not_an_action() {
        // "s" is a group, not an action
        assert!(find_action("s").is_none());
    }

    #[test]
    fn test_find_screenshot_to_clipboard_action() {
        let action = find_action("sc");
        assert!(action.is_some());
        assert_eq!(action.unwrap().title, "Screenshot to Clipboard");
    }
}
