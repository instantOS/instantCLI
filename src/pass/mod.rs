use anyhow::Result;
use clap::Subcommand;

pub mod browser;
mod clipboard;
mod content;
mod dependencies;
pub mod menu;
mod names;
pub mod operations;
mod pass_cli;
mod prompts;
mod setup;
mod sort;
mod store;
pub mod totp;
pub mod types;

#[cfg(test)]
mod tests;

#[derive(Subcommand, Debug, Clone)]
pub enum PassCommands {
    /// Set up the password store and optionally initialize Git
    Setup,
    /// Interactive pass menu (browse, copy, edit passwords)
    Menu {
        /// Open the menu in a GUI terminal window
        #[arg(long = "gui")]
        gui: bool,
    },
    /// Insert a password or OTP entry
    Add {
        /// Entry name (optional, prompts if omitted)
        #[arg(add = clap_complete::engine::ArgValueCompleter::new(
            crate::completions::pass_entry_completion
        ))]
        name: Option<String>,
        /// Store an OTP URI instead of a password
        #[arg(long)]
        otp: bool,
    },
    /// Generate a random password and store it in pass
    Generate {
        /// Entry name (optional, prompts if omitted)
        #[arg(add = clap_complete::engine::ArgValueCompleter::new(
            crate::completions::pass_entry_completion
        ))]
        name: Option<String>,
        /// Generated password length
        #[arg(long, default_value_t = types::DEFAULT_PASSWORD_LENGTH)]
        length: usize,
    },
    /// Delete a pass entry
    Delete {
        /// Entry name (optional, prompts if omitted)
        #[arg(add = clap_complete::engine::ArgValueCompleter::new(
            crate::completions::pass_entry_completion
        ))]
        name: Option<String>,
    },
    /// Copy the OTP code for an entry
    Otp {
        /// Entry name (optional, prompts if omitted)
        #[arg(add = clap_complete::engine::ArgValueCompleter::new(
            crate::completions::pass_entry_completion
        ))]
        name: Option<String>,
    },
    /// Export a decrypted entry to a file instead of copying it
    Export {
        /// Entry name (optional, prompts if omitted)
        #[arg(add = clap_complete::engine::ArgValueCompleter::new(
            crate::completions::pass_entry_completion
        ))]
        name: Option<String>,
        /// Output path (optional, prompts if omitted)
        path: Option<String>,
    },
}
use dependencies::{ensure_core_dependencies, ensure_otp_dependency};
use menu::{
    interactive_pass_menu, interactive_pass_menu_instantmenu, interactive_pass_menu_server,
    interactive_pass_tree_menu,
};
use operations::{
    copy_otp_flow, delete_entry_flow, export_entry_flow, generate_password_entry, insert_otp_entry,
    insert_password_entry,
};
use store::{ensure_password_store_dir, load_entries, password_store_dir};

pub fn pass_entry_names() -> Vec<String> {
    let Ok(store_dir) = password_store_dir() else {
        return Vec::new();
    };
    let Ok(entries) = load_entries(&store_dir) else {
        return Vec::new();
    };
    entries
        .into_iter()
        .map(|entry| entry.display_name)
        .collect()
}

fn auto_setup_applies(command: Option<&PassCommands>, list_only: bool, terminal: bool) -> bool {
    match command {
        None => !list_only,
        Some(PassCommands::Menu { gui }) => !gui,
        Some(PassCommands::Add { .. } | PassCommands::Generate { .. }) => terminal,
        _ => false,
    }
}

pub fn handle_pass_command(
    gui: bool,
    instantmenu: bool,
    debug: bool,
    list_only: bool,
    command: Option<PassCommands>,
) -> Result<i32> {
    if auto_setup_applies(command.as_ref(), list_only, setup::interactive_terminal())
        && !setup::onboard_if_new(debug)?
    {
        return Ok(1);
    }

    match command {
        Some(PassCommands::Setup) => Ok(if setup::run_setup()? { 0 } else { 1 }),
        Some(PassCommands::Menu { gui: menu_gui }) => {
            if menu_gui {
                crate::common::terminal::launch_menu_in_terminal("pass", "Pass", &[], debug)?;
                return Ok(0);
            }
            interactive_pass_tree_menu()
        }
        Some(PassCommands::Add { name, otp }) => {
            ensure_core_dependencies()?;
            if otp {
                ensure_otp_dependency()?;
                insert_otp_entry(name)?;
            } else {
                insert_password_entry(name)?;
            }
            Ok(0)
        }
        Some(PassCommands::Generate { name, length }) => {
            ensure_core_dependencies()?;
            generate_password_entry(name, length)?;
            Ok(0)
        }
        Some(PassCommands::Delete { name }) => {
            ensure_core_dependencies()?;
            delete_entry_flow(name)?;
            Ok(0)
        }
        Some(PassCommands::Otp { name }) => {
            ensure_core_dependencies()?;
            copy_otp_flow(name)?;
            Ok(0)
        }
        Some(PassCommands::Export { name, path }) => {
            ensure_core_dependencies()?;
            export_entry_flow(name, path)?;
            Ok(0)
        }
        None if instantmenu => {
            let result = interactive_pass_menu_instantmenu(debug);
            if let Err(error) = &result
                && let Err(dialog_error) = crate::menu::instantmenu::InstantmenuBackend::message(
                    Some("INS Pass"),
                    &format!("{error:#}"),
                )
            {
                eprintln!("Could not display error: {dialog_error:#}");
            }
            result
        }
        None if list_only => {
            ensure_core_dependencies()?;
            let store_dir = ensure_password_store_dir()?;
            for entry in load_entries(&store_dir)? {
                println!("{}", entry.display_name);
            }
            Ok(0)
        }
        None if gui => interactive_pass_menu_server(),
        None => interactive_pass_menu(),
    }
}
