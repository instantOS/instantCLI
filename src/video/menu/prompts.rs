use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::common::TildePath;
use crate::menu_utils::{
    ChecklistResult, ConfirmResult, FzfResult, FzfWrapper, Header, PathInputSelection,
};
use crate::ui::catppuccin::{colors, format_icon_colored, fzf_mocha_args};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

use super::types::{
    ChoiceItem, ConvertAudioChoice, OutputChoice, PreprocessBackendChoice, PromptOutcome,
    RenderOptions, RenderToggle, ToggleItem, TranscribeMode, TranscriptChoice,
};

pub fn select_transcript_choice() -> Result<Option<TranscriptChoice>> {
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

pub fn select_output_choice(title: &str, default_name: &str) -> Result<Option<OutputChoice>> {
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

pub fn resolve_output_path_from_selection(
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

pub fn select_convert_audio_choice() -> Result<Option<ConvertAudioChoice>> {
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

pub fn select_transcribe_mode() -> Result<Option<TranscribeMode>> {
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

pub fn select_render_options() -> Result<Option<RenderOptions>> {
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

pub fn select_preprocess_backend_choice() -> Result<Option<PreprocessBackendChoice>> {
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

pub fn select_choice<T: Clone>(
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

pub fn confirm_toggle(message: &str) -> Result<PromptOutcome<bool>> {
    match FzfWrapper::builder().confirm(message).confirm_dialog()? {
        ConfirmResult::Yes => Ok(PromptOutcome::Value(true)),
        ConfirmResult::No => Ok(PromptOutcome::Value(false)),
        ConfirmResult::Cancelled => Ok(PromptOutcome::Cancelled),
    }
}

pub fn confirm_action(message: &str, yes_text: &str, no_text: &str) -> Result<ConfirmResult> {
    FzfWrapper::builder()
        .confirm(message)
        .yes_text(yes_text)
        .no_text(no_text)
        .confirm_dialog()
}

pub fn prompt_with_default(prompt: &str, default: &str) -> Result<PromptOutcome<String>> {
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

pub fn prompt_optional(prompt: &str, ghost: &str) -> Result<PromptOutcome<Option<String>>> {
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

pub fn prompt_optional_path(prompt: &str, ghost: &str) -> Result<PromptOutcome<Option<PathBuf>>> {
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

pub fn default_slide_output_name(markdown_path: &Path) -> String {
    let stem = markdown_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("slide");
    format!("{stem}.jpg")
}
