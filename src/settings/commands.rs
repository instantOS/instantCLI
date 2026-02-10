use anyhow::Result;
use clap::{Subcommand, ValueHint};
use clap_complete::engine::ArgValueCompleter;

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
        #[arg(add = ArgValueCompleter::new(crate::completions::settings_category_completion))]
        category_filter: Option<String>,
    },
    /// Internal: Generate flatpak app list for install menu
    #[command(hide = true)]
    InternalGenerateFlatpakList {
        /// Filter apps by keyword (name, id, or description)
        #[arg(short = 'k', long = "keyword")]
        keyword: Option<String>,
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
        Some(SettingsCommands::InternalGenerateFlatpakList { keyword }) => {
            super::flatpak_list::generate_and_print_list(keyword.as_deref())
        }
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
    use super::category_tree::category_tree;
    use super::setting::Category;
    use colored::Colorize;

    fn collect_settings_from_tree(
        nodes: &[super::category_tree::CategoryNode],
    ) -> Vec<&'static dyn super::setting::Setting> {
        let mut settings = Vec::new();
        for node in nodes {
            if let Some(setting) = node.setting {
                settings.push(setting);
            }
            settings.extend(collect_settings_from_tree(&node.children));
        }
        settings
    }

    if categories_only {
        println!("{}", "Available Categories:".bold());
        println!();
        for category in Category::all() {
            let meta = category.meta();
            let tree = category_tree(*category);
            let count = collect_settings_from_tree(&tree).len();
            println!(
                "  {} {} ({} settings)",
                meta.id.cyan(),
                meta.title.bold(),
                count
            );
            println!("    {}", meta.description.dimmed());
            println!();
        }
        return Ok(());
    }

    if let Some(filter) = category_filter {
        let category = Category::from_id(filter)
            .ok_or_else(|| anyhow::anyhow!("Category '{}' not found", filter))?;
        let cat_meta = category.meta();

        println!("{} {}", "Category:".bold(), cat_meta.title);
        println!("{}", cat_meta.description.dimmed());
        println!();

        let tree = category_tree(category);
        let settings = collect_settings_from_tree(&tree);
        if settings.is_empty() {
            println!("  {}", "No settings in this category yet.".dimmed());
            return Ok(());
        }

        for s in settings {
            let meta = s.metadata();
            println!("  {} {}", meta.id.cyan(), meta.title.bold());
            println!("    {}", first_line(meta.summary).dimmed());
            println!();
        }
    } else {
        for category in Category::all() {
            let tree = category_tree(*category);
            let settings = collect_settings_from_tree(&tree);
            if settings.is_empty() {
                continue;
            }
            let cat_meta = category.meta();

            println!("{} {}", "Category:".bold(), cat_meta.title);
            println!("{}", cat_meta.description.dimmed());
            println!();

            for s in settings {
                let meta = s.metadata();
                println!("  {} {}", meta.id.cyan(), meta.title.bold());
                println!("    {}", first_line(meta.summary).dimmed());
            }
            println!();
        }
    }

    Ok(())
}

fn first_line(s: &str) -> &str {
    s.lines().next().unwrap_or(s)
}

impl SettingsNavigation {
    pub fn from_args(
        setting: &Option<String>,
        category: &Option<String>,
        search: bool,
    ) -> Option<Self> {
        if let Some(id) = setting {
            Some(Self::Setting(id.clone()))
        } else if let Some(id) = category {
            Some(Self::Category(id.clone()))
        } else if search {
            Some(Self::Search)
        } else {
            None
        }
    }
}

pub fn handle_settings_command(
    command: &Option<SettingsCommands>,
    navigation: Option<SettingsNavigation>,
    gui: bool,
    debug: bool,
    internal_privileged_mode: bool,
) -> Result<()> {
    if gui {
        return launch_settings_in_terminal(&navigation, debug);
    }
    dispatch_settings_command(debug, internal_privileged_mode, command.clone(), navigation)
}

fn launch_settings_in_terminal(navigation: &Option<SettingsNavigation>, debug: bool) -> Result<()> {
    let mut args: Vec<String> = vec![];

    if debug {
        args.push("--debug".to_string());
    }

    args.push("settings".to_string());

    match navigation {
        Some(SettingsNavigation::Setting(id)) => {
            args.push("--setting".to_string());
            args.push(id.clone());
        }
        Some(SettingsNavigation::Category(id)) => {
            args.push("--category".to_string());
            args.push(id.clone());
        }
        Some(SettingsNavigation::Search) => {
            args.push("--search".to_string());
        }
        None => {}
    }

    let current_exe = std::env::current_exe()?;
    let exe_str = current_exe.to_string_lossy();

    crate::common::terminal::TerminalLauncher::new(exe_str.as_ref())
        .class("ins-settings")
        .title("Settings")
        .args(&args)
        .launch()
}
