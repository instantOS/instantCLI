use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, anyhow, bail};

use crate::menu_utils::{
    ConfirmResult, DialogOutcome, FilePickerBuilder, FilePickerScope, FzfWrapper,
};

use super::clipboard::copy_secret_to_clipboard;
use super::content::{first_secret_line, should_export_output, write_export_file};
use super::dependencies::{ensure_clipboard_dependencies, ensure_otp_dependency, maybe_notify};
use super::names::{
    normalize_file_entry_name, normalize_otp_name, sanitize_entry_name, suggested_file_entry_name,
    suggested_otp_entry_name,
};
use super::pass_cli::{run_status, run_status_with_input, run_stdout};
use super::prompts::{
    confirm_export_instead, maybe_confirm_overwrite, prompt_export_destination,
    prompt_password_with_confirmation, prompt_text_value, resolve_entry_name,
    resolve_entry_name_with_suggestion,
};
use super::store::{ensure_password_store_dir, load_entries, record_frecency};
use super::types::{EntryKind, PassEntry};

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

    let len = length.to_string();
    run_status(["generate", "-f", &entry_name, &len])?;

    maybe_notify(
        "Pass",
        &format!("Generated {length}-character password for {entry_name}"),
    );
    Ok(())
}

pub(super) fn insert_otp_entry(name: Option<String>) -> Result<()> {
    insert_otp_entry_with_prefix(name, None)
}

pub(super) fn insert_otp_entry_with_prefix(
    name: Option<String>,
    prefix: Option<&str>,
) -> Result<()> {
    let raw_name = resolve_entry_name(name, prefix, "New OTP entry")?;
    let entry_name = normalize_otp_name(&raw_name);
    maybe_confirm_overwrite(&entry_name)?;

    let otp_uri = prompt_text_value(
        "OTP URI",
        Some("Paste an otpauth:// URI"),
        false,
        Some("otpauth://totp/example?secret=..."),
        None,
    )?
    .ok_or_else(|| anyhow!("OTP creation cancelled"))?;

    if !otp_uri.starts_with("otpauth://") {
        bail!("OTP entries must start with `otpauth://`");
    }

    store_otp_entry(&raw_name, &entry_name, &otp_uri)
}

pub(super) fn insert_otp_entry_from_clipboard_with_prefix(
    prefix: Option<&str>,
    otp_uri: &str,
) -> Result<()> {
    let suggestion = suggested_otp_entry_name(otp_uri);
    let raw_name = resolve_entry_name_with_suggestion(
        None,
        prefix,
        "New OTP entry from clipboard",
        suggestion.as_deref(),
    )?;
    let entry_name = normalize_otp_name(&raw_name);
    maybe_confirm_overwrite(&entry_name)?;

    store_otp_entry(&raw_name, &entry_name, otp_uri)
}

fn store_otp_entry(raw_name: &str, entry_name: &str, otp_uri: &str) -> Result<()> {
    // `pass otp insert` reads the URI from stdin; it does not accept it as an
    // argument (extra positional arguments are ignored, leaving the URI empty).
    run_status_with_input(["otp", "insert", "-f", entry_name], otp_uri)?;

    maybe_notify("Pass", &format!("Stored OTP entry for {raw_name}"));
    Ok(())
}

pub(super) fn insert_file_entry_with_prefix(prefix: Option<&str>) -> Result<()> {
    let path = match FilePickerBuilder::new()
        .scope(FilePickerScope::Files)
        .hint("Select a file to store in the password store")
        .pick_one()?
    {
        DialogOutcome::Submitted(path) => path,
        DialogOutcome::Cancelled => return Ok(()),
    };

    let suggestion = suggested_file_entry_name(&path);
    let entry_name =
        resolve_entry_name_with_suggestion(None, prefix, "New file entry", Some(&suggestion))?;
    let entry_name = normalize_file_entry_name(&entry_name);

    let data = fs::read(&path).with_context(|| format!("Failed to read {}", path.display()))?;
    insert_file(&entry_name, &data)?;
    maybe_notify(
        "Pass",
        &format!("Stored file {} as {}", path.display(), entry_name),
    );
    Ok(())
}

pub(super) fn copy_primary_entry(entry: &PassEntry) -> Result<()> {
    match entry.kind {
        EntryKind::File => export_entry(entry, None),
        EntryKind::Password => copy_password_entry(entry),
        EntryKind::Otp => copy_otp_entry(entry),
    }
}

