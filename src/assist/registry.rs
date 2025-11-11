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
        let session_type = std::env::var("XDG_SESSION_TYPE").unwrap_or_default();

        if session_type == "wayland" {
            let command = "echo 'Caffeine running - press Ctrl+C to quit' && systemd-inhibit --what=idle --who=Caffeine --why=Caffeine --mode=block sleep inf";
            utils::launch_in_terminal(command)?;
            Ok(())
        } else {
            anyhow::bail!("X11 support is work in progress. Caffeine currently only supports Wayland.");
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
        
        let session_type = std::env::var("XDG_SESSION_TYPE").unwrap_or_default();
        
        // Get clipboard contents
        let clipboard_content = if session_type == "wayland" {
            Command::new("wl-paste")
                .output()
                .context("Failed to get clipboard with wl-paste")?
                .stdout
        } else {
            Command::new("xclip")
                .args(["-selection", "clipboard", "-o"])
                .output()
                .context("Failed to get clipboard with xclip")?
                .stdout
        };
        
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
        let terminal = utils::get_terminal();
        
        Command::new(terminal)
            .arg("-e")
            .arg(temp_script.as_os_str())
            .spawn()
            .context("Failed to launch terminal with QR code")?;
        
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
}
