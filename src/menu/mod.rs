use crate::menu_utils::{
    ConfirmResult, FilePickerResult, FilePickerScope, FzfWrapper, MenuWrapper,
};
use anyhow::{Context, Result, anyhow};
use protocol::SerializableMenuItem;
use std::io::IsTerminal;
use std::path::PathBuf;

pub mod chord;
pub mod client;
mod fallback;
pub mod instantmenu;
pub mod processing;
pub mod protocol;
pub mod scratchpad_manager;
pub mod server;
pub mod slide;
pub mod tui;
use client::MenuClient;
use slide::SliderPreset;

/// Menu backend choice for rendering dialogs
#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum MenuBackend {
    /// Auto-detect best backend (TUI for interactive terminals, instantmenu for desktop/scripts)
    #[default]
    Auto,
    /// Force native instantmenu X11/Wayland GUI overlay
    #[value(alias = "im", alias = "gui")]
    Instantmenu,
    /// Force in-terminal TUI mode using fzf
    Tui,
    /// Force floating terminal scratchpad server mode
    #[value(alias = "sp")]
    Scratchpad,
}

/// Concrete resolved backend after environment/context inspection
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResolvedBackend {
    Instantmenu,
    Tui,
    Scratchpad,
}

impl MenuBackend {
    /// Resolve the backend choice to a concrete backend
    pub fn resolve(self, supports_instantmenu: bool) -> ResolvedBackend {
        match self {
            MenuBackend::Instantmenu => ResolvedBackend::Instantmenu,
            MenuBackend::Tui => ResolvedBackend::Tui,
            MenuBackend::Scratchpad => ResolvedBackend::Scratchpad,
            MenuBackend::Auto => {
                if let Ok(env_backend) = std::env::var("INS_MENU_BACKEND") {
                    match env_backend.to_lowercase().as_str() {
                        "instantmenu" | "im" | "gui" => return ResolvedBackend::Instantmenu,
                        "tui" => return ResolvedBackend::Tui,
                        "scratchpad" | "sp" => return ResolvedBackend::Scratchpad,
                        _ => {}
                    }
                }

                let has_display = std::env::var_os("WAYLAND_DISPLAY").is_some()
                    || std::env::var_os("DISPLAY").is_some();
                let is_tty = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();

                if has_display && !is_tty && supports_instantmenu {
                    ResolvedBackend::Instantmenu
                } else if has_display && !is_tty {
                    ResolvedBackend::Scratchpad
                } else {
                    ResolvedBackend::Tui
                }
            }
        }
    }
}

/// Handle menu commands for shell scripts
pub async fn handle_menu_command(command: MenuCommands, _debug: bool) -> Result<i32> {
    match command {
        MenuCommands::FallbackWorker {
            request_file,
            response_file,
        } => fallback::run_worker(&request_file, &response_file),
        MenuCommands::All => Err(anyhow!(
            "MenuCommands::All should be dispatched from main.rs"
        )),
        MenuCommands::Confirm {
            ref message,
            backend,
        } => handle_confirm(message, backend),
        MenuCommands::Message {
            ref message,
            ref title,
            backend,
        } => handle_message(message.as_deref(), title.as_deref(), backend),
        MenuCommands::Choice {
            ref prompt,
            ref prompt_option,
            ref items,
            allow_multiple,
            backend,
        } => handle_choice(
            prompt_option
                .as_deref()
                .or(prompt.as_deref())
                .unwrap_or("Select an item:"),
            items,
            allow_multiple,
            backend,
        ),
        MenuCommands::Chord {
            ref chords,
            stdin,
            backend,
        } => handle_chord(chords, stdin, backend),
        MenuCommands::Slide { spec, backend } => handle_slide(spec, backend),
        MenuCommands::Pick {
            ref start,
            dirs,
            files,
            allow_multiple,
            backend,
        } => handle_pick(start, dirs, files, allow_multiple, backend, &command),
        MenuCommands::Input {
            ref prompt,
            backend,
        } => handle_input(prompt, backend, &command),
        MenuCommands::Password {
            ref prompt,
            backend,
        } => handle_password(prompt, backend, &command),
        MenuCommands::Status => handle_status(),
        MenuCommands::Show => handle_show(),
        MenuCommands::Checklist {
            ref items,
            ref confirm,
            backend,
        } => handle_checklist(items, confirm, backend),
        MenuCommands::Spin {
            ref message,
            ref command,
            backend,
        } => handle_spin(message, command, backend),
        MenuCommands::Toast {
            ref message,
            duration,
            backend,
        } => handle_toast(message, duration, backend, &command),
        MenuCommands::Server { command } => handle_server_command(command).await,
    }
}