pub(super) fn copy_password_entry(entry: &PassEntry) -> Result<()> {
    if !entry.has_secret() {
        bail!("Entry '{}' has no password data", entry.display_name);
    }
    let key = entry.primary_key();
    let output = run_stdout(["show", key])?;

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

    ensure_clipboard_dependencies()?;
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
    let otp_key = entry.otp_key()?;
    let code = generate_otp_code(entry, otp_key)?;

    if code.is_empty() {
        bail!("OTP output for '{}' was empty", entry.display_name);
    }

    ensure_clipboard_dependencies()?;
    copy_secret_to_clipboard(code.as_bytes())?;
    maybe_notify(
        "Pass",
        &format!("Copied OTP code for {}", entry.display_name),
    );
    Ok(())
}

/// Generate an OTP code for the entry.
///
/// Steam Guard and other non-standard tokens (5-digit, custom encoder) are
/// generated locally because `oathtool`/`pass otp` cannot handle them. Standard
/// 6/7/8-digit tokens are delegated to `pass otp` to preserve existing
/// behaviour. If the stored URI cannot be parsed, we fall back to `pass otp`.
fn generate_otp_code(entry: &PassEntry, otp_key: &str) -> Result<String> {
    let raw = run_stdout(["show", otp_key])?;
    let uri = first_secret_line(&raw);

    if let Some(uri) = uri.as_deref()
        && let Ok(auth) = super::totp::OtpAuth::parse(uri)
        && !auth.oathtool_supported()
    {
        return auth
            .generate()
            .with_context(|| format!("Failed to generate OTP code for '{}'", entry.display_name));
    }

    ensure_otp_dependency()?;
    let output = run_stdout(otp_command_args(otp_key))?;
    Ok(String::from_utf8(output)
        .context("OTP output is not valid UTF-8")?
        .trim()
        .to_string())
}

pub(super) fn otp_command_args(key: &str) -> [&str; 2] {
    ["otp", key]
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

    export_entry(&entry, path)?;
    record_frecency(&entry.display_name)?;
    Ok(())
}

pub(super) fn export_entry(entry: &PassEntry, path: Option<String>) -> Result<()> {
    let key = entry.primary_key();
    let output = run_stdout(["show", key])?;
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
    Ok(())
}

pub(super) fn delete_entry_flow(name: Option<String>) -> Result<()> {
    let store_dir = ensure_password_store_dir()?;
    let entries = load_entries(&store_dir)?;
    let entry = select_entry(name, &entries, false)?.ok_or_else(|| anyhow!("Delete cancelled"))?;
    delete_entry(&entry)
}

pub(super) fn delete_entry(entry: &PassEntry) -> Result<()> {
    let confirm_message = match entry.kind {
        EntryKind::Password => format!("Delete password entry '{}'?", entry.display_name),
        EntryKind::Otp => format!("Delete OTP entry '{}'?", entry.display_name),
        EntryKind::File => format!("Delete file entry '{}'?", entry.display_name),
    };

    if !matches!(
        FzfWrapper::builder()
            .confirm(&confirm_message)
            .default_no()
            .confirm_dialog()?,
        ConfirmResult::Yes
    ) {
        return Ok(());
    }

    remove_pass_entry(entry.primary_key())?;

    maybe_notify("Pass", &format!("Deleted '{}'", entry.display_name));
    Ok(())
}

pub(super) fn remove_pass_entry(key: &str) -> Result<()> {
    run_status(["rm", "-f", key])
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
        .items(candidates)
        .select_one()?
    {
        crate::menu_utils::DialogOutcome::Submitted(entry) => Ok(Some(entry)),
        crate::menu_utils::DialogOutcome::Cancelled => Ok(None),
    }
}

pub(super) fn resolve_entry_by_name(
    entries: &[PassEntry],
    name: &str,
    otp_required: bool,
) -> Result<PassEntry> {
    let exact = name.trim();

    entries
        .iter()
        .find(|entry| entry.display_name == exact && (!otp_required || entry.has_otp()))
        .cloned()
        .ok_or_else(|| anyhow!("No pass entry matched '{}'", exact))
}

