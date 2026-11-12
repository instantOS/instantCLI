use instantos_terminal::Launcher;

pub const IDENTITY_PREFIX: &str = "org.instantos.scratchpad.";

/// A single GTK application-ID element, bounded by instantWM's name limit.
pub fn validate_name(name: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        !name.is_empty()
            && name.len() <= 64
            && name
                .bytes()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == b'_')
            && name
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-'),
        "invalid scratchpad name {name:?}: use 1–64 ASCII letters, digits, underscores or hyphens, starting with a letter or underscore"
    );
    Ok(())
}

pub fn name_from_identity(identity: &str) -> Option<&str> {
    identity
        .strip_prefix(IDENTITY_PREFIX)
        .filter(|name| validate_name(name).is_ok())
}

/// Scratchpad identity, geometry and a single typed terminal request.
#[derive(Debug, Clone)]
pub struct ScratchpadConfig {
    pub name: String,
    launch: Launcher,
    pub width_pct: u32,
    pub height_pct: u32,
}

impl ScratchpadConfig {
    pub fn new(name: String) -> Self {
        Self::with_launcher(name, Launcher::default(), 50, 60)
    }

    pub fn with_launcher(name: String, launch: Launcher, width_pct: u32, height_pct: u32) -> Self {
        Self {
            name,
            launch,
            width_pct,
            height_pct,
        }
    }

    pub fn window_class(&self) -> String {
        format!("{IDENTITY_PREFIX}{}", self.name)
    }

    pub fn workspace_name(&self) -> String {
        self.window_class()
    }

    /// Both native spawning and compositor delegation use this exact request.
    pub fn launcher(&self) -> anyhow::Result<Launcher> {
        validate_name(&self.name)?;
        Ok(self.launch.clone().app_id(self.window_class()))
    }
}

impl Default for ScratchpadConfig {
    fn default() -> Self {
        Self::new("instantscratchpad".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn portable_names_round_trip_and_invalid_names_cannot_launch() {
        for name in ["insmenu", "_notes", "Notes-2", "notes_2", &"a".repeat(64)] {
            let config = ScratchpadConfig::new(name.into());
            assert!(config.launcher().is_ok());
            assert_eq!(name_from_identity(&config.window_class()), Some(name));
        }
        for name in [
            "",
            "2notes",
            "notes.space",
            "two words",
            "日本語",
            "x\"",
            &"a".repeat(65),
        ] {
            let config = ScratchpadConfig::new(name.into());
            assert!(config.launcher().is_err(), "{name:?}");
            assert_eq!(name_from_identity(&config.window_class()), None);
        }
        assert_eq!(name_from_identity("scratchpad_notes"), None);
        assert_eq!(name_from_identity("com.mitchellh.ghostty"), None);
    }

    #[test]
    fn scratchpad_identity_does_not_change_command_arguments() {
        let config = ScratchpadConfig::with_launcher(
            "notes".into(),
            Launcher::new("printf").arg("hello world").terminal("foot"),
            50,
            60,
        );
        let launch = config.launcher().unwrap();
        let command = launch.command_for("foot");
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            [
                "--app-id",
                "org.instantos.scratchpad.notes",
                "--",
                "printf",
                "hello world"
            ]
        );
        let delegated = launch.cli_args();
        assert!(
            delegated
                .windows(2)
                .any(|args| args == ["--app-id", "org.instantos.scratchpad.notes"])
        );
        assert!(
            delegated.ends_with(&["--", "printf", "hello world"].map(std::ffi::OsString::from))
        );
    }
}
