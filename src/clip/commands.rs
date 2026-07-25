use anyhow::Result;
use clap::Subcommand;

use crate::assist::utils::copy_to_clipboard;
use crate::common::display_server::DisplayServer;
use crate::menu_utils::{ConfirmResult, FzfWrapper};
use crate::ui::prelude::*;

use super::{history, menu, service};

#[derive(Subcommand, Debug, Clone)]
pub enum ClipCommands {
    /// List clipboard history without opening a menu
    List,
    /// Restore an entry by ID (unambiguous prefixes are accepted)
    Copy { id: String },
    /// Delete an entry by ID (unambiguous prefixes are accepted)
    Delete { id: String },
    /// Clear all clipboard history
    Clear {
        /// Skip the confirmation prompt
        #[arg(long)]
        yes: bool,
    },
    /// Install, enable, and start clipboard history capture
    Enable,
    /// Stop and disable clipboard history capture
    Disable,
    /// Show clipboard history capture status
    Status,
    /// Open clipboard history settings
    Settings,
    /// Render an entry for the interactive preview pane
    #[command(hide = true)]
    Preview { id: String },
}

pub fn handle_clip_command(command: &Option<ClipCommands>, gui: bool, debug: bool) -> Result<()> {
    if gui {
        anyhow::ensure!(
            command.is_none(),
            "--gui cannot be combined with a clip subcommand"
        );
        return launch_in_terminal(debug);
    }

    match command {
        None => interactive(),
        Some(ClipCommands::List) => list(),
        Some(ClipCommands::Copy { id }) => copy(id),
        Some(ClipCommands::Delete { id }) => delete(id),
        Some(ClipCommands::Clear { yes }) => clear(*yes),
        Some(ClipCommands::Enable) => enable(),
        Some(ClipCommands::Disable) => disable(),
        Some(ClipCommands::Status) => show_status(),
        Some(ClipCommands::Settings) => settings(),
        Some(ClipCommands::Preview { id }) => preview(id),
    }
}

fn preview(id: &str) -> Result<()> {
    let backend = history::ClipBackend::detect()?;
    let entry = history::find(backend, id)?;
    super::preview::render(&entry)
}

fn interactive() -> Result<()> {
    let backend = history::ClipBackend::detect()?;
    menu::run(backend)
}

fn settings() -> Result<()> {
    super::settings::run(history::ClipBackend::detect()?)
}

fn list() -> Result<()> {
    let entries = history::load(history::ClipBackend::detect()?)?;
    if get_output_format() == OutputFormat::Json {
        println!(
            "{}",
            serde_json::to_string_pretty(&history::output_entries(&entries)?)?
        );
    } else if entries.is_empty() {
        emit(
            Level::Info,
            "clip.list.empty",
            "Clipboard history is empty.",
            None,
        );
    } else {
        for entry in entries {
            println!("{}  {}", entry.id, entry.summary);
        }
    }
    Ok(())
}

fn copy(id: &str) -> Result<()> {
    let backend = history::ClipBackend::detect()?;
    let entry = history::find(backend, id)?;
    copy_to_clipboard(&entry.decode()?, &DisplayServer::detect())?;
    emit(
        Level::Success,
        "clip.copied",
        &format!("Restored clipboard entry {}.", entry.id),
        Some(serde_json::json!({ "id": entry.id })),
    );
    Ok(())
}

fn delete(id: &str) -> Result<()> {
    let backend = history::ClipBackend::detect()?;
    let entry = history::find(backend, id)?;
    history::delete(backend, &entry.id)?;
    emit(
        Level::Success,
        "clip.deleted",
        &format!("Deleted clipboard entry {}.", entry.id),
        Some(serde_json::json!({ "id": entry.id })),
    );
    Ok(())
}

fn clear(skip_confirmation: bool) -> Result<()> {
    if !skip_confirmation
        && FzfWrapper::confirm("Permanently clear all clipboard history?")? != ConfirmResult::Yes
    {
        return Ok(());
    }
    let count = history::clear(history::ClipBackend::detect()?)?;
    emit(
        Level::Success,
        "clip.cleared",
        &format!("Cleared {count} clipboard entries."),
        Some(serde_json::json!({ "deleted": count })),
    );
    Ok(())
}

fn enable() -> Result<()> {
    let backend = history::ClipBackend::detect()?;
    if service::enable(backend)? {
        emit(
            Level::Success,
            "clip.service.enabled",
            &format!(
                "Clipboard history capture is enabled and running with {}.",
                backend.name()
            ),
            None,
        );
    }
    Ok(())
}

fn disable() -> Result<()> {
    let backend = history::ClipBackend::detect()?;
    service::disable(backend)?;
    emit(
        Level::Success,
        "clip.service.disabled",
        "Clipboard history capture is disabled.",
        None,
    );
    Ok(())
}

fn show_status() -> Result<()> {
    let backend = history::ClipBackend::detect()?;
    let status = service::status(backend);
    let entries = if status.installed {
        history::load(backend)?.len()
    } else {
        0
    };
    let data = serde_json::json!({
        "backend": status.backend,
        "installed": status.installed,
        "enabled": status.enabled,
        "active": status.active,
        "entries": entries,
    });
    if get_output_format() == OutputFormat::Json {
        println!("{}", serde_json::to_string_pretty(&data)?);
    } else {
        println!("Backend: {}", status.backend.name());
        println!(
            "Clipboard capture: {}",
            if status.active { "running" } else { "stopped" }
        );
        println!(
            "Starts on login: {}",
            if status.enabled { "yes" } else { "no" }
        );
        println!(
            "Backend installed: {}",
            if status.installed { "yes" } else { "no" }
        );
        println!("History entries: {entries}");
    }
    Ok(())
}

fn launch_in_terminal(debug: bool) -> Result<()> {
    let mut args = Vec::new();
    if debug {
        args.push("--debug".to_string());
    }
    args.push("clip".to_string());
    let executable = std::env::current_exe()?;
    crate::common::terminal::TerminalLauncher::new(executable.to_string_lossy().as_ref())
        .class("ins-clip")
        .title("Clipboard History")
        .args(args)
        .launch()
}
