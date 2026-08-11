use anyhow::Result;
use duct::cmd;
use reqwest::Client;

use crate::common::commands::command_exists;
use crate::menu_utils::FzfWrapper;
use crate::ui::prelude::{Level, emit};

use crate::video::audio::auphonic;
use crate::video::cli::SetupArgs;
use crate::video::config::VideoConfig;
use crate::video::deps;
use crate::video::support::DEEPFILTER_UVX_ARGS;

pub async fn handle_setup(args: SetupArgs) -> Result<()> {
    if !args.force && video_tools_ready()? {
        emit(
            Level::Info,
            "video.setup",
            "Video tools already configured. Use --force to recheck.",
            None,
        );
        return Ok(());
    }

    let config = VideoConfig::load()?;

    emit(
        Level::Info,
        "video.setup",
        &format!(
            "Starting video tools setup (enhancer: {})...",
            config.enhancer
        ),
        None,
    );

    // General tools — always useful (slides, music URLs, preview).
    setup_external_tools()?;

    // Only set up the configured enhancer, not all of them.
    match config.enhancer {
        crate::video::audio::EnhancerType::ClearVoice => setup_clearvoice(args.force)?,
        crate::video::audio::EnhancerType::Local => setup_local_enhancer(args.force)?,
        crate::video::audio::EnhancerType::Auphonic => setup_auphonic(args.force).await?,
        crate::video::audio::EnhancerType::None => {
            emit(
                Level::Info,
                "video.setup",
                "Audio enhancement is disabled in config. Skipping enhancer setup.",
                None,
            );
        }
    }

    // WhisperX is the ASR fallback (Granite is the default). Only set it up
    // if the user has explicitly configured it via `--backend whisperx`,
    // otherwise it just wastes a large download.
    emit(
        Level::Info,
        "video.setup.whisperx",
        "WhisperX is available as an ASR fallback ( Granite is the default). Use `ins video transcribe --backend whisperx` if you need it.",
        None,
    );

    emit(
        Level::Success,
        "video.setup",
        "Video tools setup completed successfully.",
        None,
    );
    Ok(())
}

fn video_tools_ready() -> Result<bool> {
    let config = VideoConfig::load()?;

    let external_ready = deps::ALL.iter().all(|d| d.is_installed());

    // Only check the configured enhancer, not all of them.
    let enhancer_ready = match config.enhancer {
        crate::video::audio::EnhancerType::ClearVoice => {
            command_exists("uv") && command_exists("ffmpeg")
        }
        crate::video::audio::EnhancerType::Local => {
            command_exists("uvx") && command_exists("ffmpeg") && {
                let mut dfn_args: Vec<&str> = DEEPFILTER_UVX_ARGS.to_vec();
                dfn_args.push("--version");
                cmd("uvx", &dfn_args).run().is_ok()
            }
        }
        crate::video::audio::EnhancerType::Auphonic => config.auphonic_api_key.is_some(),
        crate::video::audio::EnhancerType::None => true,
    };

    Ok(external_ready && enhancer_ready)
}

/// Check and install external system tools (yt-dlp, chromium, pandoc, mpv)
/// required by the video pipeline. These are small compared to the ML models
/// and used across features, so we always ensure they're present.
fn setup_external_tools() -> Result<()> {
    emit(
        Level::Info,
        "video.setup.tools",
        "Checking external tools (yt-dlp, chromium, pandoc, mpv)...",
        None,
    );

    let missing: Vec<&'static crate::common::package::Dependency> = deps::ALL
        .iter()
        .copied()
        .filter(|d| !d.is_installed())
        .collect();

    if missing.is_empty() {
        emit(
            Level::Success,
            "video.setup.tools",
            "All external tools available.",
            None,
        );
        return Ok(());
    }

    let names: Vec<&str> = missing.iter().map(|d| d.name).collect();
    emit(
        Level::Info,
        "video.setup.tools",
        &format!("Installing missing tools: {}...", names.join(", ")),
        None,
    );

    match crate::common::package::ensure_all(&missing) {
        Ok(crate::common::package::InstallResult::AlreadyInstalled)
        | Ok(crate::common::package::InstallResult::Installed) => {
            emit(
                Level::Success,
                "video.setup.tools",
                "External tools installed.",
                None,
            );
            Ok(())
        }
        Ok(crate::common::package::InstallResult::Declined) => {
            anyhow::bail!(
                "Required tools declined: {}. These are needed for slides, music URLs, and preview.",
                names.join(", ")
            )
        }
        Ok(crate::common::package::InstallResult::NotAvailable { name, hint }) => {
            anyhow::bail!("Required tool {name} not available: {hint}")
        }
        Ok(crate::common::package::InstallResult::Failed { reason }) => {
            anyhow::bail!("Failed to install external tools: {reason}")
        }
        Err(e) => {
            anyhow::bail!("Could not check/install external tools: {e:#}")
        }
    }
}

