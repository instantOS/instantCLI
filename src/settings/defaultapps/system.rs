use anyhow::{Context, Result};
use std::process::Command;

pub(crate) fn query_default_app(mime_type: &str) -> Result<Option<String>> {
    let output = Command::new("xdg-mime")
        .arg("query")
        .arg("default")
        .arg(mime_type)
        .output()
        .context("Failed to execute xdg-mime query")?;

    if !output.status.success() {
        return Ok(None);
    }

    let default_app = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if default_app.is_empty() {
        Ok(None)
    } else {
        Ok(Some(default_app))
    }
}

pub(crate) fn set_default_app(mime_type: &str, desktop_file: &str) -> Result<()> {
    let result = set_default_app_without_invalidation(mime_type, desktop_file);
    invalidate_default_app_previews();
    result
}

pub(crate) fn set_default_apps(mime_types: &[&str], desktop_file: &str) -> Result<()> {
    let result = mime_types.iter().try_for_each(|mime_type| {
        set_default_app_without_invalidation(mime_type, desktop_file)
            .with_context(|| format!("Failed to set default for {mime_type}"))
    });
    invalidate_default_app_previews();
    result
}

fn set_default_app_without_invalidation(mime_type: &str, desktop_file: &str) -> Result<()> {
    let status = Command::new("xdg-mime")
        .arg("default")
        .arg(desktop_file)
        .arg(mime_type)
        .status()
        .context("Failed to execute xdg-mime default")?;

    if !status.success() {
        anyhow::bail!("xdg-mime default command failed");
    }

    Ok(())
}

fn invalidate_default_app_previews() {
    for preview in [
        crate::preview::PreviewId::MimeType,
        crate::preview::PreviewId::DefaultImageViewer,
        crate::preview::PreviewId::DefaultVideoPlayer,
        crate::preview::PreviewId::DefaultAudioPlayer,
        crate::preview::PreviewId::DefaultArchiveManager,
        crate::preview::PreviewId::DefaultBrowser,
        crate::preview::PreviewId::DefaultTextEditor,
        crate::preview::PreviewId::DefaultEmail,
        crate::preview::PreviewId::DefaultFileManager,
        crate::preview::PreviewId::DefaultPdfViewer,
    ] {
        let _ = crate::preview::cache::invalidate(preview);
    }
}
