use std::process::{Command, Stdio};

use anyhow::{Context, Result, anyhow, bail};

use crate::menu_utils::{ConfirmResult, FzfResult, FzfSelectable, FzfWrapper};
use crate::ui::nerd_font::NerdFont;
use crate::ui::preview::PreviewBuilder;

use super::types::{DeleteMode, PassEntry};
use super::utils::*;

pub(super) fn insert_password_entry(name: Option<String>) -> Result<()> {
    insert_password_entry_with_prefix(name, None)
}

pub(super) fn insert_password_entry_with_prefix(
    name: Option<String>,
    prefix: Option<&str>,
) -> Result<()> {
    let entry_name = resolve_entry_name(name, prefix, "New pass entry")?;
    let password = prompt_password_with_confirmation("New password")?;
    insert_password(&entry_name, &password)?;
    maybe_notify("Pass", &format!("Stored password for {entry_name}"));
    Ok(())
}

pub(super) fn generate_password_entry(name: Option<String>, length: usize) -> Result<()> {
    generate_password_entry_with_prefix(name, None, length)
}

pub(super) fn generate_password_entry_with_prefix(
    name: Option<String>,
    prefix: Option<&str>,
    length: usize,
) -> Result<()> {
    if length == 0 {
        bail!("Password length must be greater than zero");
    }

    let entry_name = resolve_entry_name(name, prefix, "Generated pass entry")?;
    maybe_confirm_overwrite(&entry_name)?;

    let status = Command::new("pass")
        .args(["generate", "-f", &entry_name, &length.to_string()])
        .status()
        .with_context(|| format!("Failed to run `pass generate` for '{entry_name}'"))?;

    if !status.success() {
        bail!("`pass generate` failed for '{entry_name}'");
    }

    maybe_notify(
        "Pass",
        &format!("Generated {length}-character password for {entry_name}"),
    );
    Ok(())
}

pub(super) fn insert_otp_entry(name: Option<String>) -> Result<()> {
    insert_otp_entry_with_prefix(name, None)
}

pub(super) fn insert_otp_entry_with_prefix(name: Option<String>, prefix: Option<&str>) -> Result<()> {
    let raw_name = resolve_entry_name(name, prefix, "New OTP entry")?;
    let entry_name = normalize_otp_name(&raw_name);
    maybe_confirm_overwrite(&entry_name)?;

    let otp_uri = prompt_text_value(
        "OTP URI",
        Some("Paste an otpauth:// URI"),
        false,
        Some("otpauth://totp/example?secret=..."),
    )?
    .ok_or_else(|| anyhow!("OTP creation cancelled"))?;

    if !otp_uri.starts_with("otpauth://") {
        bail!("OTP entries must start with `otpauth://`");
    }

    let status = Command::new("pass")
        .args(["otp", "insert", "-f", &entry_name, &otp_uri])
        .status()
        .with_context(|| format!("Failed to store OTP entry '{entry_name}'"))?;

    if !status.success() {
        bail!("`pass otp insert` failed for '{entry_name}'");
    }

    maybe_notify("Pass", &format!("Stored OTP entry for {raw_name}"));
    Ok(())
}

pub(super) fn copy_primary_entry(entry: &PassEntry) -> Result<()> {
    if entry.has_secret() {
        copy_password_entry(entry)
    } else {
        copy_otp_entry(entry)
    }
}

pub(super) fn copy_password_entry(entry: &PassEntry) -> Result<()> {
    ensure_clipboard_dependencies()?;

    let key = entry
        .secret_key
        .as_deref()
        .ok_or_else(|| anyhow!("Entry '{}' has no password data", entry.display_name))?;
    let output = run_pass_stdout(["show", key])?;

    if should_export_output(entry, &output) && confirm_export_instead(&entry.display_name)? {
        let destination = prompt_export_destination(&entry.display_name, None)?;
        write_export_file(&destination, &output)?;
        maybe_notify(
            "Pass",
            &format!(
                "Exported '{}' to {}",
                entry.display_name,
                destination.display()
            ),
        );
        return Ok(());
    }

    let secret = first_secret_line(&output).ok_or_else(|| {
        anyhow!(
            "Entry '{}' does not contain a password line",
            entry.display_name
        )
    })?;

    copy_secret_to_clipboard(secret.as_bytes())?;
    maybe_notify(
        "Pass",
        &format!("Copied password for {}", entry.display_name),
    );
    Ok(())
}

pub(super) fn copy_otp_entry(entry: &PassEntry) -> Result<()> {
    ensure_clipboard_dependencies()?;
    ensure_otp_dependency()?;

    let otp_key = resolve_otp_key(entry)?;
    let output = run_pass_stdout(["otp", &otp_key])?;
    let code = String::from_utf8(output)
        .context("OTP output is not valid UTF-8")?
        .trim()
        .to_string();

    if code.is_empty() {
        bail!("OTP output for '{}' was empty", entry.display_name);
    }

    copy_secret_to_clipboard(code.as_bytes())?;
    maybe_notify(
        "Pass",
        &format!("Copied OTP code for {}", entry.display_name),
    );
    Ok(())
}

