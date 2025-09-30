pub mod actions;
pub mod apply;
pub mod commands;
pub mod context;
pub mod registry;
pub mod store;
pub mod ui;
pub mod users;

pub use actions::apply_clipboard_manager;
pub use commands::{dispatch_settings_command, SettingsCommands};
pub use context::SettingsContext;
pub use store::{BoolSettingKey, SettingsStore, StringSettingKey};
