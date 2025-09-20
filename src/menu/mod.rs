use crate::fzf_wrapper::{ConfirmResult, FzfOptions, FzfPreview, FzfWrapper};
use anyhow::Result;
use protocol::SerializableMenuItem;

pub mod client;
pub mod processing;
pub mod protocol;
pub mod scratchpad_manager;
pub mod server;
pub mod tui;
use client::MenuClient;

/// Handle menu commands for shell scripts
pub async fn handle_menu_command(command: MenuCommands, _debug: bool) -> Result<i32> {
    match command {
        MenuCommands::Confirm { ref message, gui } => {
            if gui {
                client::handle_gui_request(&command)
            } else {
                match FzfWrapper::confirm(message) {
                    Ok(ConfirmResult::Yes) => Ok(0),       // Yes
                    Ok(ConfirmResult::No) => Ok(1),        // No
                    Ok(ConfirmResult::Cancelled) => Ok(2), // Cancelled
                    Err(e) => {
                        eprintln!("Error: {e}");
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
                let item_list: Vec<SerializableMenuItem> = if items.is_empty() {
                    // Read from stdin if items is empty
                    use std::io::{self, Read};
                    let mut buffer = String::new();
                    io::stdin()
                        .read_to_string(&mut buffer)
                        .map_err(|e| anyhow::anyhow!("Failed to read from stdin: {}", e))?;
                    buffer
                        .lines()
                        .map(|s| SerializableMenuItem {
                            display_text: s.to_string(),
                            preview: FzfPreview::None,
                            metadata: None,
                        })
                        .collect()
                } else {
                    // Split space-separated items from command line
                    items
                        .split(' ')
                        .map(|s| SerializableMenuItem {
                            display_text: s.to_string(),
                            preview: FzfPreview::None,
                            metadata: None,
                        })
                        .collect()
                };

                let wrapper = FzfWrapper::with_options(FzfOptions {
                    prompt: Some(prompt.clone()),
                    multi_select: multi,
                    ..Default::default()
                });

                match wrapper.select(item_list) {
                    Ok(crate::fzf_wrapper::FzfResult::Selected(item)) => {
                        println!("{}", item.display_text);
                        Ok(0) // Selected
                    }
                    Ok(crate::fzf_wrapper::FzfResult::MultiSelected(items)) => {
                        for item in items {
                            println!("{}", item.display_text);
                        }
                        Ok(0) // Selected
                    }
                    Ok(crate::fzf_wrapper::FzfResult::Cancelled) => Ok(1), // Cancelled
                    Ok(crate::fzf_wrapper::FzfResult::Error(e)) => {
                        eprintln!("Error: {e}");
                        Ok(2) // Error
                    }
                    Err(e) => {
                        eprintln!("Error: {e}");
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
                        println!("{input}");
                        Ok(0) // Success
                    }
                    Err(e) => {
                        eprintln!("Error: {e}");
                        Ok(2) // Error
                    }
                }
            }
        }
        MenuCommands::Status => {
            let client = client::MenuClient::new();
            if client.is_server_running() {
                match client.status() {
                    Ok(status_info) => {
                        client::print_status_info(&status_info);
                        Ok(0)
                    }
                    Err(e) => {
                        eprintln!("Error getting server status: {e}");
                        Ok(2)
                    }
                }
            } else {
                println!("✗ Menu server is not running");
                println!("  Start the server with: instant menu server launch --inside");
                Ok(1)
            }
        }
        MenuCommands::Show => {
            let client = MenuClient::new();
            match client.show() {
                Ok(_) => Ok(0),
                Err(e) => {
                    eprintln!("✗ Failed to show scratchpad: {e}");
                    Ok(1)
                }
            }
        }
        MenuCommands::Server { command } => handle_server_command(command).await,
    }
}

/// Handle server commands
pub async fn handle_server_command(command: ServerCommands) -> Result<i32> {
    match command {
        ServerCommands::Launch {
            inside,
            no_scratchpad,
        } => {
            if inside {
                server::run_server_inside(no_scratchpad).await
            } else {
                server::run_server_launch(no_scratchpad).await
            }
        }
        ServerCommands::Stop => {
            let client = client::MenuClient::new();
            match client.stop() {
                Ok(_) => {
                    println!("✓ Menu server stopped successfully");
                    Ok(0)
                }
                Err(e) => {
                    let error_msg = e.to_string();
                    if error_msg.contains("Server is not running")
                        || error_msg.contains("Failed to connect")
                        || error_msg.contains("No such file or directory")
                        || error_msg.contains("Received empty response")
                    {
                        println!("✗ Menu server is not running");
                        Ok(1)
                    } else {
                        eprintln!("Error stopping server: {e}");
                        Ok(1)
                    }
                }
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
    /// Show the scratchpad without any other action
    Show,
    /// Get menu server status information
    Status,
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
        /// Run without a scratchpad
        #[arg(long)]
        no_scratchpad: bool,
    },
    /// Stop the running menu server
    Stop,
}
