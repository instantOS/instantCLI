use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};

use crate::menu_utils::{ConfirmResult, FzfPreview, FzfResult, FzfSelectable, FzfWrapper, Header};
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored, fzf_mocha_args};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;
use crate::video::cli::{AppendArgs, CheckArgs, RenderArgs, StatsArgs};
use crate::video::document::parse_video_document;
use crate::video::pipeline::{check, convert, stats};
use crate::video::render;

use super::file_selection::{
    discover_video_file_suggestions, discover_video_markdown_suggestions, select_markdown_file,
    select_video_file_with_suggestions,
};
use super::prompts::{
    confirm_action, prompt_optional_path, select_convert_audio_choice, select_render_options,
    select_transcript_choice,
};
use super::types::{ConvertAudioChoice, PromptOutcome, TranscriptChoice};

#[derive(Debug, Clone)]
enum ProjectMenuEntry {
    Render,
    AddRecording,
    Validate,
    Stats,
    ClearCache,
    Back,
}

impl FzfSelectable for ProjectMenuEntry {
    fn fzf_display_text(&self) -> String {
        match self {
            ProjectMenuEntry::Render => format!(
                "{} Render Edited Video",
                format_icon_colored(NerdFont::PlayCircle, colors::GREEN)
            ),
            ProjectMenuEntry::AddRecording => format!(
                "{} Add Recording",
                format_icon_colored(NerdFont::SourceMerge, colors::PEACH)
            ),
            ProjectMenuEntry::Validate => format!(
                "{} Validate Markdown",
                format_icon_colored(NerdFont::CheckCircle, colors::TEAL)
            ),
            ProjectMenuEntry::Stats => format!(
                "{} Show Timeline Stats",
                format_icon_colored(NerdFont::Chart, colors::BLUE)
            ),
            ProjectMenuEntry::ClearCache => format!(
                "{} Clear Cache",
                format_icon_colored(NerdFont::Trash, colors::RED)
            ),
            ProjectMenuEntry::Back => format!("{} Back", format_back_icon()),
        }
    }