fn handle_confirm(message: &str, backend: MenuBackend) -> Result<i32> {
    let effective_message = if message == "Are you sure?" && !std::io::stdin().is_terminal() {
        use std::io::{self, Read};
        let mut buffer = String::new();
        if io::stdin().read_to_string(&mut buffer).is_ok() && !buffer.trim().is_empty() {
            buffer.trim().to_string()
        } else {
            message.to_string()
        }
    } else {
        message.to_string()
    };

    match backend.resolve(true) {
        ResolvedBackend::Instantmenu => {
            match instantmenu::InstantmenuBackend::confirm(&effective_message) {
                Ok(ConfirmResult::Yes) => Ok(0),
                Ok(ConfirmResult::No) => Ok(1),
                Ok(ConfirmResult::Cancelled) => Ok(2),
                Err(e) => {
                    eprintln!("instantmenu error: {e}");
                    Ok(3)
                }
            }
        }
        ResolvedBackend::Scratchpad => {
            let client = MenuClient::new();
            match client.confirm(effective_message) {
                Ok(result) => Ok(result.into()),
                Err(e) => {
                    eprintln!("Scratchpad menu error: {e}");
                    Ok(3)
                }
            }
        }
        ResolvedBackend::Tui => match FzfWrapper::confirm(&effective_message) {
            Ok(ConfirmResult::Yes) => Ok(0),
            Ok(ConfirmResult::No) => Ok(1),
            Ok(ConfirmResult::Cancelled) => Ok(2),
            Err(e) => {
                eprintln!("Error: {e}");
                Ok(3)
            }
        },
    }
}

fn handle_message(message: Option<&str>, title: Option<&str>, backend: MenuBackend) -> Result<i32> {
    let effective_message = match message {
        Some(m) if !m.is_empty() => m.to_string(),
        _ => {
            use std::io::{self, Read};
            let mut buffer = String::new();
            io::stdin().read_to_string(&mut buffer).unwrap_or_default();
            buffer.trim_end_matches('\n').to_string()
        }
    };

    match backend.resolve(true) {
        ResolvedBackend::Instantmenu => {
            match instantmenu::InstantmenuBackend::message(title, &effective_message) {
                Ok(_) => Ok(0),
                Err(e) => {
                    eprintln!("instantmenu error: {e}");
                    Ok(1)
                }
            }
        }
        ResolvedBackend::Scratchpad => {
            let client = MenuClient::new();
            match client.message(title.unwrap_or("Notice").to_string(), effective_message) {
                Ok(()) => Ok(0),
                Err(e) => {
                    eprintln!("Scratchpad menu error: {e}");
                    Ok(1)
                }
            }
        }
        ResolvedBackend::Tui => {
            let mut builder = FzfWrapper::builder().message(&effective_message);
            if let Some(t) = title {
                builder = builder.title(t);
            }
            match builder.message_dialog() {
                Ok(_) => Ok(0),
                Err(e) => {
                    eprintln!("Error: {e}");
                    Ok(1)
                }
            }
        }
    }
}

