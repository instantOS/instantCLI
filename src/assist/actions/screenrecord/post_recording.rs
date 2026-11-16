use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};

use crate::assist::utils::{AreaSelectionConfig, copy_to_clipboard, show_notification};
use crate::menu::client::HostedMenuClient;

#[derive(Debug, Clone, Copy)]
enum PostRecordingAction {
    Play,
    CopyPath,
    ShowInFileManager,
    OpenYazi,
    OpenTerminal,
    Done,
}

impl PostRecordingAction {
    fn from_key(key: &str) -> Option<Self> {
        match key {
            "p" => Some(Self::Play),
            "c" => Some(Self::CopyPath),
            "f" => Some(Self::ShowInFileManager),
            "y" => Some(Self::OpenYazi),
            "t" => Some(Self::OpenTerminal),
            "d" => Some(Self::Done),
            _ => None,
        }
    }
}

fn post_recording_menu_options() -> Vec<String> {
    vec![
        "p:Play video".to_string(),
        "c:Copy path to clipboard".to_string(),
        "f:Open in file manager".to_string(),
        "y:Open with yazi".to_string(),
        "t:Open terminal in directory".to_string(),
        "d:Done (close menu)".to_string(),
    ]
}

fn recording_parent_dir(output_path: &Path) -> String {
    output_path
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| ".".to_string())
}

fn play_recording(path_str: &str) -> Result<()> {
    Command::new("xdg-open")
        .arg(path_str)
        .spawn()
        .context("Failed to open video")?;
    Ok(())
}

fn copy_recording_path(path_str: &str) -> Result<()> {
    let config = AreaSelectionConfig::new();
    copy_to_clipboard(path_str.as_bytes(), config.display_server())?;
    show_notification("Path copied", path_str)?;
    Ok(())
}

fn open_recording_in_file_manager(path_str: &str, parent_dir: &str) -> Result<()> {
    let file_uri = format!("file://{}", path_str);
    let dbus_result = Command::new("gdbus")
        .args([
            "call",
            "--session",
            "--dest",
            "org.freedesktop.FileManager1",
            "--object-path",
            "/org/freedesktop/FileManager1",
            "--method",
            "org.freedesktop.FileManager1.ShowItems",
            &format!("['{}']", file_uri),
            "''",
        ])
        .status();

    if !dbus_result.is_ok_and(|status| status.success()) {
        Command::new("xdg-open")
            .arg(parent_dir)
            .spawn()
            .context("Failed to open file manager")?;
    }

    Ok(())
}

fn open_recording_in_yazi(path_str: &str) -> Result<()> {
    instantos_terminal::Launcher::new("yazi")
        .title("Recording")
        .arg(path_str)
        .spawn_detached()?;
    Ok(())
}

fn open_terminal_in_directory(parent_dir: &str) -> Result<()> {
    instantos_terminal::Launcher::default()
        .title("Recording Directory")
        .working_directory(parent_dir)
        .spawn_detached()?;
    Ok(())
}

fn notify_recording_saved(path_str: &str) -> Result<()> {
    show_notification("Recording saved", path_str)
}

fn handle_post_recording_action(
    action: PostRecordingAction,
    path_str: &str,
    parent_dir: &str,
) -> Result<()> {
    match action {
        PostRecordingAction::Play => play_recording(path_str),
        PostRecordingAction::CopyPath => copy_recording_path(path_str),
        PostRecordingAction::ShowInFileManager => {
            open_recording_in_file_manager(path_str, parent_dir)
        }
        PostRecordingAction::OpenYazi => open_recording_in_yazi(path_str),
        PostRecordingAction::OpenTerminal => open_terminal_in_directory(parent_dir),
        PostRecordingAction::Done => notify_recording_saved(path_str),
    }
}

pub(super) fn show_post_recording_menu(output_path: &Path) -> Result<()> {
    let path_str = output_path.to_string_lossy().to_string();
    let parent_dir = recording_parent_dir(output_path);

    let client = HostedMenuClient::new();
    let selected = client.chord(post_recording_menu_options())?;
    let action = match selected {
        crate::menu_utils::DialogOutcome::Submitted(key) => {
            PostRecordingAction::from_key(&key).unwrap_or(PostRecordingAction::Done)
        }
        crate::menu_utils::DialogOutcome::Cancelled => PostRecordingAction::Done,
    };

    handle_post_recording_action(action, &path_str, &parent_dir)
}
