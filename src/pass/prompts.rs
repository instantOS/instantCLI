use std::path::PathBuf;

use anyhow::{Result, anyhow, bail};

use super::names::{
    apply_prefix_to_entry_name, entry_name_prefill, sanitize_entry_name, suggested_export_filename,
};
use super::store::ensure_password_store_dir;
use crate::menu_utils::{ConfirmResult, FzfWrapper};

pub(super) fn prompt_password_with_confirmation(prompt: &str) -> Result<String> {
    let password = match FzfWrapper::builder()
        .prompt(prompt)
        .password()
        .with_confirmation()
        .password_dialog()?
    {
        crate::menu_utils::DialogOutcome::Submitted(value) => value,
        _ => bail!("Password entry cancelled"),
    };

    if password.is_empty() {
        bail!("Password cannot be empty");
    }

    Ok(password)
}

pub(super) fn prompt_text_value(
    prompt: &str,
    header: Option<&str>,
    allow_empty: bool,
    ghost: Option<&str>,
    initial: Option<&str>,
) -> Result<Option<String>> {
    let mut base = FzfWrapper::builder().prompt(prompt);

    if let Some(header) = header {
        base = base.header(header);
    }
    if let Some(initial) = initial {
        base = base.query(initial);
    }

    let mut builder = base.input();

    if let Some(ghost) = ghost {
        builder = builder.ghost(ghost);
    }

    match builder.input_dialog()? {
        crate::menu_utils::DialogOutcome::Submitted(value) => {
            let trimmed = value.trim().to_string();
            if trimmed.is_empty() && !allow_empty {
                Ok(None)
            } else {
                Ok(Some(trimmed))
            }
        }
        crate::menu_utils::DialogOutcome::Cancelled => Ok(None),
    }
}

pub(super) fn resolve_entry_name(
    name: Option<String>,
    prefix: Option<&str>,
    header: &str,
) -> Result<String> {
    resolve_entry_name_with_suggestion(name, prefix, header, None)
}

pub(super) fn resolve_entry_name_with_suggestion(
    name: Option<String>,
    prefix: Option<&str>,
    header: &str,
    suggestion: Option<&str>,
) -> Result<String> {
    let prefix = prefix.filter(|value| !value.is_empty());

    match name {
        Some(name) => apply_prefix_to_entry_name(&sanitize_entry_name(&name)?, prefix),
        None => {
            let initial = entry_name_prefill(prefix, suggestion);
            let ghost = prefix.unwrap_or("examples/github or email/work");
            prompt_text_value(
                "Entry name",
                Some(header),
                false,
                Some(ghost),
                initial.as_deref(),
            )?
            .ok_or_else(|| anyhow!("Entry creation cancelled"))
            .and_then(|value| sanitize_entry_name(&value))
            .and_then(|value| apply_prefix_to_entry_name(&value, prefix))
        }
    }
}

pub(super) fn confirm_export_instead(display_name: &str) -> Result<bool> {
    Ok(matches!(
        FzfWrapper::builder()
            .confirm(format!(
                "'{}' looks like a large or file-like entry.\n\nExport it to a file instead of copying it to the clipboard?",
                display_name
            ))
            .yes_text("Export")
            .no_text("Clipboard")
            .confirm_dialog()?,
        ConfirmResult::Yes
    ))
}

pub(super) fn prompt_export_destination(
    display_name: &str,
    path: Option<String>,
) -> Result<PathBuf> {
    let suggested = suggested_export_filename(display_name);

    if let Some(path) = path {
        return Ok(PathBuf::from(path));
    }

    let value = prompt_text_value(
        "Export path",
        Some("Enter the destination file path"),
        false,
        Some(&suggested),
        None,
    )?
    .ok_or_else(|| anyhow!("Export cancelled"))?;

    Ok(PathBuf::from(value))
}

pub(super) fn maybe_confirm_overwrite(key: &str) -> Result<()> {
    let store_dir = ensure_password_store_dir()?;
    let path = store_dir.join(format!("{key}.gpg"));
    if !path.exists() {
        return Ok(());
    }

    let confirm = FzfWrapper::builder()
        .confirm(format!("Entry '{key}' already exists.\n\nOverwrite it?"))
        .default_no()
        .confirm_dialog()?;
    if matches!(confirm, ConfirmResult::Yes) {
        Ok(())
    } else {
        bail!("Overwrite cancelled for '{key}'")
    }
}
