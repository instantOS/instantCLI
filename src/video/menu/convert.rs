use anyhow::Result;
use std::path::PathBuf;

use crate::menu_utils::{ConfirmResult, FzfPreview, FzfResult, FzfSelectable, FzfWrapper, Header};
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored, fzf_mocha_args};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;
use crate::video::cli::{AppendArgs, ConvertArgs};
use crate::video::pipeline::convert;

use super::file_selection::{
    compute_default_output_path, discover_video_file_suggestions, select_output_path,
    select_video_file_with_suggestions,
};
use super::project::open_project_for_path;
use super::prompts::{confirm_action, select_convert_audio_choice, select_output_choice};
use super::types::{ConvertAudioChoice, OutputChoice};

#[derive(Debug, Clone)]
enum NewProjectEntry {
    Video(PathBuf),
    Add,
    Create,
    Back,
}

impl FzfSelectable for NewProjectEntry {
    fn fzf_display_text(&self) -> String {
        match self {
            NewProjectEntry::Video(path) => {
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
            NewProjectEntry::Add => format!(
                "{} Add video",
                format_icon_colored(NerdFont::Plus, colors::GREEN)
            ),
            NewProjectEntry::Create => format!(
                "{} Create project",
                format_icon_colored(NerdFont::PlayCircle, colors::PEACH)
            ),
            NewProjectEntry::Back => format!("{} Back", format_back_icon()),
        }
    }

    fn fzf_key(&self) -> String {
        match self {
            NewProjectEntry::Video(path) => format!("video:{}", path.display()),
            NewProjectEntry::Add => "!__add__".to_string(),
            NewProjectEntry::Create => "!__create__".to_string(),
            NewProjectEntry::Back => "!__back__".to_string(),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        match self {
            NewProjectEntry::Video(path) => {
                let display = path.display().to_string();
                PreviewBuilder::new()
                    .header(NerdFont::Video, "Video File")
                    .text(&display)
                    .blank()
                    .text("Select to remove from list")
                    .build()
            }
            NewProjectEntry::Add => PreviewBuilder::new()
                .header(NerdFont::Plus, "Add Video")
                .text("Add another video file to the project.")
                .build(),
            NewProjectEntry::Create => PreviewBuilder::new()
                .header(NerdFont::PlayCircle, "Create Project")
                .text("Create a new project with all selected videos.")
                .blank()
                .text("All videos will be added as sources to a single markdown file.")
                .build(),
            NewProjectEntry::Back => PreviewBuilder::new()
                .header(NerdFont::Cross, "Back")
                .text("Return to previous menu.")
                .build(),
        }
    }
}

/// Create a new project from multiple videos.
/// This creates a single markdown file with all videos as sources.
pub async fn run_new_project() -> Result<()> {
    let mut videos: Vec<PathBuf> = Vec::new();

    loop {
        let mut entries: Vec<NewProjectEntry> = videos
            .iter()
            .map(|p| NewProjectEntry::Video(p.clone()))
            .collect();

        entries.push(NewProjectEntry::Add);
        if !videos.is_empty() {
            entries.push(NewProjectEntry::Create);
        }
        entries.push(NewProjectEntry::Back);

        let header_text = if videos.is_empty() {
            "Add videos to create project".to_string()
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
                NewProjectEntry::Video(path) => {
                    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("video");
                    if let ConfirmResult::Yes =
                        confirm_action(&format!("Remove '{name}' from list?"), "Remove", "Keep")?
                    {
                        videos.retain(|p| p != &path);
                    }
                }
                NewProjectEntry::Add => {
                    let suggestions = discover_video_file_suggestions()?;
                    if let Some(path) =
                        select_video_file_with_suggestions("Select video to add", suggestions)?
                        && !videos.contains(&path)
                    {
                        videos.push(path);
                    }
                }
                NewProjectEntry::Create => {
                    if videos.is_empty() {
                        FzfWrapper::message("No videos selected")?;
                        continue;
                    }
                    return create_multi_source_project(videos).await;
                }
                NewProjectEntry::Back => return Ok(()),
            },
            FzfResult::Cancelled => return Ok(()),
            _ => return Ok(()),
        }
    }
}

/// Create a single markdown file with multiple video sources.
async fn create_multi_source_project(videos: Vec<PathBuf>) -> Result<()> {
    let audio_choice = match select_convert_audio_choice()? {
        Some(choice) => choice,
        None => return Ok(()),
    };

    let (no_preprocess, preprocessor) = match audio_choice {
        ConvertAudioChoice::UseConfig => (false, None),
        ConvertAudioChoice::Local => (false, Some(ConvertAudioChoice::Local.to_string())),
        ConvertAudioChoice::Auphonic => (false, Some(ConvertAudioChoice::Auphonic.to_string())),
        ConvertAudioChoice::Skip => (true, None),
    };

    // Use the first video to determine default output path
    let first_video = &videos[0];
    let default_output = compute_default_output_path(first_video);
    let default_name = default_output
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("project.video.md")
        .to_string();

    let output_choice = match select_output_choice("Project output", &default_name)? {
        Some(choice) => choice,
        None => return Ok(()),
    };

    let output_path = match output_choice {
        OutputChoice::Default => default_output,
        OutputChoice::Custom => {
            let start_dir = first_video.parent().map(|p| p.to_path_buf());
            match select_output_path(&default_name, start_dir)? {
                Some(path) => path,
                None => return Ok(()),
            }
        }
    };

    let force = if output_path.exists() {
        match confirm_action(
            "Markdown file already exists. Overwrite?",
            "Overwrite",
            "Cancel",
        )? {
            ConfirmResult::Yes => true,
            _ => return Ok(()),
        }
    } else {
        false
    };

    // Create the project with the first video
    convert::handle_convert(ConvertArgs {
        video: videos[0].clone(),
        transcript: None,
        out_file: Some(output_path.clone()),
        force,
        no_preprocess,
        preprocessor: preprocessor.clone(),
    })
    .await?;

    // Append additional videos
    for video in videos.into_iter().skip(1) {
        convert::handle_append(AppendArgs {
            markdown: output_path.clone(),
            video,
            transcript: None,
            force: false,
            no_preprocess,
            preprocessor: preprocessor.clone(),
        })
        .await?;
    }

    // Show success message
    let project_name = output_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("project");
    FzfWrapper::message(&format!("Project '{}' created successfully!", project_name))?;

    // Open the project menu for the new project
    open_project_for_path(&output_path).await
}
