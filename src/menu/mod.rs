use crate::menu_utils::{
    ChecklistResult, ConfirmResult, FilePickerResult, FilePickerScope, FzfPreview, FzfResult,
    FzfSelectable, FzfWrapper, Header, MenuCursor, MenuItem, MenuWrapper,
};
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored, fzf_mocha_args};
use crate::ui::nerd_font::NerdFont;
use crate::{dot, game, settings, video};
use anyhow::{Context, Result, anyhow};
use clap::ValueEnum;
use protocol::SerializableMenuItem;
use std::path::PathBuf;
use std::process::Command;

pub mod chord;
pub mod client;
mod fallback;
pub mod processing;
pub mod protocol;
pub mod scratchpad_manager;
pub mod server;
pub mod slide;
pub mod tui;
use client::MenuClient;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AllMenuEntry {
    Settings,
    Dotfiles,
    Games,
    Video,
    Quit,
}

impl FzfSelectable for AllMenuEntry {
    fn fzf_display_text(&self) -> String {
        match self {
            AllMenuEntry::Settings => format!(
                "{} Settings",
                format_icon_colored(NerdFont::Settings, colors::BLUE)
            ),
            AllMenuEntry::Dotfiles => format!(
                "{} Dotfiles",
                format_icon_colored(NerdFont::DotFile, colors::MAUVE)
            ),
            AllMenuEntry::Games => format!(
                "{} Games",
                format_icon_colored(NerdFont::Gamepad, colors::GREEN)
            ),
            AllMenuEntry::Video => format!(
                "{} Video",
                format_icon_colored(NerdFont::Video, colors::YELLOW)
            ),
            AllMenuEntry::Quit => format!("{} Back", format_back_icon()),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        match self {
            AllMenuEntry::Settings => FzfPreview::Text(
                "Open the main system settings TUI (same as `ins settings`).".to_string(),
            ),
            AllMenuEntry::Dotfiles => FzfPreview::Text(
                "Open the dotfile management menu (same as `ins dot menu`).".to_string(),
            ),
            AllMenuEntry::Games => FzfPreview::Text(
                "Open the game manager menu (same as `ins game menu`).".to_string(),
            ),
            AllMenuEntry::Video => FzfPreview::Text(
                "Open the video tools menu (same as `ins video menu`).".to_string(),
            ),
            AllMenuEntry::Quit => FzfPreview::Text(
                "Close the unified menu and return to the shell.".to_string(),
            ),
        }
    }
}

async fn run_all_menu(debug: bool) -> Result<i32> {
    let mut cursor = MenuCursor::new();

    loop {
        let entries = vec![
            MenuItem::entry(AllMenuEntry::Settings),
            MenuItem::entry(AllMenuEntry::Dotfiles),
            MenuItem::entry(AllMenuEntry::Games),
            MenuItem::entry(AllMenuEntry::Video),
            MenuItem::line(),
            MenuItem::entry(AllMenuEntry::Quit),
        ];

        let mut builder = FzfWrapper::builder()
            .header(Header::fancy("InstantCLI Menus"))
            .prompt("Select interface")
            .args(fzf_mocha_args())
            .responsive_layout();

        if let Some(index) = cursor.initial_index(&entries) {
            builder = builder.initial_index(index);
        }

        let result = builder.select_menu(entries.clone())?;

        match result {
            FzfResult::Selected(entry) => {
                cursor.update_from_key(&entry.fzf_key());
                match entry {
                    AllMenuEntry::Settings => {
                        // Match `ins settings` default behavior (non-GUI, non-privileged)
                        settings::ui::menu::run_settings_ui(debug, false, None)?;
                    }
                    AllMenuEntry::Dotfiles => {
                        // Same as `ins dot menu`
                        dot::menu::dot_menu(debug)?;
                    }
                    AllMenuEntry::Games => {
                        // Same as `ins game menu`
                        game::menu::game_menu(None)?;
                    }
                    AllMenuEntry::Video => {
                        // Same as `ins video menu`
                        video::menu::video_menu(debug).await?;
                    }
                    AllMenuEntry::Quit => return Ok(0),
                }
            }
            FzfResult::Cancelled => return Ok(1),
            FzfResult::MultiSelected(_) | FzfResult::Error(_) => return Ok(2),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum SliderPreset {
    #[value(alias = "volume")]
    Audio,
    #[value(alias = "brightness")]
    #[value(alias = "bright")]
    Brightness,
}

struct PresetConfig {
    min: i64,
    max: i64,
    value: Option<i64>,
    step: Option<i64>,
    big_step: Option<i64>,
    label: Option<String>,
    command: Vec<String>,
}

impl SliderPreset {
    fn config(self) -> PresetConfig {
        match self {
            SliderPreset::Audio => PresetConfig {
                min: 0,
                max: 100,
                value: Self::detect_audio_volume(),
                step: Some(1),
                big_step: Some(5),
                label: Some("Audio Volume".to_string()),
                command: vec![
                    "sh".to_string(),
                    "-c".to_string(),
                    Self::audio_command_script(),
                    "ins-menu-slide-audio".to_string(),
                ],
            },
            SliderPreset::Brightness => PresetConfig {
                min: 0,
                max: 100,
                value: Self::detect_brightness_level(),
                step: Some(1),
                big_step: Some(5),
                label: Some("Screen Brightness".to_string()),
                command: vec![
                    "sh".to_string(),
                    "-c".to_string(),
                    Self::brightness_command_script(),
                    "ins-menu-slide-brightness".to_string(),
                ],
            },
        }
    }

    fn detect_audio_volume() -> Option<i64> {
        Self::wpctl_volume()
    }

    fn detect_brightness_level() -> Option<i64> {
        Self::brightnessctl_percentage()
    }

    fn audio_command_script() -> String {
        let mut script = String::from("value=\"$1\"\n\n");
        script.push_str("wpctl set-volume @DEFAULT_AUDIO_SINK@ \"${value}%\" 2>/dev/null\n\n");
        script.push_str(&Self::notification_script(
            "instantcli-volume",
            "audio-volume-medium-symbolic",
            "Volume [${value}%]",
        ));
        script
    }

    fn brightness_command_script() -> String {
        let mut script = String::from("value=\"$1\"\n\n");
        script.push_str("brightnessctl --quiet set \"${value}%\" 2>/dev/null\n\n");
        script.push_str(&Self::notification_script(
            "instantcli-brightness",
            "display-brightness-medium-symbolic",
            "Brightness [${value}%]",
        ));
        script
    }

    fn notification_script(stack_tag: &str, icon: &str, label: &str) -> String {
        format!(
            "dunstify --appname instantCLI \\\n+    -h string:x-dunst-stack-tag:{stack_tag} \\\n+    -h int:value:\"${{value}}\" \\\n+    -i {icon} \\\n+    \"{label}\" 2>/dev/null",
            stack_tag = stack_tag,
            icon = icon,
            label = label
        )
    }

    fn wpctl_volume() -> Option<i64> {
        let output = Self::command_output("wpctl", &["get-volume", "@DEFAULT_AUDIO_SINK@"])?;
        let fraction = output.split_whitespace().find_map(|token| {
            let sanitized = token.trim_matches(|c: char| matches!(c, '[' | ']' | ',' | ':'));
            sanitized.parse::<f64>().ok()
        })?;

        let percent = (fraction * 100.0).trunc().clamp(0.0, 100.0);
        Some(percent as i64)
    }
    fn brightnessctl_percentage() -> Option<i64> {
        let current = Self::command_output("brightnessctl", &["get"])?
            .parse::<f64>()
            .ok()?;
        let max = Self::command_output("brightnessctl", &["max"])?
            .parse::<f64>()
            .ok()?;

        if max <= 0.0 {
            return None;
        }

        let percent = (current / max * 100.0).round().clamp(0.0, 100.0);
        Some(percent as i64)
    }

    fn command_output(program: &str, args: &[&str]) -> Option<String> {
        let output = Command::new(program).args(args).output().ok()?;
        if !output.status.success() {
            return None;
        }

        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if stdout.is_empty() {
            None
        } else {
            Some(stdout)
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
        MenuCommands::All => run_all_menu(_debug).await,
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
        MenuCommands::Message {
            ref message,
            ref title,
        } => {
            let mut builder = FzfWrapper::builder().message(message);
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

                match FzfWrapper::builder()
                    .prompt(prompt.clone())
                    .multi_select(multi)
                    .select(item_list)?
                {
                    crate::menu_utils::FzfResult::Selected(item) => {
                        println!("{}", item.display_text);
                        Ok(0) // Selected
                    }
                    crate::menu_utils::FzfResult::MultiSelected(items) => {
                        for item in items {
                            println!("{}", item.display_text);
                        }
                        Ok(0) // Selected
                    }
                    crate::menu_utils::FzfResult::Cancelled => Ok(1), // Cancelled
                    crate::menu_utils::FzfResult::Error(e) => {
                        eprintln!("Error: {e}");
                        Ok(2) // Error
                    }
                }
            }
        }
        MenuCommands::Chord {
            ref chords,
            stdin,
            gui,
        } => {
            let mut combined = chords.clone();

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

            if gui {
                let client = MenuClient::new();
                match client.chord(combined) {
                    Ok(Some(sequence)) => {
                        println!("{sequence}");
                        Ok(0)
                    }
                    Ok(None) => Ok(1),
                    Err(e) => {
                        eprintln!("GUI menu error: {e}");
                        Ok(3)
                    }
                }
            } else {
                chord::run_chord_command(&combined)
            }
        }
        MenuCommands::Slide {
            min,
            max,
            value,
            step,
            big_step,
            label,
            command,
            gui,
            preset,
        } => {
            let mut min_value = min;
            let mut max_value = max;
            let mut initial_value = value;
            let mut step_value = step;
            let mut big_step_value = big_step;
            let mut label_value = label;
            let mut command_args = command;

            if let Some(preset_kind) = preset {
                let preset_config = preset_kind.config();
                min_value = preset_config.min;
                max_value = preset_config.max;
                initial_value = initial_value.or(preset_config.value);
                step_value = step_value.or(preset_config.step);
                big_step_value = big_step_value.or(preset_config.big_step);
                label_value = label_value.or(preset_config.label);
                if command_args.is_empty() {
                    command_args = preset_config.command;
                }
            }

            if gui {
                let client = MenuClient::new();
                match client.slide(protocol::SliderRequest {
                    min: min_value,
                    max: max_value,
                    value: initial_value,
                    step: step_value,
                    big_step: big_step_value,
                    label: label_value.clone(),
                    command: command_args.clone(),
                }) {
                    Ok(Some(result)) => {
                        println!("{result}");
                        Ok(0)
                    }
                    Ok(None) => Ok(1),
                    Err(e) => {
                        eprintln!("GUI menu error: {e}");
                        Ok(3)
                    }
                }
            } else {
                let request = protocol::SliderRequest {
                    min: min_value,
                    max: max_value,
                    value: initial_value,
                    step: step_value,
                    big_step: big_step_value,
                    label: label_value,
                    command: command_args,
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
        MenuCommands::Pick {
            ref start,
            dirs,
            files,
            multi,
            gui,
        } => {
            let scope = match (dirs, files) {
                (true, false) => FilePickerScope::Directories,
                (false, true) => FilePickerScope::Files,
                (true, true) => FilePickerScope::FilesAndDirectories,
                (false, false) => FilePickerScope::Files,
            };

            if gui {
                client::handle_gui_request(&command)
            } else {
                let mut builder = MenuWrapper::file_picker().scope(scope).multi(multi);

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
        MenuCommands::Password { ref prompt, gui } => {
            if gui {
                client::handle_gui_request(&command)
            } else {
                match FzfWrapper::password(prompt) {
                    Ok(crate::menu_utils::FzfResult::Selected(password)) => {
                        println!("{password}");
                        Ok(0) // Success
                    }
                    Ok(crate::menu_utils::FzfResult::Cancelled) => Ok(1), // Cancelled
                    Ok(crate::menu_utils::FzfResult::Error(e)) => {
                        eprintln!("Error: {e}");
                        Ok(2) // Error
                    }
                    Ok(_) => Ok(1),
                    Err(e) => {
                        eprintln!("Error: {e}");
                        Ok(2) // Error
                    }
                }
            }
        }
        MenuCommands::Status => {
            let client = client::MenuClient::new();
            if client.is_fallback() {
                match client.status() {
                    Ok(status_info) => {
                        client::print_status_info(&status_info);
                        println!();
                        println!(
                            "Fallback mode: interactive dialogs run in transient kitty terminals."
                        );
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
        MenuCommands::Checklist {
            ref items,
            ref confirm,
        } => {
            // Parse items from stdin if empty, otherwise from --items arg
            let item_list: Vec<String> = if items.is_empty() {
                // Read from stdin (one item per line, like `ins menu choice`)
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
        #[arg(long, default_value = "Are you sure?")]
        message: String,
        /// Use GUI menu server instead of local fzf
        #[arg(long)]
        gui: bool,
    },
    /// Unified launcher for all major InstantCLI TUIs
    All,
    /// Show a message dialog with an OK button
    Message {
        /// Message to display
        message: String,
        /// Optional title for the message
        #[arg(long)]
        title: Option<String>,
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
    /// Show password input dialog and output password to stdout
    Password {
        /// Password prompt message
        #[arg(long, default_value = "Enter password:")]
        prompt: String,
        /// Use GUI menu server instead of local fzf
        #[arg(long)]
        gui: bool,
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
        #[arg(long)]
        multi: bool,
        /// Use GUI menu server instead of local picker
        #[arg(long)]
        gui: bool,
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
        /// Use GUI menu server instead of local chord picker
        #[arg(long)]
        gui: bool,
    },
    /// Menu server management commands
    Server {
        #[command(subcommand)]
        command: ServerCommands,
    },
    /// Show a slider prompt similar to the legacy islide utility
    Slide {
        /// Minimum slider value
        #[arg(long, default_value_t = 0)]
        min: i64,
        /// Maximum slider value
        #[arg(long, default_value_t = 100)]
        max: i64,
        /// Initial slider value
        #[arg(long = "value")]
        value: Option<i64>,
        /// Small step increment for h/l and arrow keys
        #[arg(long = "step")]
        step: Option<i64>,
        /// Large step increment for j/k and vertical arrows
        #[arg(long = "big-step")]
        big_step: Option<i64>,
        /// Optional label displayed above the slider
        #[arg(long)]
        label: Option<String>,
        /// Command to execute on value changes (value appended as final arg)
        #[arg(long = "command", value_name = "CMD", num_args = 1..)]
        command: Vec<String>,
        /// Use a preconfigured slider preset
        #[arg(long, value_enum)]
        preset: Option<SliderPreset>,
        /// Use GUI menu server instead of local slider
        #[arg(long)]
        gui: bool,
    },
    /// Show a checklist dialog for testing the checklist utility
    Checklist {
        /// Items to display in checklist (space-separated). If empty, uses sample items.
        #[arg(long, default_value = "")]
        items: String,
        /// Text for the confirm button
        #[arg(long, default_value = "Continue")]
        confirm: String,
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
