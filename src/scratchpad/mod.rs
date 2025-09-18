use crate::common::compositor::CompositorType;
use anyhow::Result;

pub mod config;
pub mod operations;
pub mod terminal;
pub mod visibility;

pub use config::ScratchpadConfig;
pub use terminal::Terminal;

/// Toggle scratchpad terminal visibility
pub fn toggle_scratchpad(compositor: &CompositorType, config: &ScratchpadConfig) -> Result<()> {
    match compositor {
        CompositorType::Sway => operations::toggle_scratchpad_sway(config),
        CompositorType::Hyprland => operations::toggle_scratchpad_hyprland(config),
        CompositorType::Other(_) => {
            eprintln!("TODO: Scratchpad toggle not implemented for this compositor");
            Ok(())
        }
    }
}

/// Show scratchpad terminal
pub fn show_scratchpad(compositor: &CompositorType, config: &ScratchpadConfig) -> Result<()> {
    match compositor {
        CompositorType::Sway => visibility::show_scratchpad_sway(config),
        CompositorType::Hyprland => visibility::show_scratchpad_hyprland(config),
        CompositorType::Other(_) => {
            eprintln!("TODO: Scratchpad show not implemented for this compositor");
            Ok(())
        }
    }
}

/// Hide scratchpad terminal
pub fn hide_scratchpad(compositor: &CompositorType, config: &ScratchpadConfig) -> Result<()> {
    match compositor {
        CompositorType::Sway => visibility::hide_scratchpad_sway(config),
        CompositorType::Hyprland => visibility::hide_scratchpad_hyprland(config),
        CompositorType::Other(_) => {
            eprintln!("TODO: Scratchpad hide not implemented for this compositor");
            Ok(())
        }
    }
}

/// Check if scratchpad terminal is currently visible
pub fn is_scratchpad_visible(
    compositor: &CompositorType,
    config: &ScratchpadConfig,
) -> Result<bool> {
    visibility::is_scratchpad_visible(compositor, config)
}

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
    Status(ScratchpadIdentifyArgs),
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

                match toggle_scratchpad(compositor, &config) {
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

                match show_scratchpad(compositor, &config) {
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

                match hide_scratchpad(compositor, &config) {
                    Ok(()) => Ok(0),
                    Err(e) => {
                        eprintln!("Error hiding scratchpad: {e}");
                        Ok(1)
                    }
                }
            }
            ScratchpadCommand::Status(args) => {
                if debug {
                    eprintln!("Check scratchpad status for: {}", args.name);
                }

                let config = ScratchpadConfig::new(args.name);

                match is_scratchpad_visible(compositor, &config) {
                    Ok(visible) => {
                        if visible {
                            println!("Scratchpad terminal is visible");
                        } else {
                            println!("Scratchpad terminal is not visible");
                        }
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