fn handle_choice(
    prompt: &str,
    items: &str,
    allow_multiple: bool,
    backend: MenuBackend,
) -> Result<i32> {
    let item_list: Vec<SerializableMenuItem> = if items.is_empty() {
        use std::io::{self, Read};
        let mut buffer = String::new();
        io::stdin()
            .read_to_string(&mut buffer)
            .map_err(|e| anyhow::anyhow!("Failed to read from stdin: {}", e))?;
        protocol::plain_choice_items_from_input(&buffer)
    } else {
        items.split(' ').map(SerializableMenuItem::plain).collect()
    };

    match backend.resolve(true) {
        ResolvedBackend::Instantmenu => {
            match instantmenu::InstantmenuBackend::choice(prompt, &item_list, allow_multiple) {
                Ok(selected) => {
                    if selected.is_empty() {
                        Ok(1)
                    } else {
                        for item in selected {
                            println!("{item}");
                        }
                        Ok(0)
                    }
                }
                Err(e) => {
                    eprintln!("instantmenu error: {e}");
                    Ok(3)
                }
            }
        }
        ResolvedBackend::Scratchpad => {
            let client = MenuClient::new();
            match client.choice(prompt.to_string(), item_list, allow_multiple) {
                Ok(selected) if selected.is_empty() => Ok(1),
                Ok(selected) => {
                    for item in selected {
                        println!("{}", item.display_text);
                    }
                    Ok(0)
                }
                Err(e) => {
                    eprintln!("Scratchpad menu error: {e}");
                    Ok(3)
                }
            }
        }
        ResolvedBackend::Tui => {
            match FzfWrapper::builder()
                .prompt(prompt.to_string())
                .multi_select(allow_multiple)
                .select(item_list)?
            {
                crate::menu_utils::FzfResult::Selected(item) => {
                    println!("{}", item.display_text);
                    Ok(0)
                }
                crate::menu_utils::FzfResult::MultiSelected(items) => {
                    for item in items {
                        println!("{}", item.display_text);
                    }
                    Ok(0)
                }
                crate::menu_utils::FzfResult::Cancelled => Ok(1),
                crate::menu_utils::FzfResult::Error(e) => {
                    eprintln!("Error: {e}");
                    Ok(2)
                }
            }
        }
    }
}

fn handle_chord(chords: &[String], stdin: bool, backend: MenuBackend) -> Result<i32> {
    let mut combined = chords.to_vec();

    if stdin {
        use std::io::{self, Read};

        let mut buffer = String::new();
        io::stdin()
            .read_to_string(&mut buffer)
            .context("Failed to read chords from stdin")?;

        for line in buffer.lines() {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                combined.push(trimmed.to_string());
            }
        }
    }

    if combined.is_empty() {
        return Err(anyhow!("Provide at least one chord specification"));
    }

    match backend.resolve(false) {
        ResolvedBackend::Scratchpad | ResolvedBackend::Instantmenu => {
            let client = MenuClient::new();
            match client.chord(combined) {
                Ok(Some(sequence)) => {
                    println!("{sequence}");
                    Ok(0)
                }
                Ok(None) => Ok(1),
                Err(e) => {
                    eprintln!("Menu error: {e}");
                    Ok(3)
                }
            }
        }
        ResolvedBackend::Tui => chord::run_chord_command(&combined),
    }
}

/// The user-facing slider spec, before preset defaults are applied.
///
/// Flattened into `MenuCommands::Slide` so the CLI arguments and the slider
/// options are one and the same type (no field-by-field repacking).
#[derive(clap::Args, Debug, Clone)]
pub struct SliderSpec {
    /// Minimum slider value
    #[arg(long, default_value_t = 0)]
    pub min: i64,
    /// Maximum slider value
    #[arg(long, default_value_t = 100)]
    pub max: i64,
    /// Initial slider value
    #[arg(long = "value")]
    pub value: Option<i64>,
    /// Small step increment for h/l and arrow keys
    #[arg(long = "step")]
    pub step: Option<i64>,
    /// Large step increment for j/k and vertical arrows
    #[arg(long = "big-step")]
    pub big_step: Option<i64>,
    /// Optional label displayed above the slider
    #[arg(long)]
    pub label: Option<String>,
    /// Command to execute on value changes (value appended as final arg)
    #[arg(long = "command", value_name = "CMD", num_args = 1..)]
    pub command: Vec<String>,
    /// Use a preconfigured slider preset
    #[arg(long, value_enum)]
    pub preset: Option<SliderPreset>,
}

impl SliderSpec {
    fn apply_preset(mut self) -> Self {
        if let Some(preset_kind) = self.preset {
            let preset_config = preset_kind.config();
            self.min = preset_config.min;
            self.max = preset_config.max;
            self.value = self.value.or(preset_config.value);
            self.step = self.step.or(preset_config.step);
            self.big_step = self.big_step.or(preset_config.big_step);
            self.label = self.label.or(preset_config.label);
            if self.command.is_empty() {
                self.command = preset_config.command;
            }
        }

        self
    }
}

