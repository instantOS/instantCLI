use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};

use crate::common::{TildePath, paths};
use crate::menu_utils::{
    ChecklistResult, ConfirmResult, FilePickerScope, FzfPreview, FzfResult, FzfSelectable,
    FzfWrapper, Header, MenuCursor, PathInputBuilder, PathInputSelection,
};
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored, fzf_mocha_args};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;
use crate::video::cli::{
    CheckArgs, ConvertArgs, PreprocessArgs, RenderArgs, SetupArgs, SlideArgs, StatsArgs,
    TranscribeArgs,
};
use crate::video::{
    audio,
    document::{frontmatter::split_frontmatter, parse_video_document},
    pipeline::{check, convert, setup, stats, transcribe},
    render, slides,
};

const DEFAULT_TRANSCRIBE_COMPUTE_TYPE: &str = "int8";
const DEFAULT_TRANSCRIBE_DEVICE: &str = "cpu";
const DEFAULT_TRANSCRIBE_VAD_METHOD: &str = "silero";

#[derive(Debug, Clone)]
enum VideoMenuEntry {
    Convert,
    Transcribe,
    Render,
    Slide,
    Check,
    Stats,
    Preprocess,
    Setup,
    CloseMenu,
}

impl FzfSelectable for VideoMenuEntry {
    fn fzf_display_text(&self) -> String {
        match self {
            VideoMenuEntry::Convert => format!(
                "{} Convert to Markdown",
                format_icon_colored(NerdFont::FileText, colors::PEACH)
            ),
            VideoMenuEntry::Transcribe => format!(
                "{} Transcribe with WhisperX",
                format_icon_colored(NerdFont::Keyboard, colors::SAPPHIRE)
            ),
            VideoMenuEntry::Render => format!(
                "{} Render Edited Video",
                format_icon_colored(NerdFont::PlayCircle, colors::GREEN)
            ),
            VideoMenuEntry::Slide => format!(
                "{} Generate Slide Image",
                format_icon_colored(NerdFont::Image, colors::YELLOW)
            ),
            VideoMenuEntry::Check => format!(
                "{} Validate Markdown",
                format_icon_colored(NerdFont::CheckCircle, colors::TEAL)
            ),
            VideoMenuEntry::Stats => format!(
                "{} Show Timeline Stats",
                format_icon_colored(NerdFont::Chart, colors::BLUE)
            ),
            VideoMenuEntry::Preprocess => format!(
                "{} Preprocess Audio",
                format_icon_colored(NerdFont::Sliders, colors::LAVENDER)
            ),
            VideoMenuEntry::Setup => format!(
                "{} Video Tool Setup",
                format_icon_colored(NerdFont::Wrench, colors::PEACH)
            ),
            VideoMenuEntry::CloseMenu => format!("{} Close Menu", format_back_icon()),
        }
    }

