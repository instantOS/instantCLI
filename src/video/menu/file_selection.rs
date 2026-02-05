use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};

use crate::common::paths;
use crate::menu_utils::{FilePickerScope, FzfPreview, PathInputBuilder};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;
use crate::video::document::{frontmatter::split_frontmatter, parse_video_document};
use crate::video::support::ffmpeg::probe_media_metadata;

use super::types::{AUDIO_EXTENSIONS, VIDEO_EXTENSIONS};

/// Generate a rich preview for a video/audio file using ffprobe metadata
fn video_file_preview(path: &Path) -> FzfPreview {
    let metadata = probe_media_metadata(path);
    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("Unknown");
    let icon = if metadata.is_audio_only() {
        NerdFont::Music
    } else {
        NerdFont::Video
    };

    let mut builder = PreviewBuilder::new().header(icon, file_name);

    if let Some(duration) = metadata.duration_display() {
        builder = builder.field("Duration", &duration);
    }
    if !metadata.is_audio_only() {
        if let Some(resolution) = metadata.resolution_display() {
            builder = builder.field("Resolution", &resolution);
        }
        if let Some(codec) = &metadata.video_codec {
            builder = builder.field("Video Codec", codec);
        }
        if let Some(fps) = metadata.framerate_display() {
            builder = builder.field("Frame Rate", &fps);
        }
    }
    if let Some(codec) = &metadata.audio_codec {
        builder = builder.field("Audio Codec", codec);
    }
    if let Some(channels) = metadata.audio_channels {
        let ch = match channels {
            1 => "Mono".into(),
            2 => "Stereo".into(),
            n => format!("{n} channels"),
        };
        builder = builder.field("Channels", &ch);
    }
    if let Some(bitrate) = metadata.bitrate_display() {
        builder = builder.field("Bitrate", &bitrate);
    }

    builder.blank().subtext(&path.to_string_lossy()).build()
}

pub fn discover_video_file_suggestions() -> Result<Vec<PathBuf>> {
    let entries = match fs::read_dir(".") {
        Ok(entries) => entries,
        Err(_) => return Ok(Vec::new()),
    };

    let mut suggestions = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        if is_video_or_audio_file(&path) {
            let canonical = path.canonicalize().unwrap_or(path);
            if !suggestions.contains(&canonical) {
                suggestions.push(canonical);
            }
        }
    }

    suggestions.sort();
    if suggestions.len() > 50 {
        suggestions.truncate(50);
    }
    Ok(suggestions)
}

pub fn is_video_or_audio_file(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase());

    match ext {
        Some(e) => VIDEO_EXTENSIONS.contains(&e.as_str()) || AUDIO_EXTENSIONS.contains(&e.as_str()),
        None => false,
    }
}

pub fn compute_default_output_path(video_path: &Path) -> PathBuf {
    let parent = video_path.parent().unwrap_or(Path::new("."));
    let stem = video_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("video");
    parent.join(format!("{stem}.video.md"))
}

pub fn select_video_file_with_suggestions(
    title: &str,
    suggestions: Vec<PathBuf>,
) -> Result<Option<PathBuf>> {
    let header = format!("{} {title}", char::from(NerdFont::Video));
    let manual_prompt = format!("{} Enter file path:", char::from(NerdFont::Edit));
    let picker_hint = format!(
        "{} Select a video or audio file",
        char::from(NerdFont::Info)
    );
    let start_dir = paths::videos_dir().ok();

    select_video_path_with_picker(
        header,
        manual_prompt,
        picker_hint,
        FilePickerScope::Files,
        start_dir,
        suggestions,
    )
}

pub fn select_video_file(title: &str) -> Result<Option<PathBuf>> {
    let header = format!("{} {title}", char::from(NerdFont::Video));
    let manual_prompt = format!("{} Enter file path:", char::from(NerdFont::Edit));
    let picker_hint = format!(
        "{} Select a video or audio file",
        char::from(NerdFont::Info)
    );
    let start_dir = paths::videos_dir().ok();

    select_video_path_with_picker(
        header,
        manual_prompt,
        picker_hint,
        FilePickerScope::Files,
        start_dir,
        Vec::new(),
    )
}

pub fn select_transcript_file() -> Result<Option<PathBuf>> {
    let header = format!("{} Select transcript file", char::from(NerdFont::FileText));
    let manual_prompt = format!("{} Enter transcript path:", char::from(NerdFont::Edit));
    let picker_hint = format!(
        "{} Select a transcript file (WhisperX JSON)",
        char::from(NerdFont::Info)
    );

    select_path_with_picker(
        header,
        manual_prompt,
        picker_hint,
        FilePickerScope::Files,
        None,
        Vec::new(),
    )
}

