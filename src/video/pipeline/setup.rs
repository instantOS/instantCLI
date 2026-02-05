use anyhow::Result;
use duct::cmd;
use reqwest::Client;

use crate::menu_utils::FzfWrapper;
use crate::ui::prelude::{Level, emit};

use crate::video::audio::auphonic;
use crate::video::cli::SetupArgs;
use crate::video::config::VideoConfig;

pub async fn handle_setup(args: SetupArgs) -> Result<()> {
    emit(
        Level::Info,
        "video.setup",
        "Starting video tools setup...",
        None,
    );

    setup_local_preprocessor(args.force)?;
    setup_auphonic(args.force).await?;
    setup_whisperx(args.force)?;

    emit(
        Level::Success,
        "video.setup",
        "Video tools setup completed successfully.",
        None,
    );
    Ok(())
}

fn setup_local_preprocessor(_force: bool) -> Result<()> {
    emit(
        Level::Info,
        "video.setup.local",
        "Checking local preprocessor dependencies...",
        None,
    );

    // Check required dependencies
    if !check_command_exists("uvx") {
        emit(
            Level::Warn,
            "video.setup.local",
            "uvx is not installed. Local preprocessing requires uvx. Install with: curl -LsSf https://astral.sh/uv/install.sh | sh",
            None,
        );
        return Ok(());
    }

    if !check_command_exists("ffmpeg") {
        emit(
            Level::Warn,
            "video.setup.local",
            "ffmpeg is not installed. Local preprocessing requires ffmpeg.",
            None,
        );
        return Ok(());
    }

    // Check optional tools
    check_deepfilternet();
    check_ffmpeg_normalize();

    emit(
        Level::Success,
        "video.setup.local",
        "Local preprocessor dependencies checked.",
        None,
    );

    Ok(())
}

fn check_command_exists(command: &str) -> bool {
    cmd!("which", command).run().is_ok()
}

fn check_deepfilternet() {
    emit(
        Level::Info,
        "video.setup.local",
        "Verifying DeepFilterNet availability (this may download dependencies)...",
        None,
    );

    let dfn_result = cmd!(
        "uvx",
        "--python",
        "3.10",
        "--from",
        "deepfilternet",
        "--with",
        "torch<2.1",
        "--with",
        "torchaudio<2.1",
        "deepFilter",
        "--version"
    )
    .stderr_to_stdout()
    .run();

    if let Err(e) = dfn_result {
        emit(
            Level::Warn,
            "video.setup.local",
            &format!(
                "DeepFilterNet check failed: {}. It may still work at runtime.",
                e
            ),
            None,
        );
    } else {
        emit(
            Level::Success,
            "video.setup.local",
            "DeepFilterNet is available.",
            None,
        );
    }
}

fn check_ffmpeg_normalize() {
    emit(
        Level::Info,
        "video.setup.local",
        "Verifying ffmpeg-normalize availability...",
        None,
    );

    let normalize_result = cmd!("uvx", "ffmpeg-normalize", "--version")
        .stderr_to_stdout()
        .run();

    if let Err(e) = normalize_result {
        emit(
            Level::Warn,
            "video.setup.local",
            &format!(
                "ffmpeg-normalize check failed: {}. It may still work at runtime.",
                e
            ),
            None,
        );
    } else {
        emit(
            Level::Success,
            "video.setup.local",
            "ffmpeg-normalize is available.",
            None,
        );
    }
}

async fn setup_auphonic(force: bool) -> Result<()> {
    emit(
        Level::Info,
        "video.setup.auphonic",
        "Checking Auphonic configuration...",
        None,
    );

    let mut config = VideoConfig::load()?;
    let client = Client::new();

    // Check existing API key if available and not forcing
    if let Some(api_key) = &config.auphonic_api_key {
        if !force {
            match verify_existing_key(&client, api_key).await {
                VerificationResult::Valid => {
                    check_and_emit_account_type(&client, api_key).await;
                    return Ok(());
                }
                VerificationResult::Invalid(e) => {
                    emit(
                        Level::Warn,
                        "video.setup.auphonic",
                        &format!("Stored API key is invalid: {}", e),
                        None,
                    );
                    // Fall through to ask for key
                }
            }
        }
    } else {
        emit(
            Level::Info,
            "video.setup.auphonic",
            "Auphonic API key not found.",
            None,
        );
    }

    // Prompt for and configure new API key
    let api_key = prompt_for_api_key()?;
    verify_and_save_new_key(&client, &api_key, &mut config).await?;

    Ok(())
}