pub(super) fn copy_otp_flow(name: Option<String>) -> Result<()> {
    let store_dir = ensure_password_store_dir()?;
    let entries = load_entries(&store_dir)?;
    let entry = select_entry(name, &entries, true)?.ok_or_else(|| anyhow!("OTP copy cancelled"))?;
    copy_otp_entry(&entry)?;
    record_frecency(&entry.display_name)?;
    Ok(())
}

pub(super) fn export_entry_flow(name: Option<String>, path: Option<String>) -> Result<()> {
    let store_dir = ensure_password_store_dir()?;
    let entries = load_entries(&store_dir)?;
    let entry = select_entry(name, &entries, false)?.ok_or_else(|| anyhow!("Export cancelled"))?;

    let key = entry.primary_key()?.to_string();
    let output = run_pass_stdout(["show", &key])?;
    let destination = prompt_export_destination(&entry.display_name, path)?;
    write_export_file(&destination, &output)?;

    maybe_notify(
        "Pass",
        &format!(
            "Exported '{}' to {}",
            entry.display_name,
            destination.display()
        ),
    );
    record_frecency(&entry.display_name)?;
    Ok(())
}

pub(super) fn delete_entry_flow(name: Option<String>) -> Result<()> {
    let store_dir = ensure_password_store_dir()?;
    let entries = load_entries(&store_dir)?;
    let entry = select_entry(name, &entries, false)?.ok_or_else(|| anyhow!("Delete cancelled"))?;

    let mode = choose_delete_mode(&entry)?;
    let confirm_message = match mode {
        DeleteMode::Secret => format!("Delete password entry '{}'?", entry.display_name),
        DeleteMode::Otp => format!("Delete OTP companion for '{}'?", entry.display_name),
        DeleteMode::Both => format!(
            "Delete password entry '{}' and its OTP companion?",
            entry.display_name
        ),
    };

    if !matches!(FzfWrapper::confirm(&confirm_message)?, ConfirmResult::Yes) {
        return Ok(());
    }

    match mode {
        DeleteMode::Secret => {
            let key = entry
                .secret_key
                .as_deref()
                .ok_or_else(|| anyhow!("Entry '{}' has no password data", entry.display_name))?;
            remove_pass_entry(key)?;
        }
        DeleteMode::Otp => {
            let key = resolve_otp_key(&entry)?;
            remove_pass_entry(&key)?;
        }
        DeleteMode::Both => {
            if let Some(key) = entry.secret_key.as_deref() {
                remove_pass_entry(key)?;
            }
            if let Some(key) = entry.otp_key.as_deref() {
                remove_pass_entry(key)?;
            }
        }
    }

    maybe_notify("Pass", &format!("Deleted '{}'", entry.display_name));
    Ok(())
}

fn choose_delete_mode(entry: &PassEntry) -> Result<DeleteMode> {
    match (entry.has_secret(), entry.has_otp()) {
        (true, false) => Ok(DeleteMode::Secret),
        (false, true) => Ok(DeleteMode::Otp),
        (true, true) => {
            #[derive(Clone)]
            struct DeleteChoice {
                label: &'static str,
                mode: DeleteMode,
                preview: &'static str,
            }

            impl FzfSelectable for DeleteChoice {
                fn fzf_display_text(&self) -> String {
                    self.label.to_string()
                }

                fn fzf_preview(&self) -> crate::menu::protocol::FzfPreview {
                    PreviewBuilder::new()
                        .header(NerdFont::Trash, self.label)
                        .text(self.preview)
                        .build()
                }

                fn fzf_key(&self) -> String {
                    self.label.to_string()
                }
            }

            let choices = vec![
                DeleteChoice {
                    label: "Delete password only",
                    mode: DeleteMode::Secret,
                    preview: "Remove the main password entry and keep the OTP companion.",
                },
                DeleteChoice {
                    label: "Delete OTP only",
                    mode: DeleteMode::Otp,
                    preview: "Remove only the OTP companion entry.",
                },
                DeleteChoice {
                    label: "Delete both",
                    mode: DeleteMode::Both,
                    preview: "Remove both the password entry and the OTP companion.",
                },
            ];

            match FzfWrapper::builder()
                .prompt("Delete mode")
                .header("Choose what to remove")
                .select(choices)?
            {
                FzfResult::Selected(choice) => Ok(choice.mode),
                _ => Err(anyhow!("Delete cancelled")),
            }
        }
        (false, false) => bail!("Entry '{}' has nothing to delete", entry.display_name),
    }
}

