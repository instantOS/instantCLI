use anyhow::Result;
use duct::cmd;
use reqwest::Client;

use crate::menu_utils::FzfWrapper;
use crate::ui::prelude::{Level, emit};

use super::audio_preprocessing::auphonic;
use super::cli::SetupArgs;
use super::config::VideoConfig;

/// Checks account type and updates config accordingly
async fn check_and_update_auphonic_config(
    client: &Client,
    api_key: &str,
    config: &mut VideoConfig,
) -> Result<()> {
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
                    "Free account detected. Consider using local preprocessor to avoid jingle insertion.",
                    None,
                );
            } else {
                emit(
                    Level::Success,
                    "video.setup.auphonic",
                    "Premium account detected.",
                    None,
                );
            }
            config.save()?;
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
    Ok(())
}

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

    // Check uvx
    if cmd!("which", "uvx").run().is_err() {
        emit(
            Level::Warn,
            "video.setup.local",
            "uvx is not installed. Local preprocessing requires uvx. Install with: curl -LsSf https://astral.sh/uv/install.sh | sh",
            None,
        );
        return Ok(());
    }

    // Check ffmpeg
    if cmd!("which", "ffmpeg").run().is_err() {
        emit(
            Level::Warn,
            "video.setup.local",
            "ffmpeg is not installed. Local preprocessing requires ffmpeg.",
            None,
        );
        return Ok(());
    }

    emit(
        Level::Info,
        "video.setup.local",
        "Verifying DeepFilterNet availability (this may download dependencies)...",
        None,
    );

    // Try running DeepFilterNet --version to trigger download/cache
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

    emit(
        Level::Success,
        "video.setup.local",
        "Local preprocessor dependencies checked.",
        None,
    );

    Ok(())
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

    if let Some(api_key) = &config.auphonic_api_key {
        if !force {
            emit(
                Level::Info,
                "video.setup.auphonic",
                "Auphonic API key found. Verifying...",
                None,
            );
            match auphonic::verify_api_key(&client, api_key).await {
                Ok(_) => {
                    emit(
                        Level::Success,
                        "video.setup.auphonic",
                        "Auphonic API key is valid.",
                        None,
                    );

                    // Check account type for existing valid keys
                    emit(
                        Level::Info,
                        "video.setup.auphonic",
                        "Checking account type...",
                        None,
                    );
                    match auphonic::get_user_info(&client, api_key).await {
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
                    return Ok(());
                }
                Err(e) => {
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

    // Ask for API key
    let prompt = "Enter your Auphonic API key (from https://auphonic.com/accounts/settings/):";
    let api_key = match FzfWrapper::input(prompt) {
        Ok(input) => input.trim().to_string(),
        Err(e) => {
            // If cancelled or error
            anyhow::bail!("Failed to get API key input: {}", e);
        }
    };

    if api_key.is_empty() {
        anyhow::bail!("API key cannot be empty.");
    }

    // Verify new key
    emit(
        Level::Info,
        "video.setup.auphonic",
        "Verifying new API key...",
        None,
    );
    auphonic::verify_api_key(&client, &api_key).await?;
    emit(
        Level::Success,
        "video.setup.auphonic",
        "API key verified.",
        None,
    );

    // Check account type and update config
    check_and_update_auphonic_config(&client, &api_key, &mut config).await?;

    // Save
    config.auphonic_api_key = Some(api_key);
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
    if cmd!("which", "uv").run().is_err() {
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
