use anyhow::{Context, Result, anyhow, bail};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use crate::menu_utils::{ConfirmResult, FzfPreview, FzfResult, FzfSelectable, FzfWrapper, Header};
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored, fzf_mocha_args};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;
use crate::video::cli::{AppendArgs, CheckArgs, RenderArgs};
use crate::video::document::parse_video_document;
use crate::video::pipeline::{check, convert};
use crate::video::render;

use super::file_selection::{
    discover_video_file_suggestions, discover_video_suggestions, select_markdown_file,
    select_output_path, select_video_file_with_suggestions,
};
use super::prompts::{
    confirm_action, prompt_optional_path, select_convert_audio_choice, select_render_options,
    select_transcript_choice,
};
use super::types::{ConvertAudioChoice, PromptOutcome, TranscriptChoice};

#[derive(Debug, Clone)]
enum ProjectMenuEntry {
    OpenRendered { path: PathBuf, is_current: bool },
    Render,
    AddRecording,
    Validate,
    Edit,
    ClearCache,
    Back,
}

impl std::fmt::Display for ProjectMenuEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProjectMenuEntry::OpenRendered { .. } => write!(f, "!__open_rendered__"),
            ProjectMenuEntry::Render => write!(f, "!__render__"),
            ProjectMenuEntry::AddRecording => write!(f, "!__add_recording__"),
            ProjectMenuEntry::Validate => write!(f, "!__validate__"),
            ProjectMenuEntry::Edit => write!(f, "!__edit__"),
            ProjectMenuEntry::ClearCache => write!(f, "!__clear_cache__"),
            ProjectMenuEntry::Back => write!(f, "!__back__"),
        }
    }
}

impl FzfSelectable for ProjectMenuEntry {
    fn fzf_display_text(&self) -> String {
        match self {
            ProjectMenuEntry::OpenRendered { is_current, .. } => {
                if *is_current {
                    format!(
                        "{} Open Rendered Video",
                        format_icon_colored(NerdFont::Video, colors::GREEN)
                    )
                } else {
                    format!(
                        "{} Open Rendered Video (outdated)",
                        format_icon_colored(NerdFont::Video, colors::YELLOW)
                    )
                }
            }
            ProjectMenuEntry::Render => format!(
                "{} Render Edited Video",
                format_icon_colored(NerdFont::PlayCircle, colors::GREEN)
            ),
            ProjectMenuEntry::AddRecording => format!(
                "{} Add Recording",
                format_icon_colored(NerdFont::SourceMerge, colors::PEACH)
            ),
            ProjectMenuEntry::Validate => format!(
                "{} Inspect Timeline",
                format_icon_colored(NerdFont::CheckCircle, colors::TEAL)
            ),
            ProjectMenuEntry::Edit => format!(
                "{} Edit Markdown",
                format_icon_colored(NerdFont::Edit, colors::MAUVE)
            ),
            ProjectMenuEntry::ClearCache => format!(
                "{} Clear Cache",
                format_icon_colored(NerdFont::Trash, colors::RED)
            ),
            ProjectMenuEntry::Back => format!("{} Back", format_back_icon()),
        }
    }

    fn fzf_key(&self) -> String {
        self.to_string()
    }