fn handle_slide(spec: SliderSpec, backend: MenuBackend) -> Result<i32> {
    let spec = spec.apply_preset();

    match backend.resolve(true) {
        ResolvedBackend::Instantmenu => match instantmenu::InstantmenuBackend::slide(&spec) {
            Ok(Some(result)) => {
                println!("{result}");
                Ok(0)
            }
            Ok(None) => Ok(1),
            Err(e) => {
                eprintln!("instantmenu error: {e}");
                Ok(3)
            }
        },
        ResolvedBackend::Scratchpad => {
            let request = protocol::SliderRequest {
                min: spec.min,
                max: spec.max,
                value: spec.value,
                step: spec.step,
                big_step: spec.big_step,
                label: spec.label,
                command: spec.command,
            };

            let client = MenuClient::new();
            match client.slide(request) {
                Ok(Some(result)) => {
                    println!("{result}");
                    Ok(0)
                }
                Ok(None) => Ok(1),
                Err(e) => {
                    eprintln!("Scratchpad menu error: {e}");
                    Ok(3)
                }
            }
        }
        ResolvedBackend::Tui => {
            let request = protocol::SliderRequest {
                min: spec.min,
                max: spec.max,
                value: spec.value,
                step: spec.step,
                big_step: spec.big_step,
                label: spec.label,
                command: spec.command,
            };

            match slide::run_slider_command(&request) {
                Ok(Some(result)) => {
                    println!("{result}");
                    Ok(0)
                }
                Ok(None) => Ok(1),
                Err(e) => {
                    eprintln!("Error: {e}");
                    Ok(2)
                }
            }
        }
    }
}

fn handle_pick(
    start: &Option<String>,
    dirs: bool,
    files: bool,
    allow_multiple: bool,
    backend: MenuBackend,
    command: &MenuCommands,
) -> Result<i32> {
    let scope = match (dirs, files) {
        (true, false) => FilePickerScope::Directories,
        (false, true) => FilePickerScope::Files,
        (true, true) => FilePickerScope::FilesAndDirectories,
        (false, false) => FilePickerScope::Files,
    };

    match backend.resolve(false) {
        ResolvedBackend::Scratchpad | ResolvedBackend::Instantmenu => {
            client::handle_scratchpad_request(command)
        }
        ResolvedBackend::Tui => {
            let mut builder = MenuWrapper::file_picker().scope(scope).multi(allow_multiple);

            if let Some(start_dir) = start.as_ref().filter(|s| !s.is_empty()) {
                builder = builder.start_dir(PathBuf::from(start_dir));
            }

            match builder.pick()? {
                FilePickerResult::Selected(path) => {
                    println!("{}", path.display());
                    Ok(0)
                }
                FilePickerResult::MultiSelected(paths) => {
                    for path in paths {
                        println!("{}", path.display());
                    }
                    Ok(0)
                }
                FilePickerResult::Cancelled => Ok(1),
            }
        }
    }
}

fn handle_input(prompt: &str, backend: MenuBackend, command: &MenuCommands) -> Result<i32> {
    match backend.resolve(true) {
        ResolvedBackend::Instantmenu => {
            match instantmenu::InstantmenuBackend::input(prompt, None, None) {
                Ok(Some(text)) => {
                    println!("{text}");
                    Ok(0)
                }
                Ok(None) => Ok(1),
                Err(e) => {
                    eprintln!("instantmenu error: {e}");
                    Ok(3)
                }
            }
        }
        ResolvedBackend::Scratchpad => client::handle_scratchpad_request(command),
        ResolvedBackend::Tui => match FzfWrapper::input(prompt) {
            Ok(input) => {
                println!("{input}");
                Ok(0)
            }
            Err(e) => {
                eprintln!("Error: {e}");
                Ok(2)
            }
        },
    }
}

fn handle_password(prompt: &str, backend: MenuBackend, command: &MenuCommands) -> Result<i32> {
    match backend.resolve(true) {
        ResolvedBackend::Instantmenu => match instantmenu::InstantmenuBackend::password(prompt) {
            Ok(Some(text)) => {
                println!("{text}");
                Ok(0)
            }
            Ok(None) => Ok(1),
            Err(e) => {
                eprintln!("instantmenu error: {e}");
                Ok(3)
            }
        },
        ResolvedBackend::Scratchpad => client::handle_scratchpad_request(command),
        ResolvedBackend::Tui => match FzfWrapper::password(prompt) {
            Ok(crate::menu_utils::FzfResult::Selected(password)) => {
                println!("{password}");
                Ok(0)
            }
            Ok(crate::menu_utils::FzfResult::Cancelled) => Ok(1),
            Ok(crate::menu_utils::FzfResult::Error(e)) => {
                eprintln!("Error: {e}");
                Ok(2)
            }
            Ok(_) => Ok(1),
            Err(e) => {
                eprintln!("Error: {e}");
                Ok(2)
            }
        },
    }
}

