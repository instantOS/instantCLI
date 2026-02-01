//! Macros for creating command-launching settings
//!
//! Provides macros to reduce boilerplate for settings that just launch external programs.

/// Create a setting that launches a GUI application in the background.
///
/// GUI applications are spawned with stdout/stderr redirected to /dev/null
/// to prevent their logs from interfering with the settings TUI.
///
/// # Arguments
/// - `$struct_name` - Name of the struct to create
/// - `$id` - Setting ID (e.g. "storage.disks")
/// - `$title` - Display title
/// - `$icon` - NerdFont icon
/// - `$summary` - Description text
/// - `$command` - Command to execute (as string literal)
/// - `$requirement` - Package requirement
///
/// # Example
/// ```ignore
/// gui_command_setting!(
///     DiskManagement,
///     "storage.disks",
///     "Disk management",
///     NerdFont::HardDrive,
///     "Launch GNOME Disks to manage drives and partitions.",
///     "gnome-disks",
///     GNOME_DISKS_PACKAGE
/// );
/// ```
#[macro_export]
macro_rules! gui_command_setting {
    (
        $struct_name:ident,
        $id:expr,
        $title:expr,
        $icon:expr,
        $summary:expr,
        $command:expr,
        $requirement:expr
    ) => {
        pub struct $struct_name;

        impl $crate::settings::setting::Setting for $struct_name {
            fn metadata(&self) -> $crate::settings::setting::SettingMetadata {
                $crate::settings::setting::SettingMetadata::builder()
                    .id($id)
                    .title($title)
                    .icon($icon)
                    .summary($summary)
                    .requirements(vec![$requirement])
                    .build()
            }

            fn setting_type(&self) -> $crate::settings::setting::SettingType {
                $crate::settings::setting::SettingType::Command
            }

            fn preview_command(&self) -> Option<String> {
                let script = $crate::ui::preview::PreviewBuilder::new()
                    .header($icon, $title)
                    .text($summary)
                    .separator()
                    .field("Command", $command)
                    .build_shell_script();
                Some(script)
            }

            fn apply(
                &self,
                ctx: &mut $crate::settings::context::SettingsContext,
            ) -> anyhow::Result<()> {
                use anyhow::Context;
                use std::path::{Path, PathBuf};
                use std::process::{Command, Stdio};

                ctx.emit_info(
                    "settings.command.launching",
                    &format!("Launching {}...", $title),
                );

                // Some tools (notably Piper) are python scripts with `#!/usr/bin/env python3`.
                // When mise is active, `python3` may resolve to a shim/interpreter without system
                // GI bindings, so force system python for these scripts.
                let resolved_path: PathBuf =
                    which::which($command).unwrap_or_else(|_| $command.into());
                let env_python3_shebang = std::fs::File::open(&resolved_path)
                    .ok()
                    .and_then(|mut f| {
                        use std::io::Read;

                        // Cap the amount of data we read so we never accidentally slurp a large
                        // binary that doesn't contain a newline early.
                        let mut buf = [0u8; 128];
                        let n = f.read(&mut buf).ok()?;
                        let slice = &buf[..n];
                        let end = slice.iter().position(|&b| b == b'\n').unwrap_or(n);
                        let mut line = &slice[..end];
                        if line.ends_with(b"\r") {
                            line = &line[..line.len().saturating_sub(1)];
                        }

                        Some(line == b"#!/usr/bin/env python3")
                    })
                    .unwrap_or(false);

                let mut command = if env_python3_shebang && Path::new("/usr/bin/python3").exists() {
                    let mut c = Command::new("/usr/bin/python3");
                    c.arg(resolved_path);
                    c.env_remove("VIRTUAL_ENV")
                        .env_remove("CONDA_PREFIX")
                        .env_remove("PYTHONHOME")
                        .env_remove("PYTHONPATH");
                    c
                } else {
                    Command::new($command)
                };

                command
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .context(concat!("launching ", $command))?;
                ctx.emit_success(
                    "settings.command.completed",
                    &format!("Launched {}", $title),
                );
                Ok(())
            }
        }
    };
}

