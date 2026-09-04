//! Clap-facing menu command definitions.

use super::{MenuBackend, SliderSpec};

#[derive(clap::Subcommand, Debug, Clone)]
pub enum MenuCommands {
    #[command(hide = true)]
    FallbackWorker {
        #[arg(long = "request-file", value_hint = clap::ValueHint::FilePath)]
        request_file: String,
        #[arg(long = "response-file", value_hint = clap::ValueHint::FilePath)]
        response_file: String,
    },
    /// Show confirmation dialog and exit with code 0 for Yes, 1 for No, 2 for Cancelled
    Confirm {
        #[arg(default_value = "Are you sure?")]
        message: String,
        #[arg(short = 'b', long = "backend", value_enum, default_value_t = MenuBackend::Auto)]
        backend: MenuBackend,
    },
    /// Unified launcher for all major InstantCLI TUIs
    All,
    /// Show a message dialog with an OK button
    Message {
        message: Option<String>,
        #[arg(long)]
        title: Option<String>,
        #[arg(short = 'b', long = "backend", value_enum, default_value_t = MenuBackend::Auto)]
        backend: MenuBackend,
    },
    /// Show selection menu and output choice(s) to stdout
    Choice {
        prompt: Option<String>,
        #[arg(long = "prompt", value_name = "PROMPT")]
        prompt_option: Option<String>,
        #[arg(long, default_value = "")]
        items: String,
        #[arg(long = "allow-multiple", visible_alias = "multi")]
        allow_multiple: bool,
        #[arg(short = 'b', long = "backend", value_enum, default_value_t = MenuBackend::Auto)]
        backend: MenuBackend,
    },
    /// Show text input dialog and output input to stdout
    Input {
        #[arg(default_value = "Type a value:")]
        prompt: String,
        #[arg(short = 'b', long = "backend", value_enum, default_value_t = MenuBackend::Auto)]
        backend: MenuBackend,
    },
    /// Show password input dialog and output password to stdout
    Password {
        #[arg(default_value = "Enter password:")]
        prompt: String,
        #[arg(short = 'b', long = "backend", value_enum, default_value_t = MenuBackend::Auto)]
        backend: MenuBackend,
    },
    /// Launch file picker and output selected path(s)
    Pick {
        #[arg(long = "start", value_hint = clap::ValueHint::AnyPath)]
        start: Option<String>,
        #[arg(long)]
        dirs: bool,
        #[arg(long)]
        files: bool,
        #[arg(long = "allow-multiple", visible_alias = "multi")]
        allow_multiple: bool,
        #[arg(short = 'b', long = "backend", value_enum, default_value_t = MenuBackend::Auto)]
        backend: MenuBackend,
    },
    /// Show the scratchpad without any other action
    Show,
    /// Get menu server status information
    Status,
    /// Show chord navigator for provided chords and print the selected sequence
    Chord {
        #[arg(value_name = "CHORD:DESCRIPTION")]
        chords: Vec<String>,
        #[arg(long)]
        stdin: bool,
        #[arg(short = 'b', long = "backend", value_enum, default_value_t = MenuBackend::Auto)]
        backend: MenuBackend,
    },
    /// Menu server management commands
    Server {
        #[command(subcommand)]
        command: ServerCommands,
    },
    /// Show a slider prompt similar to the legacy islide utility
    Slide {
        #[command(flatten)]
        spec: SliderSpec,
        #[arg(short = 'b', long = "backend", value_enum, default_value_t = MenuBackend::Auto)]
        backend: MenuBackend,
    },
    /// Show a checklist dialog for testing the checklist utility
    Checklist {
        #[arg(long, default_value = "")]
        items: String,
        #[arg(long, default_value = "Continue")]
        confirm: String,
        #[arg(short = 'b', long = "backend", value_enum, default_value_t = MenuBackend::Auto)]
        backend: MenuBackend,
    },
    /// Show a loading spinner dialog while executing a command, or until stdin is closed
    Spin {
        #[arg(short = 'm', long, default_value = "Loading...")]
        message: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<String>,
        #[arg(short = 'b', long = "backend", value_enum, default_value_t = MenuBackend::Auto)]
        backend: MenuBackend,
    },
    /// Show an ephemeral toast notification popup
    Toast {
        message: String,
        #[arg(long, short = 't', default_value_t = 3.5)]
        duration: f64,
        #[arg(short = 'b', long = "backend", value_enum, default_value_t = MenuBackend::Auto)]
        backend: MenuBackend,
    },
}

#[derive(clap::Subcommand, Debug, Clone)]
pub enum ServerCommands {
    /// Launch menu server (launches terminal with --inside mode)
    Launch {
        #[arg(long)]
        inside: bool,
        #[arg(long)]
        no_scratchpad: bool,
    },
    /// Stop the running menu server
    Stop,
}
