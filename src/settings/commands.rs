use anyhow::Result;
use clap::{Subcommand, ValueHint};

use super::apply;

/// Navigation target for direct access to settings
#[derive(Debug, Clone)]
pub enum SettingsNavigation {
    /// Navigate to a specific setting by ID
    Setting(String),
    /// Navigate to a specific category
    Category(String),
    /// Start in search mode
    Search,
}

#[derive(Subcommand, Debug, Clone)]
pub enum SettingsCommands {
    /// Reapply settings that do not persist across reboots
    Apply,
    /// List available categories and settings
    List {
        /// Show only categories
        #[arg(long = "categories")]
        categories_only: bool,
        /// Filter by category ID
        #[arg(short = 'f', long = "filter")]
        category_filter: Option<String>,
    },
    #[command(hide = true)]
    InternalApply {
        #[arg(long = "setting-id")]
        setting_id: String,
        #[arg(long = "bool-value")]
        bool_value: Option<bool>,
        #[arg(long = "string-value")]
        string_value: Option<String>,
        #[arg(long = "settings-file", value_hint = ValueHint::FilePath)]
        settings_file: Option<std::path::PathBuf>,
    },
}

pub fn dispatch_settings_command(
    debug: bool,
    privileged_flag: bool,
    command: Option<SettingsCommands>,
    navigation: Option<SettingsNavigation>,
) -> Result<()> {
    match command {
        None => super::ui::run_settings_ui(debug, privileged_flag, navigation),
        Some(SettingsCommands::Apply) => apply::run_nonpersistent_apply(debug, privileged_flag),
        Some(SettingsCommands::List {
            categories_only,
            category_filter,
        }) => list_settings(categories_only, category_filter.as_deref()),
        Some(SettingsCommands::InternalApply {
            setting_id,
            bool_value,
            string_value,
            settings_file,
        }) => apply::run_internal_apply(
            debug,
            privileged_flag,
            &setting_id,
            bool_value,
            string_value,
            settings_file,
        ),
    }
}

fn list_settings(categories_only: bool, category_filter: Option<&str>) -> Result<()> {
    use super::registry;
    use colored::Colorize;

    if categories_only {
        println!("{}", "Available Categories:".bold());
        println!();
        for category in registry::CATEGORIES {
            let count = registry::settings_for_category(category.id).len();
            println!(
                "  {} {} ({} settings)",
                category.id.cyan(),
                category.title.bold(),
                count
            );
            println!("    {}", category.description.dimmed());
            println!();
        }
        return Ok(());
    }

    if let Some(filter) = category_filter {
        let category = registry::category_by_id(filter)
            .ok_or_else(|| anyhow::anyhow!("Category '{}' not found", filter))?;

        println!("{} {}", "Category:".bold(), category.title);
        println!("{}", category.description.dimmed());
        println!();

        let settings = registry::settings_for_category(category.id);
        if settings.is_empty() {
            println!("  {}", "No settings in this category yet.".dimmed());
            return Ok(());
        }

        for setting in settings {
            println!("  {} {}", setting.id.cyan(), setting.title.bold());
            match &setting.kind {
                registry::SettingKind::Toggle { summary, .. }
                | registry::SettingKind::Choice { summary, .. }
                | registry::SettingKind::Action { summary, .. }
                | registry::SettingKind::Command { summary, .. } => {
                    println!("    {}", summary.dimmed());
                }
            }
            println!();
        }
    } else {
        for category in registry::CATEGORIES {
            println!("{} {}", "Category:".bold(), category.title);
            println!("{}", category.description.dimmed());
            println!();

            let settings = registry::settings_for_category(category.id);
            if settings.is_empty() {
                println!("  {}", "No settings in this category yet.".dimmed());
                println!();
                continue;
            }

            for setting in settings {
                println!("  {} {}", setting.id.cyan(), setting.title.bold());
                match &setting.kind {
                    registry::SettingKind::Toggle { summary, .. }
                    | registry::SettingKind::Choice { summary, .. }
                    | registry::SettingKind::Action { summary, .. }
                    | registry::SettingKind::Command { summary, .. } => {
                        println!("    {}", summary.dimmed());
                    }
                }
            }
            println!();
        }
    }

    Ok(())
}

pub fn handle_settings_command(
    command: &Option<SettingsCommands>,
    setting: &Option<String>,
    category: &Option<String>,
    search: bool,
    debug: bool,
    internal_privileged_mode: bool,
) -> Result<()> {
    let navigation = if let Some(setting_id) = setting {
        Some(SettingsNavigation::Setting(setting_id.clone()))
    } else if let Some(category_id) = category {
        Some(SettingsNavigation::Category(category_id.clone()))
    } else if search {
        Some(SettingsNavigation::Search)
    } else {
        None
    };

    dispatch_settings_command(
        debug,
        internal_privileged_mode,
        command.clone(),
        navigation,
    )
}
