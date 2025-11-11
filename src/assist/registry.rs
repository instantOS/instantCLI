use crate::common::requirements::RequiredPackage;
use crate::ui::prelude::NerdFont;

/// An assist action that can be performed
#[derive(Debug, Clone)]
pub struct AssistDefinition {
    pub key: char,
    pub title: &'static str,
    pub description: &'static str,
    pub icon: NerdFont,
    pub requirements: &'static [RequiredPackage],
    pub execute: fn() -> anyhow::Result<()>,
}

pub const ASSISTS: &[AssistDefinition] = &[
    AssistDefinition {
        key: 'c',
        title: "Caffeine",
        description: "Keep system awake (prevent sleep/idle)",
        icon: NerdFont::Lightbulb,
        requirements: &[],
        execute: assists::caffeine,
    },
    AssistDefinition {
        key: 'v',
        title: "Volume",
        description: "Adjust audio volume with slider",
        icon: NerdFont::VolumeUp,
        requirements: &[],
        execute: assists::volume,
    },
];

mod assists {
    use anyhow::Result;
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
}

/// Get assist by key character
pub fn assist_by_key(key: char) -> Option<&'static AssistDefinition> {
    ASSISTS.iter().find(|a| a.key == key)
}
