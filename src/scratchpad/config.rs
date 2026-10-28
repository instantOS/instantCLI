use instantos_terminal::Launcher;

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
        format!("scratchpad_{}", self.name)
    }

    pub fn workspace_name(&self) -> String {
        self.window_class()
    }

    /// Both native spawning and compositor delegation use this exact request.
    pub fn launcher(&self) -> Launcher {
        self.launch.clone().app_id(self.window_class())
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
    fn scratchpad_identity_does_not_change_command_arguments() {
        let config = ScratchpadConfig::with_launcher(
            "notes".into(),
            Launcher::new("printf").arg("hello world").terminal("foot"),
            50,
            60,
        );
        let launch = config.launcher();
        let command = launch.command_for("foot");
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            [
                "--app-id",
                "scratchpad_notes",
                "--",
                "printf",
                "hello world"
            ]
        );
        let delegated = launch.cli_args();
        assert!(
            delegated
                .windows(2)
                .any(|args| args == ["--app-id", "scratchpad_notes"])
        );
        assert!(
            delegated.ends_with(&["--", "printf", "hello world"].map(std::ffi::OsString::from))
        );
    }
}