    fn fzf_key(&self) -> String {
        match self {
            VideoMenuEntry::Convert => "!__convert__".to_string(),
            VideoMenuEntry::Transcribe => "!__transcribe__".to_string(),
            VideoMenuEntry::Render => "!__render__".to_string(),
            VideoMenuEntry::Slide => "!__slide__".to_string(),
            VideoMenuEntry::Check => "!__check__".to_string(),
            VideoMenuEntry::Stats => "!__stats__".to_string(),
            VideoMenuEntry::Preprocess => "!__preprocess__".to_string(),
            VideoMenuEntry::Setup => "!__setup__".to_string(),
            VideoMenuEntry::CloseMenu => "!__close_menu__".to_string(),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        match self {
            VideoMenuEntry::Convert => PreviewBuilder::new()
                .header(NerdFont::FileText, "Convert to Markdown")
                .text("Generate editable video markdown from a source file.")
                .blank()
                .text("This will:")
                .bullet("Optionally preprocess audio")
                .bullet("Transcribe with WhisperX if needed")
                .bullet("Create a markdown timeline next to the video")
                .build(),
            VideoMenuEntry::Transcribe => PreviewBuilder::new()
                .header(NerdFont::Keyboard, "Transcribe")
                .text("Generate or refresh a WhisperX transcript.")
                .blank()
                .text("Transcript output is cached for reuse.")
                .build(),
            VideoMenuEntry::Render => PreviewBuilder::new()
                .header(NerdFont::PlayCircle, "Render")
                .text("Render a video from an edited markdown timeline.")
                .blank()
                .text("Supports:")
                .bullet("Overlay slides and title cards")
                .bullet("Reels mode output")
                .bullet("Audio preprocessing caches")
                .build(),
            VideoMenuEntry::Slide => PreviewBuilder::new()
                .header(NerdFont::Image, "Generate Slide")
                .text("Render a single slide image from markdown.")
                .blank()
                .text("Useful for title cards and overlays.")
                .build(),
            VideoMenuEntry::Check => PreviewBuilder::new()
                .header(NerdFont::CheckCircle, "Validate Markdown")
                .text("Validate markdown and summarize the planned output.")
                .blank()
                .text("Shows segment counts and warnings.")
                .build(),
            VideoMenuEntry::Stats => PreviewBuilder::new()
                .header(NerdFont::Chart, "Timeline Stats")
                .text("Display statistics for a markdown timeline.")
                .blank()
                .text("Shows segments, slides, and unsupported blocks.")
                .build(),
            VideoMenuEntry::Preprocess => PreviewBuilder::new()
                .header(NerdFont::Sliders, "Preprocess Audio")
                .text("Process audio with local or Auphonic backends.")
                .blank()
                .text("Uses cached results when possible.")
                .build(),
            VideoMenuEntry::Setup => PreviewBuilder::new()
                .header(NerdFont::Wrench, "Video Tool Setup")
                .text("Install or verify video tooling.")
                .blank()
                .text("Checks local preprocessors, Auphonic, and WhisperX.")
                .build(),
            VideoMenuEntry::CloseMenu => PreviewBuilder::new()
                .header(NerdFont::Cross, "Close Menu")
                .text("Exit the video menu.")
                .build(),
        }
    }
}

#[derive(Clone)]
struct ChoiceItem<T: Clone> {
    key: &'static str,
    display: String,
    value: T,
    preview: FzfPreview,
}

impl<T: Clone> ChoiceItem<T> {
    fn new(key: &'static str, display: String, value: T, preview: FzfPreview) -> Self {
        Self {
            key,
            display,
            value,
            preview,
        }
    }
}

impl<T: Clone> FzfSelectable for ChoiceItem<T> {
    fn fzf_display_text(&self) -> String {
        self.display.clone()
    }

    fn fzf_key(&self) -> String {
        self.key.to_string()
    }

    fn fzf_preview(&self) -> FzfPreview {
        self.preview.clone()
    }
}

#[derive(Clone)]
struct ToggleItem<K: Copy> {
    key: K,
    label: String,
    checked: bool,
    preview: FzfPreview,
}

impl<K: Copy> ToggleItem<K> {
    fn new(key: K, label: String, checked: bool, preview: FzfPreview) -> Self {
        Self {
            key,
            label,
            checked,
            preview,
        }
    }
}

impl<K: Copy> FzfSelectable for ToggleItem<K> {
    fn fzf_display_text(&self) -> String {
        self.label.clone()
    }

    fn fzf_key(&self) -> String {
        self.label.clone()
    }

    fn fzf_preview(&self) -> FzfPreview {
        self.preview.clone()
    }

