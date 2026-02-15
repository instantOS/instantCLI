use anyhow::{Context, Result, anyhow};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::ui::prelude::{Level, emit};

use super::transcribe::handle_transcribe;
use crate::video::audio::{PreprocessorType, create_preprocessor, parse_preprocessor_type};
use crate::video::cli::{AppendArgs, ConvertArgs, TranscribeArgs};
use crate::video::config::{VideoConfig, VideoDirectories, VideoProjectPaths};
use crate::video::document::frontmatter::split_frontmatter;
use crate::video::document::markdown::{MarkdownMetadata, MarkdownSource, build_markdown};
use crate::video::document::{VideoMetadata, VideoSource, parse_video_document};
use crate::video::support::transcript::{TranscriptCue, parse_whisper_json};
use crate::video::support::utils::{canonicalize_existing, compute_file_hash};

pub async fn handle_convert(args: ConvertArgs) -> Result<()> {
    emit(
        Level::Info,
        "video.convert.start",
        &format!("Analyzing video {}...", args.video.display()),
        None,
    );
    let video_path = canonicalize_existing(&args.video)?;
    let video_hash = compute_file_hash(&video_path)?;

    let directories = VideoDirectories::new()?;
    let project_paths = directories.project_paths(&video_hash);
    project_paths.ensure_directories()?;

    let output_path = determine_output_path(args.out_file.clone(), &video_path)?;

    if output_path.exists() && !args.force {
        anyhow::bail!(
            "Markdown file already exists at {}. Use --force to overwrite.",
            output_path.display()
        );
    }

    // Step 1: Ensure we have a transcript
    let transcript_path = ensure_transcript(
        &video_path,
        &directories,
        &project_paths,
        args.transcript.as_ref(),
        &args,
    )
    .await?;

    // Step 2: Generate markdown output
    generate_markdown_output(
        &video_path,
        &video_hash,
        &transcript_path,
        &output_path,
    )?;

    emit(
        Level::Success,
        "video.convert.success",
        &format!("Generated markdown at {}", output_path.display()),
        None,
    );

    Ok(())
}

pub async fn handle_append(args: AppendArgs) -> Result<()> {
    emit(
        Level::Info,
        "video.append.start",
        &format!(
            "Appending {} to {}...",
            args.video.display(),
            args.markdown.display()
        ),
        None,
    );

    let MarkdownAppendContext {
        markdown_path,
        markdown_dir,
        markdown_contents,
        mut metadata,
    } = load_markdown_document(&args.markdown)?;

    let VideoAppendInput {
        video_path,
        video_hash,
        transcript_path,
        cues,
    } = prepare_video_and_transcript(&args).await?;

    let source_id = add_new_source_to_metadata(
        &mut metadata,
        &video_path,
        &markdown_dir,
        &video_hash,
        &transcript_path,
    )?;

    let new_contents = build_appended_markdown(&markdown_contents, &metadata, &cues, &source_id)?;
    fs::write(&markdown_path, new_contents.as_bytes()).with_context(|| {
        format!(
            "Failed to write markdown file to {}",
            markdown_path.display()
        )
    })?;

    emit(
        Level::Success,
        "video.append.success",
        &format!("Appended recording to {}", markdown_path.display()),
        None,
    );

    Ok(())
}

struct MarkdownAppendContext {
    markdown_path: PathBuf,
    markdown_dir: PathBuf,
    markdown_contents: String,
    metadata: VideoMetadata,
}

fn load_markdown_document(markdown_path: &Path) -> Result<MarkdownAppendContext> {
    let markdown_path = canonicalize_existing(markdown_path)?;
    let markdown_dir = markdown_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    let markdown_contents = fs::read_to_string(&markdown_path)
        .with_context(|| format!("Failed to read markdown file {}", markdown_path.display()))?;
    let document = parse_video_document(&markdown_contents, &markdown_path)?;

    Ok(MarkdownAppendContext {
        markdown_path,
        markdown_dir,
        markdown_contents,
        metadata: document.metadata,
    })
}

