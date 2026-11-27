use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};

use super::types::{EXPORT_THRESHOLD_BYTES, PassEntry};
use crate::menu_utils::{ConfirmResult, FzfWrapper};

pub(super) fn first_secret_line(output: &[u8]) -> Option<String> {
    std::str::from_utf8(output)
        .ok()?
        .lines()
        .next()
        .map(|line| line.trim_end())
        .filter(|line| !line.is_empty())
        .map(|line| line.to_string())
}

pub(super) fn should_export_output(entry: &PassEntry, output: &[u8]) -> bool {
    if entry.should_offer_export() {
        return true;
    }

    if output.len() > EXPORT_THRESHOLD_BYTES {
        return true;
    }

    std::str::from_utf8(output).is_err()
}

pub(super) fn write_export_file(destination: &Path, output: &[u8]) -> Result<()> {
    if let Some(parent) = destination.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create export directory {}", parent.display()))?;
    }

    if destination.exists()
        && !matches!(
            FzfWrapper::builder()
                .confirm(format!(
                    "{} already exists.\n\nOverwrite it?",
                    destination.display()
                ))
                .default_no()
                .confirm_dialog()?,
            ConfirmResult::Yes
        )
    {
        bail!("Export cancelled");
    }

    fs::write(destination, output)
        .with_context(|| format!("Failed to write export file {}", destination.display()))
}
