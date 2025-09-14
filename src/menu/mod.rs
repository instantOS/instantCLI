use crate::fzf_wrapper::{FzfSelectable, FzfWrapper, FzfOptions};
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
        MenuCommands::Select { prompt, items, multi } => {
            let item_list: Vec<String> = items.split(' ').map(|s| s.to_string()).collect();

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
                Ok(crate::fzf_wrapper::FzfResult::Selected(_)) => Ok(0),  // Selected
                Ok(crate::fzf_wrapper::FzfResult::MultiSelected(_)) => Ok(0),  // Selected
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
    /// Show selection menu and exit with code 0 for selection, 1 for cancel
    Select {
        /// Selection prompt message
        #[arg(long, default_value = "Select an item:")]
        prompt: String,
        /// Items to choose from (space-separated)
        #[arg(long, required = true)]
        items: String,
        /// Allow multiple selections
        #[arg(long)]
        multi: bool,
    },
}