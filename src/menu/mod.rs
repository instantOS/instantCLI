use crate::fzf_wrapper::{ConfirmResult, FzfOptions, FzfSelectable, FzfWrapper};
use anyhow::Result;

pub mod client;
pub mod protocol;
pub mod server;

/// Handle menu commands for shell scripts
pub fn handle_menu_command(command: MenuCommands, _debug: bool) -> Result<i32> {
    match command {
        MenuCommands::Confirm { ref message, gui } => {
            if gui {
                client::handle_gui_request(&command)
            } else {
                match FzfWrapper::confirm(&message) {
                    Ok(ConfirmResult::Yes) => Ok(0),       // Yes
                    Ok(ConfirmResult::No) => Ok(1),        // No
                    Ok(ConfirmResult::Cancelled) => Ok(2), // Cancelled
                    Err(e) => {
                        eprintln!("Error: {}", e);
                        Ok(3) // Error
                    }
                }
            }
        }
        MenuCommands::Choice {
            ref prompt,
            ref items,
            multi,
            gui,
        } => {
            if gui {
                client::handle_gui_request(&command)
            } else {
                let item_list: Vec<String> = if items.is_empty() {
                    // Read from stdin if items is empty
                    use std::io::{self, Read};
                    let mut buffer = String::new();
                    io::stdin()
                        .read_to_string(&mut buffer)
                        .map_err(|e| anyhow::anyhow!("Failed to read from stdin: {}", e))?;
                    buffer.lines().map(|s| s.to_string()).collect()
                } else {
                    // Split space-separated items from command line
                    items.split(' ').map(|s| s.to_string()).collect()
                };

                #[derive(Debug, Clone)]
                struct SelectItem {
                    text: String,
                }

                impl FzfSelectable for SelectItem {
                    fn fzf_display_text(&self) -> String {
                        self.text.clone()
                    }
                }

                let select_items: Vec<SelectItem> = item_list
                    .clone()
                    .into_iter()
                    .map(|text| SelectItem { text })
                    .collect();

                let wrapper = FzfWrapper::with_options(FzfOptions {
                    prompt: Some(prompt.clone()),
                    multi_select: multi,
                    additional_args: vec![],
                    ..Default::default()
                });

                match wrapper.select(select_items) {
                    Ok(crate::fzf_wrapper::FzfResult::Selected(item)) => {
                        println!("{}", item.text);
                        Ok(0) // Selected
                    }
                    Ok(crate::fzf_wrapper::FzfResult::MultiSelected(items)) => {
                        for item in items {
                            println!("{}", item.text);
                        }
                        Ok(0) // Selected
                    }
                    Ok(crate::fzf_wrapper::FzfResult::Cancelled) => Ok(1), // Cancelled
                    Ok(crate::fzf_wrapper::FzfResult::Error(e)) => {
                        eprintln!("Error: {}", e);
                        Ok(2) // Error
                    }
                    Err(e) => {
                        eprintln!("Error: {}", e);
                        Ok(2) // Error
                    }
                }
            }
        }
        MenuCommands::Input { ref prompt, gui } => {
            if gui {
                client::handle_gui_request(&command)
            } else {
                match FzfWrapper::input(prompt) {
                    Ok(input) => {
                        println!("{}", input);
                        Ok(0) // Success
                    }
                    Err(e) => {
                        eprintln!("Error: {}", e);
                        Ok(2) // Error
                    }
                }
            }
        }
        MenuCommands::Server { command } => handle_server_command(command),
    }
}

/// Handle server commands
pub fn handle_server_command(command: ServerCommands) -> Result<i32> {
    match command {
        ServerCommands::Launch { inside } => {
            if inside {
                server::run_server_inside()
            } else {
                server::run_server_launch()
            }
        }
    }
}

#[derive(clap::Subcommand, Debug, Clone)]
pub enum MenuCommands {
    /// Show confirmation dialog and exit with code 0 for Yes, 1 for No, 2 for Cancelled
    Confirm {
        /// Confirmation message to display
        #[arg(long, default_value = "Are you sure?")]
        message: String,
        /// Use GUI menu server instead of local fzf
        #[arg(long)]
        gui: bool,
    },
    /// Show selection menu and output choice(s) to stdout
    Choice {
        /// Selection prompt message
        #[arg(long, default_value = "Select an item:")]
        prompt: String,
        /// Items to choose from (space-separated). If empty, reads from stdin.
        #[arg(long, default_value = "")]
        items: String,
        /// Allow multiple selections
        #[arg(long)]
        multi: bool,
        /// Use GUI menu server instead of local fzf
        #[arg(long)]
        gui: bool,
    },
    /// Show text input dialog and output input to stdout
    Input {
        /// Input prompt message
        #[arg(long, default_value = "Type a value:")]
        prompt: String,
        /// Use GUI menu server instead of local fzf
        #[arg(long)]
        gui: bool,
    },
    /// Menu server management commands
    Server {
        #[command(subcommand)]
        command: ServerCommands,
    },
}

#[derive(clap::Subcommand, Debug, Clone)]
pub enum ServerCommands {
    /// Launch menu server (launches terminal with --inside mode)
    Launch {
        /// Launch terminal server instead of spawning external terminal
        #[arg(long)]
        inside: bool,
    },
}