pub(super) fn remove_pass_entry(key: &str) -> Result<()> {
    let status = Command::new("pass")
        .args(["rm", "-f", key])
        .status()
        .with_context(|| format!("Failed to delete pass entry '{key}'"))?;

    if !status.success() {
        bail!("`pass rm` failed for '{key}'");
    }

    Ok(())
}

pub(super) fn select_entry(
    name: Option<String>,
    entries: &[PassEntry],
    otp_required: bool,
) -> Result<Option<PassEntry>> {
    if let Some(name) = name {
        return resolve_entry_by_name(entries, &name, otp_required).map(Some);
    }

    let candidates: Vec<PassEntry> = if otp_required {
        entries
            .iter()
            .filter(|entry| entry.has_otp())
            .cloned()
            .collect()
    } else {
        entries.to_vec()
    };

    if candidates.is_empty() {
        return Ok(None);
    }

    match FzfWrapper::builder()
        .prompt(if otp_required {
            "OTP entry"
        } else {
            "Pass entry"
        })
        .header(if otp_required {
            "Select an entry with OTP support"
        } else {
            "Select a pass entry"
        })
        .select(candidates)?
    {
        FzfResult::Selected(entry) => Ok(Some(entry)),
        _ => Ok(None),
    }
}

pub(super) fn resolve_entry_by_name(
    entries: &[PassEntry],
    name: &str,
    otp_required: bool,
) -> Result<PassEntry> {
    let normalized = name.trim().trim_end_matches(".otp");

    entries
        .iter()
        .find(|entry| {
            let matches_name = entry.display_name == normalized
                || entry.secret_key.as_deref() == Some(name.trim())
                || entry.otp_key.as_deref() == Some(name.trim());
            matches_name && (!otp_required || entry.has_otp())
        })
        .cloned()
        .ok_or_else(|| anyhow!("No pass entry matched '{}'", name.trim()))
}

pub(super) fn rename_entry_interactive(entry: &PassEntry) -> Result<()> {
    let new_name = prompt_text_value_prefilled(
        "New entry name",
        Some("Rename pass entry"),
        &entry.display_name,
        false,
        Some(&entry.display_name),
    )?
    .ok_or_else(|| anyhow!("Rename cancelled"))?;

    let new_name = sanitize_entry_name(&new_name)?;
    if new_name == entry.display_name {
        return Ok(());
    }

    if let Some(secret_key) = &entry.secret_key {
        move_pass_entry(secret_key, &new_name)?;
    }
    if let Some(otp_key) = &entry.otp_key {
        move_pass_entry(otp_key, &normalize_otp_name(&new_name))?;
    }

    maybe_notify(
        "Pass",
        &format!("Renamed '{}' to '{}'", entry.display_name, new_name),
    );
    Ok(())
}

pub(super) fn upsert_otp_entry_interactive(entry: &PassEntry) -> Result<()> {
    let otp_uri = prompt_text_value(
        "OTP URI",
        Some(if entry.has_otp() {
            "Edit OTP URI"
        } else {
            "Create OTP URI"
        }),
        false,
        Some("otpauth://totp/example?secret=..."),
    )?
    .ok_or_else(|| anyhow!("OTP edit cancelled"))?;

    if !otp_uri.starts_with("otpauth://") {
        bail!("OTP entries must start with `otpauth://`");
    }

    let key = entry
        .otp_key
        .clone()
        .unwrap_or_else(|| normalize_otp_name(&entry.display_name));

    let status = Command::new("pass")
        .args(["otp", "insert", "-f", &key, &otp_uri])
        .status()
        .with_context(|| format!("Failed to update OTP entry '{key}'"))?;

    if !status.success() {
        bail!("`pass otp insert` failed for '{key}'");
    }

    maybe_notify("Pass", &format!("Updated OTP for {}", entry.display_name));
    Ok(())
}

pub(super) fn move_pass_entry(from: &str, to: &str) -> Result<()> {
    let status = Command::new("pass")
        .args(["mv", "-f", from, to])
        .status()
        .with_context(|| format!("Failed to rename pass entry '{}' to '{}'", from, to))?;

    if !status.success() {
        bail!("`pass mv` failed for '{}' -> '{}'", from, to);
    }

    Ok(())
}

pub(super) fn insert_password(key: &str, password: &str) -> Result<()> {
    maybe_confirm_overwrite(key)?;

    let mut child = Command::new("pass")
        .args(["insert", "-m", "-f", key])
        .stdin(Stdio::piped())
        .spawn()
        .with_context(|| format!("Failed to start `pass insert` for '{key}'"))?;

    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        stdin
            .write_all(password.as_bytes())
            .context("Failed to write password to `pass insert`")?;
        stdin.write_all(b"\n").ok();
    }

    let status = child
        .wait()
        .with_context(|| format!("Failed to wait for `pass insert` for '{key}'"))?;

    if !status.success() {
        bail!("`pass insert` failed for '{key}'");
    }

    Ok(())
}