    fn fzf_preview(&self) -> FzfPreview {
        match self {
            ProjectMenuEntry::OpenRendered {
                path, is_current, ..
            } => {
                let mut builder = PreviewBuilder::new()
                    .header(NerdFont::Video, "Rendered Video")
                    .field("File", &path.display().to_string());

                if *is_current {
                    builder = builder.blank().line(
                        colors::GREEN,
                        Some(NerdFont::CheckCircle),
                        "Up to date — rendered after last markdown edit",
                    );
                } else {
                    builder = builder.blank().line(
                        colors::YELLOW,
                        Some(NerdFont::Warning),
                        "Outdated — markdown has been edited since last render",
                    );
                }
                builder.build()
            }
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
                .header(NerdFont::CheckCircle, "Inspect")
                .text("Validate markdown and show timeline statistics.")
                .blank()
                .text("Shows:")
                .bullet("Source availability")
                .bullet("Planned duration")
                .bullet("Segment and slide counts")
                .bullet("Unsupported block warnings")
                .build(),
            ProjectMenuEntry::Edit => PreviewBuilder::new()
                .header(NerdFont::Edit, "Edit Markdown")
                .text("Open the markdown file in your text editor.")
                .blank()
                .text("Uses $EDITOR or falls back to 'nvim'.")
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
    let suggestions = discover_video_suggestions()?;
    let Some(markdown_path) = select_markdown_file("Select project", suggestions)? else {
        return Ok(());
    };

    open_project_for_path(&markdown_path).await
}

/// Open the project menu for a specific markdown path.
/// This is used when we already know the path (e.g., after creating a new project).
pub async fn open_project_for_path(markdown_path: &Path) -> Result<()> {
    loop {
        let mut entries = Vec::new();

        if let Some(entry) = rendered_entry_for_project(markdown_path) {
            entries.push(entry);
        }

        entries.extend([
            ProjectMenuEntry::Render,
            ProjectMenuEntry::AddRecording,
            ProjectMenuEntry::Validate,
            ProjectMenuEntry::Edit,
            ProjectMenuEntry::ClearCache,
            ProjectMenuEntry::Back,
        ]);

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
                ProjectMenuEntry::OpenRendered { ref path, .. } => {
                    show_rendered_video_menu(path)?;
                }
                ProjectMenuEntry::Render => {
                    run_render_for_project(markdown_path).await?;
                }
                ProjectMenuEntry::AddRecording => {
                    run_add_recording(markdown_path).await?;
                }
                ProjectMenuEntry::Validate => {
                    let lines = check::check_report_lines(CheckArgs {
                        markdown: markdown_path.to_path_buf(),
                    })
                    .await?;
                    show_report_dialog("Timeline Inspection", lines)?;
                }
                ProjectMenuEntry::Edit => {
                    run_edit_for_project(markdown_path)?;
                }
                ProjectMenuEntry::ClearCache => {
                    run_clear_cache(markdown_path)?;
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
        ConvertAudioChoice::Local => (false, Some(ConvertAudioChoice::Local.to_string())),
        ConvertAudioChoice::Auphonic => (false, Some(ConvertAudioChoice::Auphonic.to_string())),
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

    let reels = render_options.reels;
    let subtitles = render_options.subtitles;

    let render_mode = if reels {
        render::RenderMode::Reels
    } else {
        render::RenderMode::Standard
    };

    let mut out_file = if render_options.precache_slides {
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

    let mut force = render_options.force;
    if !render_options.precache_slides && !force {
        let markdown_dir = markdown_path.parent().unwrap_or_else(|| Path::new("."));
        let default_source = resolve_default_source_path(markdown_path, markdown_dir)?;
        loop {
            let output_path = resolve_render_output_path(
                out_file.as_ref(),
                markdown_dir,
                &default_source,
                render_mode,
            )?;
            if !output_path.exists() {
                break;
            }

            match prompt_output_conflict(&output_path)? {
                Some(OutputConflictChoice::Overwrite) => {
                    force = true;
                    break;
                }
                Some(OutputConflictChoice::Rename) => {
                    let default_name = output_path
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("output.mp4")
                        .to_string();
                    let start_dir = output_path.parent().map(|p| p.to_path_buf());
                    let Some(path) = select_output_path(&default_name, start_dir)? else {
                        return Ok(());
                    };
                    out_file = Some(path);
                }
                _ => return Ok(()),
            }
        }
    }

    let start = Instant::now();
    let output_path = render::handle_render(RenderArgs {
        markdown: markdown_path.to_path_buf(),
        out_file,
        force,
        precache_slides: render_options.precache_slides,
        dry_run: render_options.dry_run,
        reels,
        subtitles,
    })
    .await?;

    if let Some(output_path) = output_path {
        let elapsed = start.elapsed();
        show_post_render_menu(&output_path, Some(elapsed))?;
    }

    Ok(())
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

fn show_report_dialog(title: &str, lines: Vec<String>) -> Result<()> {
    if lines.is_empty() {
        FzfWrapper::message("No details available")?;
    } else {
        FzfWrapper::builder()
            .message(lines.join("\n"))
            .title(title)
            .message_dialog()?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum OutputConflictChoice {
    Overwrite,
    Rename,
    Cancel,
}

impl OutputConflictChoice {
    fn key(&self) -> &'static str {
        match self {
            OutputConflictChoice::Overwrite => "overwrite",
            OutputConflictChoice::Rename => "rename",
            OutputConflictChoice::Cancel => "cancel",
        }
    }
}

#[derive(Clone)]
struct OutputConflictOption {
    choice: OutputConflictChoice,
    label: String,
    preview: FzfPreview,
}

impl OutputConflictOption {
    fn new(choice: OutputConflictChoice, label: String, preview: FzfPreview) -> Self {
        Self {
            choice,
            label,
            preview,
        }
    }
}

impl FzfSelectable for OutputConflictOption {
    fn fzf_display_text(&self) -> String {
        self.label.clone()
    }

    fn fzf_preview(&self) -> FzfPreview {
        self.preview.clone()
    }

    fn fzf_key(&self) -> String {
        format!("output_conflict:{}", self.choice.key())
    }
}

fn prompt_output_conflict(output_path: &Path) -> Result<Option<OutputConflictChoice>> {
    let path_display = output_path.display().to_string();
    let options = vec![
        OutputConflictOption::new(
            OutputConflictChoice::Overwrite,
            format!(
                "{} Overwrite existing file",
                format_icon_colored(NerdFont::Warning, colors::YELLOW)
            ),
            PreviewBuilder::new()
                .header(NerdFont::Warning, "Overwrite output")
                .text("Replace the existing file with the new render.")
                .blank()
                .field("File", &path_display)
                .line(
                    colors::YELLOW,
                    Some(NerdFont::InfoCircle),
                    "The current file will be removed.",
                )
                .build(),
        ),
        OutputConflictOption::new(
            OutputConflictChoice::Rename,
            format!(
                "{} Choose a different output",
                format_icon_colored(NerdFont::Edit, colors::SAPPHIRE)
            ),
            PreviewBuilder::new()
                .header(NerdFont::Edit, "Choose a new output")
                .text("Pick a different file name or output folder.")
                .blank()
                .field("Current", &path_display)
                .build(),
        ),
        OutputConflictOption::new(
            OutputConflictChoice::Cancel,
            format!(
                "{} Cancel render",
                format_icon_colored(NerdFont::Cross, colors::RED)
            ),
            PreviewBuilder::new()
                .header(NerdFont::Cross, "Cancel")
                .text("Return to the project menu without rendering.")
                .blank()
                .field("Output", &path_display)
                .build(),
        ),
    ];

    let selection = FzfWrapper::builder()
        .header(Header::default(&format!(
            "Output already exists:\n{}",
            path_display
        )))
        .prompt("Select")
        .args(fzf_mocha_args())
        .responsive_layout()
        .select(options)?;

    match selection {
        FzfResult::Selected(option) => Ok(Some(option.choice)),
        _ => Ok(None),
    }
}

fn resolve_default_source_path(markdown_path: &Path, markdown_dir: &Path) -> Result<PathBuf> {
    let contents = fs::read_to_string(markdown_path)?;
    let document = parse_video_document(&contents, markdown_path)?;
    let sources = &document.metadata.sources;

    if sources.is_empty() {
        bail!("No video sources configured. Add `sources` in front matter before rendering.");
    }

    let default_id = document
        .metadata
        .default_source
        .as_ref()
        .or_else(|| sources.first().map(|source| &source.id))
        .ok_or_else(|| anyhow!("No video sources available"))?;

    let source = sources
        .iter()
        .find(|source| &source.id == default_id)
        .ok_or_else(|| anyhow!("Default source `{}` not found", default_id))?;

    let source_path = if source.source.is_absolute() {
        source.source.clone()
    } else {
        markdown_dir.join(&source.source)
    };

    Ok(source_path)
}

fn resolve_render_output_path(
    out_file: Option<&PathBuf>,
    markdown_dir: &Path,
    default_source: &Path,
    render_mode: render::RenderMode,
) -> Result<PathBuf> {
    if let Some(provided) = out_file {
        return Ok(if provided.is_absolute() {
            provided.clone()
        } else {
            markdown_dir.join(provided)
        });
    }

    let mut output = default_source.to_path_buf();
    let stem = default_source
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| {
            anyhow!(
                "Video path {} has no valid file name",
                default_source.display()
            )
        })?;
    let suffix = render_mode.output_suffix();
    output.set_file_name(format!("{stem}{suffix}.mp4"));
    Ok(output)
}

fn format_render_duration(duration: std::time::Duration) -> String {
    let total_secs = duration.as_secs();
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let secs = total_secs % 60;

    if hours > 0 {
        format!("{hours}h {minutes}m {secs}s")
    } else if minutes > 0 {
        format!("{minutes}m {secs}s")
    } else {
        format!("{secs}s")
    }
}

#[derive(Debug, Clone)]
enum PostRenderAction {
    OpenVideo,
    OpenDirectory,
    Done,
}

impl std::fmt::Display for PostRenderAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PostRenderAction::OpenVideo => write!(f, "!__open_video__"),
            PostRenderAction::OpenDirectory => write!(f, "!__open_directory__"),
            PostRenderAction::Done => write!(f, "!__done__"),
        }
    }
}

impl FzfSelectable for PostRenderAction {
    fn fzf_display_text(&self) -> String {
        match self {
            PostRenderAction::OpenVideo => format!(
                "{} Open Video",
                format_icon_colored(NerdFont::Play, colors::GREEN)
            ),
            PostRenderAction::OpenDirectory => format!(
                "{} Open Directory",
                format_icon_colored(NerdFont::FolderOpen, colors::PEACH)
            ),
            PostRenderAction::Done => format!("{} Back", format_back_icon()),
        }
    }

    fn fzf_key(&self) -> String {
        self.to_string()
    }

    fn fzf_preview(&self) -> FzfPreview {
        match self {
            PostRenderAction::OpenVideo => PreviewBuilder::new()
                .header(NerdFont::Play, "Open Video")
                .text("Open the rendered video with the default player.")
                .build(),
            PostRenderAction::OpenDirectory => PreviewBuilder::new()
                .header(NerdFont::FolderOpen, "Open Directory")
                .text("Open the output directory in the file manager.")
                .build(),
            PostRenderAction::Done => PreviewBuilder::new()
                .header(NerdFont::Cross, "Back")
                .text("Return to the project menu.")
                .build(),
        }
    }
}

fn show_post_render_menu(output_path: &Path, elapsed: Option<std::time::Duration>) -> Result<()> {
    let path_display = output_path.display().to_string();

    let header_text = if let Some(elapsed) = elapsed {
        let duration_str = format_render_duration(elapsed);
        format!("Render complete in {duration_str}\n{path_display}")
    } else {
        path_display.clone()
    };

    let entries = vec![
        PostRenderAction::OpenVideo,
        PostRenderAction::OpenDirectory,
        PostRenderAction::Done,
    ];

    let result = FzfWrapper::builder()
        .header(Header::default(&header_text))
        .prompt("Select")
        .args(fzf_mocha_args())
        .responsive_layout()
        .select(entries)?;

    if let FzfResult::Selected(action) = result {
        match action {
            PostRenderAction::OpenVideo => {
                Command::new("xdg-open")
                    .arg(&path_display)
                    .spawn()
                    .context("Failed to open video")?;
            }
            PostRenderAction::OpenDirectory => {
                let parent_dir = output_path
                    .parent()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|| ".".to_string());
                Command::new("xdg-open")
                    .arg(&parent_dir)
                    .spawn()
                    .context("Failed to open directory")?;
            }
            PostRenderAction::Done => {}
        }
    }

    Ok(())
}

fn show_rendered_video_menu(output_path: &Path) -> Result<()> {
    show_post_render_menu(output_path, None)
}

fn rendered_entry_for_project(markdown_path: &Path) -> Option<ProjectMenuEntry> {
    let markdown_dir = markdown_path.parent().unwrap_or_else(|| Path::new("."));
    let default_source = resolve_default_source_path(markdown_path, markdown_dir).ok()?;
    let output_path = resolve_render_output_path(
        None,
        markdown_dir,
        &default_source,
        render::RenderMode::Standard,
    )
    .ok()?;

    if !output_path.exists() {
        return None;
    }

    let is_current = is_render_current(markdown_path, &output_path);

    Some(ProjectMenuEntry::OpenRendered {
        path: output_path,
        is_current,
    })
}

fn is_render_current(markdown_path: &Path, output_path: &Path) -> bool {
    let Ok(md_meta) = fs::metadata(markdown_path) else {
        return false;
    };
    let Ok(out_meta) = fs::metadata(output_path) else {
        return false;
    };
    let Ok(md_modified) = md_meta.modified() else {
        return false;
    };
    let Ok(out_modified) = out_meta.modified() else {
        return false;
    };
    out_modified >= md_modified
}

fn run_edit_for_project(markdown_path: &Path) -> Result<()> {
    use std::env;

    // Get editor from environment or fall back to nvim
    let editor = env::var("EDITOR").unwrap_or_else(|_| "nvim".to_string());

    FzfWrapper::message(&format!(
        "Opening {} in {}...",
        markdown_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("file"),
        editor
    ))?;

    let status = Command::new(&editor)
        .arg(markdown_path)
        .status()
        .with_context(|| format!("Failed to open editor '{}'", editor))?;

    if !status.success() {
        bail!("Editor exited with non-zero status: {:?}", status);
    }

    Ok(())
}
