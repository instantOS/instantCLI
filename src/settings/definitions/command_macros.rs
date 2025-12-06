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
/// - `$category` - Category enum variant
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
///     Category::Storage,
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
        $category:expr,
        $icon:expr,
        $summary:expr,
        $command:expr,
        $requirement:expr
    ) => {
        pub struct $struct_name;

        impl $crate::settings::setting::Setting for $struct_name {
            fn metadata(&self) -> $crate::settings::setting::SettingMetadata {
                $crate::settings::setting::SettingMetadata {
                    id: $id,
                    title: $title,
                    category: $category,
                    icon: $icon,
                    breadcrumbs: &[$title],
                    summary: $summary,
                    requires_reapply: false,
                    requirements: &[$crate::settings::setting::Requirement::Package(
                        $requirement,
                    )],
                }
            }

            fn setting_type(&self) -> $crate::settings::setting::SettingType {
                $crate::settings::setting::SettingType::Command
            }

            fn apply(
                &self,
                ctx: &mut $crate::settings::context::SettingsContext,
            ) -> anyhow::Result<()> {
                use anyhow::Context;
                use std::process::{Command, Stdio};

                ctx.emit_info(
                    "settings.command.launching",
                    &format!("Launching {}...", $title),
                );
                Command::new($command)
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

        inventory::submit! { &$struct_name as &'static dyn $crate::settings::setting::Setting }
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
/// - `$category` - Category enum variant
/// - `$icon` - NerdFont icon
/// - `$summary` - Description text
/// - `$command` - Command to execute (as string literal)
/// - `$requirement` - Package requirement
///
/// # Example
/// ```ignore
/// tui_command_setting!(
///     LaunchWiremix,
///     "audio.wiremix",
///     "General audio settings",
///     Category::Audio,
///     NerdFont::Settings,
///     "Launch wiremix TUI to manage PipeWire routing and volumes.",
///     "wiremix",
///     WIREMIX_PACKAGE
/// );
/// ```
#[macro_export]
macro_rules! tui_command_setting {
    (
        $struct_name:ident,
        $id:expr,
        $title:expr,
        $category:expr,
        $icon:expr,
        $summary:expr,
        $command:expr,
        $requirement:expr
    ) => {
        pub struct $struct_name;

        impl $crate::settings::setting::Setting for $struct_name {
            fn metadata(&self) -> $crate::settings::setting::SettingMetadata {
                $crate::settings::setting::SettingMetadata {
                    id: $id,
                    title: $title,
                    category: $category,
                    icon: $icon,
                    breadcrumbs: &[$title],
                    summary: $summary,
                    requires_reapply: false,
                    requirements: &[$crate::settings::setting::Requirement::Package(
                        $requirement,
                    )],
                }
            }

            fn setting_type(&self) -> $crate::settings::setting::SettingType {
                $crate::settings::setting::SettingType::Command
            }

            fn apply(
                &self,
                ctx: &mut $crate::settings::context::SettingsContext,
            ) -> anyhow::Result<()> {
                use anyhow::Context;
                use duct::cmd;

                ctx.emit_info(
                    "settings.command.launching",
                    &format!("Launching {}...", $title),
                );
                cmd!($command)
                    .run()
                    .context(concat!("running ", $command))?;
                ctx.emit_success("settings.command.completed", &format!("Exited {}", $title));
                Ok(())
            }
        }

        inventory::submit! { &$struct_name as &'static dyn $crate::settings::setting::Setting }
    };
}