pub(super) fn rename_entry_interactive(entry: &PassEntry) -> Result<()> {
    let new_name = prompt_text_value(
        "New entry name",
        Some("Rename pass entry"),
        false,
        Some(&entry.display_name),
        Some(&entry.display_name),
    )?
    .ok_or_else(|| anyhow!("Rename cancelled"))?;

    if let Some(target) = rename_entry(entry, &new_name)? {
        maybe_notify(
            "Pass",
            &format!("Renamed '{}' to '{}'", entry.display_name, target),
        );
    }
    Ok(())
}

pub(super) fn rename_target(entry: &PassEntry, new_name: &str) -> Result<String> {
    let new_name = sanitize_entry_name(new_name)?;
    Ok(match entry.kind {
        EntryKind::Password => new_name,
        EntryKind::Otp => normalize_otp_name(&new_name),
        EntryKind::File => normalize_file_entry_name(&new_name),
    })
}

pub(super) fn rename_entry(entry: &PassEntry, new_name: &str) -> Result<Option<String>> {
    let target = rename_target(entry, new_name)?;
    if target == entry.display_name {
        return Ok(None);
    }

    maybe_confirm_overwrite(&target)?;
    move_pass_entry(&entry.display_name, &target)?;
    Ok(Some(target))
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
        None,
    )?
    .ok_or_else(|| anyhow!("OTP edit cancelled"))?;

    if !otp_uri.starts_with("otpauth://") {
        bail!("OTP entries must start with `otpauth://`");
    }

    let key = if entry.has_otp() {
        entry.display_name.clone()
    } else {
        normalize_otp_name(&entry.display_name)
    };

    run_status_with_input(["otp", "insert", "-f", &key], &otp_uri)?;

    maybe_notify("Pass", &format!("Updated OTP for {}", entry.display_name));
    Ok(())
}

pub(super) fn move_pass_entry(from: &str, to: &str) -> Result<()> {
    run_status(["mv", "-f", from, to])
}

pub(super) fn open_store_lazygit(store_dir: &Path) -> Result<()> {
    if !store_dir.join(".git").exists() {
        FzfWrapper::message(
            "Run `ins pass setup` to create a Git repository for your password store and use LazyGit.",
        )?;
        return Ok(());
    }

    Command::new("lazygit")
        .current_dir(store_dir)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .with_context(|| {
            format!(
                "Failed to launch LazyGit in {} (is lazygit installed?)",
                store_dir.display()
            )
        })?;

    Ok(())
}

pub(super) fn pass_edit_editor() -> &'static str {
    "nvim --clean"
}

pub(super) fn pass_edit_was_unchanged(status: std::process::ExitStatus) -> bool {
    status.code() == Some(1)
}

pub(super) fn edit_password_entry(entry: &PassEntry) -> Result<()> {
    if !entry.has_secret() {
        bail!("Entry '{}' has no password data", entry.display_name);
    }
    let key = entry.primary_key();

    let status = Command::new("pass")
        .args(["edit", key])
        .env("EDITOR", pass_edit_editor())
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .with_context(|| format!("Failed to launch `pass edit` for '{key}'"))?;

    if status.success() {
        maybe_notify(
            "Pass",
            &format!("Updated password for {}", entry.display_name),
        );
        return Ok(());
    }

    if pass_edit_was_unchanged(status) {
        return Ok(());
    }

    if !status.success() {
        bail!("`pass edit` failed for '{key}' with status {status}");
    }
    Ok(())
}

pub(super) fn insert_password(key: &str, password: &str) -> Result<()> {
    insert_entry_bytes(key, password.as_bytes(), true)
}

pub(super) fn insert_file(key: &str, data: &[u8]) -> Result<()> {
    insert_entry_bytes(key, data, false)
}

fn insert_entry_bytes(key: &str, data: &[u8], append_newline: bool) -> Result<()> {
    maybe_confirm_overwrite(key)?;

    let mut child = Command::new("pass")
        .args(["insert", "-m", "-f", key])
        .stdin(Stdio::piped())
        .spawn()
        .with_context(|| format!("Failed to start `pass insert` for '{key}'"))?;

    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        stdin
            .write_all(data)
            .context("Failed to write entry data to `pass insert`")?;
        if append_newline {
            stdin.write_all(b"\n").ok();
        }
    }

    let status = child
        .wait()
        .with_context(|| format!("Failed to wait for `pass insert` for '{key}'"))?;

    if !status.success() {
        bail!("`pass insert` failed for '{key}'");
    }

    Ok(())
}