pub fn select_markdown_file(title: &str, suggestions: Vec<PathBuf>) -> Result<Option<PathBuf>> {
    let header = format!("{} {title}", char::from(NerdFont::FileText));
    let manual_prompt = format!("{} Enter markdown path:", char::from(NerdFont::Edit));
    let picker_hint = format!("{} Select a markdown file", char::from(NerdFont::Info));

    select_path_with_picker(
        header,
        manual_prompt,
        picker_hint,
        FilePickerScope::Files,
        None,
        suggestions,
    )
}

pub fn select_output_path(
    default_name: &str,
    start_dir: Option<PathBuf>,
) -> Result<Option<PathBuf>> {
    let header = format!("{} Choose output path", char::from(NerdFont::Folder));
    let manual_prompt = format!("{} Enter output path:", char::from(NerdFont::Edit));
    let picker_hint = format!(
        "{} Pick a file or folder (folders use default name)",
        char::from(NerdFont::Info)
    );

    let mut builder = PathInputBuilder::new()
        .header(header)
        .manual_prompt(manual_prompt)
        .scope(FilePickerScope::FilesAndDirectories)
        .picker_hint(picker_hint)
        .manual_option_label(format!("{} Enter a path", char::from(NerdFont::Edit)))
        .picker_option_label(format!(
            "{} Browse with picker",
            char::from(NerdFont::FolderOpen)
        ));

    if let Some(dir) = start_dir {
        builder = builder.start_dir(dir);
    }

    let selection = builder.choose()?;
    super::prompts::resolve_output_path_from_selection(selection, default_name)
}

fn select_path_with_picker(
    header: String,
    manual_prompt: String,
    picker_hint: String,
    scope: FilePickerScope,
    start_dir: Option<PathBuf>,
    suggestions: Vec<PathBuf>,
) -> Result<Option<PathBuf>> {
    let mut builder = PathInputBuilder::new()
        .header(header)
        .manual_prompt(manual_prompt)
        .scope(scope)
        .picker_hint(picker_hint)
        .manual_option_label(format!("{} Enter a path", char::from(NerdFont::Edit)))
        .picker_option_label(format!("{} Browse files", char::from(NerdFont::FolderOpen)));

    if let Some(dir) = start_dir {
        builder = builder.start_dir(dir);
    }

    if !suggestions.is_empty() {
        builder = builder.suggested_paths(suggestions);
    }

    let selection = builder.choose()?;
    selection.to_path_buf()
}

/// Like select_path_with_picker but uses ffprobe-based video previews for suggestions
fn select_video_path_with_picker(
    header: String,
    manual_prompt: String,
    picker_hint: String,
    scope: FilePickerScope,
    start_dir: Option<PathBuf>,
    suggestions: Vec<PathBuf>,
) -> Result<Option<PathBuf>> {
    let mut builder = PathInputBuilder::new()
        .header(header)
        .manual_prompt(manual_prompt)
        .scope(scope)
        .picker_hint(picker_hint)
        .manual_option_label(format!("{} Enter a path", char::from(NerdFont::Edit)))
        .picker_option_label(format!("{} Browse files", char::from(NerdFont::FolderOpen)))
        .suggestion_preview(video_file_preview);

    if let Some(dir) = start_dir {
        builder = builder.start_dir(dir);
    }

    if !suggestions.is_empty() {
        builder = builder.suggested_paths(suggestions);
    }

    let selection = builder.choose()?;
    selection.to_path_buf()
}

pub fn discover_video_markdown_suggestions() -> Result<Vec<PathBuf>> {
    let entries = match fs::read_dir(".") {
        Ok(entries) => entries,
        Err(_) => return Ok(Vec::new()),
    };

    let mut suggestions = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        if !is_markdown_file(&path) {
            continue;
        }

        if is_video_markdown_file(&path)? {
            let canonical = path.canonicalize().unwrap_or(path);
            if !suggestions.contains(&canonical) {
                suggestions.push(canonical);
            }
        }
    }

    suggestions.sort();
    Ok(suggestions)
}

fn is_markdown_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("md") | Some("markdown")
    )
}

fn is_video_markdown_file(path: &Path) -> Result<bool> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(_) => return Ok(false),
    };

    let (front_matter, _, _) = match split_frontmatter(&contents) {
        Ok(value) => value,
        Err(_) => return Ok(false),
    };

    let front: &str = match front_matter {
        Some(value) => value,
        None => return Ok(false),
    };

    if !front.contains("sources:") {
        return Ok(false);
    }

    Ok(parse_video_document(&contents, path).is_ok())
}