async fn prepare_video_and_transcript(args: &AppendArgs) -> Result<VideoAppendInput> {
    let video_path = canonicalize_existing(&args.video)?;
    let video_hash = compute_file_hash(&video_path)?;

    let directories = VideoDirectories::new()?;
    let project_paths = directories.project_paths(&video_hash);
    project_paths.ensure_directories()?;

    let transcript_path = ensure_transcript(
        &video_path,
        &directories,
        &project_paths,
        args.transcript.as_ref(),
        &ConvertArgs {
            video: args.video.clone(),
            transcript: args.transcript.clone(),
            out_file: None,
            force: args.force,
            no_preprocess: args.no_preprocess,
            preprocessor: args.preprocessor.clone(),
        },
    )
    .await?;

    let cues = load_transcript_cues(&transcript_path)?;

    Ok(VideoAppendInput {
        video_path,
        video_hash,
        transcript_path,
        cues,
    })
}

struct VideoAppendInput {
    video_path: PathBuf,
    video_hash: String,
    transcript_path: PathBuf,
    cues: Vec<TranscriptCue>,
}

fn add_new_source_to_metadata(
    metadata: &mut VideoMetadata,
    video_path: &Path,
    markdown_dir: &Path,
    video_hash: &str,
    transcript_path: &Path,
) -> Result<String> {
    let video_name = video_path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .context("Video file name is not valid UTF-8")?;

    let relative_video_path = pathdiff::diff_paths(video_path, markdown_dir).ok_or_else(|| {
        anyhow!(
            "Failed to compute relative path from {} to {}",
            markdown_dir.display(),
            video_path.display()
        )
    })?;

    let subtitle_dir = markdown_dir.join("insvideodata");
    let subtitle_output_path = subtitle_dir.join(format!("{video_hash}.json"));
    let relative_subtitle_path = Path::new("./insvideodata").join(format!("{video_hash}.json"));
    copy_transcript(transcript_path, &subtitle_output_path)?;

    let source_id = next_source_id(metadata)?;

    metadata.sources.push(VideoSource {
        id: source_id.clone(),
        name: Some(video_name),
        source: relative_video_path,
        transcript: relative_subtitle_path,
        audio: PathBuf::new(),
        hash: Some(video_hash.to_string()),
    });

    if metadata.default_source.is_none() {
        metadata.default_source = Some(source_id.clone());
    }

    Ok(source_id)
}

fn build_appended_markdown(
    markdown_contents: &str,
    metadata: &VideoMetadata,
    cues: &[TranscriptCue],
    source_id: &str,
) -> Result<String> {
    let (_front_matter, body, _) = split_frontmatter(markdown_contents)?;
    let existing_body = body.trim_end_matches(&['\r', '\n'][..]);

    let appended_text = build_source_markdown(cues, source_id);
    let mut new_body = String::new();
    if !existing_body.is_empty() {
        new_body.push_str(existing_body);
        new_body.push_str("\n\n");
    }
    new_body.push_str(&appended_text);
    new_body.push('\n');

    let front = build_front_matter_from_metadata(metadata);
    Ok(format!("{front}\n{new_body}"))
}

/// Ensures a transcript exists for the video, generating one if needed.
/// Returns the path to the transcript file.
async fn ensure_transcript(
    video_path: &Path,
    directories: &VideoDirectories,
    project_paths: &VideoProjectPaths,
    provided_transcript: Option<&PathBuf>,
    args: &ConvertArgs,
) -> Result<PathBuf> {
    let cached_transcript_path = project_paths.transcript_cache_path().to_path_buf();

    // If user provided a transcript, use it
    if let Some(provided) = provided_transcript {
        let provided_path = canonicalize_existing(provided)?;
        copy_transcript(&provided_path, &cached_transcript_path)?;
        return Ok(cached_transcript_path);
    }

    // If transcript already cached, use it
    if cached_transcript_path.exists() {
        return Ok(cached_transcript_path);
    }

    // Generate transcript
    let audio_source = get_audio_source(video_path, args).await?;

    emit(
        Level::Info,
        "video.convert.transcribe",
        "Transcribing audio (this may take a while)...",
        None,
    );

    handle_transcribe(TranscribeArgs {
        video: audio_source.clone(),
        compute_type: "int8".to_string(),
        device: "cpu".to_string(),
        model: None,
        vad_method: "silero".to_string(),
        force: false,
    })?;

    // If we transcribed processed audio, relocate transcript to video's cache
    relocate_transcript_if_needed(
        video_path,
        &audio_source,
        directories,
        &cached_transcript_path,
    )?;

    if !cached_transcript_path.exists() {
        anyhow::bail!(
            "Transcript not found at {} even after attempting transcription.",
            cached_transcript_path.display()
        );
    }

    Ok(cached_transcript_path)
}