    fn fzf_key(&self) -> String {
        match self {
            ProjectMenuEntry::Render => "!__render__".to_string(),
            ProjectMenuEntry::AddRecording => "!__add_recording__".to_string(),
            ProjectMenuEntry::Validate => "!__validate__".to_string(),
            ProjectMenuEntry::Stats => "!__stats__".to_string(),
            ProjectMenuEntry::ClearCache => "!__clear_cache__".to_string(),
            ProjectMenuEntry::Back => "!__back__".to_string(),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        match self {
            ProjectMenuEntry::Render => PreviewBuilder::new()
                .header(NerdFont::PlayCircle, "Render")
                .text("Render a video from an edited markdown timeline.")
                .blank()
                .text("Supports:")
                .bullet("Overlay slides and title cards")
                .bullet("Reels mode output")
                .bullet("Audio preprocessing caches")
                .build(),
            ProjectMenuEntry::AddRecording => PreviewBuilder::new()
                .header(NerdFont::SourceMerge, "Add Recording")
                .text("Add another video recording to this project.")
                .blank()
                .text("This will:")
                .bullet("Transcribe the new recording")
                .bullet("Add it as a new source")
                .bullet("Append timestamped segments to the timeline")
                .build(),
            ProjectMenuEntry::Validate => PreviewBuilder::new()
                .header(NerdFont::CheckCircle, "Validate Markdown")
                .text("Validate markdown and summarize the planned output.")
                .blank()
                .text("Shows segment counts and warnings.")
                .build(),
            ProjectMenuEntry::Stats => PreviewBuilder::new()
                .header(NerdFont::Chart, "Timeline Stats")
                .text("Display statistics for a markdown timeline.")
                .blank()
                .text("Shows segments, slides, and unsupported blocks.")
                .build(),
            ProjectMenuEntry::ClearCache => PreviewBuilder::new()
                .header(NerdFont::Trash, "Clear Cache")
                .text("Delete cached files for this project.")
                .blank()
                .text("This includes:")
                .bullet("Preprocessed audio")
                .bullet("Transcript cache")
                .bullet("Generated slides")
                .build(),
            ProjectMenuEntry::Back => PreviewBuilder::new()
                .header(NerdFont::Cross, "Back")
                .text("Return to the main video menu.")
                .build(),
        }
    }
}

pub async fn run_project_menu() -> Result<()> {
    let suggestions = discover_video_markdown_suggestions()?;
    let Some(markdown_path) = select_markdown_file("Select project", suggestions)? else {
        return Ok(());
    };

    loop {
        let entries = vec![
            ProjectMenuEntry::Render,
            ProjectMenuEntry::AddRecording,
            ProjectMenuEntry::Validate,
            ProjectMenuEntry::Stats,
            ProjectMenuEntry::ClearCache,
            ProjectMenuEntry::Back,
        ];

        let project_name = markdown_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("Project");

        let result = FzfWrapper::builder()
            .header(Header::fancy(project_name))
            .prompt("Select")
            .args(fzf_mocha_args())
            .responsive_layout()
            .select(entries)?;

        match result {
            FzfResult::Selected(entry) => match entry {
                ProjectMenuEntry::Render => {
                    run_render_for_project(&markdown_path).await?;
                }
                ProjectMenuEntry::AddRecording => {
                    run_add_recording(&markdown_path).await?;
                }
                ProjectMenuEntry::Validate => {
                    check::handle_check(CheckArgs {
                        markdown: markdown_path.clone(),
                    })?;
                }
                ProjectMenuEntry::Stats => {
                    stats::handle_stats(StatsArgs {
                        markdown: markdown_path.clone(),
                    })?;
                }
                ProjectMenuEntry::ClearCache => {
                    run_clear_cache(&markdown_path)?;
                }
                ProjectMenuEntry::Back => return Ok(()),
            },
            FzfResult::Cancelled => return Ok(()),
            _ => return Ok(()),
        }
    }
}

async fn run_add_recording(markdown_path: &Path) -> Result<()> {
    let suggestions = discover_video_file_suggestions()?;
    let Some(video_path) = select_video_file_with_suggestions("Select video to add", suggestions)?
    else {
        return Ok(());
    };

    let transcript_choice = match select_transcript_choice()? {
        Some(choice) => choice,
        None => return Ok(()),
    };

    let transcript_path = match transcript_choice {
        TranscriptChoice::Auto => None,
        TranscriptChoice::Provide => {
            use super::file_selection::select_transcript_file;
            match select_transcript_file()? {
                Some(path) => Some(path),
                None => return Ok(()),
            }
        }
    };

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

    convert::handle_append(AppendArgs {
        markdown: markdown_path.to_path_buf(),
        video: video_path,
        transcript: transcript_path,
        force: false,
        no_preprocess,
        preprocessor,
    })
    .await
}

async fn run_render_for_project(markdown_path: &Path) -> Result<()> {
    let render_options = match select_render_options()? {
        Some(options) => options,
        None => return Ok(()),
    };

    let mut reels = render_options.reels;
    let mut subtitles = render_options.subtitles;

    if subtitles && !reels {
        match confirm_action(
            "Subtitles are only supported in reels mode. Enable reels?",
            "Enable reels",
            "Disable subtitles",
        )? {
            ConfirmResult::Yes => reels = true,
            ConfirmResult::No => subtitles = false,
            ConfirmResult::Cancelled => return Ok(()),
        }
    }

    let out_file = if render_options.precache_slides {
        None
    } else {
        match prompt_optional_path(
            "Output path (optional)",
            "Leave empty for default output path",
        )? {
            PromptOutcome::Value(value) => value,
            PromptOutcome::Cancelled => return Ok(()),
        }
    };

    let force = if !render_options.force {
        if let Some(ref out) = out_file {
            if out.exists() {
                match confirm_action("Output file exists. Overwrite?", "Overwrite", "Cancel")? {
                    ConfirmResult::Yes => true,
                    _ => return Ok(()),
                }
            } else {
                false
            }
        } else {
            false
        }
    } else {
        true
    };

    render::handle_render(RenderArgs {
        markdown: markdown_path.to_path_buf(),
        out_file,
        force,
        precache_slides: render_options.precache_slides,
        dry_run: render_options.dry_run,
        reels,
        subtitles,
    })
}

fn run_clear_cache(markdown_path: &Path) -> Result<()> {
    use crate::video::config::VideoDirectories;
    use crate::video::support::utils::compute_file_hash;

    match confirm_action(
        "Delete all cached files for this project?",
        "Delete",
        "Cancel",
    )? {
        ConfirmResult::Yes => {}
        _ => return Ok(()),
    }

    let contents = fs::read_to_string(markdown_path)?;
    let doc = parse_video_document(&contents, markdown_path)?;
    let directories = VideoDirectories::new()?;

    let mut cleared_count = 0;
    for source in &doc.metadata.sources {
        if let Ok(hash) = compute_file_hash(&source.source) {
            let project_paths = directories.project_paths(&hash);
            let transcript_dir = project_paths.transcript_dir();

            if transcript_dir.exists() {
                fs::remove_dir_all(transcript_dir)?;
                cleared_count += 1;
            }
        }
    }

    if cleared_count > 0 {
        FzfWrapper::message(&format!("Cleared cache for {} source(s)", cleared_count))?;
    } else {
        FzfWrapper::message("No cache directories found")?;
    }

    Ok(())
}
