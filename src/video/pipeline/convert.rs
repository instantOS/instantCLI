use anyhow::{Context, Result, anyhow};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::ui::prelude::{Level, emit};

use super::speech_review::review_audio;
use super::transcribe::handle_transcribe;
use crate::video::cli::{AppendArgs, ConvertArgs, TranscribeArgs};
use crate::video::config::{VideoCachePaths, VideoDirectories};
use crate::video::document::frontmatter::split_frontmatter;
use crate::video::document::markdown::{build_markdown, format_cue_line};
use crate::video::document::{VideoMetadata, VideoSource, parse_video_document};
use crate::video::support::speech_activity::SpeechActivity;
use crate::video::support::transcript::{
    TranscriptCue, is_transcriber_control_text, parse_whisper_json,
};
use crate::video::support::utils::{canonicalize_existing, compute_file_hash, copy_overwriting};

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
    let cache_paths = directories.cache_paths(&video_hash, args.language);
    cache_paths.ensure_directories()?;

    let output_path = determine_output_path(args.out_file.clone(), &video_path)?;

    if output_path.exists() && !args.force {
        anyhow::bail!(
            "Markdown file already exists at {}. Use --force to overwrite.",
            output_path.display()
        );
    }

    // Step 1: Ensure we have a transcript
    let transcript_path =
        ensure_transcript(&video_path, &cache_paths, args.transcript.as_ref(), &args).await?;

    // Step 2: Generate markdown output
    let speech_activity = review_audio(
        &video_path,
        cache_paths.transcript_dir(),
        &directories.models_dir(),
    );
    generate_markdown_output(
        &video_path,
        &video_hash,
        &transcript_path,
        &output_path,
        speech_activity.as_ref(),
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
        speech_activity,
    } = prepare_video_and_transcript(&args).await?;

    let source_id = add_new_source_to_metadata(
        &mut metadata,
        &video_path,
        &markdown_dir,
        &video_hash,
        &transcript_path,
    )?;

    let new_contents = build_appended_markdown(
        &markdown_contents,
        &metadata,
        &cues,
        &source_id,
        speech_activity.as_ref(),
    )?;
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
    let cache_paths = directories.cache_paths(&video_hash, args.language);
    cache_paths.ensure_directories()?;

    let transcript_path = ensure_transcript(
        &video_path,
        &cache_paths,
        args.transcript.as_ref(),
        &ConvertArgs {
            video: args.video.clone(),
            transcript: args.transcript.clone(),
            out_file: None,
            force: args.force,
            language: args.language,
        },
    )
    .await?;

    let cues = load_transcript_cues(&transcript_path)?;
    let speech_activity = review_audio(
        &video_path,
        cache_paths.transcript_dir(),
        &directories.models_dir(),
    );

    Ok(VideoAppendInput {
        video_path,
        video_hash,
        transcript_path,
        cues,
        speech_activity,
    })
}

struct VideoAppendInput {
    video_path: PathBuf,
    video_hash: String,
    transcript_path: PathBuf,
    cues: Vec<TranscriptCue>,
    speech_activity: Option<SpeechActivity>,
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
    copy_overwriting(transcript_path, &subtitle_output_path, "transcript")?;

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
    speech_activity: Option<&SpeechActivity>,
) -> Result<String> {
    let (_front_matter, body, _) = split_frontmatter(markdown_contents)?;
    let existing_body = body.trim_end_matches(&['\r', '\n'][..]);

    let appended_text = build_source_markdown(cues, source_id, speech_activity);
    let mut new_body = String::new();
    if !existing_body.is_empty() {
        new_body.push_str(existing_body);
        new_body.push_str("\n\n");
    }
    new_body.push_str(&appended_text);
    new_body.push('\n');

    let front = metadata.render_frontmatter();
    Ok(format!("{front}\n{new_body}"))
}

/// Ensures a transcript exists for the video, generating one if needed.
/// Returns the path to the transcript file.
async fn ensure_transcript(
    video_path: &Path,
    cache_paths: &VideoCachePaths,
    provided_transcript: Option<&PathBuf>,
    args: &ConvertArgs,
) -> Result<PathBuf> {
    let cached_transcript_path = cache_paths.transcript_cache_path().to_path_buf();

    // If user provided a transcript, use it
    if let Some(provided) = provided_transcript {
        let provided_path = canonicalize_existing(provided)?;
        copy_overwriting(&provided_path, &cached_transcript_path, "transcript")?;
        return Ok(cached_transcript_path);
    }

    // If transcript already cached, use it
    if cached_transcript_path.exists() {
        return Ok(cached_transcript_path);
    }

    // Generate transcript on the RAW source audio: ASR models are robust to
    // noise, so the (final-output) enhancer never feeds the transcription.
    let audio_source = video_path;

    emit(
        Level::Info,
        "video.convert.transcribe",
        "Transcribing audio (this may take a while)...",
        None,
    );

    handle_transcribe(TranscribeArgs {
        video: audio_source.to_path_buf(),
        compute_type: "int8".to_string(),
        device: "cpu".to_string(),
        model: None,
        vad_method: "silero".to_string(),
        force: false,
        backend: crate::video::cli::TranscribeBackend::Auto,
        granite_model: None,
        language: args.language,
    })?;

    if !cached_transcript_path.exists() {
        anyhow::bail!(
            "Transcript not found at {} even after attempting transcription.",
            cached_transcript_path.display()
        );
    }

    Ok(cached_transcript_path)
}

