use anyhow::Result;
use clap::Subcommand;

pub mod browser;
pub mod menu;
pub mod operations;
pub mod types;
pub mod utils;

#[cfg(test)]
mod tests;

#[derive(Subcommand, Debug, Clone)]
pub enum PassCommands {
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
        #[arg(long, default_value_t = 20)]
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
use menu::{interactive_pass_menu, interactive_pass_menu_server};
use operations::{
    copy_otp_flow, delete_entry_flow, export_entry_flow, generate_password_entry, insert_otp_entry,
    insert_password_entry,
};
use utils::ensure_otp_dependency;
use utils::{ensure_core_dependencies, ensure_password_store_dir, load_entries};

pub fn pass_entry_names() -> Vec<String> {
    let Ok(store_dir) = utils::password_store_dir() else {
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

pub fn handle_pass_command(
    gui: bool,
    debug: bool,
    list_only: bool,
    command: Option<PassCommands>,
) -> Result<i32> {
    match command {
        Some(PassCommands::Menu { gui: menu_gui }) => {
            if menu_gui {
                crate::common::terminal::launch_menu_in_terminal("pass", "Pass", &[], debug)?;
                return Ok(0);
            }
            interactive_pass_menu()
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
            ensure_otp_dependency()?;
            copy_otp_flow(name)?;
            Ok(0)
        }
        Some(PassCommands::Export { name, path }) => {
            ensure_core_dependencies()?;
            export_entry_flow(name, path)?;
            Ok(0)
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