    fn fzf_initial_checked_state(&self) -> bool {
        self.checked
    }
}

#[derive(Clone, Debug)]
enum PromptOutcome<T> {
    Value(T),
    Cancelled,
}

#[derive(Clone, Copy)]
enum ConvertAudioChoice {
    UseConfig,
    Local,
    Auphonic,
    Skip,
}

#[derive(Clone, Copy)]
enum TranscriptChoice {
    Auto,
    Provide,
}

#[derive(Clone, Copy)]
enum OutputChoice {
    Default,
    Custom,
}

#[derive(Clone, Copy)]
enum TranscribeMode {
    Defaults,
    Customize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RenderToggle {
    Reels,
    Subtitles,
    Precache,
    DryRun,
    Force,
}

#[derive(Clone, Copy)]
enum PreprocessBackendChoice {
    Local,
    Auphonic,
    None,
}

pub async fn video_menu(_debug: bool) -> Result<()> {
    let mut cursor = MenuCursor::new();
    loop {
        let entry = match select_video_menu_entry(&mut cursor)? {
            Some(entry) => entry,
            None => return Ok(()),
        };

        match entry {
            VideoMenuEntry::Convert => run_convert().await?,
            VideoMenuEntry::Transcribe => run_transcribe().await?,
            VideoMenuEntry::Render => run_render().await?,
            VideoMenuEntry::Slide => run_slide().await?,
            VideoMenuEntry::Check => run_check().await?,
            VideoMenuEntry::Stats => run_stats().await?,
            VideoMenuEntry::Preprocess => run_preprocess().await?,
            VideoMenuEntry::Setup => run_setup().await?,
            VideoMenuEntry::CloseMenu => return Ok(()),
        }
    }
}

fn select_video_menu_entry(cursor: &mut MenuCursor) -> Result<Option<VideoMenuEntry>> {
    let entries = vec![
        VideoMenuEntry::Convert,
        VideoMenuEntry::Transcribe,
        VideoMenuEntry::Render,
        VideoMenuEntry::Check,
        VideoMenuEntry::Stats,
        VideoMenuEntry::Slide,
        VideoMenuEntry::Preprocess,
        VideoMenuEntry::Setup,
        VideoMenuEntry::CloseMenu,
    ];

    let mut builder = FzfWrapper::builder()
        .header(Header::fancy("Video Menu"))
        .prompt("Select")
        .args(fzf_mocha_args())
        .responsive_layout();

    if let Some(index) = cursor.initial_index(&entries) {
        builder = builder.initial_index(index);
    }

    let result = builder.select(entries.clone())?;

    match result {
        FzfResult::Selected(entry) => {
            cursor.update(&entry, &entries);
            Ok(Some(entry))
        }
        FzfResult::Cancelled => Ok(None),
        _ => Ok(None),
    }
}

async fn run_convert() -> Result<()> {
    let Some(video_path) = select_video_file("Select source video")? else {
        return Ok(());
    };

    let transcript_choice = match select_transcript_choice()? {
        Some(choice) => choice,
        None => return Ok(()),
    };

    let transcript_path = match transcript_choice {
        TranscriptChoice::Auto => None,
        TranscriptChoice::Provide => match select_transcript_file()? {
            Some(path) => Some(path),
            None => return Ok(()),
        },
    };

    let default_output_name = default_convert_output_name(&video_path);
    let output_choice = match select_output_choice("Markdown output", &default_output_name)? {
        Some(choice) => choice,
        None => return Ok(()),
    };

    let output_path = match output_choice {
        OutputChoice::Default => None,
        OutputChoice::Custom => {
            let start_dir = video_path.parent().map(|p| p.to_path_buf());
            match select_output_path(&default_output_name, start_dir)? {
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

    let force = match confirm_toggle("Enable force overwrite and reprocess audio?")? {
        PromptOutcome::Value(force) => force,
        PromptOutcome::Cancelled => return Ok(()),
    };

    convert::handle_convert(ConvertArgs {
        video: video_path,
        transcript: transcript_path,
        out_file: output_path,
        force,
        no_preprocess,
        preprocessor,
    })
    .await
}

async fn run_transcribe() -> Result<()> {
    let Some(video_path) = select_video_file("Select video or audio for transcription")? else {
        return Ok(());
    };

    let mode = match select_transcribe_mode()? {
        Some(mode) => mode,
        None => return Ok(()),
    };

    let mut compute_type = DEFAULT_TRANSCRIBE_COMPUTE_TYPE.to_string();
    let mut device = DEFAULT_TRANSCRIBE_DEVICE.to_string();
    let mut vad_method = DEFAULT_TRANSCRIBE_VAD_METHOD.to_string();
    let mut model = None;

    if matches!(mode, TranscribeMode::Customize) {
        compute_type =
            match prompt_with_default("Whisper compute type", DEFAULT_TRANSCRIBE_COMPUTE_TYPE)? {
                PromptOutcome::Value(value) => value,
                PromptOutcome::Cancelled => return Ok(()),
            };

        device = match prompt_with_default("Target device", DEFAULT_TRANSCRIBE_DEVICE)? {
            PromptOutcome::Value(value) => value,
            PromptOutcome::Cancelled => return Ok(()),
        };

        vad_method = match prompt_with_default("VAD method", DEFAULT_TRANSCRIBE_VAD_METHOD)? {
            PromptOutcome::Value(value) => value,
            PromptOutcome::Cancelled => return Ok(()),
        };

        model = match prompt_optional("Whisper model (optional)", "Leave empty for default")? {
            PromptOutcome::Value(value) => value,
            PromptOutcome::Cancelled => return Ok(()),
        };
    }

    let force = match confirm_toggle("Re-generate transcript even if cached?")? {
        PromptOutcome::Value(force) => force,
        PromptOutcome::Cancelled => return Ok(()),
    };

    transcribe::handle_transcribe(TranscribeArgs {
        video: video_path,
        compute_type,
        device,
        model,
        vad_method,
        force,
    })
}

async fn run_render() -> Result<()> {
    let suggestions = discover_video_markdown_suggestions()?;
    let Some(markdown_path) = select_markdown_file("Select markdown for rendering", suggestions)?
    else {
        return Ok(());
    };

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

    render::handle_render(RenderArgs {
        markdown: markdown_path,
        out_file,
        force: render_options.force,
        precache_slides: render_options.precache_slides,
        dry_run: render_options.dry_run,
        reels,
        subtitles,
    })
}

async fn run_slide() -> Result<()> {
    let Some(markdown_path) = select_markdown_file("Select markdown for slide", Vec::new())? else {
        return Ok(());
    };

    let reels = match confirm_toggle("Render in reels (9:16) format?")? {
        PromptOutcome::Value(value) => value,
        PromptOutcome::Cancelled => return Ok(()),
    };

    let default_output_name = default_slide_output_name(&markdown_path);
    let output_choice = match select_output_choice("Slide output", &default_output_name)? {
        Some(choice) => choice,
        None => return Ok(()),
    };

    let out_file = match output_choice {
        OutputChoice::Default => None,
        OutputChoice::Custom => {
            let start_dir = markdown_path.parent().map(|p| p.to_path_buf());
            match select_output_path(&default_output_name, start_dir)? {
                Some(path) => Some(path),
                None => return Ok(()),
            }
        }
    };

    slides::cli::handle_slide(SlideArgs {
        markdown: markdown_path,
        out_file,
        reels,
    })
}

async fn run_check() -> Result<()> {
    let Some(markdown_path) = select_markdown_file("Select markdown to validate", Vec::new())?
    else {
        return Ok(());
    };

    check::handle_check(CheckArgs {
        markdown: markdown_path,
    })
}

async fn run_stats() -> Result<()> {
    let Some(markdown_path) = select_markdown_file("Select markdown for stats", Vec::new())? else {
        return Ok(());
    };

    stats::handle_stats(StatsArgs {
        markdown: markdown_path,
    })
}

async fn run_preprocess() -> Result<()> {
    let Some(input_path) = select_video_file("Select audio or video to preprocess")? else {
        return Ok(());
    };

    let backend_choice = match select_preprocess_backend_choice()? {
        Some(choice) => choice,
        None => return Ok(()),
    };

    let backend = match backend_choice {
        PreprocessBackendChoice::Local => "local".to_string(),
        PreprocessBackendChoice::Auphonic => "auphonic".to_string(),
        PreprocessBackendChoice::None => "none".to_string(),
    };

    let (api_key, preset) = if matches!(backend_choice, PreprocessBackendChoice::Auphonic) {
        let api_key = match prompt_optional(
            "Auphonic API key (optional)",
            "Leave empty to use configured API key",
        )? {
            PromptOutcome::Value(value) => value,
            PromptOutcome::Cancelled => return Ok(()),
        };

        let preset = match prompt_optional(
            "Auphonic preset UUID (optional)",
            "Leave empty to use configured preset",
        )? {
            PromptOutcome::Value(value) => value,
            PromptOutcome::Cancelled => return Ok(()),
        };

        (api_key, preset)
    } else {
        (None, None)
    };

    let force = match confirm_toggle("Force reprocess even if cached?")? {
        PromptOutcome::Value(force) => force,
        PromptOutcome::Cancelled => return Ok(()),
    };

    audio::handle_preprocess(PreprocessArgs {
        input_file: input_path,
        backend,
        force,
        preset,
        api_key,
    })
    .await
}

async fn run_setup() -> Result<()> {
    let force = match confirm_toggle("Force setup even if already configured?")? {
        PromptOutcome::Value(force) => force,
        PromptOutcome::Cancelled => return Ok(()),
    };

    setup::handle_setup(SetupArgs { force }).await
}

fn select_video_file(title: &str) -> Result<Option<PathBuf>> {
    let header = format!("{} {title}", char::from(NerdFont::Video));
    let manual_prompt = format!("{} Enter file path:", char::from(NerdFont::Edit));
    let picker_hint = format!(
        "{} Select a video or audio file",
        char::from(NerdFont::Info)
    );
    let start_dir = paths::videos_dir().ok();

    select_path_with_picker(
        header,
        manual_prompt,
        picker_hint,
        FilePickerScope::Files,
        start_dir,
        Vec::new(),
    )
}

fn select_transcript_file() -> Result<Option<PathBuf>> {
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

fn select_markdown_file(title: &str, suggestions: Vec<PathBuf>) -> Result<Option<PathBuf>> {
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

fn discover_video_markdown_suggestions() -> Result<Vec<PathBuf>> {
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

    let front = match front_matter {
        Some(value) => value,
        None => return Ok(false),
    };

    if !front.contains("video:") {
        return Ok(false);
    }

    Ok(parse_video_document(&contents, path).is_ok())
}

fn select_transcript_choice() -> Result<Option<TranscriptChoice>> {
    let items = vec![
        ChoiceItem::new(
            "auto",
            format!(
                "{} Use cached or auto transcription",
                format_icon_colored(NerdFont::Refresh, colors::TEAL)
            ),
            TranscriptChoice::Auto,
            PreviewBuilder::new()
                .header(NerdFont::Refresh, "Auto transcript")
                .text("Use cached transcript or generate one automatically.")
                .build(),
        ),
        ChoiceItem::new(
            "provide",
            format!(
                "{} Provide transcript file",
                format_icon_colored(NerdFont::FileText, colors::PEACH)
            ),
            TranscriptChoice::Provide,
            PreviewBuilder::new()
                .header(NerdFont::FileText, "Provide transcript")
                .text("Select an existing WhisperX JSON transcript file.")
                .build(),
        ),
    ];

    select_choice("Transcript", "Select", items)
}

fn select_output_choice(title: &str, default_name: &str) -> Result<Option<OutputChoice>> {
    let items = vec![
        ChoiceItem::new(
            "default",
            format!(
                "{} Use default output",
                format_icon_colored(NerdFont::Check, colors::GREEN)
            ),
            OutputChoice::Default,
            PreviewBuilder::new()
                .header(NerdFont::Check, "Default output")
                .text(&format!("Default file name: {default_name}"))
                .build(),
        ),
        ChoiceItem::new(
            "custom",
            format!(
                "{} Choose output path",
                format_icon_colored(NerdFont::FolderOpen, colors::SAPPHIRE)
            ),
            OutputChoice::Custom,
            PreviewBuilder::new()
                .header(NerdFont::FolderOpen, "Custom output")
                .text("Pick or enter a custom output path.")
                .build(),
        ),
    ];

    select_choice(title, "Select", items)
}

fn select_output_path(default_name: &str, start_dir: Option<PathBuf>) -> Result<Option<PathBuf>> {
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
    resolve_output_path_from_selection(selection, default_name)
}

fn resolve_output_path_from_selection(
    selection: PathInputSelection,
    default_name: &str,
) -> Result<Option<PathBuf>> {
    match selection {
        PathInputSelection::Manual(input) => {
            let trimmed = input.trim();
            if trimmed.is_empty() {
                return Ok(None);
            }
            let tilde_path = TildePath::from_str(trimmed)?;
            let mut path = tilde_path.into_path_buf();
            let treat_as_dir = trimmed.ends_with('/')
                || trimmed.ends_with(std::path::MAIN_SEPARATOR)
                || path.is_dir();
            if treat_as_dir {
                path = path.join(default_name);
            }
            Ok(Some(path))
        }
        PathInputSelection::Picker(path) | PathInputSelection::WinePrefix(path) => {
            let resolved = if path.is_dir() {
                path.join(default_name)
            } else {
                path
            };
            Ok(Some(resolved))
        }
        PathInputSelection::Cancelled => Ok(None),
    }
}

fn select_convert_audio_choice() -> Result<Option<ConvertAudioChoice>> {
    let items = vec![
        ChoiceItem::new(
            "config",
            format!(
                "{} Use configured preprocessor",
                format_icon_colored(NerdFont::Settings, colors::TEAL)
            ),
            ConvertAudioChoice::UseConfig,
            PreviewBuilder::new()
                .header(NerdFont::Settings, "Use configuration")
                .text("Use the preprocessor configured in video.toml.")
                .build(),
        ),
        ChoiceItem::new(
            "local",
            format!(
                "{} Force local preprocessing",
                format_icon_colored(NerdFont::Cpu, colors::GREEN)
            ),
            ConvertAudioChoice::Local,
            PreviewBuilder::new()
                .header(NerdFont::Cpu, "Local preprocessing")
                .text("Use DeepFilterNet and ffmpeg-normalize locally.")
                .build(),
        ),
        ChoiceItem::new(
            "auphonic",
            format!(
                "{} Force Auphonic preprocessing",
                format_icon_colored(NerdFont::CloudSync, colors::BLUE)
            ),
            ConvertAudioChoice::Auphonic,
            PreviewBuilder::new()
                .header(NerdFont::CloudSync, "Auphonic preprocessing")
                .text("Use Auphonic cloud processing (requires API key).")
                .build(),
        ),
        ChoiceItem::new(
            "skip",
            format!(
                "{} Skip preprocessing",
                format_icon_colored(NerdFont::ArrowRight, colors::OVERLAY1)
            ),
            ConvertAudioChoice::Skip,
            PreviewBuilder::new()
                .header(NerdFont::ArrowRight, "Skip preprocessing")
                .text("Use raw audio from the video file.")
                .build(),
        ),
    ];

    select_choice("Audio preprocessing", "Select", items)
}

fn select_transcribe_mode() -> Result<Option<TranscribeMode>> {
    let items = vec![
        ChoiceItem::new(
            "defaults",
            format!(
                "{} Use defaults",
                format_icon_colored(NerdFont::Check, colors::GREEN)
            ),
            TranscribeMode::Defaults,
            PreviewBuilder::new()
                .header(NerdFont::Check, "Defaults")
                .text("Use default WhisperX settings.")
                .build(),
        ),
        ChoiceItem::new(
            "custom",
            format!(
                "{} Customize settings",
                format_icon_colored(NerdFont::Sliders, colors::SAPPHIRE)
            ),
            TranscribeMode::Customize,
            PreviewBuilder::new()
                .header(NerdFont::Sliders, "Customize")
                .text("Set compute type, device, model, and VAD options.")
                .build(),
        ),
    ];

    select_choice("Transcribe", "Select", items)
}

fn select_render_options() -> Result<Option<RenderOptions>> {
    let items = vec![
        ToggleItem::new(
            RenderToggle::Reels,
            format!(
                "{} Reels mode (9:16)",
                format_icon_colored(NerdFont::Video, colors::PEACH)
            ),
            false,
            PreviewBuilder::new()
                .header(NerdFont::Video, "Reels mode")
                .text("Render in 9:16 format for short-form platforms.")
                .build(),
        ),
        ToggleItem::new(
            RenderToggle::Subtitles,
            format!(
                "{} Burn subtitles",
                format_icon_colored(NerdFont::FileText, colors::SAPPHIRE)
            ),
            false,
            PreviewBuilder::new()
                .header(NerdFont::FileText, "Subtitles")
                .text("Burn subtitles into the output video (reels only).")
                .build(),
        ),
        ToggleItem::new(
            RenderToggle::Precache,
            format!(
                "{} Pre-cache slides only",
                format_icon_colored(NerdFont::Refresh, colors::TEAL)
            ),
            false,
            PreviewBuilder::new()
                .header(NerdFont::Refresh, "Pre-cache slides")
                .text("Generate slide assets without rendering the final video.")
                .build(),
        ),
        ToggleItem::new(
            RenderToggle::DryRun,
            format!(
                "{} Dry run (print ffmpeg)",
                format_icon_colored(NerdFont::Terminal, colors::BLUE)
            ),
            false,
            PreviewBuilder::new()
                .header(NerdFont::Terminal, "Dry run")
                .text("Print the ffmpeg command without executing.")
                .build(),
        ),
        ToggleItem::new(
            RenderToggle::Force,
            format!(
                "{} Force overwrite",
                format_icon_colored(NerdFont::Warning, colors::YELLOW)
            ),
            false,
            PreviewBuilder::new()
                .header(NerdFont::Warning, "Force overwrite")
                .text("Overwrite existing output files.")
                .build(),
        ),
    ];

    let selection = FzfWrapper::builder()
        .checklist("Save")
        .prompt("Toggle")
        .header(Header::default(
            "Select render options. Toggle with Enter, then choose Save.",
        ))
        .args(fzf_mocha_args())
        .responsive_layout()
        .checklist_dialog(items)?;

    match selection {
        ChecklistResult::Confirmed(items) => {
            let has = |target| items.iter().any(|item| item.key == target);
            Ok(Some(RenderOptions {
                reels: has(RenderToggle::Reels),
                subtitles: has(RenderToggle::Subtitles),
                precache_slides: has(RenderToggle::Precache),
                dry_run: has(RenderToggle::DryRun),
                force: has(RenderToggle::Force),
            }))
        }
        ChecklistResult::Cancelled | ChecklistResult::Action(_) => Ok(None),
    }
}

fn select_preprocess_backend_choice() -> Result<Option<PreprocessBackendChoice>> {
    let config = crate::video::config::VideoConfig::load().ok();
    let has_api_key = config
        .as_ref()
        .and_then(|cfg| cfg.auphonic_api_key.as_ref())
        .is_some();
    let has_preset = config
        .as_ref()
        .and_then(|cfg| cfg.auphonic_preset_uuid.as_ref())
        .is_some();

    let auphonic_status = match (has_api_key, has_preset) {
        (true, true) => "Configured API key and preset",
        (true, false) => "API key set, preset missing",
        (false, _) => "No API key configured",
    };

    let items = vec![
        ChoiceItem::new(
            "local",
            format!(
                "{} Local preprocessing",
                format_icon_colored(NerdFont::Cpu, colors::GREEN)
            ),
            PreprocessBackendChoice::Local,
            PreviewBuilder::new()
                .header(NerdFont::Cpu, "Local backend")
                .text("Run DeepFilterNet and ffmpeg-normalize locally.")
                .build(),
        ),
        ChoiceItem::new(
            "auphonic",
            format!(
                "{} Auphonic backend",
                format_icon_colored(NerdFont::CloudSync, colors::BLUE)
            ),
            PreprocessBackendChoice::Auphonic,
            PreviewBuilder::new()
                .header(NerdFont::CloudSync, "Auphonic backend")
                .text("Use Auphonic cloud processing.")
                .blank()
                .subtext(auphonic_status)
                .build(),
        ),
        ChoiceItem::new(
            "none",
            format!(
                "{} No preprocessing",
                format_icon_colored(NerdFont::ArrowRight, colors::OVERLAY1)
            ),
            PreprocessBackendChoice::None,
            PreviewBuilder::new()
                .header(NerdFont::ArrowRight, "No preprocessing")
                .text("Skip preprocessing entirely.")
                .build(),
        ),
    ];

    select_choice("Preprocess", "Select", items)
}

fn select_choice<T: Clone>(
    title: &str,
    prompt: &str,
    items: Vec<ChoiceItem<T>>,
) -> Result<Option<T>> {
    let result = FzfWrapper::builder()
        .header(Header::fancy(title))
        .prompt(prompt)
        .args(fzf_mocha_args())
        .responsive_layout()
        .select(items)?;

    match result {
        FzfResult::Selected(item) => Ok(Some(item.value)),
        _ => Ok(None),
    }
}

fn confirm_toggle(message: &str) -> Result<PromptOutcome<bool>> {
    match FzfWrapper::builder().confirm(message).confirm_dialog()? {
        ConfirmResult::Yes => Ok(PromptOutcome::Value(true)),
        ConfirmResult::No => Ok(PromptOutcome::Value(false)),
        ConfirmResult::Cancelled => Ok(PromptOutcome::Cancelled),
    }
}

fn confirm_action(message: &str, yes_text: &str, no_text: &str) -> Result<ConfirmResult> {
    FzfWrapper::builder()
        .confirm(message)
        .yes_text(yes_text)
        .no_text(no_text)
        .confirm_dialog()
}

fn prompt_with_default(prompt: &str, default: &str) -> Result<PromptOutcome<String>> {
    let result = FzfWrapper::builder()
        .input()
        .prompt(prompt)
        .query(default)
        .ghost("Press Enter to keep default")
        .input_result()?;

    match result {
        FzfResult::Selected(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                Ok(PromptOutcome::Value(default.to_string()))
            } else {
                Ok(PromptOutcome::Value(trimmed.to_string()))
            }
        }
        FzfResult::Cancelled => Ok(PromptOutcome::Cancelled),
        _ => Ok(PromptOutcome::Cancelled),
    }
}

fn prompt_optional(prompt: &str, ghost: &str) -> Result<PromptOutcome<Option<String>>> {
    let result = FzfWrapper::builder()
        .input()
        .prompt(prompt)
        .ghost(ghost)
        .input_result()?;

    match result {
        FzfResult::Selected(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                Ok(PromptOutcome::Value(None))
            } else {
                Ok(PromptOutcome::Value(Some(trimmed.to_string())))
            }
        }
        FzfResult::Cancelled => Ok(PromptOutcome::Cancelled),
        _ => Ok(PromptOutcome::Cancelled),
    }
}

fn prompt_optional_path(prompt: &str, ghost: &str) -> Result<PromptOutcome<Option<PathBuf>>> {
    let result = FzfWrapper::builder()
        .input()
        .prompt(prompt)
        .ghost(ghost)
        .input_result()?;

    match result {
        FzfResult::Selected(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                Ok(PromptOutcome::Value(None))
            } else {
                let path = TildePath::from_str(trimmed)?.into_path_buf();
                Ok(PromptOutcome::Value(Some(path)))
            }
        }
        FzfResult::Cancelled => Ok(PromptOutcome::Cancelled),
        _ => Ok(PromptOutcome::Cancelled),
    }
}

fn default_convert_output_name(video_path: &Path) -> String {
    let stem = video_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("video");
    format!("{stem}.video.md")
}

fn default_slide_output_name(markdown_path: &Path) -> String {
    let stem = markdown_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("slide");
    format!("{stem}.jpg")
}

struct RenderOptions {
    reels: bool,
    subtitles: bool,
    precache_slides: bool,
    dry_run: bool,
    force: bool,
}
