use std::fs;
use std::io::{ErrorKind, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use anyhow::{Context, Result, bail, ensure};
use tempfile::NamedTempFile;

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

/// Find the `otpauth://` line in a pass entry, matching `pass otp`'s behavior
/// of scanning the whole entry rather than only the password line.
pub(super) fn find_otp_uri_line(output: &[u8]) -> Option<&str> {
    std::str::from_utf8(output)
        .ok()?
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("otpauth://"))
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
    let parent = destination
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .with_context(|| format!("Failed to create export directory {}", parent.display()))?;

    let exists = match destination.symlink_metadata() {
        Ok(metadata) if metadata.is_file() => true,
        Ok(_) => bail!(
            "Export destination must be a regular file: {}",
            destination.display()
        ),
        Err(error) if error.kind() == ErrorKind::NotFound => false,
        Err(error) => {
            return Err(error).with_context(|| {
                format!(
                    "Failed to inspect export destination {}",
                    destination.display()
                )
            });
        }
    };
    if exists
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

    let mut export = NamedTempFile::new_in(parent)
        .with_context(|| format!("Failed to create a private export in {}", parent.display()))?;
    export
        .as_file()
        .set_permissions(fs::Permissions::from_mode(0o600))
        .context("Failed to set private export permissions")?;
    let permissions = export
        .as_file()
        .metadata()
        .context("Failed to verify private export permissions")?
        .permissions();
    ensure!(
        permissions.mode() & 0o777 == 0o600,
        "Export directory does not support owner-only file permissions"
    );
    export
        .write_all(output)
        .context("Failed to write export data")?;

    let result = if exists {
        export.persist(destination)
    } else {
        export.persist_noclobber(destination)
    };
    result
        .map_err(|error| error.error)
        .with_context(|| format!("Failed to install export file {}", destination.display()))?;
    Ok(())
}
