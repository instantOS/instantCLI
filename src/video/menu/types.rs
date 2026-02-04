use crate::menu_utils::{FzfPreview, FzfSelectable};
use crate::ui::catppuccin::{colors, format_back_icon, format_icon_colored};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

pub const DEFAULT_TRANSCRIBE_COMPUTE_TYPE: &str = "int8";
pub const DEFAULT_TRANSCRIBE_DEVICE: &str = "cpu";
pub const DEFAULT_TRANSCRIBE_VAD_METHOD: &str = "silero";

pub const VIDEO_EXTENSIONS: &[&str] = &[
    "mp4", "mkv", "webm", "mov", "m4v", "avi", "wmv", "flv", "ts", "mts", "m2ts",
];
pub const AUDIO_EXTENSIONS: &[&str] = &["mp3", "wav", "flac", "m4a", "ogg", "aac", "wma", "aiff"];

#[derive(Debug, Clone)]
pub enum VideoMenuEntry {
    Convert,
    Transcribe,
    Project,
    Append,
    Slide,
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
            VideoMenuEntry::Project => format!(
                "{} Project",
                format_icon_colored(NerdFont::Folder, colors::GREEN)
            ),
            VideoMenuEntry::Append => format!(
                "{} Add Recording to Markdown",
                format_icon_colored(NerdFont::SourceMerge, colors::PEACH)
            ),
            VideoMenuEntry::Slide => format!(
                "{} Generate Slide Image",
                format_icon_colored(NerdFont::Image, colors::YELLOW)
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
            VideoMenuEntry::Project => "!__project__".to_string(),
            VideoMenuEntry::Append => "!__append__".to_string(),
            VideoMenuEntry::Slide => "!__slide__".to_string(),
            VideoMenuEntry::Preprocess => "!__preprocess__".to_string(),
            VideoMenuEntry::Setup => "!__setup__".to_string(),
            VideoMenuEntry::CloseMenu => "!__close_menu__".to_string(),
        }
    }

    fn fzf_preview(&self) -> FzfPreview {
        match self {
            VideoMenuEntry::Convert => PreviewBuilder::new()
                .header(NerdFont::FileText, "Convert to Markdown")
                .text("Generate editable video markdown from source files.")
                .blank()
                .text("This will:")
                .bullet("Build a list of videos to convert")
                .bullet("Optionally preprocess audio")
                .bullet("Transcribe with WhisperX if needed")
                .bullet("Create markdown timelines for each video")
                .build(),
            VideoMenuEntry::Transcribe => PreviewBuilder::new()
                .header(NerdFont::Keyboard, "Transcribe")
                .text("Generate or refresh a WhisperX transcript.")
                .blank()
                .text("Transcript output is cached for reuse.")
                .build(),
            VideoMenuEntry::Project => PreviewBuilder::new()
                .header(NerdFont::Folder, "Project")
                .text("Work with an existing video project.")
                .blank()
                .text("Actions:")
                .bullet("Render edited video")
                .bullet("Validate markdown")
                .bullet("Show timeline stats")
                .bullet("Clear cache")
                .build(),
            VideoMenuEntry::Append => PreviewBuilder::new()
                .header(NerdFont::SourceMerge, "Append recording")
                .text("Add another recording to an existing video markdown.")
                .blank()
                .text("This will:")
                .bullet("Transcribe the new clip")
                .bullet("Append a new source to front matter")
                .bullet("Add timestamped segments to the timeline")
                .build(),
            VideoMenuEntry::Slide => PreviewBuilder::new()
                .header(NerdFont::Image, "Generate Slide")
                .text("Render a single slide image from markdown.")
                .blank()
                .text("Useful for title cards and overlays.")
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
pub struct ChoiceItem<T: Clone> {
    pub key: &'static str,
    pub display: String,
    pub value: T,
    pub preview: FzfPreview,
}

impl<T: Clone> ChoiceItem<T> {
    pub fn new(key: &'static str, display: String, value: T, preview: FzfPreview) -> Self {
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
pub struct ToggleItem<K: Copy> {
    pub key: K,
    pub label: String,
    pub checked: bool,
    pub preview: FzfPreview,
}

impl<K: Copy> ToggleItem<K> {
    pub fn new(key: K, label: String, checked: bool, preview: FzfPreview) -> Self {
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
pub enum PromptOutcome<T> {
    Value(T),
    Cancelled,
}

#[derive(Clone, Copy)]
pub enum ConvertAudioChoice {
    UseConfig,
    Local,
    Auphonic,
    Skip,
}

#[derive(Clone, Copy)]
pub enum TranscriptChoice {
    Auto,
    Provide,
}

#[derive(Clone, Copy)]
pub enum OutputChoice {
    Default,
    Custom,
}

#[derive(Clone, Copy)]
pub enum TranscribeMode {
    Defaults,
    Customize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RenderToggle {
    Reels,
    Subtitles,
    Precache,
    DryRun,
    Force,
}

#[derive(Clone, Copy)]
pub enum PreprocessBackendChoice {
    Local,
    Auphonic,
    None,
}

pub struct RenderOptions {
    pub reels: bool,
    pub subtitles: bool,
    pub precache_slides: bool,
    pub dry_run: bool,
    pub force: bool,
}