fn handle_status() -> Result<i32> {
    let client = client::MenuClient::new();
    if client.is_fallback() {
        match client.status() {
            Ok(status_info) => {
                client::print_status_info(&status_info);
                println!();
                println!("Fallback mode: interactive dialogs run in transient kitty terminals.");
                Ok(0)
            }
            Err(e) => {
                eprintln!("Error getting fallback status: {e}");
                Ok(2)
            }
        }
    } else if client.is_server_running() {
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
        println!(
            "  Start the server with: {} menu server launch --inside",
            env!("CARGO_BIN_NAME")
        );
        Ok(1)
    }
}

fn handle_show() -> Result<i32> {
    let client = MenuClient::new();
    match client.show() {
        Ok(_) => Ok(0),
        Err(e) => {
            eprintln!("✗ Failed to show scratchpad: {e}");
            Ok(1)
        }
    }
}

fn handle_checklist(items: &str, confirm: &str, backend: MenuBackend) -> Result<i32> {
    let item_list: Vec<String> = if items.is_empty() {
        use std::io::{self, Read};
        let mut buffer = String::new();
        io::stdin()
            .read_to_string(&mut buffer)
            .map_err(|e| anyhow::anyhow!("Failed to read from stdin: {}", e))?;
        buffer.lines().map(|s| s.to_string()).collect()
    } else {
        items.split(' ').map(|s| s.to_string()).collect()
    };

    match backend.resolve(true) {
        ResolvedBackend::Instantmenu => {
            match instantmenu::InstantmenuBackend::checklist(&item_list, confirm) {
                Ok(Some(selected)) => {
                    for item in selected {
                        println!("{item}");
                    }
                    Ok(0)
                }
                Ok(None) => Ok(1),
                Err(e) => {
                    eprintln!("instantmenu error: {e}");
                    Ok(3)
                }
            }
        }
        ResolvedBackend::Scratchpad | ResolvedBackend::Tui => {
            match FzfWrapper::builder()
                .prompt("Select items")
                .header("Enter on item toggles it | Enter on Continue confirms")
                .initial_index(item_list.len().saturating_sub(1))
                .checklist(confirm)
                .checklist_dialog(item_list)?
            {
                crate::menu_utils::ChecklistResult::Confirmed(selected) => {
                    for item in selected {
                        println!("{}", item);
                    }
                    Ok(0)
                }
                crate::menu_utils::ChecklistResult::Action(action) => {
                    println!("{}", action.text);
                    Ok(0)
                }
                crate::menu_utils::ChecklistResult::Cancelled => Ok(1),
            }
        }
    }
}

fn handle_spin(message: &str, command: &[String], backend: MenuBackend) -> Result<i32> {
    match backend.resolve(true) {
        ResolvedBackend::Instantmenu => instantmenu::InstantmenuBackend::spin(message, command),
        ResolvedBackend::Scratchpad | ResolvedBackend::Tui => {
            use indicatif::{ProgressBar, ProgressStyle};
            use std::time::Duration;

            let pb = ProgressBar::new_spinner();
            pb.set_style(
                ProgressStyle::default_spinner()
                    .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
                    .template("{spinner:.green} {msg}")
                    .unwrap_or_else(|_| ProgressStyle::default_spinner()),
            );
            pb.set_message(message.to_string());
            pb.enable_steady_tick(Duration::from_millis(80));

            if command.is_empty() {
                use std::io::Read;
                let mut stdin = std::io::stdin();
                let mut buf = [0u8; 128];
                while let Ok(n) = stdin.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                }
                pb.finish_and_clear();
                return Ok(0);
            }

            let status = std::process::Command::new(&command[0])
                .args(&command[1..])
                .stdin(std::process::Stdio::inherit())
                .stdout(std::process::Stdio::inherit())
                .stderr(std::process::Stdio::inherit())
                .status();

            pb.finish_and_clear();

            match status {
                Ok(s) => Ok(s.code().unwrap_or(1)),
                Err(e) => {
                    eprintln!("Failed to execute command: {e}");
                    Ok(1)
                }
            }
        }
    }
}