/// Gets the audio source for transcription using the configured preprocessor.
async fn get_audio_source(video_path: &Path, args: &ConvertArgs) -> Result<PathBuf> {
    // Skip preprocessing entirely if requested
    if args.no_preprocess {
        return Ok(video_path.to_path_buf());
    }

    let config = VideoConfig::load()?;

    // Determine preprocessor type: CLI flag > config
    let preprocessor_type = match &args.preprocessor {
        Some(s) => parse_preprocessor_type(s)?,
        None => config.preprocessor.clone(),
    };

    // Skip if preprocessor is None
    if preprocessor_type == PreprocessorType::None {
        return Ok(video_path.to_path_buf());
    }

    let preprocessor = create_preprocessor(&preprocessor_type, &config);

    if !preprocessor.is_available() {
        emit(
            Level::Warn,
            "video.convert.preprocessor_unavailable",
            &format!(
                "Preprocessor '{}' is not available. Falling back to original video.",
                preprocessor.name()
            ),
            None,
        );
        return Ok(video_path.to_path_buf());
    }

    match preprocessor.process(video_path, args.force).await {
        Ok(result) => Ok(result.output_path),
        Err(e) => {
            emit(
                Level::Warn,
                "video.convert.preprocess_failed",
                &format!(
                    "Audio preprocessing failed: {}. Falling back to original video.",
                    e
                ),
                None,
            );
            Ok(video_path.to_path_buf())
        }
    }
}

/// Relocates transcript from audio's cache to video's cache if needed.
fn relocate_transcript_if_needed(
    video_path: &Path,
    audio_source: &Path,
    directories: &VideoDirectories,
    cached_transcript_path: &Path,
) -> Result<()> {
    if audio_source == video_path {
        return Ok(());
    }

    let audio_hash = compute_file_hash(audio_source)?;
    let audio_project_paths = directories.project_paths(&audio_hash);
    let generated_transcript = audio_project_paths.transcript_cache_path();

    if generated_transcript.exists() {
        emit(
            Level::Debug,
            "video.convert.relocate",
            &format!(
                "Moving transcript from {} to {}",
                generated_transcript.display(),
                cached_transcript_path.display()
            ),
            None,
        );
        copy_transcript(generated_transcript, cached_transcript_path)?;
    }

    Ok(())
}

/// Generates the markdown output file from a transcript.
fn generate_markdown_output(
    video_path: &Path,
    video_hash: &str,
    transcript_path: &Path,
    output_path: &Path,
) -> Result<()> {
    let markdown_dir = output_path.parent().unwrap_or_else(|| Path::new("."));
    let subtitle_dir = markdown_dir.join("insvideodata");
    let subtitle_output_path = subtitle_dir.join(format!("{video_hash}.json"));
    let relative_subtitle_path = Path::new("./insvideodata").join(format!("{video_hash}.json"));

    copy_transcript(transcript_path, &subtitle_output_path)?;

    let transcript_contents = fs::read_to_string(transcript_path)
        .with_context(|| format!("Failed to read transcript at {}", transcript_path.display()))?;

    let mut cues = parse_whisper_json(&transcript_contents)?;
    for cue in &mut cues {
        cue.source_id = "a".to_string();
    }

    let video_name = video_path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .context("Video file name is not valid UTF-8")?;

    // Compute relative path from markdown directory to video file
    let relative_video_path = pathdiff::diff_paths(video_path, markdown_dir).ok_or_else(|| {
        anyhow!(
            "Failed to compute relative path from {} to {}",
            markdown_dir.display(),
            video_path.display()
        )
    })?;

    let sources = vec![MarkdownSource {
        id: "a",
        name: Some(video_name.as_str()),
        video_hash,
        video_source: &relative_video_path,
        transcript_source: &relative_subtitle_path,
    }];
    let metadata = MarkdownMetadata {
        sources: &sources,
        default_source: "a",
    };

    let markdown = build_markdown(&cues, &metadata);

    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create output directory {}", parent.display()))?;
    }

    fs::write(output_path, markdown.as_bytes())
        .with_context(|| format!("Failed to write markdown file to {}", output_path.display()))?;

    emit(
        Level::Info,
        "video.convert.subtitles",
        &format!("Stored subtitles at {}", subtitle_output_path.display()),
        None,
    );

    Ok(())
}