/// Generates the markdown output file from a transcript.
fn generate_markdown_output(
    video_path: &Path,
    video_hash: &str,
    transcript_path: &Path,
    output_path: &Path,
    speech_activity: Option<&SpeechActivity>,
) -> Result<()> {
    let markdown_dir = output_path.parent().unwrap_or_else(|| Path::new("."));
    let subtitle_dir = markdown_dir.join("insvideodata");
    let subtitle_output_path = subtitle_dir.join(format!("{video_hash}.json"));
    let relative_subtitle_path = Path::new("./insvideodata").join(format!("{video_hash}.json"));

    copy_overwriting(transcript_path, &subtitle_output_path, "transcript")?;

    let transcript_contents = fs::read_to_string(transcript_path)
        .with_context(|| format!("Failed to read transcript at {}", transcript_path.display()))?;

    let mut cues = parse_whisper_json(&transcript_contents)?;
    cues.retain(|cue| !is_transcriber_control_text(&cue.text));
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

    let metadata = VideoMetadata {
        sources: vec![VideoSource {
            id: "a".to_string(),
            name: Some(video_name),
            source: relative_video_path,
            transcript: relative_subtitle_path,
            audio: PathBuf::new(),
            hash: Some(video_hash.to_string()),
        }],
        default_source: Some("a".to_string()),
    };

    let markdown = build_markdown(&cues, &metadata, speech_activity);

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
    let mut cues = parse_whisper_json(&transcript_contents)?;
    cues.retain(|cue| !is_transcriber_control_text(&cue.text));
    Ok(cues)
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
    speech_activity: Option<&SpeechActivity>,
) -> String {
    cues.iter()
        .map(|cue| format_cue_line(source_id, cue, speech_activity))
        .collect::<Vec<_>>()
        .join("\n")
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::video::document::{DocumentBlock, SegmentKind};

    #[test]
    fn convert_and_append_annotate_without_changing_transcripts_or_authored_text() -> Result<()> {
        let temporary = tempfile::tempdir()?;
        let video = temporary.path().join("source.mp4");
        let transcript = temporary.path().join("source.json");
        let markdown_path = temporary.path().join("source.video.md");
        let json = r#"{"segments":[
            {"start":0,"end":1,"text":"garbled noise"},
            {"start":2,"end":3,"text":"real speech"}
        ]}"#;
        fs::write(&transcript, json)?;
        let activity = SpeechActivity::from_json(
            r#"{"version":1,"duration_seconds":3,"speech":[{"start":2,"end":3}]}"#,
        )?;
        generate_markdown_output(&video, "hash", &transcript, &markdown_path, Some(&activity))?;
        assert_eq!(fs::read_to_string(&transcript)?, json);
        assert_eq!(
            fs::read_to_string(temporary.path().join("insvideodata/hash.json"))?,
            json
        );
        let markdown = fs::read_to_string(&markdown_path)?;
        assert_eq!(markdown.matches("<!-- REVIEW:").count(), 1);
        assert!(markdown.contains("`a#0@00:00.0-00:01.0` garbled noise"));

        let mut metadata = parse_video_document(&markdown, &markdown_path)?.metadata;
        metadata.sources.push(VideoSource {
            id: "b".into(),
            name: None,
            source: "second.mp4".into(),
            transcript: "second.json".into(),
            audio: PathBuf::new(),
            hash: None,
        });
        let cues = load_transcript_cues(&transcript)?;
        let authored_body =
            "`a#1@00:02.0-00:03.0` manually corrected speech\n\n<!-- my editing note -->\n";
        let existing = format!("{}\n{authored_body}", metadata.render_frontmatter());
        let appended = build_appended_markdown(&existing, &metadata, &cues, "b", Some(&activity))?;
        assert!(appended.contains(authored_body.trim_end()));
        assert_eq!(appended.matches("<!-- REVIEW:").count(), 1);
        assert!(appended.contains("`b#0@00:00.0-00:01.0` garbled noise"));
        let parsed = parse_video_document(&appended, &markdown_path)?;
        let dialogue = parsed
            .blocks
            .iter()
            .filter_map(|block| match block {
                DocumentBlock::Segment(segment) if segment.kind == SegmentKind::Dialogue => {
                    Some(segment)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(dialogue.len(), 3);
        assert_eq!(dialogue[0].text, "manually corrected speech");
        assert_eq!(dialogue[1].source_id, "b");
        assert_eq!(dialogue[1].cue_id, Some(0));
        assert_eq!(dialogue[1].text, "garbled noise");
        assert_eq!(dialogue[2].cue_id, Some(1));
        assert_eq!(dialogue[2].text, "real speech");
        Ok(())
    }
}