/// Create a setting that launches a TUI application synchronously.
///
/// TUI applications run in the foreground and take over the terminal.
/// They block until the user exits the application.
///
/// # Arguments
/// - `$struct_name` - Name of the struct to create
/// - `$id` - Setting ID (e.g. "audio.wiremix")
/// - `$title` - Display title
/// - `$icon` - NerdFont icon
/// - `$summary` - Description text
/// - `$command` - Command to execute (as string literal)
/// - `$requirement` - Package requirement
/// - `$search_keywords` - Optional array of search keywords for discoverability
///
/// # Example
/// ```ignore
/// tui_command_setting!(
///     LaunchWiremix,
///     "audio.wiremix",
///     "General audio settings",
///     NerdFont::Settings,
///     "Launch wiremix TUI to manage PipeWire routing and volumes.",
///     "wiremix",
///     WIREMIX_PACKAGE,
///     &["audio", "volume", "sound"]
/// );
/// ```
#[macro_export]
macro_rules! tui_command_setting {
    (
        $struct_name:ident,
        $id:expr,
        $title:expr,
        $icon:expr,
        $summary:expr,
        $command:expr,
        $requirement:expr
        $(, $search_keywords:expr)?
    ) => {
        pub struct $struct_name;

        impl $crate::settings::setting::Setting for $struct_name {
            fn metadata(&self) -> $crate::settings::setting::SettingMetadata {
                $crate::settings::setting::SettingMetadata::builder()
                    .id($id)
                    .title($title)
                    .icon($icon)
                    .summary($summary)
                    .requirements(vec![$requirement])
                    $(.search_keywords($search_keywords))?
                    .build()
            }

            fn setting_type(&self) -> $crate::settings::setting::SettingType {
                $crate::settings::setting::SettingType::Command
            }

            fn preview_command(&self) -> Option<String> {
                let script = $crate::ui::preview::PreviewBuilder::new()
                    .header($icon, $title)
                    .text($summary)
                    .separator()
                    .field("Command", $command)
                    .build_shell_script();
                Some(script)
            }

            fn apply(
                &self,
                ctx: &mut $crate::settings::context::SettingsContext,
            ) -> anyhow::Result<()> {
                use anyhow::Context;

                ctx.emit_info(
                    "settings.command.launching",
                    &format!("Launching {}...", $title),
                );

                // Use the existing run_tui_program function which properly handles:
                // - /dev/tty for terminal control
                // - SIGINT (Ctrl+C) signal handling
                // - spawn_blocking for async runtime compatibility
                //
                // We use block_in_place because apply() is sync but we're in a tokio runtime
                let result = tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::try_current()
                        .context("No tokio runtime available")?
                        .block_on($crate::common::terminal::run_tui_program($command, &[]))
                })?;

                if result {
                    ctx.emit_success("settings.command.completed", &format!("Exited {}", $title));
                } else {
                    ctx.emit_info(
                        "settings.command.cancelled",
                        &format!("{} was cancelled", $title),
                    );
                }
                Ok(())
            }
        }
    };
}

/// Create a simple boolean toggle setting with custom success messages.
///
/// This macro reduces boilerplate for settings that just toggle a boolean value
/// and display a success message. The setting will automatically flip its value
/// when applied and emit appropriate success messages.
///
/// # Arguments
/// - `$struct_name` - Name of the struct to create
/// - `$id` - Setting ID (e.g. "system.welcome_autostart")
/// - `$title` - Display title
/// - `$icon` - NerdFont icon
/// - `$summary` - Description text
/// - `$default` - Default boolean value (true or false)
/// - `$enabled_msg` - Message to show when enabled
/// - `$disabled_msg` - Message to show when disabled
///
/// # Example
/// ```ignore
/// simple_toggle_setting!(
///     WelcomeAutostart,
///     "system.welcome_autostart",
///     "Welcome app on startup",
///     NerdFont::Home,
///     "Show the welcome application automatically when logging in.",
///     true,
///     "Welcome app will appear on next startup",
///     "Welcome app autostart has been disabled"
/// );
/// ```
#[macro_export]
macro_rules! simple_toggle_setting {
    (
        $struct_name:ident,
        $id:expr,
        $title:expr,
        $icon:expr,
        $summary:expr,
        $default:expr,
        $enabled_msg:expr,
        $disabled_msg:expr
    ) => {
        pub struct $struct_name;

        impl $struct_name {
            const KEY: $crate::settings::store::BoolSettingKey =
                $crate::settings::store::BoolSettingKey::new($id, $default);
        }

        impl $crate::settings::setting::Setting for $struct_name {
            fn metadata(&self) -> $crate::settings::setting::SettingMetadata {
                $crate::settings::setting::SettingMetadata::builder()
                    .id($id)
                    .title($title)
                    .icon($icon)
                    .summary($summary)
                    .build()
            }

            fn setting_type(&self) -> $crate::settings::setting::SettingType {
                $crate::settings::setting::SettingType::Toggle { key: Self::KEY }
            }

            fn apply(
                &self,
                ctx: &mut $crate::settings::context::SettingsContext,
            ) -> anyhow::Result<()> {
                let current = ctx.bool(Self::KEY);
                let target = !current;
                ctx.set_bool(Self::KEY, target);

                if target {
                    ctx.emit_success(concat!($id, ".enabled"), $enabled_msg);
                } else {
                    ctx.emit_success(concat!($id, ".disabled"), $disabled_msg);
                }

                Ok(())
            }
        }
    };
}
