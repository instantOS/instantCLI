use crate::fzf_wrapper::{FzfOptions, FzfSelectable, FzfWrapper};
use anyhow::Result;

/// Handle menu commands for shell scripts
pub fn handle_menu_command(command: MenuCommands, _debug: bool) -> Result<i32> {
    match command {
        MenuCommands::Confirm { message, default } => {
            let default_bool = default.parse::<bool>().unwrap_or(false);
            match FzfWrapper::confirm(&message, default_bool) {
                Ok(true) => Ok(0),  // Yes
                Ok(false) => Ok(1), // No
                Err(e) => {
                    eprintln!("Error: {}", e);
                    Ok(2) // Error
                }
            }
        }
        MenuCommands::Choice {
            prompt,
            items,
            multi,
        } => {
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
                prompt: Some(prompt),
                multi_select: multi,
                height: Some("40%".to_string()),
                preview_window: None,
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
}

#[derive(clap::Subcommand, Debug, Clone)]
pub enum MenuCommands {
    /// Show confirmation dialog and exit with code 0 for Yes, 1 for No
    Confirm {
        /// Confirmation message to display
        #[arg(long, default_value = "Are you sure?")]
        message: String,
        /// Default value if user cancels (true/false)
        #[arg(long, default_value = "false")]
        default: String,
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
    },
}