fn handle_toast(
    message: &str,
    duration: f64,
    backend: MenuBackend,
    command: &MenuCommands,
) -> Result<i32> {
    match backend.resolve(true) {
        ResolvedBackend::Instantmenu => {
            match instantmenu::InstantmenuBackend::toast(message, duration) {
                Ok(()) => Ok(0),
                Err(e) => {
                    eprintln!("instantmenu error: {e}");
                    Ok(1)
                }
            }
        }
        ResolvedBackend::Scratchpad => client::handle_scratchpad_request(command),
        ResolvedBackend::Tui => {
            eprintln!("{message}");
            Ok(0)
        }
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
    #[command(hide = true)]
    FallbackWorker {
        #[arg(long = "request-file", value_hint = clap::ValueHint::FilePath)]
        request_file: String,
        #[arg(long = "response-file", value_hint = clap::ValueHint::FilePath)]
        response_file: String,
    },
    /// Show confirmation dialog and exit with code 0 for Yes, 1 for No, 2 for Cancelled
    Confirm {
        /// Confirmation message to display
        #[arg(default_value = "Are you sure?")]
        message: String,
        /// Menu backend choice (auto, instantmenu/im, tui, scratchpad/sp)
        #[arg(short = 'b', long = "backend", value_enum, default_value_t = MenuBackend::Auto)]
        backend: MenuBackend,
    },
    /// Unified launcher for all major InstantCLI TUIs
    All,
    /// Show a message dialog with an OK button
    Message {
        /// Message to display (if omitted, reads from stdin)
        message: Option<String>,
        /// Optional title for the message
        #[arg(long)]
        title: Option<String>,
        /// Menu backend choice (auto, instantmenu/im, tui, scratchpad/sp)
        #[arg(short = 'b', long = "backend", value_enum, default_value_t = MenuBackend::Auto)]
        backend: MenuBackend,
    },
    /// Show selection menu and output choice(s) to stdout
    Choice {
        /// Selection prompt message (positional form)
        prompt: Option<String>,
        /// Selection prompt message (compatible long-option form)
        #[arg(long = "prompt", value_name = "PROMPT")]
        prompt_option: Option<String>,
        /// Items to choose from (space-separated). If empty, reads from stdin.
        #[arg(long, default_value = "")]
        items: String,
        /// Allow multiple selections
        #[arg(long = "allow-multiple", visible_alias = "multi")]
        allow_multiple: bool,
        /// Menu backend choice (auto, instantmenu/im, tui, scratchpad/sp)
        #[arg(short = 'b', long = "backend", value_enum, default_value_t = MenuBackend::Auto)]
        backend: MenuBackend,
    },
    /// Show text input dialog and output input to stdout
    Input {
        /// Input prompt message
        #[arg(default_value = "Type a value:")]
        prompt: String,
        /// Menu backend choice (auto, instantmenu/im, tui, scratchpad/sp)
        #[arg(short = 'b', long = "backend", value_enum, default_value_t = MenuBackend::Auto)]
        backend: MenuBackend,
    },
    /// Show password input dialog and output password to stdout
    Password {
        /// Password prompt message
        #[arg(default_value = "Enter password:")]
        prompt: String,
        /// Menu backend choice (auto, instantmenu/im, tui, scratchpad/sp)
        #[arg(short = 'b', long = "backend", value_enum, default_value_t = MenuBackend::Auto)]
        backend: MenuBackend,
    },
    /// Launch file picker and output selected path(s)
    Pick {
        /// Starting directory for the picker
        #[arg(long = "start", value_hint = clap::ValueHint::AnyPath)]
        start: Option<String>,
        /// Restrict selection to directories (defaults to files)
        #[arg(long)]
        dirs: bool,
        /// Allow selecting files (enabled by default)
        #[arg(long)]
        files: bool,
        /// Allow multiple selections
        #[arg(long = "allow-multiple", visible_alias = "multi")]
        allow_multiple: bool,
        /// Menu backend choice (auto, tui, scratchpad/sp)
        #[arg(short = 'b', long = "backend", value_enum, default_value_t = MenuBackend::Auto)]
        backend: MenuBackend,
    },
    /// Show the scratchpad without any other action
    Show,
    /// Get menu server status information
    Status,
    /// Show chord navigator for provided chords and print the selected sequence
    Chord {
        /// Chord definitions in the form `keys:description`
        #[arg(value_name = "CHORD:DESCRIPTION")]
        chords: Vec<String>,
        /// Read additional chord definitions from stdin (one per line)
        #[arg(long)]
        stdin: bool,
        /// Menu backend choice (auto, tui, scratchpad/sp)
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
        /// Menu backend choice (auto, instantmenu/im, tui, scratchpad/sp)
        #[arg(short = 'b', long = "backend", value_enum, default_value_t = MenuBackend::Auto)]
        backend: MenuBackend,
    },
    /// Show a checklist dialog for testing the checklist utility
    Checklist {
        /// Items to display in checklist (space-separated). If empty, uses sample items.
        #[arg(long, default_value = "")]
        items: String,
        /// Text for the confirm button
        #[arg(long, default_value = "Continue")]
        confirm: String,
        /// Menu backend choice (auto, instantmenu/im, tui, scratchpad/sp)
        #[arg(short = 'b', long = "backend", value_enum, default_value_t = MenuBackend::Auto)]
        backend: MenuBackend,
    },
    /// Show a loading spinner dialog while executing a command, or until stdin is closed
    Spin {
        /// Message to display alongside the spinner
        #[arg(short = 'm', long, default_value = "Loading...")]
        message: String,
        /// Command to execute (all trailing arguments)
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<String>,
        /// Menu backend choice (auto, instantmenu/im, tui, scratchpad/sp)
        #[arg(short = 'b', long = "backend", value_enum, default_value_t = MenuBackend::Auto)]
        backend: MenuBackend,
    },
    /// Show an ephemeral toast notification popup
    Toast {
        /// Message to display in the toast notification
        message: String,
        /// Duration in seconds for the toast to remain visible
        #[arg(long, short = 't', default_value_t = 3.5)]
        duration: f64,
        /// Menu backend choice (auto, instantmenu/im, tui, scratchpad/sp)
        #[arg(short = 'b', long = "backend", value_enum, default_value_t = MenuBackend::Auto)]
        backend: MenuBackend,
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

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser, Debug)]
    struct MenuCli {
        #[command(subcommand)]
        command: MenuCommands,
    }

    #[test]
    fn test_menu_backend_cli_parsing() {
        let cli =
            MenuCli::try_parse_from(["ins-menu", "confirm", "--backend", "instantmenu"]).unwrap();
        if let MenuCommands::Confirm { backend, .. } = cli.command {
            assert_eq!(backend, MenuBackend::Instantmenu);
        } else {
            panic!("Expected Confirm command");
        }

        let cli = MenuCli::try_parse_from(["ins-menu", "confirm", "-b", "im"]).unwrap();
        if let MenuCommands::Confirm { backend, .. } = cli.command {
            assert_eq!(backend, MenuBackend::Instantmenu);
        } else {
            panic!("Expected Confirm command");
        }

        let cli = MenuCli::try_parse_from(["ins-menu", "confirm", "-b", "gui"]).unwrap();
        if let MenuCommands::Confirm { backend, .. } = cli.command {
            assert_eq!(backend, MenuBackend::Instantmenu);
        } else {
            panic!("Expected Confirm command");
        }

        let cli = MenuCli::try_parse_from(["ins-menu", "confirm", "-b", "tui"]).unwrap();
        if let MenuCommands::Confirm { backend, .. } = cli.command {
            assert_eq!(backend, MenuBackend::Tui);
        } else {
            panic!("Expected Confirm command");
        }

        let cli = MenuCli::try_parse_from(["ins-menu", "confirm", "-b", "scratchpad"]).unwrap();
        if let MenuCommands::Confirm { backend, .. } = cli.command {
            assert_eq!(backend, MenuBackend::Scratchpad);
        } else {
            panic!("Expected Confirm command");
        }

        let cli = MenuCli::try_parse_from(["ins-menu", "confirm", "-b", "sp"]).unwrap();
        if let MenuCommands::Confirm { backend, .. } = cli.command {
            assert_eq!(backend, MenuBackend::Scratchpad);
        } else {
            panic!("Expected Confirm command");
        }
    }

    #[test]
    fn slider_preset_is_applied_before_backend_dispatch() {
        let cli = MenuCli::try_parse_from([
            "ins-menu",
            "slide",
            "--preset",
            "audio",
            "--step",
            "2",
            "--label",
            "Custom volume",
            "--backend",
            "instantmenu",
        ])
        .unwrap();
        let MenuCommands::Slide { spec, .. } = cli.command else {
            panic!("Expected Slide command");
        };

        let spec = spec.apply_preset();
        assert_eq!(spec.min, 0);
        assert_eq!(spec.max, 100);
        assert_eq!(spec.step, Some(2));
        assert_eq!(spec.big_step, Some(5));
        assert_eq!(spec.label.as_deref(), Some("Custom volume"));
        assert_eq!(spec.command.first().map(String::as_str), Some("sh"));
    }

    #[test]
    fn choice_accepts_positional_and_compatible_prompt_option() {
        let positional = MenuCli::try_parse_from(["ins-menu", "choice", "Pick one"]).unwrap();
        let MenuCommands::Choice {
            prompt,
            prompt_option,
            ..
        } = positional.command
        else {
            panic!("Expected Choice command");
        };
        assert_eq!(prompt.as_deref(), Some("Pick one"));
        assert_eq!(prompt_option, None);

        let compatible =
            MenuCli::try_parse_from(["ins-menu", "choice", "--prompt", "Pick another"]).unwrap();
        let MenuCommands::Choice {
            prompt,
            prompt_option,
            ..
        } = compatible.command
        else {
            panic!("Expected Choice command");
        };
        assert_eq!(prompt, None);
        assert_eq!(prompt_option.as_deref(), Some("Pick another"));
    }

    #[test]
    fn test_menu_spin_cli_parsing() {
        let cli = MenuCli::try_parse_from([
            "ins-menu",
            "spin",
            "-m",
            "Testing connection...",
            "--",
            "sleep",
            "1",
        ])
        .unwrap();
        if let MenuCommands::Spin {
            message,
            command,
            backend,
        } = cli.command
        {
            assert_eq!(message, "Testing connection...");
            assert_eq!(command, vec!["sleep", "1"]);
            assert_eq!(backend, MenuBackend::Auto);
        } else {
            panic!("Expected Spin command");
        }
    }

    #[test]
    fn test_menu_toast_cli_parsing() {
        let cli =
            MenuCli::try_parse_from(["ins-menu", "toast", "-t", "5.0", "Copied to clipboard"])
                .unwrap();
        if let MenuCommands::Toast {
            message,
            duration,
            backend,
        } = cli.command
        {
            assert_eq!(message, "Copied to clipboard");
            assert_eq!(duration, 5.0);
            assert_eq!(backend, MenuBackend::Auto);
        } else {
            panic!("Expected Toast command");
        }
    }

    #[test]
    fn test_menu_backend_resolution() {
        assert_eq!(
            MenuBackend::Instantmenu.resolve(true),
            ResolvedBackend::Instantmenu
        );
        assert_eq!(MenuBackend::Tui.resolve(true), ResolvedBackend::Tui);
        assert_eq!(
            MenuBackend::Scratchpad.resolve(true),
            ResolvedBackend::Scratchpad
        );
    }

    #[test]
    fn test_choice_and_pick_allow_multiple_flag() {
        let choice_allow_multiple = MenuCli::try_parse_from([
            "ins-menu",
            "choice",
            "--allow-multiple",
            "--items",
            "a b c",
        ])
        .unwrap();
        let MenuCommands::Choice { allow_multiple, .. } = choice_allow_multiple.command else {
            panic!("Expected Choice command");
        };
        assert!(allow_multiple);

        let choice_multi_alias =
            MenuCli::try_parse_from(["ins-menu", "choice", "--multi", "--items", "a b c"]).unwrap();
        let MenuCommands::Choice { allow_multiple, .. } = choice_multi_alias.command else {
            panic!("Expected Choice command");
        };
        assert!(allow_multiple);

        let pick_allow_multiple =
            MenuCli::try_parse_from(["ins-menu", "pick", "--allow-multiple"]).unwrap();
        let MenuCommands::Pick { allow_multiple, .. } = pick_allow_multiple.command else {
            panic!("Expected Pick command");
        };
        assert!(allow_multiple);

        let pick_multi_alias = MenuCli::try_parse_from(["ins-menu", "pick", "--multi"]).unwrap();
        let MenuCommands::Pick { allow_multiple, .. } = pick_multi_alias.command else {
            panic!("Expected Pick command");
        };
        assert!(allow_multiple);
    }
}
