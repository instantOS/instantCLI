use anyhow::Result;

use crate::video::audio;
use crate::video::cli::{PreprocessArgs, SetupArgs, SlideArgs, TranscribeArgs};
use crate::video::pipeline::{setup, transcribe};
use crate::video::slides;

use super::file_selection::{select_markdown_file, select_output_path, select_video_file};
use super::prompts::{
    confirm_toggle, default_slide_output_name, prompt_optional, prompt_with_default,
    select_output_choice, select_preprocess_backend_choice, select_transcribe_mode,
};
use super::types::{
    DEFAULT_TRANSCRIBE_COMPUTE_TYPE, DEFAULT_TRANSCRIBE_DEVICE, DEFAULT_TRANSCRIBE_VAD_METHOD,
    OutputChoice, PreprocessBackendChoice, PromptOutcome, TranscribeMode,
};

pub async fn run_transcribe() -> Result<()> {
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

    transcribe::handle_transcribe(TranscribeArgs {
        video: video_path,
        compute_type,
        device,
        model,
        vad_method,
        force: false,
    })
}

pub async fn run_slide() -> Result<()> {
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

pub async fn run_preprocess() -> Result<()> {
    let Some(input_path) = select_video_file("Select audio or video to preprocess")? else {
        return Ok(());
    };

    let backend_choice = match select_preprocess_backend_choice()? {
        Some(choice) => choice,
        None => return Ok(()),
    };

    let backend = backend_choice.to_string();

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

pub async fn run_setup() -> Result<()> {
    let force = match confirm_toggle("Force setup even if already configured?")? {
        PromptOutcome::Value(force) => force,
        PromptOutcome::Cancelled => return Ok(()),
    };

    setup::handle_setup(SetupArgs { force }).await
}
