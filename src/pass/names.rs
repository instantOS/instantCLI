use std::path::Path;

use anyhow::{Result, bail};

/// Initial name shown in the add prompt, starting inside the current folder.
pub(super) fn entry_name_prefill(prefix: Option<&str>, suggestion: Option<&str>) -> Option<String> {
    match (prefix.filter(|value| !value.is_empty()), suggestion) {
        (Some(prefix), Some(suggestion)) => Some(format!("{prefix}/{suggestion}")),
        (Some(prefix), None) => Some(format!("{prefix}/")),
        (None, Some(suggestion)) => Some(suggestion.to_string()),
        (None, None) => None,
    }
}

pub(super) fn apply_prefix_to_entry_name(name: &str, prefix: Option<&str>) -> Result<String> {
    match prefix {
        Some(prefix) if !prefix.is_empty() && !name.contains('/') && name != prefix => {
            sanitize_entry_name(&format!("{prefix}/{name}"))
        }
        _ => Ok(name.to_string()),
    }
}

pub(super) fn sanitize_entry_name(name: &str) -> Result<String> {
    let trimmed = name.trim().trim_matches('/');
    if trimmed.is_empty() {
        bail!("Entry name cannot be empty");
    }
    if trimmed.contains('\n') || trimmed.contains('\r') {
        bail!("Entry name cannot contain newlines");
    }
    if trimmed.contains("..") {
        bail!("Entry name cannot contain `..`");
    }
    Ok(trimmed.to_string())
}

pub(super) fn normalize_otp_name(name: &str) -> String {
    if name.ends_with(".otp") {
        name.to_string()
    } else {
        format!("{name}.otp")
    }
}

/// File entries use a `.file` suffix so the browser exports them instead of
/// trying to copy their contents to the clipboard.
pub(super) fn normalize_file_entry_name(name: &str) -> String {
    if name.ends_with(".file") {
        name.to_string()
    } else {
        format!("{name}.file")
    }
}

pub(super) fn suggested_file_entry_name(path: &Path) -> String {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".to_string());
    normalize_file_entry_name(&name)
}

/// Undo the `.file` marker for exported filenames while preserving the source
/// extension, e.g. `contract.pdf.file` exports as `contract.pdf`.
pub(super) fn suggested_export_filename(display_name: &str) -> String {
    let name = display_name.rsplit('/').next().unwrap_or(display_name);

    if let Some(source_name) = name.strip_suffix(".file")
        && source_name.contains('.')
    {
        return source_name.to_string();
    }

    name.to_string()
}

/// Build an entry-name suggestion from the issuer (or account) of an OTP URI.
pub(super) fn suggested_otp_entry_name(uri: &str) -> Option<String> {
    let auth = super::totp::OtpAuth::parse(uri).ok()?;
    let raw = auth.issuer().or_else(|| auth.account())?;

    let mut name = String::new();
    for ch in raw.trim().chars() {
        if ch.is_control() {
            continue;
        }
        if ch.is_whitespace() || matches!(ch, '/' | '\\' | ':') {
            if !name.is_empty() && !name.ends_with('-') {
                name.push('-');
            }
        } else {
            name.push(ch);
        }
    }

    sanitize_entry_name(name.trim_matches('-')).ok()
}