fn load_transcript_cues(path: &Path) -> Result<Vec<TranscriptCue>> {
    let transcript_contents = fs::read_to_string(path)
        .with_context(|| format!("Failed to read transcript at {}", path.display()))?;
    parse_whisper_json(&transcript_contents)
}

fn next_source_id(metadata: &crate::video::document::VideoMetadata) -> Result<String> {
    let mut used = HashSet::new();
    for source in &metadata.sources {
        used.insert(source.id.clone());
    }

    for ch in 'a'..='z' {
        let candidate = ch.to_string();
        if !used.contains(&candidate) {
            return Ok(candidate);
        }
    }

    let mut idx = 1;
    loop {
        let candidate = format!("s{idx}");
        if !used.contains(&candidate) {
            return Ok(candidate);
        }
        idx += 1;
    }
}

fn build_source_markdown(
    cues: &[crate::video::support::transcript::TranscriptCue],
    source_id: &str,
) -> String {
    let mut lines = Vec::with_capacity(cues.len());
    for cue in cues {
        lines.push(format!(
            "`{}@{}-{}` {}",
            source_id,
            format_timestamp(cue.start),
            format_timestamp(cue.end),
            cue.text.trim()
        ));
    }
    lines.join("\n")
}

fn build_front_matter_from_metadata(metadata: &crate::video::document::VideoMetadata) -> String {
    let timestamp = chrono::Utc::now().to_rfc3339();
    let default_source = metadata
        .default_source
        .as_ref()
        .cloned()
        .unwrap_or_else(|| "a".to_string());
    let mut source_lines = Vec::new();
    for source in &metadata.sources {
        let source_id = yaml_quote(&source.id);
        let video_source = yaml_quote(&source.source.to_string_lossy());
        let transcript_source = yaml_quote(&source.transcript.to_string_lossy());
        let video_hash = yaml_quote(source.hash.as_deref().unwrap_or(""));
        let name = yaml_quote(source.name.as_deref().unwrap_or(""));
        source_lines.push(format!(
            "- id: {source_id}\n  hash: {video_hash}\n  name: {name}\n  source: {video_source}\n  transcript: {transcript_source}"
        ));
    }
    if source_lines.is_empty() {
        return format!(
            "---\ndefault_source: {default_source}\nsources: []\ngenerated_at: '{timestamp}'\n---",
            default_source = yaml_quote(&default_source),
        );
    }

    let sources_block = source_lines.join("\n");
    format!(
        "---\ndefault_source: {default_source}\nsources:\n{sources}\ngenerated_at: '{timestamp}'\n---",
        default_source = yaml_quote(&default_source),
        sources = sources_block,
    )
}

fn format_timestamp(duration: std::time::Duration) -> String {
    crate::video::document::markdown::format_timestamp(duration)
}

fn copy_transcript(src: &Path, dest: &Path) -> Result<()> {
    if src == dest {
        return Ok(());
    }
    if dest.exists() {
        return Ok(());
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!("Failed to create transcript directory {}", parent.display())
        })?;
    }
    fs::copy(src, dest).with_context(|| {
        format!(
            "Failed to copy transcript from {} to {}",
            src.display(),
            dest.display()
        )
    })?;
    Ok(())
}

fn determine_output_path(output: Option<PathBuf>, video_path: &Path) -> Result<PathBuf> {
    match output {
        Some(path) => Ok(path),
        None => {
            // Default to <videoname>.md next to the video file
            let video_stem = video_path
                .file_stem()
                .context("Video file has no stem")?
                .to_string_lossy();
            let mut default_output = video_path.to_path_buf();
            default_output.set_file_name(format!("{}.video.md", video_stem));
            Ok(default_output)
        }
    }
}

fn yaml_quote(value: &str) -> String {
    if value.is_empty() {
        "''".to_string()
    } else {
        format!("'{}'", value.replace('\'', "''"))
    }
}

