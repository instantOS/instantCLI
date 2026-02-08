use crate::common::compositor::CompositorType;
use anyhow::Result;
use colored::*;

pub mod config;
pub mod terminal;

pub use config::ScratchpadConfig;
pub use terminal::Terminal;

/// Shared arguments for scratchpad commands that create/configure terminals
#[derive(clap::Args, Debug, Clone)]
pub struct ScratchpadCreateArgs {
    /// Scratchpad name (used as prefix for window class)
    #[arg(long, default_value = "instantscratchpad")]
    pub name: String,
    /// Terminal command to launch
    #[arg(long, default_value = "kitty")]
    pub terminal: String,
    /// Command to run inside the terminal (e.g., "fish", "ranger", "yazi")
    #[arg(long)]
    pub command: Option<String>,
    /// Terminal width as percentage of screen
    #[arg(long, default_value = "50")]
    pub width_pct: u32,
    /// Terminal height as percentage of screen
    #[arg(long, default_value = "60")]
    pub height_pct: u32,
}

/// Shared arguments for scratchpad commands that only need identification
#[derive(clap::Args, Debug, Clone)]
pub struct ScratchpadIdentifyArgs {
    /// Scratchpad name (used as prefix for window class)
    #[arg(long, default_value = "instantscratchpad")]
    pub name: String,
}

/// Arguments for scratchpad status command
#[derive(clap::Args, Debug, Clone)]
pub struct ScratchpadStatusArgs {
    /// Optional specific scratchpad name to check (if not provided, shows all)
    #[arg(long)]
    pub name: Option<String>,
}

/// Scratchpad subcommands
#[derive(clap::Subcommand, Debug, Clone)]
pub enum ScratchpadCommand {
    /// Toggle scratchpad terminal visibility
    Toggle(ScratchpadCreateArgs),
    /// Show scratchpad terminal
    Show(ScratchpadCreateArgs),
    /// Hide scratchpad terminal
    Hide(ScratchpadIdentifyArgs),
    /// Check scratchpad terminal status
    Status(ScratchpadStatusArgs),
}

impl ScratchpadCommand {
    pub fn run(self, compositor: &CompositorType, debug: bool) -> Result<i32> {
        match self {
            ScratchpadCommand::Toggle(args) => {
                if debug {
                    eprintln!("Toggle scratchpad with config:");
                    eprintln!("  name: {}", args.name);
                    eprintln!("  terminal: {}", args.terminal);
                    eprintln!("  command: {:?}", args.command);
                    eprintln!("  width_pct: {}", args.width_pct);
                    eprintln!("  height_pct: {}", args.height_pct);
                }

                let config = ScratchpadConfig::with_params(
                    args.name,
                    Terminal::from(args.terminal),
                    args.command,
                    args.width_pct,
                    args.height_pct,
                );

                match compositor.provider().toggle(&config) {
                    Ok(()) => Ok(0),
                    Err(e) => {
                        eprintln!("Error toggling scratchpad: {e}");
                        Ok(1)
                    }
                }
            }
            ScratchpadCommand::Show(args) => {
                if debug {
                    eprintln!("Show scratchpad with config:");
                    eprintln!("  name: {}", args.name);
                    eprintln!("  terminal: {}", args.terminal);
                    eprintln!("  command: {:?}", args.command);
                    eprintln!("  width_pct: {}", args.width_pct);
                    eprintln!("  height_pct: {}", args.height_pct);
                }

                let config = ScratchpadConfig::with_params(
                    args.name,
                    Terminal::from(args.terminal),
                    args.command,
                    args.width_pct,
                    args.height_pct,
                );

                match compositor.provider().show(&config) {
                    Ok(()) => Ok(0),
                    Err(e) => {
                        eprintln!("Error showing scratchpad: {e}");
                        Ok(1)
                    }
                }
            }
            ScratchpadCommand::Hide(args) => {
                if debug {
                    eprintln!("Hide scratchpad: {}", args.name);
                }

                let config = ScratchpadConfig::new(args.name);

                match compositor.provider().hide(&config) {
                    Ok(()) => Ok(0),
                    Err(e) => {
                        eprintln!("Error hiding scratchpad: {e}");
                        Ok(1)
                    }
                }
            }
            ScratchpadCommand::Status(args) => {
                if debug {
                    eprintln!("Check scratchpad status");
                    if let Some(ref name) = args.name {
                        eprintln!("  Specific scratchpad: {name}");
                    } else {
                        eprintln!("  Showing all scratchpads");
                    }
                }

                match compositor.get_all_scratchpad_windows() {
                    Ok(windows) => {
                        if windows.is_empty() {
                            println!("No scratchpad terminals found.");
                            return Ok(0);
                        }

                        // If a specific scratchpad name was requested, filter for it
                        let filtered_windows: Vec<_> = if let Some(ref name) = args.name {
                            windows.into_iter().filter(|w| w.name == *name).collect()
                        } else {
                            windows
                        };

                        if filtered_windows.is_empty() {
                            if let Some(name) = args.name {
                                println!("No scratchpad terminal found with name: {}", name);
                            } else {
                                println!("No scratchpad terminals found.");
                            }
                            return Ok(0);
                        }

                        // Display header
                        println!("{}", "Scratchpad Terminal Status".bold().underline());
                        println!();

                        for window in &filtered_windows {
                            let status_indicator = if window.visible {
                                "●".green()
                            } else {
                                "○".bright_black()
                            };

                            let status_text = if window.visible {
                                "visible".green()
                            } else {
                                "hidden".bright_black()
                            };

                            println!("  {} {}", status_indicator, window.name.cyan());
                            println!("     Title: {}", window.title);
                            println!("     Class: {}", window.window_class);
                            println!("     Status: {status_text}");
                            println!();
                        }

                        // Summary
                        let total_count = filtered_windows.len();
                        let visible_count = filtered_windows.iter().filter(|w| w.visible).count();
                        let hidden_count = total_count - visible_count;

                        println!(
                            "Summary: {} total, {} visible, {} hidden",
                            total_count.to_string().bold(),
                            visible_count.to_string().green(),
                            hidden_count.to_string().bright_black()
                        );

                        Ok(0)
                    }
                    Err(e) => {
                        eprintln!("Error checking scratchpad status: {e}");
                        Ok(2)
                    }
                }
            }
        }
    }
}