enum VerificationResult {
    Valid,
    Invalid(String),
}

async fn verify_existing_key(client: &Client, api_key: &str) -> VerificationResult {
    emit(
        Level::Info,
        "video.setup.auphonic",
        "Auphonic API key found. Verifying...",
        None,
    );
    match auphonic::verify_api_key(client, api_key).await {
        Ok(_) => {
            emit(
                Level::Success,
                "video.setup.auphonic",
                "Auphonic API key is valid.",
                None,
            );
            VerificationResult::Valid
        }
        Err(e) => VerificationResult::Invalid(e.to_string()),
    }
}

async fn check_and_emit_account_type(client: &Client, api_key: &str) {
    emit(
        Level::Info,
        "video.setup.auphonic",
        "Checking account type...",
        None,
    );
    match auphonic::get_user_info(client, api_key).await {
        Ok(user_info) => {
            if auphonic::is_free_account(&user_info) {
                emit(
                    Level::Warn,
                    "video.setup.auphonic",
                    "Free account detected. Consider using local preprocessor (default) to avoid jingle insertion.",
                    None,
                );
            } else {
                emit(
                    Level::Success,
                    "video.setup.auphonic",
                    "Premium account detected. You can use 'preprocessor = \"auphonic\"' in config.",
                    None,
                );
            }
        }
        Err(e) => {
            emit(
                Level::Warn,
                "video.setup.auphonic",
                &format!(
                    "Failed to check account type ({}). Current setting will be maintained.",
                    e
                ),
                None,
            );
        }
    }
}

fn prompt_for_api_key() -> Result<String> {
    let prompt = "Enter your Auphonic API key (from https://auphonic.com/accounts/settings/):";
    let api_key = match FzfWrapper::input(prompt) {
        Ok(input) => input.trim().to_string(),
        Err(e) => {
            anyhow::bail!("Failed to get API key input: {}", e);
        }
    };

    if api_key.is_empty() {
        anyhow::bail!("API key cannot be empty.");
    }

    Ok(api_key)
}

async fn verify_and_save_new_key(
    client: &Client,
    api_key: &str,
    config: &mut VideoConfig,
) -> Result<()> {
    emit(
        Level::Info,
        "video.setup.auphonic",
        "Verifying new API key...",
        None,
    );
    auphonic::verify_api_key(client, api_key).await?;
    emit(
        Level::Success,
        "video.setup.auphonic",
        "API key verified.",
        None,
    );

    // This is best-effort (non-fatal) and only emits UI messages.
    check_and_emit_account_type(client, api_key).await;

    config.auphonic_api_key = Some(api_key.to_string());
    config.save()?;
    emit(
        Level::Success,
        "video.setup.auphonic",
        "Auphonic configuration saved.",
        None,
    );

    Ok(())
}

fn setup_whisperx(_force: bool) -> Result<()> {
    emit(
        Level::Info,
        "video.setup.whisperx",
        "Checking WhisperX setup...",
        None,
    );

    // Check if uv is installed
    if !check_command_exists("uv") {
        emit(
            Level::Warn,
            "video.setup.whisperx",
            "uv is not installed. Please install uv first to use WhisperX management.",
            None,
        );
        // We can't really proceed if uv is missing as per current transcribe implementation which uses uvx
        return Ok(());
    }

    // Check if whisperx is already runnable
    // The plan says "Predownload the whisper uv stuff needed if possible."
    // `uvx` runs tools from ephemeral environments usually, but `uv tool install` installs them.
    // The `transcribe.rs` uses `uvx`. `uvx` caches tools.
    // Running `uvx whisperx --version` should trigger the download/cache if not present.

    emit(
        Level::Info,
        "video.setup.whisperx",
        "Verifying WhisperX availability (this may download dependencies)...",
        None,
    );

    let output = cmd!("uvx", "whisperx", "--version")
        .stderr_to_stdout()
        .run();

    match output {
        Ok(_) => {
            emit(
                Level::Success,
                "video.setup.whisperx",
                "WhisperX is ready.",
                None,
            );
        }
        Err(e) => {
            emit(
                Level::Error,
                "video.setup.whisperx",
                &format!("Failed to run WhisperX: {}", e),
                None,
            );
            anyhow::bail!("WhisperX setup failed.");
        }
    }

    Ok(())
}