fn setup_clearvoice(_force: bool) -> Result<()> {
    emit(
        Level::Info,
        "video.setup.clearvoice",
        "Checking ClearVoice enhancer dependencies...",
        None,
    );

    if !command_exists("uv") {
        emit(
            Level::Warn,
            "video.setup.clearvoice",
            "uv is not installed. ClearVoice enhancement requires uv. Install with: curl -LsSf https://astral.sh/uv/install.sh | sh",
            None,
        );
        return Ok(());
    }

    if !command_exists("ffmpeg") {
        emit(
            Level::Warn,
            "video.setup.clearvoice",
            "ffmpeg is not installed. ClearVoice enhancement requires ffmpeg.",
            None,
        );
        return Ok(());
    }

    // ClearVoice runs via `uv run --with clearvoice` (same pattern as Granite
    // and whisperx). The Python package and model are fetched on first use.
    emit(
        Level::Success,
        "video.setup.clearvoice",
        "ClearVoice dependencies ready (package + model fetched on first enhance).",
        None,
    );
    Ok(())
}

fn setup_local_enhancer(_force: bool) -> Result<()> {
    emit(
        Level::Info,
        "video.setup.local",
        "Checking local enhancer dependencies...",
        None,
    );

    // Check required dependencies
    if !command_exists("uvx") {
        emit(
            Level::Warn,
            "video.setup.local",
            "uvx is not installed. Local enhancement requires uvx. Install with: curl -LsSf https://astral.sh/uv/install.sh | sh",
            None,
        );
        return Ok(());
    }

    if !command_exists("ffmpeg") {
        emit(
            Level::Warn,
            "video.setup.local",
            "ffmpeg is not installed. Local enhancement requires ffmpeg.",
            None,
        );
        return Ok(());
    }

    // Check optional tools
    check_deepfilternet();

    emit(
        Level::Success,
        "video.setup.local",
        "Local enhancer dependencies checked.",
        None,
    );

    Ok(())
}

/// Runs a `uvx <args> --version` availability check and reports the outcome.
fn check_uvx_tool(tool_name: &str, check_args: &[&str], note: &str) {
    emit(
        Level::Info,
        "video.setup.local",
        &format!("Verifying {tool_name} availability{note}..."),
        None,
    );

    let result = cmd("uvx", check_args).stderr_to_stdout().run();

    if let Err(e) = result {
        emit(
            Level::Warn,
            "video.setup.local",
            &format!(
                "{tool_name} check failed: {}. It may still work at runtime.",
                e
            ),
            None,
        );
    } else {
        emit(
            Level::Success,
            "video.setup.local",
            &format!("{tool_name} is available."),
            None,
        );
    }
}

fn check_deepfilternet() {
    let mut dfn_args: Vec<&str> = DEEPFILTER_UVX_ARGS.to_vec();
    dfn_args.push("--version");
    check_uvx_tool(
        "DeepFilterNet",
        &dfn_args,
        " (this may download dependencies)",
    );
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
            if user_info.is_free_account() {
                emit(
                    Level::Warn,
                    "video.setup.auphonic",
                    "Free account detected. Consider using local enhancer (default) to avoid jingle insertion.",
                    None,
                );
            } else {
                emit(
                    Level::Success,
                    "video.setup.auphonic",
                    "Premium account detected. You can use 'enhancer = \"auphonic\"' in config.",
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
