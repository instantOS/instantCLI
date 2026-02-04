use anyhow::Result;
use std::path::PathBuf;

use crate::menu_utils::{ConfirmResult, FzfPreview, FzfResult, FzfSelectable, FzfWrapper, Header};
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored, fzf_mocha_args};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;
use crate::video::cli::ConvertArgs;
use crate::video::pipeline::convert;

use super::file_selection::{
    compute_default_output_path, discover_video_file_suggestions,
    select_video_file_with_suggestions,
};
use super::prompts::{confirm_action, select_convert_audio_choice};
use super::types::ConvertAudioChoice;

#[derive(Debug, Clone)]
enum ConvertListEntry {
    Video(PathBuf),
    Add,
    Convert,
    Back,
}

impl FzfSelectable for ConvertListEntry {
    fn fzf_display_text(&self) -> String {
        match self {
            ConvertListEntry::Video(path) => {
                let name = path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown");
                format!(
                    "{} {}",
                    format_icon_colored(NerdFont::Video, colors::LAVENDER),
                    name
                )
            }
            ConvertListEntry::Add => format!(
                "{} Add video",
                format_icon_colored(NerdFont::Plus, colors::GREEN)
            ),
            ConvertListEntry::Convert => format!(
                "{} Convert all",
                format_icon_colored(NerdFont::PlayCircle, colors::PEACH)
            ),
            ConvertListEntry::Back => format!("{} Back", format_back_icon()),
        }
    }

    fn fzf_key(&self) -> String {
        match self {
            ConvertListEntry::Video(path) => format!("video:{}", path.display()),
            ConvertListEntry::Add => "!__add__".to_string(),
            ConvertListEntry::Convert => "!__convert__".to_string(),
            ConvertListEntry::Back => "!__back__".to_string(),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        match self {
            ConvertListEntry::Video(path) => {
                let display = path.display().to_string();
                PreviewBuilder::new()
                    .header(NerdFont::Video, "Video File")
                    .text(&display)
                    .blank()
                    .text("Select to remove from list")
                    .build()
            }
            ConvertListEntry::Add => PreviewBuilder::new()
                .header(NerdFont::Plus, "Add Video")
                .text("Add another video file to the conversion list.")
                .build(),
            ConvertListEntry::Convert => PreviewBuilder::new()
                .header(NerdFont::PlayCircle, "Convert All")
                .text("Convert all videos in the list to markdown.")
                .build(),
            ConvertListEntry::Back => PreviewBuilder::new()
                .header(NerdFont::Cross, "Back")
                .text("Return to previous menu.")
                .build(),
        }
    }
}

pub async fn run_convert_multi() -> Result<()> {
    let mut videos: Vec<PathBuf> = Vec::new();

    loop {
        let mut entries: Vec<ConvertListEntry> = videos
            .iter()
            .map(|p| ConvertListEntry::Video(p.clone()))
            .collect();

        entries.push(ConvertListEntry::Add);
        if !videos.is_empty() {
            entries.push(ConvertListEntry::Convert);
        }
        entries.push(ConvertListEntry::Back);

        let header_text = if videos.is_empty() {
            "Add videos to convert".to_string()
        } else {
            format!("{} video(s) selected", videos.len())
        };

        let result = FzfWrapper::builder()
            .header(Header::fancy(&header_text))
            .prompt("Select")
            .args(fzf_mocha_args())
            .responsive_layout()
            .select(entries)?;

        match result {
            FzfResult::Selected(entry) => match entry {
                ConvertListEntry::Video(path) => {
                    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("video");
                    if let ConfirmResult::Yes =
                        confirm_action(&format!("Remove '{name}' from list?"), "Remove", "Keep")?
                    {
                        videos.retain(|p| p != &path);
                    }
                }
                ConvertListEntry::Add => {
                    let suggestions = discover_video_file_suggestions()?;
                    if let Some(path) =
                        select_video_file_with_suggestions("Select video to add", suggestions)?
                        && !videos.contains(&path)
                    {
                        videos.push(path);
                    }
                }
                ConvertListEntry::Convert => {
                    if videos.is_empty() {
                        FzfWrapper::message("No videos to convert")?;
                        continue;
                    }
                    return run_convert_batch(videos).await;
                }
                ConvertListEntry::Back => return Ok(()),
            },
            FzfResult::Cancelled => return Ok(()),
            _ => return Ok(()),
        }
    }
}

async fn run_convert_batch(videos: Vec<PathBuf>) -> Result<()> {
    let audio_choice = match select_convert_audio_choice()? {
        Some(choice) => choice,
        None => return Ok(()),
    };

    let (no_preprocess, preprocessor) = match audio_choice {
        ConvertAudioChoice::UseConfig => (false, None),
        ConvertAudioChoice::Local => (false, Some("local".to_string())),
        ConvertAudioChoice::Auphonic => (false, Some("auphonic".to_string())),
        ConvertAudioChoice::Skip => (true, None),
    };

    let conflicts: Vec<PathBuf> = videos
        .iter()
        .filter_map(|v| {
            let out = compute_default_output_path(v);
            if out.exists() { Some(out) } else { None }
        })
        .collect();

    let force = if !conflicts.is_empty() {
        let msg = format!(
            "{} existing markdown file(s) will be overwritten. Continue?",
            conflicts.len()
        );
        match confirm_action(&msg, "Overwrite", "Cancel")? {
            ConfirmResult::Yes => true,
            _ => return Ok(()),
        }
    } else {
        false
    };

    for video_path in videos {
        convert::handle_convert(ConvertArgs {
            video: video_path,
            transcript: None,
            out_file: None,
            force,
            no_preprocess,
            preprocessor: preprocessor.clone(),
        })
        .await?;
    }

    Ok(())
}
